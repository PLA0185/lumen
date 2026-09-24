//! 项目、分类、标签的管理用例（任务书 §4.2）。
//!
//! 三者的语义边界（界面必须说清，否则用户会困惑）：
//! - **项目**：任务的集合/列表，一个任务最多属于一个项目。可归档、可合并。
//! - **分类**：用于统计"类别占比"的归类维度（§7），与项目正交。
//! - **标签**：跨项目的横向标记，一个任务可有多个。
//!
//! 命名唯一性只在"未删除"范围内生效（数据库用部分唯一索引实现），
//! 因此删除后可以重新使用同名，符合直觉。

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder, Row, Sqlite};
use tauri::State;

use crate::commands::AppState;
use crate::db::{to_db_time, utc_now, Db};
use crate::error::{AppError, AppResult};

/// 名称长度上限，防止超长文本进入列表渲染
const NAME_MAX: usize = 100;
/// 描述长度上限
const DESC_MAX: usize = 2_000;

// =============================================================================
// 模型
// =============================================================================

/// 项目视图
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: String,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub sort_order: i64,
    pub is_favorite: i64,
    pub is_archived: i64,
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

/// 分类视图
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: String,
    pub description: String,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

/// 标签视图
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

/// 带任务计数的项目（列表页需要显示"该项目下有几项未完成"）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWithCount {
    #[serde(flatten)]
    pub project: Project,
    /// 未完成任务数
    pub open_count: i64,
    /// 全部未删除任务数
    pub total_count: i64,
}

/// 带计数的标签
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagWithCount {
    #[serde(flatten)]
    pub tag: Tag,
    pub task_count: i64,
}

/// 删除策略：删除项目/分类/标签时如何处理关联任务（§4.2 要求"说明关联任务如何处理"）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrphanStrategy {
    /// 让关联任务变成"无项目/无分类"（保留任务本身，最安全）
    Detach,
    /// 同时软删除关联任务（回收站可恢复）
    CascadeSoftDelete,
}

/// 删除前的影响预览（§3「重要操作可撤销或二次确认」）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteImpact {
    /// 受影响的未删除任务数
    pub affected_tasks: i64,
    /// 受影响的已完成任务数（单独列出，因为用户通常更在意历史）
    pub completed_tasks: i64,
    /// 直接子项数量（如项目的分段、标签的任务关联）
    pub related_records: i64,
}

// =============================================================================
// 输入类型
// =============================================================================

/// 创建项目
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectInput {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

/// 更新项目（None = 不修改）
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i64>,
    #[serde(default)]
    pub is_favorite: Option<bool>,
    #[serde(default)]
    pub is_archived: Option<bool>,
}

/// 创建分类
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCategoryInput {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

/// 更新分类
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCategoryInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i64>,
}

/// 创建标签
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTagInput {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

/// 更新标签
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTagInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub sort_order: Option<i64>,
}

/// 合并请求：把 source 合并进 target，source 随后被删除
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeInput {
    /// 被合并掉的源 ID 列表
    pub source_ids: Vec<String>,
    /// 保留的目标 ID
    pub target_id: String,
}

// =============================================================================
// 校验工具
// =============================================================================

/// 校验名称：非空、去首尾空白、限长
fn validate_name(raw: &str) -> AppResult<String> {
    let n = raw.trim();
    if n.is_empty() {
        return Err(AppError::validation("名称不能为空"));
    }
    if n.chars().count() > NAME_MAX {
        return Err(AppError::validation(format!(
            "名称过长（{} 字符），上限 {NAME_MAX} 字符",
            n.chars().count()
        )));
    }
    Ok(n.to_string())
}

/// 校验颜色：只接受 #RGB / #RRGGBB，避免把任意字符串写进样式
fn validate_color(raw: Option<&String>) -> AppResult<Option<String>> {
    match raw {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => {
            let t = s.trim();
            let ok = (t.len() == 4 || t.len() == 7)
                && t.starts_with('#')
                && t[1..].chars().all(|c| c.is_ascii_hexdigit());
            if ok {
                Ok(Some(t.to_lowercase()))
            } else {
                Err(AppError::validation(format!("颜色格式不正确：{t}"))
                    .with_hint("请使用 #RGB 或 #RRGGBB 形式，例如 #4F46E5"))
            }
        }
    }
}

