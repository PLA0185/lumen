//! 重复任务的端到端集成测试（任务书 §5 的验收例）。
//!
//! ## 为什么单独一个文件
//!
//! §5 给出的验收例是一个**跨多个操作的序列**，单独测每个操作无法覆盖
//! "操作之间的相互影响"。这里把整条链路跑一遍：
//!
//! > 创建周一/三/五重复任务 → 完成周一那次 → 单独改周三那次的标题 →
//! > 从下周一起修改未来任务的时间 → 跳过某一次 → 修改整个系列的优先级 →
//! > 模拟重启后重新读取，确认过去完成记录、例外、未来新规则都正确
//!
//! 这些用例直接调用业务实现（`*_impl(&AppState, ...)`），
//! 不经过 Tauri 的 `State`，因此无需构造 App 实例。

use crate::commands::AppState;
use crate::db::Db;
use crate::models::Task;
use crate::recurrence_service::recurring_scope_info_inner;
use crate::recurrence_service::{
    create_recurring_impl, delete_recurring_impl, edit_instance_impl, list_instances,
    skip_occurrence_impl, CreateRecurringInput, DeleteMode, EditScope, InstancePatch,
};
// `try_get` 来自 Row trait，必须显式引入（trait 方法不会自动可见）
use sqlx::Row;

/// 建一个临时库并返回状态句柄。调用方负责删除目录。
async fn setup(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("lumen-{name}-{}", uuid::Uuid::now_v7()));
    let db = Db::init(&dir).await.expect("初始化数据库");
    (AppState::new(db), dir)
}

/// 按 occurrence_key 找实例
async fn find_by_occurrence(state: &AppState, series_id: &str, key: &str) -> Option<Task> {
    list_instances(state, series_id)
        .await
        .ok()?
        .into_iter()
        .find(|t| t.occurrence_key.as_deref() == Some(key))
}

/// 某个本地日期对应的 UTC key（测试固定用 Asia/Shanghai，+08:00）
fn shanghai_key(date: &str, time: &str) -> String {
    // 北京时间 → UTC 需减 8 小时
    let local =
        chrono::NaiveDateTime::parse_from_str(&format!("{date}T{time}"), "%Y-%m-%dT%H:%M:%S")
            .expect("构造本地时间");
    let utc = local - chrono::Duration::hours(8);
    format!("{}.000Z", utc.format("%Y-%m-%dT%H:%M:%S"))
}

