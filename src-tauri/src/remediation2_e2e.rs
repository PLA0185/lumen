//! 第二轮整改的端到端回归（《Lumen 第二轮整改任务书》§11 / §14）。
//!
//! ## 为什么这些测试必须"真的造很多数据"
//!
//! 本批测试针对的缺陷都有一个共同特征：**只有数据量超过某个阈值才会暴露**。
//!
//! | 缺陷 | 为什么小数据测不出来 |
//! | --- | --- |
//! | 列表固定 `limit: 500` | 501 条之前一切正常，第 501 条开始静默消失 |
//! | 分页排序并列 | 只有存在重复 `sort_order` 时才可能翻页重复/漏项 |
//! | 回收站确认数量 | 列表没加载全时，`tasks.length` 与真实总数才不一致 |
//! | 报告截断边界 | 恰好 100000 条时才误报 `truncated` |
//!
//! 因此这里用 SQL 递归 CTE 批量插入，而不是逐条 `create_task`——
//! 后者造 2000 条要几秒钟，造 10 万条根本跑不完。
//!
//! ## 与 `commands_e2e` 的分工
//!
//! `commands_e2e` 测的是**语义边界**（副本、合并、收件箱、原子性），
//! 每条用例只需要少量数据；这里测的是**规模与边界值**。

use std::collections::HashSet;

use crate::attachments::{cleanup_orphans_impl, delete_copied_files};
use crate::commands::{
    count_tasks_impl, list_tasks_impl, purge_all_deleted_impl, purge_task_impl, report_all_impl,
    AppState, REPORT_MAX_ROWS,
};
use crate::db::Db;
use crate::models::TaskQuery;
use sqlx::Row;

/// 建临时库；调用方负责删除目录
async fn setup(name: &str) -> (AppState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("lumen-{name}-{}", uuid::Uuid::now_v7()));
    let db = Db::init(&dir).await.expect("初始化数据库");
    (AppState::new(db), dir)
}

fn cleanup(dir: std::path::PathBuf) {
    let _ = std::fs::remove_dir_all(dir);
}

/// 批量插入任务，返回插入的 id 列表。
///
/// 参数：
/// - `n`：条数；
/// - `prefix`：id/标题前缀（用于区分不同批次）；
/// - `status`：状态；
/// - `deleted`：是否放进回收站；
/// - `sort_order`：排序值（传固定值可以制造大量并列，专门测分页稳定性）；
/// - `project_id`：归属项目（`None` 表示没有项目，用于收件箱）。
async fn seed_tasks(
    db: &Db,
    n: i64,
    prefix: &str,
    status: &str,
    deleted: bool,
    sort_order: f64,
    project_id: Option<&str>,
) -> Vec<String> {
    let now = crate::db::to_db_time(crate::db::utc_now());
    let deleted_at: Option<String> = if deleted { Some(now.clone()) } else { None };
    // 递归 CTE 一次插入：比 n 次 INSERT 快两个数量级。
    // 标题与 id 都带 `-`，便于用 `LIKE 'prefix-%'` 选取其中一部分做后续 UPDATE。
    sqlx::query(
        "WITH RECURSIVE seq(i) AS (
             SELECT 1 UNION ALL SELECT i + 1 FROM seq WHERE i < ?1
         )
         INSERT INTO tasks (id, title, status, sort_order, created_at, updated_at,
                            deleted_at, project_id, is_pinned)
         SELECT ?2 || '-' || i, ?2 || '-' || i || ' 任务', ?3, ?4, ?5, ?5, ?6, ?7, 0
         FROM seq",
    )
    .bind(n)
    .bind(prefix)
    .bind(status)
    .bind(sort_order)
    .bind(&now)
    .bind(&deleted_at)
    .bind(project_id)
    .execute(db.pool())
    .await
    .expect("批量插入任务");

    (1..=n).map(|i| format!("{prefix}-{i}")).collect()
}