/// 校验描述
fn validate_desc(raw: Option<&String>) -> AppResult<Option<String>> {
    match raw {
        None => Ok(None),
        Some(s) => {
            if s.chars().count() > DESC_MAX {
                return Err(AppError::validation(format!(
                    "描述过长（{} 字符），上限 {DESC_MAX} 字符",
                    s.chars().count()
                )));
            }
            Ok(Some(s.clone()))
        }
    }
}

/// 取下一个排序值
async fn next_sort_order(state: &AppState, table: &str) -> AppResult<i64> {
    let sql = match table {
        "projects" => "SELECT COALESCE(MAX(sort_order), 0) + 1 AS n FROM projects",
        "categories" => "SELECT COALESCE(MAX(sort_order), 0) + 1 AS n FROM categories",
        "tags" => "SELECT COALESCE(MAX(sort_order), 0) + 1 AS n FROM tags",
        _ => return Err(AppError::internal("内部错误：非法表名")),
    };
    let row = sqlx::query(sql).fetch_one(state.db.pool()).await?;
    Ok(row.try_get::<i64, _>("n")?)
}

/// 名称冲突时给出可操作的提示（§10 可理解的错误）
fn conflict_err(kind: &str, name: &str) -> AppError {
    AppError::conflict(format!("已存在同名{kind}：{name}"))
        .with_hint("请换一个名称，或先合并/重命名已有的同名项")
}

// =============================================================================
// 项目
// =============================================================================

/// 列出项目（可选包含已归档）
#[tauri::command]
pub async fn project_list(
    state: State<'_, AppState>,
    include_archived: bool,
) -> AppResult<Vec<ProjectWithCount>> {
    let db = &state.db;
    let mut b = QueryBuilder::<Sqlite>::new(
        "SELECT p.*,
            (SELECT COUNT(*) FROM tasks t
               WHERE t.project_id = p.id AND t.deleted_at IS NULL
                 AND t.status NOT IN ('done', 'archived')) AS open_count,
            (SELECT COUNT(*) FROM tasks t
               WHERE t.project_id = p.id AND t.deleted_at IS NULL) AS total_count
         FROM projects p
         WHERE p.deleted_at IS NULL",
    );
    if !include_archived {
        b.push(" AND p.is_archived = 0");
    }
    // 收藏优先，然后按手动排序
    b.push(" ORDER BY p.is_favorite DESC, p.sort_order ASC, p.name ASC");

    let rows = b.build().fetch_all(db.pool()).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(ProjectWithCount {
            project: Project::from_row(&r)?,
            open_count: r.try_get("open_count")?,
            total_count: r.try_get("total_count")?,
        });
    }
    Ok(out)
}

