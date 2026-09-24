//! 本轮新增能力的端到端集成测试：复制任务、拖拽排序、组织项合并。
//!
//! ## 为什么用 `*_impl` 而不是 `#[tauri::command]`
//!
//! 命令函数的第一个参数是 `tauri::State`，它只能由 Tauri 运行时构造。
//! 因此本轮把这些命令的业务主体抽成 `*_impl(&Db, ...)`，命令本身只剩
//! 一行转发——测试直接调 `*_impl`，验的仍是真正跑在生产路径上的代码。
//!
//! 覆盖点刻意选择"容易写错又不容易被发现"的语义：
//! - 副本必须是**未完成**、且不复制附件（附件原件是共享的）；
//! - 连续复制要产生「（副本）」「（副本 2）」这样的可区分名字；
//! - 排序用中点插入法，只动一行，其它任务的相对顺序不能变；
//! - 合并要转移任务、软删除源项，并且**回收站里的任务不受影响**。

use crate::commands::{
    create_task_impl, duplicate_task_impl, list_tasks_impl, reorder_task_impl, update_task_impl,
    AppState, ReorderInput,
};
use crate::db::Db;
use crate::models::{CreateTaskInput, TaskQuery, UpdateTaskInput};
use crate::organize::{
    merge_category_impl, merge_project_impl, merge_tag_impl, MergeInput,
};
use sqlx::Row;

/// 建临时库；调用方负责删除目录
async fn setup(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("lumen-{name}-{}", uuid::Uuid::now_v7()));
    let db = Db::init(&dir).await.expect("初始化数据库");
    (AppState::new(db), dir)
}

fn task(title: &str) -> CreateTaskInput {
    CreateTaskInput {
        title: title.to_string(),
        description: None,
        note_md: None,
        link_url: None,
        priority: None,
        project_id: None,
        category_id: None,
        planned_at: None,
        has_planned_time: None,
        due_at: None,
        has_due_time: None,
        estimated_minutes: None,
        status: "todo".to_string(),
        is_pinned: None,
        is_favorite: None,
        period_type: None,
        tag_ids: vec![],
    }
}

/// 直接插一个组织项（项目 / 分类 / 标签），返回 id
async fn insert_org(db: &Db, table: &str, name: &str) -> String {
    let id = uuid::Uuid::now_v7().to_string();
    let now = crate::db::to_db_time(crate::db::utc_now());
    let sql = match table {
        "projects" => {
            "INSERT INTO projects (id, name, sort_order, is_favorite, is_archived, created_at, updated_at)
             VALUES (?1, ?2, 0, 0, 0, ?3, ?3)"
        }
        "categories" => {
            "INSERT INTO categories (id, name, sort_order, created_at, updated_at)
             VALUES (?1, ?2, 0, ?3, ?3)"
        }
        "tags" => {
            "INSERT INTO tags (id, name, sort_order, created_at, updated_at)
             VALUES (?1, ?2, 0, ?3, ?3)"
        }
        other => panic!("未支持的表：{other}"),
    };
    sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
        .bind(&id)
        .bind(name)
        .bind(&now)
        .execute(db.pool())
        .await
        .expect("插入组织项");
    id
}

/// 按 sort_order 列出任务标题（排序断言用）
async fn titles_in_order(db: &Db) -> Vec<String> {
    let rows =
        sqlx::query("SELECT title FROM tasks WHERE deleted_at IS NULL ORDER BY sort_order ASC")
            .fetch_all(db.pool())
            .await
            .expect("查询顺序");
    rows.iter()
        .map(|r| r.try_get::<String, _>("title").expect("读取标题"))
        .collect()
}

/// 取某任务的某个列（用于断言归属是否真的改了）
async fn col_str(db: &Db, id: &str, column: &str) -> Option<String> {    let sql = format!("SELECT {column} AS v FROM tasks WHERE id = ?1");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(db.pool())
        .await
        .expect("查询任务");
    row.try_get::<Option<String>, _>("v").expect("读取列")
}