/// §5 验收例：周一/三/五重复任务的完整生命周期
#[tokio::test]
async fn acceptance_weekly_mon_wed_fri_full_lifecycle() {
    let (state, dir) = setup("rec-accept").await;

    // ------------------------------------------------------------------
    // 1. 创建周一/三/五重复任务，起点为 2026-09-21（周一）09:00
    // ------------------------------------------------------------------
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "写周报".into(),
            description: Some("每周一三五".into()),
            priority: Some(1),
            project_id: None,
            category_id: None,
            estimated_minutes: Some(30),
            tag_ids: vec![],
            rrule: "FREQ=WEEKLY;BYDAY=MO,WE,FR".into(),
            tzid: Some("Asia/Shanghai".into()),
            dtstart_local: "2026-09-21T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(30),
        },
    )
    .await
    .expect("应能创建重复任务");

    assert!(created.series_id.len() > 10);
    assert!(
        created.created_count >= 4,
        "30 天内应物化多次发生，实际 {}",
        created.created_count
    );
    assert!(
        created.description.contains("周一"),
        "{}",
        created.description
    );

    let series_id = created.series_id.clone();
    let instances = list_instances(&state, &series_id).await.unwrap();
    let dates: Vec<String> = instances
        .iter()
        .filter_map(|t| t.occurrence_key.clone())
        .collect();
    // 09-21(一) 09-23(三) 09-25(五) 09-28(一) 09-30(三) 10-02(五) …
    assert_eq!(dates[0], shanghai_key("2026-09-21", "09:00:00"));
    assert_eq!(dates[1], shanghai_key("2026-09-23", "09:00:00"));
    assert_eq!(dates[2], shanghai_key("2026-09-25", "09:00:00"));

    // ------------------------------------------------------------------
    // 2. 完成周一那次（09-21）——只完成该次，不影响系列
    // ------------------------------------------------------------------
    let mon_key = shanghai_key("2026-09-21", "09:00:00");
    let mon = find_by_occurrence(&state, &series_id, &mon_key)
        .await
        .expect("应存在周一那次");
    let now = crate::db::to_db_time(crate::db::utc_now());
    sqlx::query("UPDATE tasks SET status = 'done', completed_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(&mon.id)
        .execute(state.db.pool())
        .await
        .unwrap();

    let mon_after = find_by_occurrence(&state, &series_id, &mon_key)
        .await
        .unwrap();
    assert_eq!(mon_after.status, "done");
    assert!(mon_after.completed_at.is_some(), "完成时间必须真实记录");

    // 其它发生不受影响
    let wed_key = shanghai_key("2026-09-23", "09:00:00");
    let wed = find_by_occurrence(&state, &series_id, &wed_key)
        .await
        .unwrap();
    assert_eq!(wed.status, "todo", "完成一次不应影响其它发生");

    // ------------------------------------------------------------------
    // 3. 单独改周三那次的标题（仅此次）
    // ------------------------------------------------------------------
    let r = edit_instance_impl(
        &state,
        wed.id.clone(),
        EditScope::ThisOnly,
        InstancePatch {
            title: Some("写月报（本周特例）".into()),
            ..Default::default()
        },
        None,
        false,
    )
    .await
    .expect("仅此次修改应成功");

    assert_eq!(r.affected, 1);
    assert_eq!(r.affected_history, 0);

    let wed_after = find_by_occurrence(&state, &series_id, &wed_key)
        .await
        .unwrap();
    assert_eq!(wed_after.title, "写月报（本周特例）");
    assert_eq!(
        wed_after.occurrence_kind.as_deref(),
        Some("exception"),
        "单独改过的实例必须标记为例外，否则改规则时会被冲掉"
    );
    // occurrence_key 必须保持不变——它是"这一次"的稳定身份
    assert_eq!(wed_after.occurrence_key.as_deref(), Some(wed_key.as_str()));

    // 其它发生的标题不变
    let fri = find_by_occurrence(&state, &series_id, &shanghai_key("2026-09-25", "09:00:00"))
        .await
        .unwrap();
    assert_eq!(fri.title, "写周报", "单独修改不应影响其它发生");
    assert_eq!(mon_after.title, "写周报");

    // ------------------------------------------------------------------
    // 4. 从下周一（09-28）起修改未来发生的时间
    //    —— 通过"此次及以后"新增一个分段
    // ------------------------------------------------------------------
    let next_mon_key = shanghai_key("2026-09-28", "09:00:00");
    let next_mon = find_by_occurrence(&state, &series_id, &next_mon_key)
        .await
        .expect("应存在下周一那次");

    let r = edit_instance_impl(
        &state,
        next_mon.id.clone(),
        EditScope::ThisAndFuture,
        InstancePatch {
            estimated_minutes: Some(60),
            ..Default::default()
        },
        Some("FREQ=WEEKLY;BYDAY=MO,WE,FR".into()),
        true,
    )
    .await
    .expect("此次及以后修改应成功");

    assert!(
        r.message.contains("规则版本") || r.affected > 0 || r.regenerated > 0,
        "结果说明应反映重算情况：{}",
        r.message
    );

    // 分段应已建立
    let seg_count: i64 =
        sqlx::query("SELECT COUNT(*) AS n FROM task_series_segments WHERE series_id = ?1")
            .bind(&series_id)
            .fetch_one(state.db.pool())
            .await
            .unwrap()
            .try_get("n")
            .unwrap();
    assert_eq!(seg_count, 1, "应新增 1 个规则分段");

    // ------------------------------------------------------------------
    // 5. 历史保护：过去已完成的那次必须完好无损
    // ------------------------------------------------------------------
    let mon_final = find_by_occurrence(&state, &series_id, &mon_key)
        .await
        .unwrap();
    assert_eq!(mon_final.status, "done", "历史完成状态不能因改规则而丢失");
    assert!(mon_final.completed_at.is_some(), "历史完成时间必须保留");

    // 例外也必须在重算后保留
    let wed_final = find_by_occurrence(&state, &series_id, &wed_key)
        .await
        .unwrap();
    assert_eq!(
        wed_final.title, "写月报（本周特例）",
        "用户单独改过的实例不能在改规则时被冲掉"
    );

    // ------------------------------------------------------------------
    // 6. 跳过某一次（09-25 周五）——系列继续，只是这一次不发生
    // ------------------------------------------------------------------
    let fri_id = fri.id.clone();
    let r = skip_occurrence_impl(&state, fri_id.clone())
        .await
        .expect("跳过某一次应成功");
    assert!(r.message.contains("跳过"), "{}", r.message);

    // 该次实例应已软删除
    let fri_gone = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?1")
        .bind(&fri_id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert!(fri_gone.deleted_at.is_some(), "跳过的实例应进回收站");

    // 跳过记录应存在（防止重新物化时又生成）
    let skip_count: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM task_series_skips WHERE series_id = ?1 AND occurrence_key = ?2",
    )
    .bind(&series_id)
    .bind(shanghai_key("2026-09-25", "09:00:00"))
    .fetch_one(state.db.pool())
    .await
    .unwrap()
    .try_get("n")
    .unwrap();
    assert_eq!(skip_count, 1, "必须留下跳过记录");

    // 系列本身不受影响：其它发生仍在
    let remaining = list_instances(&state, &series_id).await.unwrap();
    assert!(
        remaining.len() >= 3,
        "跳过一次不应影响系列其它发生，实际剩 {}",
        remaining.len()
    );

    // ------------------------------------------------------------------
    // 7. 修改整个系列的优先级
    // ------------------------------------------------------------------
    // 注意：第 4 步的"此次及以后"会重算未来实例，之前拿到的 task_id 可能
    // 已被删除，因此这里必须**重新查询**当前有效的实例 ID，
    // 而不能复用第 4 步之前的变量。
    let current = find_by_occurrence(&state, &series_id, &next_mon_key)
        .await
        .expect("重算后应仍存在下周一那次");
    let r = edit_instance_impl(
        &state,
        current.id.clone(),
        EditScope::WholeSeries,
        InstancePatch {
            priority: Some(3),
            ..Default::default()
        },
        None,
        true,
    )
    .await
    .expect("整个系列修改应成功");
    assert!(r.affected > 0, "应同步到若干未完成实例");

    // 未完成实例的优先级都应变为 3
    let unfinished: Vec<Task> = list_instances(&state, &series_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|t| t.status != "done")
        .collect();
    for t in &unfinished {
        assert_eq!(t.priority, 3, "未完成实例「{}」的优先级应同步为 3", t.title);
    }

    // 已完成的历史实例字段**不变**（§5 保护历史）
    let mon_check = find_by_occurrence(&state, &series_id, &mon_key)
        .await
        .unwrap();
    assert_eq!(mon_check.status, "done");
    assert_eq!(mon_check.priority, 1, "已完成历史不应被追改字段");

    // ------------------------------------------------------------------
    // 8. 模拟重启：重新打开数据库，确认所有状态都持久化了
    // ------------------------------------------------------------------
    drop(state);
    let db2 = Db::init(&dir).await.expect("重新打开数据库");
    let state2 = AppState::new(db2);

    let after_restart = list_instances(&state2, &series_id).await.unwrap();
    let mon_r = after_restart
        .iter()
        .find(|t| t.occurrence_key.as_deref() == Some(mon_key.as_str()))
        .expect("重启后历史实例仍在");
    assert_eq!(mon_r.status, "done", "重启后完成状态必须还在");
    assert!(mon_r.completed_at.is_some());

    let wed_r = after_restart
        .iter()
        .find(|t| t.occurrence_key.as_deref() == Some(wed_key.as_str()))
        .expect("重启后例外实例仍在");
    assert_eq!(wed_r.title, "写月报（本周特例）", "重启后例外必须还在");
    assert_eq!(wed_r.occurrence_kind.as_deref(), Some("exception"));

    let seg_r: i64 =
        sqlx::query("SELECT COUNT(*) AS n FROM task_series_segments WHERE series_id = ?1")
            .bind(&series_id)
            .fetch_one(state2.db.pool())
            .await
            .unwrap()
            .try_get("n")
            .unwrap();
    assert_eq!(seg_r, 1, "重启后分段必须还在");

    let skip_r: i64 =
        sqlx::query("SELECT COUNT(*) AS n FROM task_series_skips WHERE series_id = ?1")
            .bind(&series_id)
            .fetch_one(state2.db.pool())
            .await
            .unwrap()
            .try_get("n")
            .unwrap();
    assert_eq!(skip_r, 1, "重启后跳过记录必须还在");

    // 唯一性仍被数据库强制：同系列同 occurrence_key 不可能有两条
    let dup: i64 = sqlx::query(
        "SELECT COUNT(*) AS n FROM (
            SELECT series_id, occurrence_key FROM tasks
            WHERE series_id = ?1 AND occurrence_key IS NOT NULL
            GROUP BY series_id, occurrence_key HAVING COUNT(*) > 1)",
    )
    .bind(&series_id)
    .fetch_one(state2.db.pool())
    .await
    .unwrap()
    .try_get("n")
    .unwrap();
    assert_eq!(dup, 0, "不允许出现重复的 (series_id, occurrence_key)");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 历史保护：改"此次及以后"时若会影响已完成历史且未确认，必须被拒绝