/// 创建项目
#[tauri::command]
pub async fn project_create(
    state: State<'_, AppState>,
    input: CreateProjectInput,
) -> AppResult<Project> {
    let name = validate_name(&input.name)?;
    let color = validate_color(input.color.as_ref())?;
    let desc = validate_desc(input.description.as_ref())?.unwrap_or_default();
    let now = to_db_time(utc_now());
    let id = uuid::Uuid::now_v7().to_string();
    let sort_order = next_sort_order(&state, "projects").await?;

    let r = sqlx::query(
        "INSERT INTO projects (id, name, description, color, icon, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&desc)
    .bind(&color)
    .bind(input.icon.as_deref().filter(|s| !s.trim().is_empty()))
    .bind(sort_order)
    .bind(&now)
    .execute(state.db.pool())
    .await;

    if let Err(e) = r {
        // 命中部分唯一索引时转成可读提示
        if is_unique_violation(&e) {
            return Err(conflict_err("项目", &name));
        }
        return Err(e.into());
    }

    get_project(&state, &id).await
}

/// 读取单个项目
async fn get_project(state: &AppState, id: &str) -> AppResult<Project> {
    get_project_row(&state.db, id).await
}

/// 按 ID 读项目（带 `deleted_at`，更新前要判断是否在回收站里）
async fn get_project_row(db: &Db, id: &str) -> AppResult<Project> {
    let p = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = ?1")
        .bind(id)
        .fetch_optional(db.pool())
        .await?;
    p.ok_or_else(|| AppError::not_found("项目", id))
}

/// 更新项目
#[tauri::command]
pub async fn project_update(
    state: State<'_, AppState>,
    id: String,
    input: UpdateProjectInput,
) -> AppResult<Project> {
    update_project_impl(&state.db, &id, input).await
}

/// 更新项目的实现（与 Tauri 解耦，便于集成测试直接调用）。
pub async fn update_project_impl(
    db: &Db,
    id: &str,
    input: UpdateProjectInput,
) -> AppResult<Project> {
    let existing = get_project_row(db, id).await?;
    if existing.deleted_at.is_some() {
        return Err(AppError::conflict("项目已在回收站中，请先恢复"));
    }

    let name = match &input.name {
        Some(n) => Some(validate_name(n)?),
        None => None,
    };
    let color = validate_color(input.color.as_ref())?;
    let desc = validate_desc(input.description.as_ref())?;
    let now = to_db_time(utc_now());

    let mut b = QueryBuilder::<Sqlite>::new("UPDATE projects SET ");
    let mut sep = b.separated(", ");
    if let Some(n) = &name {
        sep.push("name = ").push_bind_unseparated(n.clone());
    }
    if let Some(d) = &desc {
        sep.push("description = ").push_bind_unseparated(d.clone());
    }
    if input.color.is_some() {
        sep.push("color = ").push_bind_unseparated(color.clone());
    }
    if let Some(i) = input.icon.as_deref() {
        sep.push("icon = ")
            .push_bind_unseparated(if i.trim().is_empty() {
                None
            } else {
                Some(i.to_string())
            });
    }
    if let Some(o) = input.sort_order {
        sep.push("sort_order = ").push_bind_unseparated(o);
    }
    if let Some(f) = input.is_favorite {
        sep.push("is_favorite = ").push_bind_unseparated(f as i64);
    }
    if let Some(a) = input.is_archived {
        sep.push("is_archived = ").push_bind_unseparated(a as i64);
        // 归档时间与归档标志必须同时维护，否则"归档于何时"会丢失
        sep.push("archived_at = ")
            .push_bind_unseparated(if a { Some(now.clone()) } else { None });
    }
    sep.push("updated_at = ").push_bind_unseparated(now);
    b.push(" WHERE id = ").push_bind(id);

    let r = b.build().execute(db.pool()).await;
    if let Err(e) = r {
        if is_unique_violation(&e) {
            return Err(conflict_err("项目", name.as_deref().unwrap_or("")));
        }
        return Err(e.into());
    }
    get_project_row(db, id).await
}

/// 归档 / 取消归档项目
#[tauri::command]
pub async fn project_set_archived(
    state: State<'_, AppState>,
    id: String,
    archived: bool,
) -> AppResult<Project> {
    get_project(&state, &id).await?;
    let now = to_db_time(utc_now());
    sqlx::query(
        "UPDATE projects SET is_archived = ?1, archived_at = ?2, updated_at = ?3 WHERE id = ?4",
    )
    .bind(archived as i64)
    .bind(if archived { Some(now.clone()) } else { None })
    .bind(&now)
    .bind(&id)
    .execute(state.db.pool())
    .await?;
    get_project(&state, &id).await
}

/// 删除影响预览：让用户在确认前看到"会发生什么"（§3、§4.2）
#[tauri::command]
pub async fn project_delete_impact(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<DeleteImpact> {
    get_project(&state, &id).await?;
    impact_for(&state, "project_id", &id).await
}

/// 计算某个"归属字段"下的任务影响面
async fn impact_for(state: &AppState, column: &str, id: &str) -> AppResult<DeleteImpact> {
    let sql = match column {
        "project_id" => {
            "SELECT
                COUNT(*) FILTER (WHERE status NOT IN ('done','archived')) AS open_n,
                COUNT(*) FILTER (WHERE status IN ('done','archived')) AS done_n
             FROM tasks WHERE deleted_at IS NULL AND project_id = ?1"
        }
        "category_id" => {
            "SELECT
                COUNT(*) FILTER (WHERE status NOT IN ('done','archived')) AS open_n,
                COUNT(*) FILTER (WHERE status IN ('done','archived')) AS done_n
             FROM tasks WHERE deleted_at IS NULL AND category_id = ?1"
        }
        _ => return Err(AppError::internal("内部错误：非法字段")),
    };
    let row = sqlx::query(sql).bind(id).fetch_one(state.db.pool()).await?;
    Ok(DeleteImpact {
        affected_tasks: row.try_get("open_n")?,
        completed_tasks: row.try_get("done_n")?,
        related_records: 0,
    })
}

/// 删除项目，并按策略处理关联任务
#[tauri::command]
pub async fn project_delete(
    state: State<'_, AppState>,
    id: String,
    strategy: OrphanStrategy,
) -> AppResult<i64> {
    get_project(&state, &id).await?;
    let now = to_db_time(utc_now());
    let mut tx = state.db.pool().begin().await?;

    // 两种策略都必须给出明确的影响面数字，供 UI 反馈（§4.2）
    let affected: i64 = match strategy {
        // 只解除归属，任务本身完整保留——默认且最安全的做法
        OrphanStrategy::Detach => sqlx::query(
            "UPDATE tasks SET project_id = NULL, updated_at = ?1
             WHERE project_id = ?2 AND deleted_at IS NULL",
        )
        .bind(&now)
        .bind(&id)
        .execute(&mut *tx)
        .await?
        .rows_affected() as i64,

        // 任务进回收站，之后仍可从回收站恢复
        OrphanStrategy::CascadeSoftDelete => sqlx::query(
            "UPDATE tasks SET deleted_at = ?1, updated_at = ?1
             WHERE project_id = ?2 AND deleted_at IS NULL",
        )
        .bind(&now)
        .bind(&id)
        .execute(&mut *tx)
        .await?
        .rows_affected() as i64,
    };

    // 项目本身软删除（保留记录以便"撤销"与审计）
    sqlx::query("UPDATE projects SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(&id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(affected)
}

/// 合并项目：把源项目的任务全部转到目标项目，然后删除源项目
#[tauri::command]
pub async fn project_merge(state: State<'_, AppState>, input: MergeInput) -> AppResult<i64> {
    merge_project_impl(&state.db, &input).await
}

/// 合并项目的实现（与 Tauri 解耦，便于集成测试直接调用）。
pub async fn merge_project_impl(db: &Db, input: &MergeInput) -> AppResult<i64> {
    if input.source_ids.is_empty() {
        return Err(AppError::validation("请选择要合并的项目"));
    }
    if input.source_ids.iter().any(|s| s == &input.target_id) {
        return Err(AppError::validation("目标项目不能同时作为被合并项"));
    }
    ensure_org_row_exists(db, "projects", "项目", &input.target_id).await?;

    let now = to_db_time(utc_now());
    let mut tx = db.pool().begin().await?;
    let mut moved = 0i64;

    for src in &input.source_ids {
        ensure_org_row_exists(db, "projects", "项目", src).await?;
        moved += sqlx::query(
            "UPDATE tasks SET project_id = ?1, updated_at = ?2
             WHERE project_id = ?3 AND deleted_at IS NULL",
        )
        .bind(&input.target_id)
        .bind(&now)
        .bind(src)
        .execute(&mut *tx)
        .await?
        .rows_affected() as i64;

        sqlx::query("UPDATE projects SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2")
            .bind(&now)
            .bind(src)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(moved)
}

// =============================================================================
// 分类
// =============================================================================

/// 列出分类
#[tauri::command]
pub async fn category_list(state: State<'_, AppState>) -> AppResult<Vec<Category>> {
    let rows = sqlx::query_as::<_, Category>(
        "SELECT * FROM categories WHERE deleted_at IS NULL ORDER BY sort_order ASC, name ASC",
    )
    .fetch_all(state.db.pool())
    .await?;
    Ok(rows)
}

/// 创建分类
#[tauri::command]
pub async fn category_create(
    state: State<'_, AppState>,
    input: CreateCategoryInput,
) -> AppResult<Category> {
    let name = validate_name(&input.name)?;
    let color = validate_color(input.color.as_ref())?;
    let desc = validate_desc(input.description.as_ref())?.unwrap_or_default();
    let now = to_db_time(utc_now());
    let id = uuid::Uuid::now_v7().to_string();
    let sort_order = next_sort_order(&state, "categories").await?;

    let r = sqlx::query(
        "INSERT INTO categories (id, name, description, color, icon, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&desc)
    .bind(&color)
    .bind(input.icon.as_deref().filter(|s| !s.trim().is_empty()))
    .bind(sort_order)
    .bind(&now)
    .execute(state.db.pool())
    .await;
    if let Err(e) = r {
        if is_unique_violation(&e) {
            return Err(conflict_err("分类", &name));
        }
        return Err(e.into());
    }
    get_category(&state, &id).await
}

async fn get_category(state: &AppState, id: &str) -> AppResult<Category> {
    get_category_row(&state.db, id).await
}

/// 按 ID 读分类（不经过 `State`，供实现函数与测试复用）
async fn get_category_row(db: &Db, id: &str) -> AppResult<Category> {
    let c = sqlx::query_as::<_, Category>("SELECT * FROM categories WHERE id = ?1")
        .bind(id)
        .fetch_optional(db.pool())
        .await?;
    c.ok_or_else(|| AppError::not_found("分类", id))
}

/// 更新分类
#[tauri::command]
pub async fn category_update(
    state: State<'_, AppState>,
    id: String,
    input: UpdateCategoryInput,
) -> AppResult<Category> {
    update_category_impl(&state.db, &id, input).await
}

/// 更新分类的实现（与 Tauri 解耦，便于集成测试直接调用）。
pub async fn update_category_impl(
    db: &Db,
    id: &str,
    input: UpdateCategoryInput,
) -> AppResult<Category> {
    get_category_row(db, id).await?;
    let name = match &input.name {
        Some(n) => Some(validate_name(n)?),
        None => None,
    };
    let color = validate_color(input.color.as_ref())?;
    let desc = validate_desc(input.description.as_ref())?;
    let now = to_db_time(utc_now());

    let mut b = QueryBuilder::<Sqlite>::new("UPDATE categories SET ");
    let mut sep = b.separated(", ");
    if let Some(n) = &name {
        sep.push("name = ").push_bind_unseparated(n.clone());
    }
    if let Some(d) = &desc {
        sep.push("description = ").push_bind_unseparated(d.clone());
    }
    if input.color.is_some() {
        sep.push("color = ").push_bind_unseparated(color.clone());
    }
    if let Some(i) = input.icon.as_deref() {
        sep.push("icon = ")
            .push_bind_unseparated(if i.trim().is_empty() {
                None
            } else {
                Some(i.to_string())
            });
    }
    if let Some(o) = input.sort_order {
        sep.push("sort_order = ").push_bind_unseparated(o);
    }
    sep.push("updated_at = ").push_bind_unseparated(now);
    b.push(" WHERE id = ").push_bind(id);

    let r = b.build().execute(db.pool()).await;
    if let Err(e) = r {
        if is_unique_violation(&e) {
            return Err(conflict_err("分类", name.as_deref().unwrap_or("")));
        }
        return Err(e.into());
    }
    get_category_row(db, id).await
}

/// 分类删除影响预览
#[tauri::command]
pub async fn category_delete_impact(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<DeleteImpact> {
    get_category(&state, &id).await?;
    impact_for(&state, "category_id", &id).await
}

/// 删除分类
#[tauri::command]
pub async fn category_delete(
    state: State<'_, AppState>,
    id: String,
    strategy: OrphanStrategy,
) -> AppResult<i64> {
    get_category(&state, &id).await?;
    let now = to_db_time(utc_now());
    let mut tx = state.db.pool().begin().await?;

    let affected = match strategy {
        OrphanStrategy::Detach => sqlx::query(
            "UPDATE tasks SET category_id = NULL, updated_at = ?1
             WHERE category_id = ?2 AND deleted_at IS NULL",
        )
        .bind(&now)
        .bind(&id)
        .execute(&mut *tx)
        .await?
        .rows_affected() as i64,
        OrphanStrategy::CascadeSoftDelete => sqlx::query(
            "UPDATE tasks SET deleted_at = ?1, updated_at = ?1
             WHERE category_id = ?2 AND deleted_at IS NULL",
        )
        .bind(&now)
        .bind(&id)
        .execute(&mut *tx)
        .await?
        .rows_affected() as i64,
    };

    sqlx::query("UPDATE categories SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(affected)
}

/// 合并分类：把源分类的任务全部转到目标分类，然后删除源分类。
///
/// 与 `project_merge` 同一套语义：只改归属，不改任务本身，
/// 也不碰已删除的任务（回收站里的记录保留它当时的分类）。
#[tauri::command]
pub async fn category_merge(state: State<'_, AppState>, input: MergeInput) -> AppResult<i64> {
    merge_category_impl(&state.db, &input).await
}

/// 合并分类的实现（与 Tauri 解耦，便于集成测试直接调用）。
pub async fn merge_category_impl(db: &Db, input: &MergeInput) -> AppResult<i64> {
    if input.source_ids.is_empty() {
        return Err(AppError::validation("请选择要合并的分类"));
    }
    if input.source_ids.iter().any(|s| s == &input.target_id) {
        return Err(AppError::validation("目标分类不能同时作为被合并项"));
    }
    ensure_org_row_exists(db, "categories", "分类", &input.target_id).await?;

    let now = to_db_time(utc_now());
    let mut tx = db.pool().begin().await?;
    let mut moved = 0i64;

    for src in &input.source_ids {
        ensure_org_row_exists(db, "categories", "分类", src).await?;
        moved += sqlx::query(
            "UPDATE tasks SET category_id = ?1, updated_at = ?2
             WHERE category_id = ?3 AND deleted_at IS NULL",
        )
        .bind(&input.target_id)
        .bind(&now)
        .bind(src)
        .execute(&mut *tx)
        .await?
        .rows_affected() as i64;

        sqlx::query("UPDATE categories SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2")
            .bind(&now)
            .bind(src)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(moved)
}

/// 确认某个组织项存在且未删除；表名与名称只来自本文件内的字面量。
async fn ensure_org_row_exists(db: &Db, table: &str, label: &str, id: &str) -> AppResult<()> {
    let sql = format!("SELECT COUNT(*) AS n FROM {table} WHERE id = ?1 AND deleted_at IS NULL");
    let n: i64 = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .fetch_one(db.pool())
        .await?
        .try_get("n")?;
    if n == 0 {
        return Err(AppError::not_found(label, id));
    }
    Ok(())
}

// =============================================================================
// 标签
// =============================================================================

/// 列出标签（带任务计数，便于用户清理无用标签）
#[tauri::command]
pub async fn tag_list(state: State<'_, AppState>) -> AppResult<Vec<TagWithCount>> {
    let rows = sqlx::query(
        "SELECT t.*,
            (SELECT COUNT(*) FROM task_tags tt
               JOIN tasks k ON k.id = tt.task_id
               WHERE tt.tag_id = t.id AND k.deleted_at IS NULL) AS task_count
         FROM tags t
         WHERE t.deleted_at IS NULL
         ORDER BY t.sort_order ASC, t.name ASC",
    )
    .fetch_all(state.db.pool())
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(TagWithCount {
            tag: Tag::from_row(&r)?,
            task_count: r.try_get("task_count")?,
        });
    }
    Ok(out)
}

/// 创建标签
#[tauri::command]
pub async fn tag_create(state: State<'_, AppState>, input: CreateTagInput) -> AppResult<Tag> {
    let name = validate_name(&input.name)?;
    let color = validate_color(input.color.as_ref())?;
    let now = to_db_time(utc_now());
    let id = uuid::Uuid::now_v7().to_string();
    let sort_order = next_sort_order(&state, "tags").await?;

    let r = sqlx::query(
        "INSERT INTO tags (id, name, color, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&color)
    .bind(sort_order)
    .bind(&now)
    .execute(state.db.pool())
    .await;
    if let Err(e) = r {
        if is_unique_violation(&e) {
            return Err(conflict_err("标签", &name));
        }
        return Err(e.into());
    }
    get_tag(&state, &id).await
}

async fn get_tag(state: &AppState, id: &str) -> AppResult<Tag> {
    let t = sqlx::query_as::<_, Tag>("SELECT * FROM tags WHERE id = ?1")
        .bind(id)
        .fetch_optional(state.db.pool())
        .await?;
    t.ok_or_else(|| AppError::not_found("标签", id))
}

/// 更新标签
#[tauri::command]
pub async fn tag_update(
    state: State<'_, AppState>,
    id: String,
    input: UpdateTagInput,
) -> AppResult<Tag> {
    update_tag_impl(&state.db, &id, input).await
}

/// 更新标签的实现（与 Tauri 解耦，便于集成测试直接调用）。
pub async fn update_tag_impl(db: &Db, id: &str, input: UpdateTagInput) -> AppResult<Tag> {
    get_tag_by_id(db, id).await?;
    let name = match &input.name {
        Some(n) => Some(validate_name(n)?),
        None => None,
    };
    let color = validate_color(input.color.as_ref())?;
    let now = to_db_time(utc_now());

    let mut b = QueryBuilder::<Sqlite>::new("UPDATE tags SET ");
    let mut sep = b.separated(", ");
    if let Some(n) = &name {
        sep.push("name = ").push_bind_unseparated(n.clone());
    }
    if input.color.is_some() {
        sep.push("color = ").push_bind_unseparated(color.clone());
    }
    if let Some(o) = input.sort_order {
        sep.push("sort_order = ").push_bind_unseparated(o);
    }
    sep.push("updated_at = ").push_bind_unseparated(now);
    b.push(" WHERE id = ").push_bind(id);

    let r = b.build().execute(db.pool()).await;
    if let Err(e) = r {
        if is_unique_violation(&e) {
            return Err(conflict_err("标签", name.as_deref().unwrap_or("")));
        }
        return Err(e.into());
    }
    get_tag_by_id(db, id).await
}

/// 按 ID 读标签（不经过 `State`，供实现函数与测试复用）
async fn get_tag_by_id(db: &Db, id: &str) -> AppResult<Tag> {
    let t = sqlx::query_as::<_, Tag>("SELECT * FROM tags WHERE id = ?1")
        .bind(id)
        .fetch_optional(db.pool())
        .await?;
    t.ok_or_else(|| AppError::not_found("标签", id))
}

/// 删除标签：只解除任务关联，绝不删除任务本身
#[tauri::command]
pub async fn tag_delete(state: State<'_, AppState>, id: String) -> AppResult<i64> {
    get_tag(&state, &id).await?;
    let now = to_db_time(utc_now());
    let mut tx = state.db.pool().begin().await?;

    // 先统计有多少任务在用这个标签，用于反馈给用户
    let used: i64 = sqlx::query("SELECT COUNT(*) AS n FROM task_tags WHERE tag_id = ?1")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?
        .try_get("n")?;

    sqlx::query("DELETE FROM task_tags WHERE tag_id = ?1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE tags SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2")
        .bind(&now)
        .bind(&id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(used)
}

/// 合并标签：把源标签的关联转到目标标签，并处理重复关联
#[tauri::command]
pub async fn tag_merge(state: State<'_, AppState>, input: MergeInput) -> AppResult<i64> {
    merge_tag_impl(&state.db, &input).await
}

/// 合并标签的实现（与 Tauri 解耦，便于集成测试直接调用）。
pub async fn merge_tag_impl(db: &Db, input: &MergeInput) -> AppResult<i64> {
    if input.source_ids.is_empty() {
        return Err(AppError::validation("请选择要合并的标签"));
    }
    if input.source_ids.iter().any(|s| s == &input.target_id) {
        return Err(AppError::validation("目标标签不能同时作为被合并项"));
    }
    ensure_org_row_exists(db, "tags", "标签", &input.target_id).await?;

    let now = to_db_time(utc_now());
    let mut tx = db.pool().begin().await?;
    let mut moved = 0i64;

    for src in &input.source_ids {
        ensure_org_row_exists(db, "tags", "标签", src).await?;
        // INSERT OR IGNORE：任务若已同时拥有目标标签，不会产生重复关联
        moved += sqlx::query(
            "INSERT OR IGNORE INTO task_tags (task_id, tag_id)
             SELECT task_id, ?1 FROM task_tags WHERE tag_id = ?2",
        )
        .bind(&input.target_id)
        .bind(src)
        .execute(&mut *tx)
        .await?
        .rows_affected() as i64;

        sqlx::query("DELETE FROM task_tags WHERE tag_id = ?1")
            .bind(src)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE tags SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2")
            .bind(&now)
            .bind(src)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(moved)
}

/// 读取某任务的标签（前端表单需要）
#[tauri::command]
pub async fn task_tags_get(state: State<'_, AppState>, task_id: String) -> AppResult<Vec<Tag>> {
    let rows = sqlx::query_as::<_, Tag>(
        "SELECT t.* FROM tags t
         JOIN task_tags tt ON tt.tag_id = t.id
         WHERE tt.task_id = ?1 AND t.deleted_at IS NULL
         ORDER BY t.sort_order ASC, t.name ASC",
    )
    .bind(&task_id)
    .fetch_all(state.db.pool())
    .await?;
    Ok(rows)
}

/// 设置某任务的标签集合（整体替换，语义明确且便于撤销）
#[tauri::command]
pub async fn task_tags_set(
    state: State<'_, AppState>,
    task_id: String,
    tag_ids: Vec<String>,
) -> AppResult<i64> {
    // 确认任务存在
    let exists: i64 = sqlx::query("SELECT COUNT(*) AS n FROM tasks WHERE id = ?1")
        .bind(&task_id)
        .fetch_one(state.db.pool())
        .await?
        .try_get("n")?;
    if exists == 0 {
        return Err(AppError::not_found("任务", &task_id));
    }
    let recurring: Option<String> = sqlx::query_scalar("SELECT series_id FROM tasks WHERE id = ?1")
        .bind(&task_id)
        .fetch_one(state.db.pool())
        .await?;
    if recurring.is_some() {
        return Err(AppError::conflict("重复任务标签请通过范围编辑接口修改"));
    }

    // 逐个校验标签存在，避免写入悬空关联
    for tid in tag_ids.iter().filter(|s| !s.is_empty()) {
        get_tag(&state, tid).await?;
    }

    let mut tx = state.db.pool().begin().await?;
    sqlx::query("DELETE FROM task_tags WHERE task_id = ?1")
        .bind(&task_id)
        .execute(&mut *tx)
        .await?;

    let mut n = 0i64;
    for tid in tag_ids.iter().filter(|s| !s.is_empty()) {
        n += sqlx::query("INSERT OR IGNORE INTO task_tags (task_id, tag_id) VALUES (?1, ?2)")
            .bind(&task_id)
            .bind(tid)
            .execute(&mut *tx)
            .await?
            .rows_affected() as i64;
    }
    tx.commit().await?;
    Ok(n)
}

/// 判断 sqlx 错误是否为唯一约束冲突
fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.message().contains("UNIQUE"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_validation_trims_and_rejects_blank() {
        assert_eq!(validate_name("  工作  ").unwrap(), "工作");
        assert!(validate_name("").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name("\t\n").is_err());
    }

    #[test]
    fn name_length_counts_chars_not_bytes() {
        assert!(validate_name(&"名".repeat(NAME_MAX)).is_ok());
        assert!(validate_name(&"名".repeat(NAME_MAX + 1)).is_err());
    }

    #[test]
    fn color_only_accepts_hex_forms() {
        assert_eq!(
            validate_color(Some(&"#fff".to_string())).unwrap().unwrap(),
            "#fff"
        );
        assert_eq!(
            validate_color(Some(&"#4F46E5".to_string()))
                .unwrap()
                .unwrap(),
            "#4f46e5",
            "应统一为小写，避免同一颜色出现两种写法"
        );
        // 为空视为"未设置"而不是错误
        assert_eq!(validate_color(None).unwrap(), None);
        assert_eq!(validate_color(Some(&String::new())).unwrap(), None);
    }

    /// 颜色会被写进样式，必须拒绝任意字符串（§10 校验所有输入）
    #[test]
    fn color_rejects_injection_attempts() {
        for bad in [
            "red",
            "rgb(1,2,3)",
            "#12345",
            "#gggggg",
            "expression(alert(1))",
            "#fff; background: url(x)",
            "javascript:alert(1)",
        ] {
            assert!(
                validate_color(Some(&bad.to_string())).is_err(),
                "应拒绝非法颜色：{bad}"
            );
        }
    }

    #[test]
    fn description_length_is_bounded() {
        assert!(validate_desc(Some(&"a".repeat(DESC_MAX))).is_ok());
        assert!(validate_desc(Some(&"a".repeat(DESC_MAX + 1))).is_err());
        assert!(validate_desc(None).unwrap().is_none());
    }

    #[test]
    fn conflict_message_is_actionable() {
        let e = conflict_err("项目", "工作");
        assert!(e.message.contains("工作"));
        assert!(e.hint.is_some(), "名称冲突必须给出可操作建议");
    }

    /// 删除策略的两种语义必须稳定（§4.2 要求说明关联任务如何处理）
    #[test]
    fn orphan_strategy_deserializes_from_snake_case() {
        let a: OrphanStrategy = serde_json::from_str("\"detach\"").unwrap();
        let b: OrphanStrategy = serde_json::from_str("\"cascade_soft_delete\"").unwrap();
        assert_eq!(a, OrphanStrategy::Detach);
        assert_eq!(b, OrphanStrategy::CascadeSoftDelete);
        // 未知值必须报错，不能静默按默认处理（否则可能误删任务）
        assert!(serde_json::from_str::<OrphanStrategy>("\"hard_delete\"").is_err());
    }
}