// =============================================================================
// 部分更新（这一组是"本该早就存在"的测试）
// =============================================================================
//
// 背景：`QueryBuilder::separated()` 的 `Separated::push()` **本身就会写入
// 分隔符**，因此 `sep.push("title = ").push_bind(v)` 会生成
// `title = , ?1`——SQL 语法错误。这个写法在三个文件里出现过 52 次，
// 意味着「编辑任务 / 编辑项目 / 编辑分类 / 编辑标签 / 编辑子任务」
// **全都保存不了**。之前的单元测试只测了校验函数，没有真的执行 UPDATE，
// 所以一直没暴露；是实机验收（在悬浮窗里改任务标题）才把它抓出来。
//
// 下面这些用例直接跑真实的 UPDATE，任何人再把 `push_bind` 写回去都会红。

#[tokio::test]
async fn update_single_field_generates_valid_sql() {
    let (state, dir) = setup("update-single").await;
    let db = &state.db;

    let t = create_task_impl(db, task("原标题")).await.unwrap();
    let updated = update_task_impl(
        db,
        &t.id,
        UpdateTaskInput {
            title: Some("新标题".into()),
            ..Default::default()
        },
    )
    .await
    .expect("只改标题也必须能保存");

    assert_eq!(updated.title, "新标题");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn update_many_fields_generates_valid_sql() {
    let (state, dir) = setup("update-many").await;
    let db = &state.db;

    let project = insert_org(db, "projects", "工作").await;
    let category = insert_org(db, "categories", "事务").await;
    let t = create_task_impl(db, task("多字段更新")).await.unwrap();

    let updated = update_task_impl(
        db,
        &t.id,
        UpdateTaskInput {
            title: Some("多字段更新（已改）".into()),
            description: Some("补充说明".into()),
            note_md: Some("## 备注".into()),
            status: Some("doing".into()),
            priority: Some(2),
            project_id: Some(project.clone()),
            category_id: Some(category.clone()),
            planned_at: Some("2026-10-01T01:00:00.000Z".into()),
            has_planned_time: Some(true),
            due_at: Some("2026-10-02T09:00:00.000Z".into()),
            has_due_time: Some(true),
            estimated_minutes: Some(45),
            is_pinned: Some(true),
            is_favorite: Some(true),
            period_type: Some("week".into()),
            ..Default::default()
        },
    )
    .await
    .expect("多字段一起更新必须能保存");

    assert_eq!(updated.title, "多字段更新（已改）");
    assert_eq!(updated.status, "doing");
    assert_eq!(updated.priority, 2);
    assert_eq!(updated.project_id.as_deref(), Some(project.as_str()));
    assert_eq!(updated.category_id.as_deref(), Some(category.as_str()));
    assert_eq!(updated.planned_at.as_deref(), Some("2026-10-01T01:00:00.000Z"));
    assert_eq!(updated.has_planned_time, 1);
    assert_eq!(updated.due_at.as_deref(), Some("2026-10-02T09:00:00.000Z"));
    assert_eq!(updated.estimated_minutes, Some(45));
    assert_eq!(updated.is_pinned, 1);
    assert_eq!(updated.is_favorite, 1);
    assert_eq!(updated.period_type, "week");

    // 清空语义也要真的生效
    let cleared = update_task_impl(
        db,
        &t.id,
        UpdateTaskInput {
            clear_planned_at: true,
            clear_project: true,
            ..Default::default()
        },
    )
    .await
    .expect("清空字段必须能保存");
    assert!(cleared.planned_at.is_none(), "计划时间应被清空");
    assert_eq!(cleared.has_planned_time, 0);
    assert!(cleared.project_id.is_none(), "项目应被清空");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn update_completion_time_is_recorded_and_cleared() {
    let (state, dir) = setup("update-complete").await;
    let db = &state.db;

    let t = create_task_impl(db, task("完成与撤销")).await.unwrap();
    let done = update_task_impl(
        db,
        &t.id,
        UpdateTaskInput {
            status: Some("done".into()),
            ..Default::default()
        },
    )
    .await
    .expect("标记完成必须能保存");
    assert_eq!(done.status, "done");
    assert!(done.completed_at.is_some(), "完成时间必须真实写入");

    let undone = update_task_impl(
        db,
        &t.id,
        UpdateTaskInput {
            status: Some("todo".into()),
            ..Default::default()
        },
    )
    .await
    .expect("撤销完成必须能保存");
    assert!(undone.completed_at.is_none(), "撤销完成后应清空完成时间");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn org_updates_generate_valid_sql() {
    // 项目 / 分类 / 标签的重命名走的是同一套 Separated 写法，
    // 一起纳入回归，避免只修任务那一处。
    let (state, dir) = setup("update-org").await;
    let db = &state.db;

    let p = insert_org(db, "projects", "旧项目").await;
    let c = insert_org(db, "categories", "旧分类").await;
    let g = insert_org(db, "tags", "旧标签").await;

    crate::organize::update_project_impl(
        db,
        &p,
        crate::organize::UpdateProjectInput {
            name: Some("新项目".into()),
            description: Some("说明".into()),
            color: Some("#4f46e5".into()),
            icon: None,
            sort_order: Some(5),
            is_favorite: Some(true),
            is_archived: Some(true),
        },
    )
    .await
    .expect("更新项目必须能保存");

    crate::organize::update_category_impl(
        db,
        &c,
        crate::organize::UpdateCategoryInput {
            name: Some("新分类".into()),
            description: Some("说明".into()),
            color: Some("#22c55e".into()),
            icon: None,
            sort_order: Some(3),
        },
    )
    .await
    .expect("更新分类必须能保存");

    crate::organize::update_tag_impl(
        db,
        &g,
        crate::organize::UpdateTagInput {
            name: Some("新标签".into()),
            color: Some("#ef4444".into()),
            sort_order: Some(2),
        },
    )
    .await
    .expect("更新标签必须能保存");

    let name: String = sqlx::query("SELECT name FROM projects WHERE id = ?1")
        .bind(&p)
        .fetch_one(db.pool())
        .await
        .unwrap()
        .try_get("name")
        .unwrap();
    assert_eq!(name, "新项目");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn subtask_update_generates_valid_sql() {
    let (state, dir) = setup("update-subtask").await;
    let db = &state.db;

    let t = create_task_impl(db, task("带子任务")).await.unwrap();
    let sub = crate::subtasks::create_subtask_impl(db, &t.id, "第一步")
        .await
        .expect("创建子任务");

    let done = crate::subtasks::update_subtask_impl(
        db,
        &sub.id,
        crate::subtasks::SubtaskPatch {
            title: Some("第一步（已改）".into()),
            is_done: Some(true),
            sort_order: Some(2.0),
        },
    )
    .await
    .expect("更新子任务必须能保存");
    assert_eq!(done.title, "第一步（已改）");
    assert_eq!(done.is_done, 1);
    assert!(done.completed_at.is_some(), "勾选子任务要写入完成时间");

    let undone = crate::subtasks::update_subtask_impl(
        db,
        &sub.id,
        crate::subtasks::SubtaskPatch {
            title: None,
            is_done: Some(false),
            sort_order: None,
        },
    )
    .await
    .expect("取消勾选必须能保存");
    assert_eq!(undone.is_done, 0);
    assert!(undone.completed_at.is_none());

    let _ = std::fs::remove_dir_all(dir);
}

// =============================================================================
// 收件箱筛选语义（整改任务书 §2）
// =============================================================================
//
// 曾经的 Bug：前端用 `projectId = null` 表达"没有项目的任务"，但 null 过 IPC
// 会变成 Rust 的 `None`，与"不限制项目"完全一样，于是收件箱退化成"所有未完成
// 任务"。现在用显式的 `without_project` 区分三种语义；下面按任务书 §2.4 的
// 三条任务（A/B/C）逐一验收。

/// 按查询条件取任务标题（排序后，便于断言）
async fn titles_of(db: &Db, query: TaskQuery) -> Vec<String> {
    let mut v: Vec<String> = list_tasks_impl(db, query)
        .await
        .expect("查询任务")
        .into_iter()
        .map(|t| t.title)
        .collect();
    v.sort();
    v
}

#[tokio::test]
async fn inbox_shows_only_tasks_without_project() {
    let (state, dir) = setup("inbox-semantics").await;
    let db = &state.db;
    let project = insert_org(db, "projects", "P1").await;

    // A：未归属项目、todo
    create_task_impl(db, task("A-无项目待办")).await.unwrap();
    // B：归属项目 P1、todo
    create_task_impl(
        db,
        CreateTaskInput {
            title: "B-有项目待办".into(),
            project_id: Some(project.clone()),
            ..task("")
        },
    )
    .await
    .unwrap();
    // C：未归属项目、已完成
    let c = create_task_impl(db, task("C-无项目已完成")).await.unwrap();
    sqlx::query("UPDATE tasks SET status = 'done', completed_at = '2026-09-23T02:00:00.000Z' WHERE id = ?1")
        .bind(&c.id)
        .execute(db.pool())
        .await
        .unwrap();

    // 收件箱：只有 A
    let inbox = titles_of(
        db,
        TaskQuery {
            without_project: true,
            statuses: vec!["todo".into(), "doing".into(), "waiting".into()],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        inbox,
        vec!["A-无项目待办"],
        "收件箱应只含未归属项目的未完成任务"
    );

    // 「全部任务」仍要能看到 B
    let all = titles_of(
        db,
        TaskQuery {
            statuses: vec!["todo".into(), "doing".into(), "waiting".into(), "done".into()],
            ..Default::default()
        },
    )
    .await;
    assert!(all.contains(&"B-有项目待办".to_string()), "全部任务应包含 B");
    assert_eq!(all.len(), 3, "全部任务应有 3 条，实际：{all:?}");

    // 按指定项目筛选不受 without_project 影响
    let only_p1 = titles_of(
        db,
        TaskQuery {
            project_id: Some(project.clone()),
            statuses: vec!["todo".into()],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(only_p1, vec!["B-有项目待办"]);

    // 同时传两个条件时行为必须确定：without_project 优先
    let both = titles_of(
        db,
        TaskQuery {
            without_project: true,
            project_id: Some(project),
            statuses: vec!["todo".into()],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        both,
        vec!["A-无项目待办"],
        "without_project 应优先于 project_id"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn none_project_id_alone_means_unlimited() {
    // 反向保护：不传 without_project 时，project_id 为 None 表示"不限项目"，
    // 不能被误当成"只要没有项目的"。这条测试锁住"null ≠ 空项目"这个语义。
    let (state, dir) = setup("inbox-null").await;
    let db = &state.db;
    let project = insert_org(db, "projects", "P1").await;
    create_task_impl(db, task("无项目")).await.unwrap();
    create_task_impl(
        db,
        CreateTaskInput {
            title: "有项目".into(),
            project_id: Some(project),
            ..task("")
        },
    )
    .await
    .unwrap();

    let unlimited = titles_of(
        db,
        TaskQuery {
            project_id: None,
            statuses: vec!["todo".into()],
            ..Default::default()
        },
    )
    .await;
    assert_eq!(unlimited.len(), 2, "不传条件时应返回全部，实际：{unlimited:?}");

    let _ = std::fs::remove_dir_all(dir);
}

// =============================================================================
// 复制任务
// =============================================================================

#[tokio::test]
async fn duplicate_resets_status_and_copies_relations() {
    let (state, dir) = setup("dup-basic").await;
    let db = &state.db;

    let tag = insert_org(db, "tags", "紧急").await;
    let project = insert_org(db, "projects", "工作").await;

    let src = create_task_impl(
        db,
        CreateTaskInput {
            title: "写季度总结".into(),
            description: Some("含数据部分".into()),
            priority: Some(3),
            project_id: Some(project.clone()),
            tag_ids: vec![tag.clone()],
            estimated_minutes: Some(90),
            ..task("")
        },
    )
    .await
    .expect("创建源任务");

    // 源任务先完成，并加两个子任务（其中一个已完成）
    sqlx::query("UPDATE tasks SET status = 'done', completed_at = '2026-09-23T02:00:00.000Z' WHERE id = ?1")
        .bind(&src.id)
        .execute(db.pool())
        .await
        .unwrap();
    for (t, done) in [("收集数据", 1), ("画图", 0)] {
        sqlx::query(
            "INSERT INTO subtasks (id, task_id, title, is_done, sort_order, completed_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 0, NULL, '2026-09-23T02:00:00.000Z', '2026-09-23T02:00:00.000Z')",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(&src.id)
        .bind(t)
        .bind(done)
        .execute(db.pool())
        .await
        .unwrap();
    }
    // 一条已触发过的提醒：副本必须把 fired_at 清空，否则副本的提醒永远不会响
    sqlx::query(
        "INSERT INTO reminders (id, task_id, kind, offset_minutes, remind_at, is_enabled, fired_at, created_at, updated_at)
         VALUES (?1, ?2, 'before_planned', -10, '2026-09-23T01:50:00.000Z', 1, '2026-09-23T01:50:00.000Z', '2026-09-23T02:00:00.000Z', '2026-09-23T02:00:00.000Z')",
    )
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(&src.id)
    .execute(db.pool())
    .await
    .unwrap();

    let r = duplicate_task_impl(db, &src.id).await.expect("复制任务");
    let new_id = r["newTaskId"].as_str().unwrap().to_string();

    assert_eq!(r["title"].as_str().unwrap(), "写季度总结（副本）");
    assert_eq!(r["copiedTags"].as_i64().unwrap(), 1, "标签应被复制");
    assert_eq!(r["copiedSubtasks"].as_i64().unwrap(), 2, "子任务应被复制");
    assert_eq!(r["copiedReminders"].as_i64().unwrap(), 1, "提醒应被复制");

    let copy = crate::commands::get_task_row(db, &new_id)
        .await
        .expect("读取副本");
    assert_eq!(copy.status, "todo", "副本必须是未完成");
    assert!(copy.completed_at.is_none(), "副本不应带完成时间");
    assert_eq!(copy.priority, 3, "优先级应保留");
    assert_eq!(copy.project_id.as_deref(), Some(project.as_str()));
    assert_eq!(copy.estimated_minutes, Some(90));

    // 子任务进度重置：副本的两个子任务都必须是未完成
    let done_sub: i64 = sqlx::query("SELECT COUNT(*) AS n FROM subtasks WHERE task_id = ?1 AND is_done = 1")
        .bind(&new_id)
        .fetch_one(db.pool())
        .await
        .unwrap()
        .try_get("n")
        .unwrap();
    assert_eq!(done_sub, 0, "副本的子任务进度应重置");

    // 提醒的 fired_at 必须清空
    let fired: Option<String> = sqlx::query("SELECT fired_at FROM reminders WHERE task_id = ?1")
        .bind(&new_id)
        .fetch_one(db.pool())
        .await
        .unwrap()
        .try_get("fired_at")
        .unwrap();
    assert!(fired.is_none(), "副本的提醒必须重新可触发");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn duplicate_generates_distinct_titles() {
    let (state, dir) = setup("dup-names").await;
    let db = &state.db;

    let src = create_task_impl(db, task("整理照片")).await.unwrap();
    let a = duplicate_task_impl(db, &src.id).await.unwrap();
    let b = duplicate_task_impl(db, &src.id).await.unwrap();

    assert_eq!(a["title"].as_str().unwrap(), "整理照片（副本）");
    assert_eq!(
        b["title"].as_str().unwrap(),
        "整理照片（副本 2）",
        "第二次复制必须换一个可区分的名字"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn duplicate_rejects_trashed_task() {
    let (state, dir) = setup("dup-trash").await;
    let db = &state.db;

    let src = create_task_impl(db, task("已删任务")).await.unwrap();
    sqlx::query("UPDATE tasks SET deleted_at = '2026-09-23T02:00:00.000Z' WHERE id = ?1")
        .bind(&src.id)
        .execute(db.pool())
        .await
        .unwrap();

    let err = duplicate_task_impl(db, &src.id).await.unwrap_err();
    assert!(
        err.message.contains("回收站"),
        "应给出可读原因，实际：{}",
        err.message
    );

    let _ = std::fs::remove_dir_all(dir);
}

// =============================================================================
// 拖拽排序
// =============================================================================

#[tokio::test]
async fn reorder_places_task_before_target_without_disturbing_others() {
    let (state, dir) = setup("reorder-before").await;
    let db = &state.db;

    let mut ids = Vec::new();
    for t in ["A", "B", "C", "D"] {
        ids.push(create_task_impl(db, task(t)).await.unwrap().id);
    }

    let order = titles_in_order;

    assert_eq!(order(db).await, vec!["A", "B", "C", "D"]);

    // 把 D 拖到 B 前面：期望 A B 之间插入 D
    reorder_task_impl(
        db,
        &ReorderInput {
            moved_id: ids[3].clone(),
            before_id: Some(ids[1].clone()),
        },
    )
    .await
    .expect("排序");

    assert_eq!(order(db).await, vec!["A", "D", "B", "C"]);

    // 把 A 拖到末尾
    reorder_task_impl(
        db,
        &ReorderInput {
            moved_id: ids[0].clone(),
            before_id: None,
        },
    )
    .await
    .expect("移到末尾");

    assert_eq!(order(db).await, vec!["D", "B", "C", "A"]);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn reorder_survives_repeated_inserts_at_same_spot() {
    // 中点插入法会让相邻差值不断减半；这里连续 60 次往同一位置插入，
    // 验证"差值过小就重新编号"的兜底分支真的会生效，顺序不会乱。
    let (state, dir) = setup("reorder-renumber").await;
    let db = &state.db;

    let a = create_task_impl(db, task("头")).await.unwrap().id;
    let b = create_task_impl(db, task("尾")).await.unwrap().id;
    for i in 0..60 {
        let mid = create_task_impl(db, task(&format!("中间{i}"))).await.unwrap().id;
        reorder_task_impl(
            db,
            &ReorderInput {
                moved_id: mid,
                before_id: Some(b.clone()),
            },
        )
        .await
        .expect("反复插入同一位置");
    }

    let rows = sqlx::query("SELECT title FROM tasks WHERE deleted_at IS NULL ORDER BY sort_order ASC")
        .fetch_all(db.pool())
        .await
        .unwrap();
    let titles: Vec<String> = rows
        .iter()
        .map(|r| r.try_get::<String, _>("title").unwrap())
        .collect();

    assert_eq!(titles.first().unwrap(), "头", "第一个任务不应被挤走");
    assert_eq!(titles.last().unwrap(), "尾", "最后一个任务不应被挤走");
    assert_eq!(titles.len(), 62, "不应丢任务");

    // 差值必须仍然互不相等（否则说明重编号没生效，顺序会退化成不确定）
    let orders: Vec<f64> = sqlx::query("SELECT sort_order FROM tasks WHERE deleted_at IS NULL ORDER BY sort_order ASC")
        .fetch_all(db.pool())
        .await
        .unwrap()
        .iter()
        .map(|r| r.try_get::<f64, _>("sort_order").unwrap())
        .collect();
    let mut sorted = orders.clone();
    sorted.dedup();
    assert_eq!(sorted.len(), orders.len(), "sort_order 出现重复值：{orders:?}");
    assert!(a != b);

    let _ = std::fs::remove_dir_all(dir);
}

// =============================================================================
// 合并组织项
// =============================================================================

#[tokio::test]
async fn merge_category_moves_open_tasks_and_soft_deletes_source() {
    let (state, dir) = setup("merge-category").await;
    let db = &state.db;

    let keep = insert_org(db, "categories", "工作").await;
    let gone = insert_org(db, "categories", "工作事务").await;

    let t1 = create_task_impl(
        db,
        CreateTaskInput {
            title: "任务一".into(),
            category_id: Some(gone.clone()),
            ..task("")
        },
    )
    .await
    .unwrap();
    let t2 = create_task_impl(
        db,
        CreateTaskInput {
            title: "任务二".into(),
            category_id: Some(gone.clone()),
            ..task("")
        },
    )
    .await
    .unwrap();

    let moved = merge_category_impl(
        db,
        &MergeInput {
            source_ids: vec![gone.clone()],
            target_id: keep.clone(),
        },
    )
    .await
    .expect("合并分类");
    assert_eq!(moved, 2);

    assert_eq!(col_str(db, &t1.id, "category_id").await.as_deref(), Some(keep.as_str()));
    assert_eq!(col_str(db, &t2.id, "category_id").await.as_deref(), Some(keep.as_str()));

    // 源分类被软删除：列表查不到，但记录仍在（可审计）
    let alive: i64 = sqlx::query("SELECT COUNT(*) AS n FROM categories WHERE id = ?1 AND deleted_at IS NULL")
        .bind(&gone)
        .fetch_one(db.pool())
        .await
        .unwrap()
        .try_get("n")
        .unwrap();
    assert_eq!(alive, 0, "源分类应已从可见列表移除");
    let still_there: i64 = sqlx::query("SELECT COUNT(*) AS n FROM categories WHERE id = ?1")
        .bind(&gone)
        .fetch_one(db.pool())
        .await
        .unwrap()
        .try_get("n")
        .unwrap();
    assert_eq!(still_there, 1, "软删除应保留记录");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn merge_skips_trashed_tasks() {
    // 回收站里的任务保留它"当时的"归属，不应被合并动到
    let (state, dir) = setup("merge-trash").await;
    let db = &state.db;

    let keep = insert_org(db, "categories", "保留").await;
    let gone = insert_org(db, "categories", "待合并").await;

    let t = create_task_impl(
        db,
        CreateTaskInput {
            title: "进回收站的任务".into(),
            category_id: Some(gone.clone()),
            ..task("")
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE tasks SET deleted_at = '2026-09-23T02:00:00.000Z' WHERE id = ?1")
        .bind(&t.id)
        .execute(db.pool())
        .await
        .unwrap();

    let moved = merge_category_impl(
        db,
        &MergeInput {
            source_ids: vec![gone.clone()],
            target_id: keep.clone(),
        },
    )
    .await
    .expect("合并分类");
    assert_eq!(moved, 0, "回收站任务不应被计入转移数");
    assert_eq!(
        col_str(db, &t.id, "category_id").await.as_deref(),
        Some(gone.as_str()),
        "回收站任务应保留原归属"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn merge_project_and_tag_behave_same_way() {
    let (state, dir) = setup("merge-multi").await;
    let db = &state.db;

    // 项目
    let p_keep = insert_org(db, "projects", "主线").await;
    let p_gone = insert_org(db, "projects", "旧项目").await;
    let t = create_task_impl(
        db,
        CreateTaskInput {
            title: "跨项目任务".into(),
            project_id: Some(p_gone.clone()),
            ..task("")
        },
    )
    .await
    .unwrap();
    assert_eq!(
        merge_project_impl(
            db,
            &MergeInput {
                source_ids: vec![p_gone.clone()],
                target_id: p_keep.clone(),
            }
        )
        .await
        .unwrap(),
        1
    );
    assert_eq!(col_str(db, &t.id, "project_id").await.as_deref(), Some(p_keep.as_str()));

    // 标签：任务同时拥有两个标签时，合并后不能出现重复关联
    let g_keep = insert_org(db, "tags", "紧急").await;
    let g_gone = insert_org(db, "tags", "急").await;
    for tag in [&g_keep, &g_gone] {
        sqlx::query("INSERT INTO task_tags (task_id, tag_id) VALUES (?1, ?2)")
            .bind(&t.id)
            .bind(tag)
            .execute(db.pool())
            .await
            .unwrap();
    }
    merge_tag_impl(
        db,
        &MergeInput {
            source_ids: vec![g_gone.clone()],
            target_id: g_keep.clone(),
        },
    )
    .await
    .expect("合并标签");

    let links: i64 = sqlx::query("SELECT COUNT(*) AS n FROM task_tags WHERE task_id = ?1 AND tag_id = ?2")
        .bind(&t.id)
        .bind(&g_keep)
        .fetch_one(db.pool())
        .await
        .unwrap()
        .try_get("n")
        .unwrap();
    assert_eq!(links, 1, "合并后不应产生重复的标签关联");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn merge_rejects_invalid_requests() {
    let (state, dir) = setup("merge-invalid").await;
    let db = &state.db;
    let a = insert_org(db, "categories", "甲").await;
    let b = insert_org(db, "categories", "乙").await;

    // 空源列表
    let e1 = merge_category_impl(
        db,
        &MergeInput {
            source_ids: vec![],
            target_id: a.clone(),
        },
    )
    .await
    .unwrap_err();
    assert!(e1.message.contains("请选择"), "实际：{}", e1.message);

    // 目标同时作为源
    let e2 = merge_category_impl(
        db,
        &MergeInput {
            source_ids: vec![a.clone(), b.clone()],
            target_id: a.clone(),
        },
    )
    .await
    .unwrap_err();
    assert!(e2.message.contains("不能同时"), "实际：{}", e2.message);

    // 不存在的源
    let e3 = merge_category_impl(
        db,
        &MergeInput {
            source_ids: vec!["不存在的-id".into()],
            target_id: b.clone(),
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(e3.code, crate::error::ErrorCode::NotFound),
        "不存在的源应报 NotFound，实际：{:?}",
        e3.code
    );

    // 两个分类都应还活着
    let alive: i64 = sqlx::query("SELECT COUNT(*) AS n FROM categories WHERE deleted_at IS NULL")
        .fetch_one(db.pool())
        .await
        .unwrap()
        .try_get("n")
        .unwrap();
    assert_eq!(alive, 2, "失败的合并不应删除任何分类");

    let _ = std::fs::remove_dir_all(dir);
}