/// 逐页把一个查询读全，返回读到的 id（顺序即后端返回顺序）
async fn read_all_pages(db: &Db, base: &TaskQuery, page_size: i64) -> Vec<String> {
    let mut out = Vec::new();
    let mut offset = 0i64;
    loop {
        let page = list_tasks_impl(
            db,
            TaskQuery {
                limit: Some(page_size),
                offset: Some(offset),
                ..base.clone()
            },
        )
        .await
        .expect("分页读取");
        if page.is_empty() {
            break;
        }
        offset += page.len() as i64;
        out.extend(page.into_iter().map(|t| t.id));
        // 防御：万一实现有问题导致不推进，这里明确失败而不是死循环
        assert!(offset <= 1_000_000, "分页读取未收敛，offset={offset}");
    }
    out
}

fn q_all() -> TaskQuery {
    TaskQuery {
        statuses: vec!["todo".into()],
        sort_by: Some("manual".into()),
        ..Default::default()
    }
}

fn q_trash() -> TaskQuery {
    TaskQuery {
        deleted_only: true,
        statuses: vec![],
        ..Default::default()
    }
}

// =============================================================================
// §14.2 分页
// =============================================================================

/// 1200 条必须能逐页读全，不重不漏（§14.2 / §4.7）。
///
/// 修复前的情况：前端写死 `limit: 500` 且没有"加载更多"，
/// 第 501 条之后**数据库里有、界面上永远看不到**。
#[tokio::test]
async fn pagination_reads_all_1200_rows_without_gaps_or_duplicates() {
    let (state, dir) = setup("page1200").await;
    let db = &state.db;
    // 全部使用同一个 sort_order：这是最坏情况，逼出"并列时的全序"问题
    let inserted = seed_tasks(db, 1200, "p", "todo", false, 0.0, None).await;

    let total = count_tasks_impl(db, &q_all()).await.unwrap();
    assert_eq!(total, 1200, "count 必须与插入条数一致");

    let seen = read_all_pages(db, &q_all(), 200).await;
    assert_eq!(
        seen.len(),
        1200,
        "必须能读到第 1200 条，而不是停在第 500 条"
    );

    let uniq: HashSet<&String> = seen.iter().collect();
    assert_eq!(uniq.len(), 1200, "分页出现重复项");

    let expected: HashSet<String> = inserted.into_iter().collect();
    let got: HashSet<String> = seen.into_iter().collect();
    assert_eq!(got, expected, "分页结果与插入集合不一致（有漏项或多出项）");

    cleanup(dir);
}

/// 排序键全部并列时，两遍读取顺序必须完全一致（§4.6）。
///
/// 这条挡的是"分页依赖稳定排序"这个前提：只要 `ORDER BY` 不是全序，
/// SQLite 就没有义务在两页之间保持相对顺序，翻页必然出现重复/漏项。
#[tokio::test]
async fn pagination_order_is_stable_when_sort_keys_tie() {
    let (state, dir) = setup("pagestable").await;
    let db = &state.db;
    seed_tasks(db, 600, "tie", "todo", false, 0.0, None).await;

    let first = read_all_pages(db, &q_all(), 100).await;
    let second = read_all_pages(db, &q_all(), 100).await;
    assert_eq!(first, second, "同样的查询两次读取顺序必须一致");

    // 换一种页大小也必须得到同一顺序（页大小不该影响结果集顺序）
    let with_other_page = read_all_pages(db, &q_all(), 137).await;
    assert_eq!(first, with_other_page, "页大小变化不应改变结果顺序");

    cleanup(dir);
}