#[tokio::test]
async fn edit_future_refuses_without_history_confirmation() {
    let (state, dir) = setup("rec-history").await;

    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "每日站会".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("Asia/Shanghai".into()),
            dtstart_local: "2026-09-21T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(10),
        },
    )
    .await
    .unwrap();
    let sid = created.series_id.clone();

    let all = list_instances(&state, &sid).await.unwrap();
    assert!(all.len() >= 3);

    // 把中间那次标记为已完成（制造历史）
    let mid = &all[1];
    let now = crate::db::to_db_time(crate::db::utc_now());
    sqlx::query("UPDATE tasks SET status = 'done', completed_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(&mid.id)
        .execute(state.db.pool())
        .await
        .unwrap();

    // 从最后一次起改规则，且**不确认**影响历史 → 必须被拒绝
    let last = all.last().unwrap();
    let err = edit_instance_impl(
        &state,
        last.id.clone(),
        EditScope::ThisAndFuture,
        InstancePatch {
            estimated_minutes: Some(15),
            ..Default::default()
        },
        None,
        false, // 未确认
    )
    .await
    .expect_err("未确认影响历史时必须拒绝");

    assert!(
        err.message.contains("历史"),
        "错误信息应说明会影响历史：{}",
        err.message
    );
    assert!(err.hint.is_some(), "应给出可操作的下一步提示");

    // 确认后应能成功
    edit_instance_impl(
        &state,
        last.id.clone(),
        EditScope::ThisAndFuture,
        InstancePatch {
            estimated_minutes: Some(15),
            ..Default::default()
        },
        None,
        true,
    )
    .await
    .expect("确认后应允许执行");

    // 历史仍然是完成状态
    let mid_after = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?1")
        .bind(&mid.id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(mid_after.status, "done");
    assert!(mid_after.completed_at.is_some());

    let _ = std::fs::remove_dir_all(&dir);
}

/// 单次取消：显示为"该次跳过"，绝不取消整条系列
#[tokio::test]
async fn skip_one_occurrence_keeps_series_alive() {
    let (state, dir) = setup("rec-skip").await;

    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "晨跑".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("Asia/Shanghai".into()),
            dtstart_local: "2026-09-21T07:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(7),
        },
    )
    .await
    .unwrap();
    let sid = created.series_id.clone();
    let before = list_instances(&state, &sid).await.unwrap().len();

    let target = list_instances(&state, &sid).await.unwrap()[2].clone();
    skip_occurrence_impl(&state, target.id.clone())
        .await
        .unwrap();

    let after = list_instances(&state, &sid).await.unwrap().len();
    assert_eq!(after, before - 1, "只应少这一次");

    // 系列本身仍存在，且仍能继续物化未来的发生
    let series_exists: i64 = sqlx::query("SELECT COUNT(*) AS n FROM task_series WHERE id = ?1")
        .bind(&sid)
        .fetch_one(state.db.pool())
        .await
        .unwrap()
        .try_get("n")
        .unwrap();
    assert_eq!(series_exists, 1, "系列必须仍然存在");

    // 再物化一次不应把跳过的那次重新生成出来
    let new_key = crate::db::to_db_time(crate::db::utc_now() + chrono::Duration::days(30));
    crate::recurrence_service::recurring_materialize_inner(
        &state,
        sid.clone(),
        "2026-01-01T00:00:00.000Z".into(),
        new_key,
    )
    .await
    .unwrap();

    let skipped_key = target.occurrence_key.clone().unwrap();
    let regenerated = sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE series_id = ?1 AND occurrence_key = ?2 AND deleted_at IS NULL",
    )
    .bind(&sid)
    .bind(&skipped_key)
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert!(regenerated.is_empty(), "被跳过的那一次不应被重新物化出来");

    // 而其它未来的发生应该新生成了
    let total_after = list_instances(&state, &sid).await.unwrap().len();
    assert!(total_after > after, "系列应继续产生新的发生");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 删除范围：整个系列删除后，实例软删除且系列消失
