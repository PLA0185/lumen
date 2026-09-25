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
    recurring_materialize_inner, skip_occurrence_impl, CreateRecurringInput, DeleteMode, EditScope,
    InstancePatch,
};
// `try_get` 来自 Row trait，必须显式引入（trait 方法不会自动可见）
use sqlx::Row;

/// 建一个临时库并返回状态句柄。调用方负责删除目录。
async fn setup(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("lumen-{name}-{}", uuid::Uuid::now_v7()));
    let db = Db::init(&dir).await.expect("初始化数据库");
    (AppState::new(db), dir)
}

#[tokio::test]
async fn old_series_uses_persistent_template_and_tags() {
    let (state, dir) = setup("rec-old-template").await;
    let now = crate::db::to_db_time(crate::db::utc_now());
    sqlx::query(
        "INSERT INTO tags (id, name, sort_order, created_at, updated_at)
                 VALUES ('rec-tag', '重复标签', 0, ?1, ?1)",
    )
    .bind(&now)
    .execute(state.db.pool())
    .await
    .unwrap();
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "原始标题".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec!["rec-tag".into()],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2018-01-01T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(1),
        },
    )
    .await
    .unwrap();
    let first = list_instances(&state, &created.series_id).await.unwrap()[0].clone();
    edit_instance_impl(
        &state,
        first.id,
        EditScope::ThisOnly,
        InstancePatch {
            title: Some("仅此次标题".into()),
            ..Default::default()
        },
        None,
        false,
    )
    .await
    .unwrap();
    let count = recurring_materialize_inner(
        &state,
        created.series_id.clone(),
        "2026-04-01T00:00:00.000Z".into(),
        "2026-04-04T00:00:00.000Z".into(),
    )
    .await
    .unwrap();
    assert_eq!(count, 3);
    let rows: Vec<Task> = sqlx::query_as(
        "SELECT * FROM tasks WHERE series_id = ?1 AND occurrence_key >= '2026-04-01'
         AND occurrence_key < '2026-04-04' ORDER BY occurrence_key",
    )
    .bind(&created.series_id)
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|t| t.title == "原始标题"));
    assert!(rows.iter().all(|t| t.occurrence_index.unwrap_or(0) > 3000));
    for row in &rows {
        let tags: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_tags WHERE task_id = ?1")
            .bind(&row.id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
        assert_eq!(tags, 1);
    }
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn v5_upgrade_recovers_template_without_using_completed_or_exception() {
    let (state, dir) = setup("rec-v5-upgrade").await;
    let now = crate::db::to_db_time(crate::db::utc_now());
    sqlx::query(
        "INSERT INTO tags (id, name, sort_order, created_at, updated_at)
                 VALUES ('upgrade-tag', '升级标签', 0, ?1, ?1)",
    )
    .bind(&now)
    .execute(state.db.pool())
    .await
    .unwrap();
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "稳定模板".into(),
            description: None,
            priority: Some(2),
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec!["upgrade-tag".into()],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2026-10-01T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(5),
        },
    )
    .await
    .unwrap();
    let before = list_instances(&state, &created.series_id).await.unwrap();
    sqlx::query("UPDATE tasks SET status='done', completed_at=?1, title='历史标题' WHERE id=?2")
        .bind(&now)
        .bind(&before[0].id)
        .execute(state.db.pool())
        .await
        .unwrap();
    edit_instance_impl(
        &state,
        before[1].id.clone(),
        EditScope::ThisOnly,
        InstancePatch {
            title: Some("例外标题".into()),
            ..Default::default()
        },
        None,
        false,
    )
    .await
    .unwrap();
    let keys_before: Vec<(String,)> = sqlx::query_as(
        "SELECT occurrence_key FROM tasks WHERE series_id=?1 ORDER BY occurrence_key",
    )
    .bind(&created.series_id)
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    for statement in [
        "DROP TABLE task_series_rebuilds",
        "DROP TABLE task_series_tags",
        "DROP TABLE task_series_template",
    ] {
        sqlx::query(statement)
            .execute(state.db.pool())
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version=6")
        .execute(state.db.pool())
        .await
        .unwrap();
    state.db.pool().close().await;
    let upgraded = Db::init(&dir).await.unwrap();
    let template: String =
        sqlx::query_scalar("SELECT title FROM task_series_template WHERE series_id=?1")
            .bind(&created.series_id)
            .fetch_one(upgraded.pool())
            .await
            .unwrap();
    assert_eq!(template, "稳定模板");
    let tag_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_series_tags WHERE series_id=?1 AND tag_id='upgrade-tag'",
    )
    .bind(&created.series_id)
    .fetch_one(upgraded.pool())
    .await
    .unwrap();
    assert_eq!(tag_count, 1);
    let keys_after: Vec<(String,)> = sqlx::query_as(
        "SELECT occurrence_key FROM tasks WHERE series_id=?1 ORDER BY occurrence_key",
    )
    .bind(&created.series_id)
    .fetch_all(upgraded.pool())
    .await
    .unwrap();
    assert_eq!(keys_after, keys_before);
    upgraded.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn whole_series_rule_change_removes_old_future_schedule() {
    let (state, dir) = setup("rec-whole-rule").await;
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "每日检查".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2026-10-01T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(30),
        },
    )
    .await
    .unwrap();
    let first = list_instances(&state, &created.series_id).await.unwrap()[0].clone();
    let result = edit_instance_impl(
        &state,
        first.id,
        EditScope::WholeSeries,
        InstancePatch::default(),
        Some("FREQ=WEEKLY;BYDAY=TH".into()),
        true,
    )
    .await
    .unwrap();
    assert!(result.regenerated > 0);
    let dates: Vec<(String,)> = sqlx::query_as(
        "SELECT occurrence_key FROM tasks WHERE series_id = ?1 AND deleted_at IS NULL
         AND occurrence_key >= '2026-10-01' AND occurrence_key < '2026-10-16'
         ORDER BY occurrence_key",
    )
    .bind(&created.series_id)
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert_eq!(dates.len(), 3, "旧 daily schedule 不应留在未来");
    assert!(dates[0].0.starts_with("2026-10-01T09:00"));
    assert!(dates[1].0.starts_with("2026-10-08T09:00"));
    assert!(dates[2].0.starts_with("2026-10-15T09:00"));
    // 未来请求重复物化也不能把旧 daily 规则重新带回来。
    let added = recurring_materialize_inner(
        &state,
        created.series_id.clone(),
        "2026-10-01T00:00:00.000Z".into(),
        "2026-10-16T00:00:00.000Z".into(),
    )
    .await
    .unwrap();
    assert_eq!(added, 0);
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn rule_rebuild_preserves_attachment_and_trashed_occurrence() {
    let (state, dir) = setup("rec-preserve-attachment-trash").await;
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "保留实例".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2026-10-01T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(10),
        },
    )
    .await
    .unwrap();
    let rows = list_instances(&state, &created.series_id).await.unwrap();
    let attached = &rows[1];
    let trashed = &rows[2];
    let file_name = format!("{}.pdf", uuid::Uuid::now_v7());
    let stored_path = format!("attachments/{file_name}");
    let file = state.db.data_dir().join(&stored_path);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"user attachment").unwrap();
    let now = crate::db::to_db_time(crate::db::utc_now());
    sqlx::query(
        "INSERT INTO attachments
         (id, task_id, file_name, storage_mode, stored_path, created_at)
         VALUES ('keep-copy', ?1, ?2, 'copied', ?3, ?4)",
    )
    .bind(&attached.id)
    .bind(&file_name)
    .bind(&stored_path)
    .bind(&now)
    .execute(state.db.pool())
    .await
    .unwrap();
    skip_occurrence_impl(&state, trashed.id.clone())
        .await
        .unwrap();

    edit_instance_impl(
        &state,
        rows[0].id.clone(),
        EditScope::WholeSeries,
        InstancePatch::default(),
        Some("FREQ=WEEKLY;BYDAY=TH".into()),
        true,
    )
    .await
    .unwrap();

    let retained: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE id IN (?1, ?2)")
        .bind(&attached.id)
        .bind(&trashed.id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(retained, 2, "带附件或在回收站的实例不得因改规则而硬删除");
    let attachment_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM attachments WHERE id = 'keep-copy'")
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(attachment_count, 1);
    crate::attachments::cleanup_orphans_impl(&state.db)
        .await
        .unwrap();
    assert!(
        file.exists(),
        "仍被实例引用的 copied attachment 不得被孤儿清理删除"
    );
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn rule_rebuild_preserves_child_data_and_reverted_instance_state() {
    let (state, dir) = setup("rec-preserve-child-state").await;
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "保留用户数据".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2026-10-01T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(10),
        },
    )
    .await
    .unwrap();
    let rows = list_instances(&state, &created.series_id).await.unwrap();
    let now = crate::db::to_db_time(crate::db::utc_now());
    sqlx::query(
        "INSERT INTO subtasks (id, task_id, title, created_at, updated_at)
         VALUES ('preserve-subtask', ?1, '需要完成', ?2, ?2)",
    )
    .bind(&rows[1].id)
    .bind(&now)
    .execute(state.db.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO reminders (id, task_id, kind, remind_at, created_at, updated_at)
         VALUES ('preserve-reminder', ?1, 'custom', ?2, ?3, ?3)",
    )
    .bind(&rows[2].id)
    .bind("2026-10-03T08:00:00.000Z")
    .bind(&now)
    .execute(state.db.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO task_dependencies (task_id, depends_on_id, created_at)
         VALUES (?1, ?2, ?3)",
    )
    .bind(&rows[3].id)
    .bind(&rows[4].id)
    .bind(&now)
    .execute(state.db.pool())
    .await
    .unwrap();
    // Even if the visible status is later reverted, this occurrence was used.
    for status in ["doing", "todo"] {
        sqlx::query("UPDATE tasks SET status = ?1 WHERE id = ?2")
            .bind(status)
            .bind(&rows[5].id)
            .execute(state.db.pool())
            .await
            .unwrap();
    }

    edit_instance_impl(
        &state,
        rows[0].id.clone(),
        EditScope::WholeSeries,
        InstancePatch::default(),
        Some("FREQ=WEEKLY;BYDAY=TH".into()),
        true,
    )
    .await
    .unwrap();
    for row in &rows[1..=5] {
        let retained: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE id = ?1")
            .bind(&row.id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
        assert_eq!(retained, 1, "已使用的实例 {} 不得硬删", row.id);
    }
    for table in ["subtasks", "reminders", "task_dependencies"] {
        let query = format!("SELECT COUNT(*) FROM {table}");
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(query))
            .fetch_one(state.db.pool())
            .await
            .unwrap();
        assert_eq!(count, 1, "关联数据 {table} 不得丢失");
    }
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn delete_from_occurrence_prevents_future_materialization() {
    let (state, dir) = setup("rec-terminate-from").await;
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "停止未来".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2026-10-01T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(5),
        },
    )
    .await
    .unwrap();
    let rows = list_instances(&state, &created.series_id).await.unwrap();
    delete_recurring_impl(&state, rows[1].id.clone(), DeleteMode::ThisAndFuture, true)
        .await
        .unwrap();
    let added = recurring_materialize_inner(
        &state,
        created.series_id.clone(),
        "2026-11-01T00:00:00.000Z".into(),
        "2026-11-15T00:00:00.000Z".into(),
    )
    .await
    .unwrap();
    assert_eq!(added, 0, "停止系列后浏览未来日历不得重新生成实例");
    let visible: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE series_id = ?1 AND deleted_at IS NULL",
    )
    .bind(&created.series_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(visible, 1);
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn count_and_until_bound_materialization_across_dst() {
    let (state, dir) = setup("rec-end-dst").await;
    for (rrule, expected) in [
        ("FREQ=DAILY;COUNT=3", 3usize),
        ("FREQ=DAILY;UNTIL=20260308", 2usize),
    ] {
        let created = create_recurring_impl(
            &state,
            CreateRecurringInput {
                title: rrule.into(),
                description: None,
                priority: None,
                project_id: None,
                category_id: None,
                estimated_minutes: None,
                tag_ids: vec![],
                rrule: rrule.into(),
                tzid: Some("America/New_York".into()),
                dtstart_local: "2026-03-07T09:00:00".into(),
                has_start_time: Some(true),
                due_local: None,
                materialize_days: Some(20),
            },
        )
        .await
        .unwrap();
        let rows = list_instances(&state, &created.series_id).await.unwrap();
        assert_eq!(rows.len(), expected, "{rrule}");
        let later = recurring_materialize_inner(
            &state,
            created.series_id,
            "2026-04-01T00:00:00.000Z".into(),
            "2026-04-20T00:00:00.000Z".into(),
        )
        .await
        .unwrap();
        assert_eq!(later, 0, "{rrule} 的结束条件不得在未来失效");
        if rrule.contains("COUNT") {
            assert!(rows[0].occurrence_key.as_deref().unwrap().contains("14:00"));
            assert!(rows[1].occurrence_key.as_deref().unwrap().contains("13:00"));
        }
    }
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn timezone_change_keeps_history_and_uses_new_dst_offset() {
    let (state, dir) = setup("rec-change-timezone").await;
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "跨时区".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2027-03-08T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(1),
        },
    )
    .await
    .unwrap();
    let range_start = "2027-03-08T00:00:00.000Z".to_string();
    let range_end = "2027-03-18T00:00:00.000Z".to_string();
    recurring_materialize_inner(
        &state,
        created.series_id.clone(),
        range_start.clone(),
        range_end.clone(),
    )
    .await
    .unwrap();
    let old = find_by_occurrence(&state, &created.series_id, "2027-03-08T09:00:00.000Z")
        .await
        .unwrap();
    let target = find_by_occurrence(&state, &created.series_id, "2027-03-10T09:00:00.000Z")
        .await
        .unwrap();
    edit_instance_impl(
        &state,
        target.id,
        EditScope::ThisAndFuture,
        InstancePatch {
            tzid: Some("America/New_York".into()),
            ..Default::default()
        },
        Some("FREQ=DAILY".into()),
        false,
    )
    .await
    .unwrap();
    recurring_materialize_inner(&state, created.series_id.clone(), range_start, range_end)
        .await
        .unwrap();
    let old_after = find_by_occurrence(&state, &created.series_id, "2027-03-08T09:00:00.000Z")
        .await
        .unwrap();
    assert_eq!(old.id, old_after.id, "旧实例身份不能被新时区重解释");
    let rows = list_instances(&state, &created.series_id).await.unwrap();
    assert!(rows
        .iter()
        .any(|t| t.occurrence_key.as_deref() == Some("2027-03-11T14:00:00.000Z")));
    assert!(rows
        .iter()
        .any(|t| t.occurrence_key.as_deref() == Some("2027-03-14T13:00:00.000Z")));
    assert!(!rows
        .iter()
        .any(|t| t.occurrence_key.as_deref() == Some("2027-03-11T09:00:00.000Z")));
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn newer_whole_series_rule_supersedes_old_future_segment() {
    let (state, dir) = setup("rec-segment-supersede").await;
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "分段覆盖".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2027-01-01T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(1),
        },
    )
    .await
    .unwrap();
    let sid = &created.series_id;
    recurring_materialize_inner(
        &state,
        sid.clone(),
        "2027-09-01T00:00:00.000Z".into(),
        "2027-10-20T00:00:00.000Z".into(),
    )
    .await
    .unwrap();
    let future = find_by_occurrence(&state, sid, "2027-10-01T09:00:00.000Z")
        .await
        .unwrap();
    edit_instance_impl(
        &state,
        future.id,
        EditScope::ThisAndFuture,
        InstancePatch::default(),
        Some("FREQ=WEEKLY".into()),
        false,
    )
    .await
    .unwrap();
    let first = find_by_occurrence(&state, sid, "2027-01-01T09:00:00.000Z")
        .await
        .unwrap();
    edit_instance_impl(
        &state,
        first.id,
        EditScope::WholeSeries,
        InstancePatch::default(),
        Some("FREQ=MONTHLY".into()),
        false,
    )
    .await
    .unwrap();
    recurring_materialize_inner(
        &state,
        sid.clone(),
        "2027-10-01T00:00:00.000Z".into(),
        "2027-10-20T00:00:00.000Z".into(),
    )
    .await
    .unwrap();
    assert!(find_by_occurrence(&state, sid, "2027-10-01T09:00:00.000Z")
        .await
        .is_some());
    assert!(
        find_by_occurrence(&state, sid, "2027-10-08T09:00:00.000Z")
            .await
            .is_none(),
        "较旧的未来每周分段不得在整系列月规则之后复活"
    );
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn tag_merge_updates_future_series_template() {
    use crate::organize::{merge_tag_impl, MergeInput};
    let (state, dir) = setup("rec-tag-merge").await;
    let now = crate::db::to_db_time(crate::db::utc_now());
    for id in ["old-tag", "new-tag"] {
        sqlx::query(
            "INSERT INTO tags (id, name, sort_order, created_at, updated_at)
            VALUES (?1, ?1, 0, ?2, ?2)",
        )
        .bind(id)
        .bind(&now)
        .execute(state.db.pool())
        .await
        .unwrap();
    }
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "标签合并".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec!["old-tag".into()],
            rrule: "FREQ=WEEKLY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2027-01-01T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(1),
        },
    )
    .await
    .unwrap();
    merge_tag_impl(
        &state.db,
        &MergeInput {
            source_ids: vec!["old-tag".into()],
            target_id: "new-tag".into(),
        },
    )
    .await
    .unwrap();
    recurring_materialize_inner(
        &state,
        created.series_id.clone(),
        "2027-02-01T00:00:00.000Z".into(),
        "2027-02-28T00:00:00.000Z".into(),
    )
    .await
    .unwrap();
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT tt.tag_id FROM task_tags tt
        JOIN tasks t ON t.id = tt.task_id WHERE t.series_id = ?1",
    )
    .bind(&created.series_id)
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert_eq!(ids, vec!["new-tag"]);
    let canonical: Vec<String> =
        sqlx::query_scalar("SELECT tag_id FROM task_series_tags WHERE series_id = ?1")
            .bind(&created.series_id)
            .fetch_all(state.db.pool())
            .await
            .unwrap();
    assert_eq!(canonical, vec!["new-tag"]);
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn project_merge_updates_future_series_template() {
    use crate::organize::{merge_project_impl, MergeInput};
    let (state, dir) = setup("rec-project-merge").await;
    let now = crate::db::to_db_time(crate::db::utc_now());
    for id in ["old-project", "new-project"] {
        sqlx::query(
            "INSERT INTO projects (id, name, sort_order, created_at, updated_at)
            VALUES (?1, ?1, 0, ?2, ?2)",
        )
        .bind(id)
        .bind(&now)
        .execute(state.db.pool())
        .await
        .unwrap();
    }
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "项目合并".into(),
            description: None,
            priority: None,
            project_id: Some("old-project".into()),
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=WEEKLY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2027-01-01T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(1),
        },
    )
    .await
    .unwrap();
    merge_project_impl(
        &state.db,
        &MergeInput {
            source_ids: vec!["old-project".into()],
            target_id: "new-project".into(),
        },
    )
    .await
    .unwrap();
    recurring_materialize_inner(
        &state,
        created.series_id.clone(),
        "2027-02-01T00:00:00.000Z".into(),
        "2027-02-28T00:00:00.000Z".into(),
    )
    .await
    .unwrap();
    let project_ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT project_id FROM tasks
        WHERE series_id = ?1 AND deleted_at IS NULL",
    )
    .bind(&created.series_id)
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert_eq!(project_ids, vec!["new-project"]);
    let canonical: String =
        sqlx::query_scalar("SELECT project_id FROM task_series_template WHERE series_id = ?1")
            .bind(&created.series_id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(canonical, "new-project");
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn project_cascade_delete_stops_future_series_materialization() {
    use crate::organize::{project_delete_impl, OrphanStrategy};
    let (state, dir) = setup("rec-project-delete").await;
    let now = crate::db::to_db_time(crate::db::utc_now());
    sqlx::query(
        "INSERT INTO projects (id, name, sort_order, created_at, updated_at)
        VALUES ('delete-project', 'delete-project', 0, ?1, ?1)",
    )
    .bind(&now)
    .execute(state.db.pool())
    .await
    .unwrap();
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "删除项目".into(),
            description: None,
            priority: None,
            project_id: Some("delete-project".into()),
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2027-01-01T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(1),
        },
    )
    .await
    .unwrap();
    project_delete_impl(
        &state.db,
        "delete-project",
        OrphanStrategy::CascadeSoftDelete,
    )
    .await
    .unwrap();
    let later = recurring_materialize_inner(
        &state,
        created.series_id.clone(),
        "2027-02-01T00:00:00.000Z".into(),
        "2027-02-28T00:00:00.000Z".into(),
    )
    .await
    .unwrap();
    assert_eq!(later, 0);
    let cutoff: Option<String> =
        sqlx::query_scalar("SELECT terminated_from_occurrence_key FROM task_series WHERE id = ?1")
            .bind(&created.series_id)
            .fetch_one(state.db.pool())
            .await
            .unwrap();
    assert_eq!(cutoff.as_deref(), Some("1970-01-01T00:00:00.000Z"));
    let live: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks
        WHERE series_id = ?1 AND deleted_at IS NULL",
    )
    .bind(&created.series_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(live, 0);
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn concurrent_materialization_is_idempotent() {
    let (state, dir) = setup("rec-concurrent").await;
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "并发物化".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2026-10-01T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(1),
        },
    )
    .await
    .unwrap();
    let from = "2026-11-01T00:00:00.000Z".to_string();
    let to = "2026-11-11T00:00:00.000Z".to_string();
    let (a, b) = tokio::join!(
        recurring_materialize_inner(&state, created.series_id.clone(), from.clone(), to.clone()),
        recurring_materialize_inner(&state, created.series_id.clone(), from, to)
    );
    assert!(a.is_ok(), "first concurrent request: {a:?}");
    assert!(b.is_ok(), "second concurrent request: {b:?}");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE series_id=?1 AND occurrence_key >= '2026-11-01'
         AND occurrence_key < '2026-11-11'",
    )
    .bind(&created.series_id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(count, 10);
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn restoring_skipped_occurrence_clears_skip_marker() {
    let (state, dir) = setup("rec-restore-skip").await;
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "可恢复的发生".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2026-10-01T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(3),
        },
    )
    .await
    .unwrap();
    let first = list_instances(&state, &created.series_id).await.unwrap()[0].clone();
    skip_occurrence_impl(&state, first.id.clone())
        .await
        .unwrap();
    let restored = crate::commands::restore_task_impl(&state.db, &first.id)
        .await
        .unwrap();
    assert!(restored.deleted_at.is_none());
    let skipped: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_series_skips WHERE series_id=?1 AND occurrence_key=?2",
    )
    .bind(&created.series_id)
    .bind(first.occurrence_key.unwrap())
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(skipped, 0);
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn new_segment_never_leaks_across_anchor() {
    let (state, dir) = setup("rec-segment-boundary").await;
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "周一安排".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=WEEKLY;BYDAY=MO".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2026-09-21T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(60),
        },
    )
    .await
    .unwrap();
    let before = list_instances(&state, &created.series_id).await.unwrap();
    let anchor = before
        .iter()
        .find(|t| t.occurrence_key.as_deref() == Some("2026-10-05T09:00:00.000Z"))
        .unwrap();
    edit_instance_impl(
        &state,
        anchor.id.clone(),
        EditScope::ThisAndFuture,
        InstancePatch::default(),
        Some("FREQ=WEEKLY;BYDAY=WE".into()),
        true,
    )
    .await
    .unwrap();
    let after = list_instances(&state, &created.series_id).await.unwrap();
    assert!(after
        .iter()
        .any(|t| t.occurrence_key.as_deref() == Some("2026-09-28T09:00:00.000Z")));
    assert!(!after
        .iter()
        .any(|t| t.occurrence_key.as_deref() == Some("2026-09-30T09:00:00.000Z")));
    assert!(!after
        .iter()
        .any(|t| t.occurrence_key.as_deref() == Some("2026-10-12T09:00:00.000Z")));
    assert!(after
        .iter()
        .any(|t| t.occurrence_key.as_deref() == Some("2026-10-07T09:00:00.000Z")));
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn whole_series_title_survives_prior_segment_and_late_materialization() {
    let (state, dir) = setup("rec-whole-title").await;
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "原标题".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: None,
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2026-10-01T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(12),
        },
    )
    .await
    .unwrap();
    let rows = list_instances(&state, &created.series_id).await.unwrap();
    edit_instance_impl(
        &state,
        rows[2].id.clone(),
        EditScope::ThisAndFuture,
        InstancePatch {
            title: Some("分段标题".into()),
            ..Default::default()
        },
        None,
        true,
    )
    .await
    .unwrap();
    let selected = list_instances(&state, &created.series_id).await.unwrap()[3].clone();
    edit_instance_impl(
        &state,
        selected.id,
        EditScope::WholeSeries,
        InstancePatch {
            title: Some("系列新标题".into()),
            ..Default::default()
        },
        None,
        true,
    )
    .await
    .unwrap();
    recurring_materialize_inner(
        &state,
        created.series_id.clone(),
        "2027-04-01T00:00:00.000Z".into(),
        "2027-04-04T00:00:00.000Z".into(),
    )
    .await
    .unwrap();
    let later: Vec<(String,)> = sqlx::query_as(
        "SELECT title FROM tasks WHERE series_id = ?1 AND occurrence_key >= '2027-04-01'
         AND occurrence_key < '2027-04-04'",
    )
    .bind(&created.series_id)
    .fetch_all(state.db.pool())
    .await
    .unwrap();
    assert_eq!(later.len(), 3);
    assert!(later.iter().all(|row| row.0 == "系列新标题"));
    state.db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
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

    // 方案 A：系列内容只同步普通 generated 实例；用户单独改过的
    // occurrence 作为整体例外保留，即使这次只改了标题也不覆盖优先级。
    let unfinished: Vec<Task> = list_instances(&state, &series_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|t| t.status != "done")
        .collect();
    for t in &unfinished {
        if t.occurrence_kind.as_deref() == Some("exception") {
            assert_eq!(t.priority, 1, "单次例外「{}」应保留原优先级", t.title);
        } else {
            assert_eq!(t.priority, 3, "普通实例「{}」的优先级应同步为 3", t.title);
        }
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

    let whole_err = edit_instance_impl(
        &state,
        last.id.clone(),
        EditScope::WholeSeries,
        InstancePatch {
            priority: Some(2),
            ..Default::default()
        },
        None,
        false,
    )
    .await
    .expect_err("整个系列也必须在后端强制历史确认");
    assert!(whole_err.message.contains("历史"));

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
    assert_eq!(info2["recurrenceEndKind"], "never");
    assert_eq!(
        info2["openFromHere"].as_i64().unwrap(),
        all.len() as i64 - 2,
        "此次及以后的数量必须按 occurrence cutoff 精确计算"
    );
    // 三种范围都要有可读说明（§5 要求"不适用的选项要禁用并说明原因"）
    assert!(info2["notes"]["thisOnly"].as_str().is_some());
    assert!(info2["notes"]["thisAndFuture"].as_str().is_some());
    assert!(info2["notes"]["thisAndFuture"]
        .as_str()
        .unwrap()
        .contains("当前已生成"));
    assert!(info2["notes"]["wholeSeries"].as_str().is_some());

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn this_only_content_and_tags_commit_atomically_without_changing_siblings() {
    let (state, dir) = setup("rec-this-only-content").await;
    let now = crate::db::to_db_time(crate::db::utc_now());
    sqlx::query(
        "INSERT INTO tags (id, name, sort_order, created_at, updated_at)
         VALUES ('one-off-tag', '单次标签', 0, ?1, ?1)",
    )
    .bind(&now)
    .execute(state.db.pool())
    .await
    .unwrap();
    let created = create_recurring_impl(
        &state,
        CreateRecurringInput {
            title: "系列原题".into(),
            description: None,
            priority: None,
            project_id: None,
            category_id: None,
            estimated_minutes: Some(30),
            tag_ids: vec![],
            rrule: "FREQ=DAILY".into(),
            tzid: Some("UTC".into()),
            dtstart_local: "2026-09-24T09:00:00".into(),
            has_start_time: Some(true),
            due_local: None,
            materialize_days: Some(2),
        },
    )
    .await
    .unwrap();
    let rows = list_instances(&state, &created.series_id).await.unwrap();
    assert!(rows.len() >= 2);
    let first = &rows[0];
    let sibling = &rows[1];

    let broad = edit_instance_impl(
        &state,
        first.id.clone(),
        EditScope::ThisAndFuture,
        InstancePatch {
            note_md: Some("不允许悄悄丢失的分段备注".into()),
            ..Default::default()
        },
        None,
        true,
    )
    .await;
    assert!(broad.is_err(), "缺少分段模型的字段不能接受未来范围编辑");

    let invalid = edit_instance_impl(
        &state,
        first.id.clone(),
        EditScope::ThisOnly,
        InstancePatch {
            note_md: Some("不能部分保存".into()),
            tag_ids: Some(vec!["missing-tag".into()]),
            ..Default::default()
        },
        None,
        false,
    )
    .await;
    assert!(invalid.is_err(), "无效标签必须回滚整个编辑");
    let after_failure: Task = sqlx::query_as("SELECT * FROM tasks WHERE id = ?1")
        .bind(&first.id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(after_failure.note_md, first.note_md);
    assert_eq!(after_failure.occurrence_kind, first.occurrence_kind);

    edit_instance_impl(
        &state,
        first.id.clone(),
        EditScope::ThisOnly,
        InstancePatch {
            note_md: Some("仅这次的备注".into()),
            link_url: Some("https://example.com/one".into()),
            period_type: Some("week".into()),
            clear_estimated_minutes: true,
            tag_ids: Some(vec!["one-off-tag".into()]),
            ..Default::default()
        },
        None,
        false,
    )
    .await
    .unwrap();
    let edited: Task = sqlx::query_as("SELECT * FROM tasks WHERE id = ?1")
        .bind(&first.id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(edited.note_md, "仅这次的备注");
    assert_eq!(edited.link_url.as_deref(), Some("https://example.com/one"));
    assert_eq!(edited.period_type, "week");
    assert_eq!(edited.estimated_minutes, None);
    assert_eq!(edited.occurrence_kind.as_deref(), Some("exception"));
    let tag_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_tags WHERE task_id = ?1 AND tag_id = 'one-off-tag'",
    )
    .bind(&first.id)
    .fetch_one(state.db.pool())
    .await
    .unwrap();
    assert_eq!(tag_count, 1);
    let unchanged: Task = sqlx::query_as("SELECT * FROM tasks WHERE id = ?1")
        .bind(&sibling.id)
        .fetch_one(state.db.pool())
        .await
        .unwrap();
    assert_eq!(unchanged.note_md, sibling.note_md);
    assert_eq!(unchanged.estimated_minutes, sibling.estimated_minutes);
    let _ = std::fs::remove_dir_all(&dir);
}