/// `count` 与 `list` 在**每一种筛选条件**下都必须一致（§10.1）。
///
/// 这条直接对着"UI 显示 1200、列表实际只有 1137"那个风险写的：
/// 两者共用 `apply_task_filters`，但只有测试才能保证以后不会有人拆开。
#[tokio::test]
async fn count_matches_list_under_every_filter() {
    let (state, dir) = setup("countmatch").await;
    let db = &state.db;

    // 项目、标签、分类各造一份数据
    let project_id = uuid::Uuid::now_v7().to_string();
    let other_project_id = uuid::Uuid::now_v7().to_string();
    let tag_id = uuid::Uuid::now_v7().to_string();
    let category_id = uuid::Uuid::now_v7().to_string();
    let now = crate::db::to_db_time(crate::db::utc_now());
    for (id, table, name) in [
        (&project_id, "projects", "项目甲"),
        (&other_project_id, "projects", "项目乙"),
        (&tag_id, "tags", "标签甲"),
        (&category_id, "categories", "分类甲"),
    ] {
        let sql = match table {
            "projects" => {
                "INSERT INTO projects (id, name, sort_order, is_favorite, is_archived, created_at, updated_at)
                 VALUES (?1, ?2, 0, 0, 0, ?3, ?3)"
            }
            "tags" => {
                "INSERT INTO tags (id, name, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, 0, ?3, ?3)"
            }
            _ => {
                "INSERT INTO categories (id, name, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, 0, ?3, ?3)"
            }
        };
        sqlx::query(sqlx::AssertSqlSafe(sql.to_string()))
            .bind(id)
            .bind(name)
            .bind(&now)
            .execute(db.pool())
            .await
            .unwrap();
    }

    // 400 条有项目、300 条无项目（收件箱）、200 条已完成、150 条回收站
    seed_tasks(db, 400, "proj", "todo", false, 1.0, Some(&project_id)).await;
    seed_tasks(db, 300, "inbox", "todo", false, 2.0, None).await;
    seed_tasks(db, 200, "done", "done", false, 3.0, None).await;
    seed_tasks(db, 150, "trash", "todo", true, 4.0, None).await;

    // 给其中 50 条打标签、设分类、设计划时间（用来验证条件确实生效）
    sqlx::query(
        "INSERT INTO task_tags (task_id, tag_id)
         SELECT id, ?1 FROM tasks WHERE title LIKE 'proj-%' LIMIT 50",
    )
    .bind(&tag_id)
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query("UPDATE tasks SET category_id = ?1 WHERE title LIKE 'proj-1%'")
        .bind(&category_id)
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE tasks SET planned_at = ?1 WHERE title LIKE 'proj-2%'")
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE tasks SET due_at = ?1, status = 'todo' WHERE title LIKE 'proj-3%'")
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

    let cases: Vec<(&str, TaskQuery)> = vec![
        ("未删除+状态", q_all()),
        ("全部未归档", TaskQuery::default()),
        ("回收站", q_trash()),
        (
            "按项目",
            TaskQuery {
                project_id: Some(project_id.clone()),
                ..Default::default()
            },
        ),
        (
            "收件箱 withoutProject",
            TaskQuery {
                without_project: true,
                statuses: vec!["todo".into()],
                ..Default::default()
            },
        ),
        (
            "按分类",
            TaskQuery {
                category_id: Some(category_id.clone()),
                ..Default::default()
            },
        ),
        (
            "按标签",
            TaskQuery {
                tag_ids: vec![tag_id.clone()],
                ..Default::default()
            },
        ),
        (
            "按优先级",
            TaskQuery {
                priorities: vec![0],
                ..Default::default()
            },
        ),
        (
            "按计划时间",
            TaskQuery {
                planned_from: Some(now.clone()),
                planned_to: Some(now.clone()),
                ..Default::default()
            },
        ),
        (
            "按截止时间",
            TaskQuery {
                due_from: Some(now.clone()),
                due_to: Some(now.clone()),
                ..Default::default()
            },
        ),
        (
            "搜索标题",
            TaskQuery {
                search: Some("proj-1".into()),
                ..Default::default()
            },
        ),
        (
            "搜索项目名的任务",
            TaskQuery {
                search: Some("项目甲".into()),
                ..Default::default()
            },
        ),
        (
            "搜索标签名",
            TaskQuery {
                search: Some("标签甲".into()),
                ..Default::default()
            },
        ),
        (
            "非重复任务",
            TaskQuery {
                is_recurring: Some(false),
                ..Default::default()
            },
        ),
        (
            "逾期",
            TaskQuery {
                overdue_only: true,
                ..Default::default()
            },
        ),
        (
            "已完成",
            TaskQuery {
                statuses: vec!["done".into()],
                ..Default::default()
            },
        ),
    ];

    for (label, query) in cases {
        let total = count_tasks_impl(db, &query).await.unwrap();
        let listed = read_all_pages(db, &query, 100).await.len() as i64;
        assert_eq!(
            total, listed,
            "「{label}」条件下 count={total} 与列表实际条数={listed} 不一致"
        );
    }

    // 抽样确认条件真的起作用（否则"两边都是 0"也会通过）
    let inbox = count_tasks_impl(
        db,
        &TaskQuery {
            without_project: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        inbox >= 300,
        "收件箱至少应有 300 条无项目任务，实际 {inbox}"
    );
    let trash = count_tasks_impl(db, &q_trash()).await.unwrap();
    assert_eq!(trash, 150, "回收站应有 150 条");
    let by_tag = count_tasks_impl(
        db,
        &TaskQuery {
            tag_ids: vec![tag_id],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(by_tag, 50, "标签下应有 50 条");

    cleanup(dir);
}

// =============================================================================
// §14.3 回收站
// =============================================================================

/// 回收站 1200 条：总数、清空数量、以及"确认数量 = 实际删除数量"。
#[tokio::test]
async fn trash_count_drives_purge_confirmation_and_purge_result() {
    let (state, dir) = setup("trash1200").await;
    let db = &state.db;
    seed_tasks(db, 1200, "t", "todo", true, 0.0, None).await;
    // 未删除的任务不该被清空回收站带走
    seed_tasks(db, 30, "keep", "todo", false, 0.0, None).await;

    let trash_total = count_tasks_impl(db, &q_trash()).await.unwrap();
    assert_eq!(trash_total, 1200, "回收站总数必须是 1200");

    // 界面只加载第一页时，用来做确认的数量仍然来自 count（=1200），
    // 而不是"这一页读到了多少条"
    let first_page = list_tasks_impl(
        db,
        TaskQuery {
            limit: Some(100),
            offset: Some(0),
            ..q_trash()
        },
    )
    .await
    .unwrap();
    assert_eq!(first_page.len(), 100, "第一页只加载 100 条");
    assert_ne!(
        first_page.len() as i64,
        trash_total,
        "已加载条数与真实总数必须不同——这正是原缺陷的现场"
    );

    let r = purge_all_deleted_impl(db).await.unwrap();
    assert_eq!(r.purged, 1200, "清空回收站必须返回真实删除数");
    assert_eq!(r.purged, trash_total, "返回数量必须与确认时显示的数量一致");
    assert_eq!(count_tasks_impl(db, &q_trash()).await.unwrap(), 0);
    assert_eq!(
        count_tasks_impl(db, &q_all()).await.unwrap(),
        30,
        "未删除的任务不能被误删"
    );

    cleanup(dir);
}

/// 单个永久删除仍然只允许作用在回收站内的任务上
#[tokio::test]
async fn purge_single_task_refuses_live_task() {
    let (state, dir) = setup("purgeguard").await;
    let db = &state.db;
    let live = seed_tasks(db, 1, "live", "todo", false, 0.0, None).await;
    let err = purge_task_impl(db, &live[0]).await.unwrap_err();
    assert!(err.message.contains("回收站"), "实际：{}", err.message);
    assert_eq!(count_tasks_impl(db, &q_all()).await.unwrap(), 1);

    cleanup(dir);
}

// =============================================================================
// §14.4 附件副本清理
// =============================================================================

/// 在受控附件目录里造一个"copied 附件"，返回 (附件 id, 文件绝对路径)
async fn make_copied_attachment(
    db: &Db,
    task_id: &str,
    content: &str,
) -> (String, std::path::PathBuf) {
    let id = uuid::Uuid::now_v7().to_string();
    let dir = db.data_dir().join("attachments");
    std::fs::create_dir_all(&dir).unwrap();
    // 文件名规则与 attachment_add 一致：<uuid>.txt
    let file = dir.join(format!("{id}.txt"));
    std::fs::write(&file, content).unwrap();

    let rel = format!("attachments/{id}.txt");
    let now = crate::db::to_db_time(crate::db::utc_now());
    sqlx::query(
        "INSERT INTO attachments
            (id, task_id, file_name, mime_type, byte_size, sha256, storage_mode,
             external_path, stored_path, created_at)
         VALUES (?1, ?2, ?3, 'text/plain', ?4, NULL, 'copied', NULL, ?5, ?6)",
    )
    .bind(&id)
    .bind(task_id)
    .bind(format!("{id}.txt"))
    .bind(content.len() as i64)
    .bind(&rel)
    .bind(&now)
    .execute(db.pool())
    .await
    .unwrap();

    (id, file)
}

/// 造一条 reference 模式的附件记录（指向用户目录外的原文件），返回原文件路径
async fn make_reference_attachment(
    db: &Db,
    task_id: &str,
    outside_file: &std::path::Path,
) -> String {
    let id = uuid::Uuid::now_v7().to_string();
    let now = crate::db::to_db_time(crate::db::utc_now());
    sqlx::query(
        "INSERT INTO attachments
            (id, task_id, file_name, mime_type, byte_size, sha256, storage_mode,
             external_path, stored_path, created_at)
         VALUES (?1, ?2, 'original.pdf', 'application/pdf', 10, NULL, 'reference', ?3, NULL, ?4)",
    )
    .bind(&id)
    .bind(task_id)
    .bind(outside_file.to_string_lossy().to_string())
    .bind(&now)
    .execute(db.pool())
    .await
    .unwrap();
    id
}

/// copied 附件随永久删除消失；reference 的原文件一根汗毛都不能动（§6.6 / §14.4）
#[tokio::test]
async fn purge_task_removes_copied_copy_but_never_touches_original() {
    let (state, dir) = setup("attpurge").await;
    let db = &state.db;

    let ids = seed_tasks(db, 1, "a", "todo", true, 0.0, None).await;
    let task_id = &ids[0];

    // 用户原始文件放在数据目录之外
    let outside_dir = std::env::temp_dir().join(format!("lumen-outside-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&outside_dir).unwrap();
    let original = outside_dir.join("original.pdf");
    std::fs::write(&original, b"%PDF-1.4 fake").unwrap();

    let (_att, copy) = make_copied_attachment(db, task_id, "副本内容").await;
    make_reference_attachment(db, task_id, &original).await;

    assert!(copy.exists(), "副本应已创建");
    assert!(original.exists());

    let r = purge_task_impl(db, task_id).await.unwrap();
    assert_eq!(r.purged, 1);

    assert!(!copy.exists(), "copied 副本必须随任务一起被清理");
    assert!(original.exists(), "reference 模式的原文件绝对不能被删除");
    let left: i64 = sqlx::query("SELECT COUNT(*) AS n FROM attachments")
        .fetch_one(db.pool())
        .await
        .unwrap()
        .get("n");
    assert_eq!(left, 0, "附件记录应随任务级联删除");

    cleanup(dir);
    let _ = std::fs::remove_dir_all(outside_dir);
}

/// 清空回收站要清理**多个**任务的副本文件
#[tokio::test]
async fn purge_all_removes_every_copied_file() {
    let (state, dir) = setup("attpurgeall").await;
    let db = &state.db;

    let ids = seed_tasks(db, 5, "many", "todo", true, 0.0, None).await;
    let mut files = Vec::new();
    for id in &ids {
        let (_a, f) = make_copied_attachment(db, id, "x").await;
        files.push(f);
    }
    // 不在回收站里的任务，其副本不能被清空回收站带走
    let live = seed_tasks(db, 1, "live", "todo", false, 0.0, None).await;
    let (_a, live_file) = make_copied_attachment(db, &live[0], "y").await;

    let r = purge_all_deleted_impl(db).await.unwrap();
    assert_eq!(r.purged, 5);
    for f in &files {
        assert!(!f.exists(), "回收站任务的副本应被清理：{}", f.display());
    }
    assert!(live_file.exists(), "未删除任务的副本不能被清理");

    cleanup(dir);
}

/// 孤儿扫描：只删"受控目录内 + UUID 命名 + 数据库无引用"的文件（§6.5 / §14.4）
#[tokio::test]
async fn orphan_cleanup_only_removes_managed_unreferenced_files() {
    let (state, dir) = setup("orphan").await;
    let db = &state.db;
    let ids = seed_tasks(db, 1, "o", "todo", false, 0.0, None).await;

    // 1) 仍被引用的副本 —— 必须保留
    let (_att, referenced) = make_copied_attachment(db, &ids[0], "keep me").await;
    // 2) 无引用的 Lumen 命名文件 —— 应被删除
    let orphan_dir = db.data_dir().join("attachments");
    let orphan_name = format!("{}.bin", uuid::Uuid::now_v7());
    let orphan = orphan_dir.join(&orphan_name);
    std::fs::write(&orphan, b"orphan").unwrap();
    // 3) 用户自己放进来的文件（非 UUID 命名）—— 无论如何都不能删
    let user_file = orphan_dir.join("我的笔记.txt");
    std::fs::write(&user_file, b"user data").unwrap();
    // 4) 子目录 —— 不递归、不删除
    let sub = orphan_dir.join("子目录");
    std::fs::create_dir_all(&sub).unwrap();

    let r = cleanup_orphans_impl(db).await.unwrap();
    assert_eq!(
        r.removed, 1,
        "只应删掉那一个孤儿文件：{:?}",
        r.removed_files
    );
    assert_eq!(
        r.removed_files,
        vec![orphan_name],
        "删掉的必须是孤儿文件本身"
    );
    assert!(!orphan.exists(), "孤立副本应被清理");
    assert!(referenced.exists(), "仍被引用的副本不能被删");
    assert!(user_file.exists(), "用户自己放进目录的文件不能被删");
    assert!(sub.exists(), "子目录不能被删");
    assert!(r.kept >= 1 && r.skipped >= 2, "统计要如实反映保留与跳过");

    cleanup(dir);
}

/// 路径越界：数据库里的记录指向受控目录之外时，绝不能删那个文件（§14.4 最后一条）
#[tokio::test]
async fn delete_copied_files_refuses_paths_outside_the_controlled_dir() {
    let (state, dir) = setup("attescape").await;
    let db = &state.db;

    // 受控目录之外的"用户文件"
    let outside = std::env::temp_dir().join(format!("lumen-escape-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&outside).unwrap();
    let victim = outside.join("重要文件.txt");
    std::fs::write(&victim, b"do not delete").unwrap();

    // 三种越界写法都要挡住
    let attempts = vec![
        victim.to_string_lossy().to_string(), // 绝对路径
        "../../重要文件.txt".to_string(),     // 相对穿越
        format!(
            "attachments/../../{}",
            victim.file_name().unwrap().to_string_lossy()
        ), // 绕一圈
    ];
    let (removed, failed) = delete_copied_files(db.data_dir(), &attempts);
    assert_eq!(removed, 0, "一个越界文件都不该删掉");
    assert_eq!(failed, attempts.len(), "越界尝试都应被记为跳过");
    assert!(victim.exists(), "受控目录之外的文件必须原样保留");

    cleanup(dir);
    let _ = std::fs::remove_dir_all(outside);
}

// =============================================================================
// §14.5 报告上限边界
// =============================================================================

/// 99999 / 100000 / 100001 三个边界（§7.4）：
/// 只有**确实还有没导出的**记录时才算截断。
///
/// 这条测试是整套里最慢的：它必须真的存在 10 万条数据（不能 mock），
/// 而且要把它们**真实导出三遍**。实测耗时与"深分页会线性变慢"这个观察
/// 一并记在 `docs/work-log.md` 第 8 轮里。
#[tokio::test]
async fn report_truncation_is_exact_at_the_export_limit() {
    let (state, dir) = setup("reportlimit").await;
    let db = &state.db;

    let base = REPORT_MAX_ROWS - 1; // 99999
    let t0 = std::time::Instant::now();
    seed_tasks(db, base, "r", "todo", false, 0.0, None).await;
    eprintln!("[timing] 插入 {base} 条：{:?}", t0.elapsed());

    // ---- 99999 ----
    let t1 = std::time::Instant::now();
    let p = report_all_impl(db, q_all()).await.unwrap();
    eprintln!("[timing] 导出 {} 条：{:?}", p.total, t1.elapsed());
    assert_eq!(p.total, base, "应取到全部 99999 条");
    assert!(!p.truncated, "不足上限时绝不能报截断");

    // ---- 100000（恰好等于上限）----
    seed_tasks_single(db, "r-one", "todo", false).await;
    let t2 = std::time::Instant::now();
    let p = report_all_impl(db, q_all()).await.unwrap();
    eprintln!("[timing] 导出 {} 条：{:?}", p.total, t2.elapsed());
    assert_eq!(p.total, REPORT_MAX_ROWS, "应取到 100000 条");
    assert!(
        !p.truncated,
        "恰好 100000 条时没有第 100001 条，不得误报截断（§7.1 的原始缺陷）"
    );

    // ---- 100001 ----
    seed_tasks_single(db, "r-two", "todo", false).await;
    let t3 = std::time::Instant::now();
    let p = report_all_impl(db, q_all()).await.unwrap();
    eprintln!("[timing] 导出 {} 条（应截断）：{:?}", p.total, t3.elapsed());
    assert_eq!(p.total, REPORT_MAX_ROWS, "截断后返回的仍是 100000 条");
    assert!(p.truncated, "确实还有一条没导出，必须如实告知");

    cleanup(dir);
}

/// 单条插入（边界测试需要在已有数据上"加一条"）
async fn seed_tasks_single(db: &Db, id: &str, status: &str, deleted: bool) {
    let now = crate::db::to_db_time(crate::db::utc_now());
    let deleted_at: Option<String> = if deleted { Some(now.clone()) } else { None };
    sqlx::query(
        "INSERT INTO tasks (id, title, status, sort_order, created_at, updated_at, deleted_at)
         VALUES (?1, ?1, ?2, 0, ?3, ?3, ?4)",
    )
    .bind(id)
    .bind(status)
    .bind(&now)
    .bind(&deleted_at)
    .execute(db.pool())
    .await
    .expect("插入单条任务");
}

// =============================================================================
// §11 2000 条数据量专项回归
// =============================================================================

/// 任务书 §11 要求的那份数据：500 未完成 + 500 已完成 + 500 回收站 + 500 带项目/标签/时间。
///
/// 一次性把各视图都验一遍（分页 / 收件箱 / 已完成 / 回收站 / 搜索 / 项目 / 标签 / 排序），
/// 并断言"count 与列表条数一致"这条总不变量在每个视图下都成立。
#[tokio::test]
async fn regression_2000_rows_across_every_view() {
    let (state, dir) = setup("reg2000").await;
    let db = &state.db;
    let now = crate::db::to_db_time(crate::db::utc_now());

    // 项目与标签
    let project_id = uuid::Uuid::now_v7().to_string();
    let tag_id = uuid::Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO projects (id, name, sort_order, is_favorite, is_archived, created_at, updated_at)
         VALUES (?1, '项目甲', 0, 0, 0, ?2, ?2)",
    )
    .bind(&project_id)
    .bind(&now)
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tags (id, name, sort_order, created_at, updated_at) VALUES (?1, '标签甲', 0, ?2, ?2)",
    )
    .bind(&tag_id)
    .bind(&now)
    .execute(db.pool())
    .await
    .unwrap();

    seed_tasks(db, 500, "open", "todo", false, 0.0, None).await; // 500 未完成
    seed_tasks(db, 500, "done", "done", false, 1.0, None).await; // 500 已完成
    seed_tasks(db, 500, "trash", "todo", true, 2.0, None).await; // 500 回收站
    seed_tasks(db, 500, "meta", "todo", false, 3.0, Some(&project_id)).await; // 500 带归属

    // 其中 200 条打标签、200 条有计划时间、200 条有截止时间（部分重叠，制造真实分布）
    sqlx::query("INSERT INTO task_tags (task_id, tag_id) SELECT id, ?1 FROM tasks WHERE title LIKE 'meta-%' LIMIT 200")
        .bind(&tag_id)
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE tasks SET planned_at = ?1 WHERE title LIKE 'meta-1%'")
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE tasks SET due_at = ?1 WHERE title LIKE 'meta-2%'")
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

    let cases: Vec<(&str, TaskQuery)> = vec![
        ("全部未归档", TaskQuery::default()),
        (
            "全部任务（含回收站）",
            TaskQuery {
                include_deleted: true,
                statuses: vec![],
                ..Default::default()
            },
        ),
        ("未完成", q_all()),
        (
            "已完成",
            TaskQuery {
                statuses: vec!["done".into()],
                ..Default::default()
            },
        ),
        ("回收站", q_trash()),
        (
            "收件箱",
            TaskQuery {
                without_project: true,
                statuses: vec!["todo".into()],
                ..Default::default()
            },
        ),
        (
            "按项目",
            TaskQuery {
                project_id: Some(project_id.clone()),
                ..Default::default()
            },
        ),
        (
            "按标签",
            TaskQuery {
                tag_ids: vec![tag_id.clone()],
                ..Default::default()
            },
        ),
        (
            "搜索 meta-1",
            TaskQuery {
                search: Some("meta-1".into()),
                ..Default::default()
            },
        ),
        (
            "按标题排序",
            TaskQuery {
                sort_by: Some("title".into()),
                ..Default::default()
            },
        ),
        (
            "按创建时间倒序",
            TaskQuery {
                sort_by: Some("created".into()),
                sort_desc: Some(true),
                ..Default::default()
            },
        ),
        (
            "按截止时间",
            TaskQuery {
                sort_by: Some("due".into()),
                ..Default::default()
            },
        ),
        (
            "逾期",
            TaskQuery {
                overdue_only: true,
                ..Default::default()
            },
        ),
    ];

    for (label, query) in cases {
        let total = count_tasks_impl(db, &query).await.unwrap();
        let ids = read_all_pages(db, &query, 250).await;
        assert_eq!(
            total,
            ids.len() as i64,
            "「{label}」视图：count={total}，实际读到 {} 条",
            ids.len()
        );
        let uniq: HashSet<&String> = ids.iter().collect();
        assert_eq!(uniq.len(), ids.len(), "「{label}」视图分页出现重复项");
        assert!(total > 0, "「{label}」视图不该是空的（数据造错了）");
    }

    // 几个视图的具体数字，防止"条件写错但两边都错成一样"
    assert_eq!(count_tasks_impl(db, &q_trash()).await.unwrap(), 500);
    assert_eq!(
        count_tasks_impl(
            db,
            &TaskQuery {
                statuses: vec!["done".into()],
                ..Default::default()
            }
        )
        .await
        .unwrap(),
        500
    );
    assert_eq!(
        count_tasks_impl(
            db,
            &TaskQuery {
                project_id: Some(project_id),
                ..Default::default()
            }
        )
        .await
        .unwrap(),
        500
    );
    assert_eq!(
        count_tasks_impl(
            db,
            &TaskQuery {
                tag_ids: vec![tag_id],
                ..Default::default()
            }
        )
        .await
        .unwrap(),
        200
    );

    // 清空回收站的数量也要对得上（界面确认数量用的就是这个值）
    let r = purge_all_deleted_impl(db).await.unwrap();
    assert_eq!(r.purged, 500, "清空回收站应删除 500 条");
    assert_eq!(count_tasks_impl(db, &q_all()).await.unwrap(), 1000);

    cleanup(dir);
}