#[tokio::test]
async fn delete_whole_series_soft_deletes_instances() {
    let (state, dir) = setup("rec-del").await;

    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "季度复盘".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=MONTHLY;BYMONTHDAY=1".into(),
            tzid: Some("Asia/Shanghai".into()),
            dtstart_local: "2026-10-01T10:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(200),
        },
    )
    .await
    .unwrap();
    let sid = created.series_id.clone();
    let some = list_instances(&state, &sid).await.unwrap();
    assert!(!some.is_empty());
    let first_id = some[0].id.clone();

    let r = delete_recurring_impl(&state, first_id.clone(), DeleteMode::WholeSeries, true)
        .await
        .expect("删除整个系列应成功");
    assert!(r.affected >= 1);

    // 系列应消失
    let n: i64 = sqlx::query("SELECT COUNT(*) AS n FROM task_series WHERE id = ?1")
        .bind(&sid)
        .fetch_one(state.db.pool())
        .await
        .unwrap()
        .try_get("n")
        .unwrap();
    assert_eq!(n, 0, "系列记录应被删除");

    // 实例应软删除（仍在表中，可在回收站恢复）
    let t = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?1")
        .bind(&first_id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert!(t.deleted_at.is_some(), "实例应进回收站而不是被硬删除");
    assert!(
        t.series_id.is_none(),
        "实例应解除与系列的关联，避免留下悬挂引用"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 范围信息接口：界面据此决定是否弹出范围选择、哪些选项可用
#[tokio::test]
async fn scope_info_reports_recurrence_and_history() {
    let (state, dir) = setup("rec-scope").await;

    // 非重复任务
    let now = crate::db::to_db_time(crate::db::utc_now());
    sqlx::query(
        "INSERT INTO tasks (id, title, status, created_at, updated_at, sort_order, occurrence_kind, is_exception)
         VALUES ('plain', '普通任务', 'todo', ?1, ?1, 0, 'single', 0)",
    )
    .bind(&now)
    .execute(state.db.pool())
    .await
    .unwrap();

    let info = recurring_scope_info_inner(&state, "plain".to_string())
        .await
        .unwrap();
    assert_eq!(info["isRecurring"], false);
    assert!(info["reason"].as_str().unwrap().contains("不重复"));

    // 重复任务
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "重复任务".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("Asia/Shanghai".into()),
            dtstart_local: "2026-09-21T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(5),
        },
    )
    .await
    .unwrap();

    let all = list_instances(&state, &created.series_id).await.unwrap();
    let info2 = recurring_scope_info_inner(&state, all[2].id.clone())
        .await
        .unwrap();
    assert_eq!(info2["isRecurring"], true);
    assert!(info2["seriesId"].as_str().is_some());
    assert!(info2["occurrenceKey"].as_str().is_some());
    assert!(info2["totalInstances"].as_i64().unwrap() >= 3);
    // 三种范围都要有可读说明（§5 要求"不适用的选项要禁用并说明原因"）
    assert!(info2["notes"]["thisOnly"].as_str().is_some());
    assert!(info2["notes"]["thisAndFuture"].as_str().is_some());
    assert!(info2["notes"]["wholeSeries"].as_str().is_some());

    let _ = std::fs::remove_dir_all(&dir);
}
