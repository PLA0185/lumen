//! 子任务与任务依赖（任务书 §4.1）。
//!
//! 两个概念刻意分开建模，避免混淆：
//! - **子任务**：把一件事拆成几步，属于"分解"，有进度概念；
//! - **依赖**：A 必须在 B 之后做，属于"约束"，只存在先后关系。
//!
//! 依赖的**循环检测**在应用层实现（数据库无法表达递归约束）。
//! 这里用 DFS 而非"只查一层"，因为环形依赖可能跨越任意多层：
//! A→B→C→A 这种三层环，只查直接依赖是发现不了的。

use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row, Sqlite};
use tauri::State;

use crate::commands::AppState;
use crate::db::{to_db_time, utc_now};
use crate::error::{AppError, AppResult};

/// 子任务标题上限
const TITLE_MAX: usize = 500;
/// 依赖查询的最大深度，防止异常数据导致无限遍历
const MAX_DEPTH: usize = 64;

// =============================================================================
// 子任务
// =============================================================================

/// 子任务视图
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Subtask {
    pub id: String,
    pub task_id: String,
    pub title: String,
    pub is_done: i64,
    pub sort_order: f64,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 任务的子任务进度（§4.1「进度显示」）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtaskProgress {
    pub task_id: String,
    pub total: i64,
    pub done: i64,
    /// 0–100 的整数百分比；无子任务时为 None（而不是 0，
    /// 因为"没有子任务"与"完成 0%"是两件不同的事）
    pub percent: Option<i64>,
}

/// 创建子任务
#[tauri::command]
pub async fn subtask_create(
    state: State<'_, AppState>,
    task_id: String,
    title: String,
) -> AppResult<Subtask> {
    let t = title.trim();
    if t.is_empty() {
        return Err(AppError::validation("子任务标题不能为空"));
    }
    if t.chars().count() > TITLE_MAX {
        return Err(AppError::validation(format!(
            "子任务标题过长（{} 字符），上限 {TITLE_MAX} 字符",
            t.chars().count()
        )));
    }

    let exists: i64 = sqlx::query("SELECT COUNT(*) AS n FROM tasks WHERE id = ?1 AND deleted_at IS NULL")
        .bind(&task_id)
        .fetch_one(state.db.pool())
        .await?
        .try_get("n")?;
    if exists == 0 {
        return Err(AppError::not_found("任务", &task_id));
    }

    let id = uuid::Uuid::now_v7().to_string();
    let now = to_db_time(utc_now());
    let row = sqlx::query("SELECT COALESCE(MAX(sort_order), 0) + 1 AS n FROM subtasks WHERE task_id = ?1")
        .bind(&task_id)
        .fetch_one(state.db.pool())
        .await?;
    let sort_order: f64 = row.try_get("n")?;

    sqlx::query(
        "INSERT INTO subtasks (id, task_id, title, is_done, sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, 0, ?4, ?5, ?5)",
    )
    .bind(&id)
    .bind(&task_id)
    .bind(t)
    .bind(sort_order)
    .bind(&now)
    .execute(state.db.pool())
    .await?;

    get_subtask(&state, &id).await
}

async fn get_subtask(state: &AppState, id: &str) -> AppResult<Subtask> {
    let s = sqlx::query_as::<_, Subtask>("SELECT * FROM subtasks WHERE id = ?1")
        .bind(id)
        .fetch_optional(state.db.pool())
        .await?;
    s.ok_or_else(|| AppError::not_found("子任务", id))
}

/// 列出某任务的子任务
#[tauri::command]
pub async fn subtask_list(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<Vec<Subtask>> {
    let rows = sqlx::query_as::<_, Subtask>(
        "SELECT * FROM subtasks WHERE task_id = ?1 ORDER BY sort_order ASC, created_at ASC",
    )
    .bind(&task_id)
    .fetch_all(state.db.pool())
    .await?;
    Ok(rows)
}

/// 更新子任务（标题 / 完成状态 / 排序）
#[tauri::command]
pub async fn subtask_update(
    state: State<'_, AppState>,
    id: String,
    title: Option<String>,
    is_done: Option<bool>,
    sort_order: Option<f64>,
) -> AppResult<Subtask> {
    get_subtask(&state, &id).await?;

    let new_title = match &title {
        Some(t) => {
            let v = t.trim();
            if v.is_empty() {
                return Err(AppError::validation("子任务标题不能为空"));
            }
            if v.chars().count() > TITLE_MAX {
                return Err(AppError::validation(format!(
                    "子任务标题过长（{} 字符），上限 {TITLE_MAX} 字符",
                    v.chars().count()
                )));
            }
            Some(v.to_string())
        }
        None => None,
    };

    let now = to_db_time(utc_now());
    let mut b = QueryBuilder::<Sqlite>::new("UPDATE subtasks SET ");
    let mut sep = b.separated(", ");
    if let Some(t) = &new_title {
        sep.push("title = ").push_bind(t.clone());
    }
    if let Some(d) = is_done {
        sep.push("is_done = ").push_bind(d as i64);
        // 完成时间必须真实记录，与主任务同一规则（§4.1）
        sep.push("completed_at = ").push_bind(if d { Some(now.clone()) } else { None });
    }
    if let Some(o) = sort_order {
        sep.push("sort_order = ").push_bind(o);
    }
    sep.push("updated_at = ").push_bind(now);
    b.push(" WHERE id = ").push_bind(&id);

    b.build().execute(state.db.pool()).await?;
    get_subtask(&state, &id).await
}

/// 删除子任务
#[tauri::command]
pub async fn subtask_delete(state: State<'_, AppState>, id: String) -> AppResult<i64> {
    get_subtask(&state, &id).await?;
    let n = sqlx::query("DELETE FROM subtasks WHERE id = ?1")
        .bind(&id)
        .execute(state.db.pool())
        .await?
        .rows_affected() as i64;
    Ok(n)
}

/// 查询某任务的子任务进度
#[tauri::command]
pub async fn subtask_progress(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<SubtaskProgress> {
    let row = sqlx::query(
        "SELECT COUNT(*) AS total, COALESCE(SUM(is_done), 0) AS done
         FROM subtasks WHERE task_id = ?1",
    )
    .bind(&task_id)
    .fetch_one(state.db.pool())
    .await?;
    let total: i64 = row.try_get("total")?;
    let done: i64 = row.try_get("done")?;
    Ok(SubtaskProgress {
        task_id,
        total,
        done,
        percent: if total > 0 {
            Some((done * 100) / total)
        } else {
            None
        },
    })
}

/// 批量查询多个任务的子任务进度（列表页一次拿全，避免 N+1 查询）
#[tauri::command]
pub async fn subtask_progress_batch(
    state: State<'_, AppState>,
    task_ids: Vec<String>,
) -> AppResult<Vec<SubtaskProgress>> {
    if task_ids.is_empty() {
        return Ok(Vec::new());
    }
    if task_ids.len() > 2000 {
        return Err(AppError::validation("单次查询最多 2000 个任务"));
    }

    let mut b = QueryBuilder::<Sqlite>::new(
        "SELECT task_id, COUNT(*) AS total, COALESCE(SUM(is_done), 0) AS done
         FROM subtasks WHERE task_id IN (",
    );
    let mut sep = b.separated(", ");
    for id in &task_ids {
        sep.push_bind(id.clone());
    }
    sep.push_unseparated(") GROUP BY task_id");

    let rows = b.build().fetch_all(state.db.pool()).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let total: i64 = r.try_get("total")?;
        let done: i64 = r.try_get("done")?;
        out.push(SubtaskProgress {
            task_id: r.try_get("task_id")?,
            total,
            done,
            percent: if total > 0 {
                Some((done * 100) / total)
            } else {
                None
            },
        });
    }
    Ok(out)
}

// =============================================================================
// 任务依赖
// =============================================================================

/// 依赖项（含被依赖任务的摘要，界面直接可用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyItem {
    /// 前置任务的 ID
    pub depends_on_id: String,
    pub title: String,
    pub status: String,
    pub due_at: Option<String>,
    /// 前置任务是否已完成（界面据此提示"可开始"或"被阻塞"）
    pub is_done: bool,
}

/// 添加依赖
///
/// 会做三层校验：自依赖、目标存在、**循环依赖**。
#[tauri::command]
pub async fn dependency_add(
    state: State<'_, AppState>,
    task_id: String,
    depends_on_id: String,
) -> AppResult<i64> {
    if task_id == depends_on_id {
        return Err(AppError::validation("任务不能依赖自己"));
    }

    // 两个任务都必须存在且未删除
    for (id, label) in [(&task_id, "任务"), (&depends_on_id, "被依赖的任务")] {
        let n: i64 =
            sqlx::query("SELECT COUNT(*) AS n FROM tasks WHERE id = ?1 AND deleted_at IS NULL")
                .bind(id)
                .fetch_one(state.db.pool())
                .await?
                .try_get("n")?;
        if n == 0 {
            return Err(AppError::not_found(label, id));
        }
    }

    // 循环检测：若 depends_on_id 已经（直接或间接）依赖 task_id，
    // 那么再加入 task_id → depends_on_id 就会成环。
    if would_create_cycle(&state, &task_id, &depends_on_id).await? {
        return Err(AppError::conflict("该依赖会造成循环依赖，已阻止").with_hint(
            "例如 A 依赖 B，同时 B 又依赖 A。请先解除其中一条依赖关系再试",
        ));
    }

    let now = to_db_time(utc_now());
    let n = sqlx::query(
        "INSERT OR IGNORE INTO task_dependencies (task_id, depends_on_id, created_at)
         VALUES (?1, ?2, ?3)",
    )
    .bind(&task_id)
    .bind(&depends_on_id)
    .bind(&now)
    .execute(state.db.pool())
    .await?
    .rows_affected() as i64;

    Ok(n)
}

/// 判断加入 `task_id → depends_on_id` 后是否形成环。
///
/// 思路：从 `depends_on_id` 出发沿"我依赖谁"的边做 DFS，
/// 若能走到 `task_id`，说明已经存在 `depends_on_id ⇒ task_id` 的路径，
/// 再加反向边即构成环。
async fn would_create_cycle(
    state: &AppState,
    task_id: &str,
    depends_on_id: &str,
) -> AppResult<bool> {
    // 一次性把全部依赖边读进内存后遍历，避免递归中反复查库。
    // 任务依赖图的规模远小于任务总数，这样做的代价可接受且逻辑更清晰。
    let rows = sqlx::query("SELECT task_id, depends_on_id FROM task_dependencies")
        .fetch_all(state.db.pool())
        .await?;

    let mut edges: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for r in rows {
        let from: String = r.try_get("task_id")?;
        let to: String = r.try_get("depends_on_id")?;
        edges.entry(from).or_default().push(to);
    }

    // 从 depends_on_id 出发，看能否到达 task_id
    let mut stack = vec![depends_on_id.to_string()];
    let mut visited = std::collections::HashSet::new();
    let mut steps = 0usize;

    while let Some(cur) = stack.pop() {
        if cur == task_id {
            return Ok(true);
        }
        if !visited.insert(cur.clone()) {
            continue;
        }
        steps += 1;
        if steps > MAX_DEPTH * MAX_DEPTH {
            // 数据异常（超大图）时保守拒绝，而不是让请求卡死
            return Err(AppError::conflict("依赖关系过于复杂，已阻止本次操作")
                .with_hint("请联系支持并检查是否存在异常数据"));
        }
        if let Some(next) = edges.get(&cur) {
            for n in next {
                stack.push(n.clone());
            }
        }
    }
    Ok(false)
}

/// 列出某任务的前置依赖（"我必须等谁"）
#[tauri::command]
pub async fn dependency_list(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<Vec<DependencyItem>> {
    let rows = sqlx::query(
        "SELECT d.depends_on_id AS id, t.title, t.status, t.due_at
         FROM task_dependencies d
         JOIN tasks t ON t.id = d.depends_on_id
         WHERE d.task_id = ?1 AND t.deleted_at IS NULL
         ORDER BY t.due_at IS NULL, t.due_at ASC, t.title ASC",
    )
    .bind(&task_id)
    .fetch_all(state.db.pool())
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let status: String = r.try_get("status")?;
        out.push(DependencyItem {
            depends_on_id: r.try_get("id")?,
            title: r.try_get("title")?,
            is_done: status == "done" || status == "archived",
            status,
            due_at: r.try_get("due_at")?,
        });
    }
    Ok(out)
}

/// 列出"谁在等我"（反向依赖，帮助用户判断优先级）
#[tauri::command]
pub async fn dependency_dependents(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<Vec<DependencyItem>> {
    let rows = sqlx::query(
        "SELECT d.task_id AS id, t.title, t.status, t.due_at
         FROM task_dependencies d
         JOIN tasks t ON t.id = d.task_id
         WHERE d.depends_on_id = ?1 AND t.deleted_at IS NULL
         ORDER BY t.due_at IS NULL, t.due_at ASC",
    )
    .bind(&task_id)
    .fetch_all(state.db.pool())
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let status: String = r.try_get("status")?;
        out.push(DependencyItem {
            depends_on_id: r.try_get("id")?,
            title: r.try_get("title")?,
            is_done: status == "done" || status == "archived",
            status,
            due_at: r.try_get("due_at")?,
        });
    }
    Ok(out)
}

/// 删除依赖
#[tauri::command]
pub async fn dependency_remove(
    state: State<'_, AppState>,
    task_id: String,
    depends_on_id: String,
) -> AppResult<i64> {
    let n = sqlx::query("DELETE FROM task_dependencies WHERE task_id = ?1 AND depends_on_id = ?2")
        .bind(&task_id)
        .bind(&depends_on_id)
        .execute(state.db.pool())
        .await?
        .rows_affected() as i64;
    if n == 0 {
        return Err(AppError::not_found(
            "依赖关系",
            &format!("{task_id} → {depends_on_id}"),
        ));
    }
    Ok(n)
}

/// 检查某任务是否被未完成的前置任务阻塞
#[tauri::command]
pub async fn dependency_is_blocked(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<bool> {
    let row = sqlx::query(
        "SELECT COUNT(*) AS n
         FROM task_dependencies d
         JOIN tasks t ON t.id = d.depends_on_id
         WHERE d.task_id = ?1
           AND t.deleted_at IS NULL
           AND t.status NOT IN ('done', 'archived')",
    )
    .bind(&task_id)
    .fetch_one(state.db.pool())
    .await?;
    Ok(row.try_get::<i64, _>("n")? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用内存中的边集复现循环检测的核心算法，
    /// 使算法本身可被单元测试覆盖（真实函数需要数据库）。
    fn has_path(
        edges: &std::collections::HashMap<String, Vec<String>>,
        from: &str,
        to: &str,
    ) -> bool {
        let mut stack = vec![from.to_string()];
        let mut visited = std::collections::HashSet::new();
        while let Some(cur) = stack.pop() {
            if cur == to {
                return true;
            }
            if !visited.insert(cur.clone()) {
                continue;
            }
            if let Some(next) = edges.get(&cur) {
                for n in next {
                    stack.push(n.clone());
                }
            }
        }
        false
    }

    fn graph(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, Vec<String>> {
        let mut m: std::collections::HashMap<String, Vec<String>> = Default::default();
        for (a, b) in pairs {
            m.entry(a.to_string()).or_default().push(b.to_string());
        }
        m
    }

    #[test]
    fn self_cycle_is_detected() {
        // A 依赖 A：从 A 出发必然到达 A
        let g = graph(&[("A", "A")]);
        assert!(has_path(&g, "A", "A"));
    }

    #[test]
    fn two_node_cycle_is_detected() {
        // A→B（A 依赖 B）。若再加 B→A，则应检测到环：
        // 从 A 出发能到达 B，说明已存在 A⇒B 路径。
        let g = graph(&[("A", "B")]);
        assert!(has_path(&g, "A", "B"), "应发现已有的 A⇒B 路径");
        assert!(!has_path(&g, "B", "A"), "反向此时尚不存在");
    }

    /// 关键用例：三层环只查直接依赖是发现不了的
    #[test]
    fn three_level_cycle_is_detected() {
        let g = graph(&[("A", "B"), ("B", "C"), ("C", "A")]);
        assert!(has_path(&g, "A", "C"), "A 应能间接到达 C");
        assert!(has_path(&g, "C", "A"), "C 应能间接到达 A");
        assert!(has_path(&g, "B", "A"), "B 应能间接到达 A");
    }

    #[test]
    fn diamond_graph_is_acyclic() {
        // A→B, A→C, B→D, C→D 是合法的菱形，不是环
        let g = graph(&[("A", "B"), ("A", "C"), ("B", "D"), ("C", "D")]);
        assert!(has_path(&g, "A", "D"));
        assert!(!has_path(&g, "D", "A"), "菱形不应被判为环");
    }

    #[test]
    fn long_chain_terminates() {
        // 长链也必须能正常结束（验证 visited 去重有效）
        let mut g: std::collections::HashMap<String, Vec<String>> = Default::default();
        for i in 0..500 {
            g.entry(format!("n{i}")).or_default().push(format!("n{}", i + 1));
        }
        assert!(has_path(&g, "n0", "n500"));
        assert!(!has_path(&g, "n500", "n0"));
    }

    #[test]
    fn title_validation_rejects_blank_and_whitespace() {
        // 与 subtask_create 中的校验规则保持一致
        let check = |s: &str| {
            let t = s.trim();
            !t.is_empty() && t.chars().count() <= TITLE_MAX
        };
        assert!(check("第一步"));
        assert!(!check(""));
        assert!(!check("   "));
        assert!(check(&"字".repeat(TITLE_MAX)));
        assert!(!check(&"字".repeat(TITLE_MAX + 1)));
    }
}
