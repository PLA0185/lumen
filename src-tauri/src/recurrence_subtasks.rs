//! Explicit structural scope; completion always belongs to a single occurrence.
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};
use tauri::State;

use crate::commands::AppState;
use crate::db::{to_db_time, utc_now, Db};
use crate::error::{AppError, AppResult};
use crate::subtasks::Subtask;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtaskTemplate {
    pub id: String,
    pub title: String,
    pub sort_order: f64,
}

pub fn parse_templates(raw: &str) -> AppResult<Vec<SubtaskTemplate>> {
    let templates: Vec<SubtaskTemplate> = serde_json::from_str(raw)
        .map_err(|e| AppError::validation(format!("重复子任务模板损坏：{e}")))?;
    let mut ids = std::collections::HashSet::new();
    if templates.len() > 1000 {
        return Err(AppError::validation("一个系列最多 1000 个子任务模板"));
    }
    for item in &templates {
        validate_title(&item.title)?;
        if uuid::Uuid::parse_str(&item.id).is_err()
            || !ids.insert(&item.id)
            || !item.sort_order.is_finite()
        {
            return Err(AppError::validation("重复子任务模板的身份或排序不合法"));
        }
    }
    Ok(templates)
}

fn validate_title(title: &str) -> AppResult<&str> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 500 {
        return Err(AppError::validation("子任务标题应为 1–500 个字符"));
    }
    Ok(title)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtaskScope {
    ThisOnly,
    WholeSeries,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtaskCompletionMode {
    Free,
    Sequential,
}

impl SubtaskCompletionMode {
    fn as_db(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Sequential => "sequential",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubtaskAction {
    Create { title: String },
    Rename { id: String, title: String },
    Delete { id: String },
    CopyPrevious,
    Reorder { ordered_ids: Vec<String> },
    SetCompletionMode { mode: SubtaskCompletionMode },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubtaskChange {
    pub task_id: String,
    pub scope: Option<SubtaskScope>,
    pub action: SubtaskAction,
}

pub async fn insert_templates(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: &str,
    templates: &[SubtaskTemplate],
    now: &str,
) -> AppResult<()> {
    for item in templates {
        sqlx::query("INSERT INTO subtasks
            (id,task_id,title,sort_order,series_template_id,is_done,completed_at,created_at,updated_at)
            VALUES (?1,?2,?3,?4,?5,0,NULL,?6,?6)")
            .bind(uuid::Uuid::now_v7().to_string()).bind(task_id).bind(&item.title)
            .bind(item.sort_order).bind(&item.id).bind(now).execute(&mut **tx).await?;
    }
    Ok(())
}

/// The legacy structural IPC must not silently assume a scope for recurring tasks.
pub async fn require_non_recurring(db: &Db, task_id: &str) -> AppResult<()> {
    let series: Option<String> =
        sqlx::query_scalar("SELECT series_id FROM tasks WHERE id=?1 AND deleted_at IS NULL")
            .bind(task_id)
            .fetch_optional(db.pool())
            .await?
            .ok_or_else(|| AppError::not_found("任务", task_id))?;
    if series.is_some() {
        return Err(AppError::validation(
            "请先选择子任务修改范围：仅这一次或整个重复系列",
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn subtask_change(
    state: State<'_, AppState>,
    input: SubtaskChange,
) -> AppResult<Vec<Subtask>> {
    change_impl(&state.db, input).await
}

#[tauri::command]
pub async fn subtask_completion_mode_get(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<SubtaskCompletionMode> {
    let mode: String = sqlx::query_scalar(
        "SELECT subtask_completion_mode FROM tasks WHERE id=?1 AND deleted_at IS NULL",
    )
    .bind(&task_id)
    .fetch_optional(state.db.pool())
    .await?
    .ok_or_else(|| AppError::not_found("任务", &task_id))?;
    match mode.as_str() {
        "free" => Ok(SubtaskCompletionMode::Free),
        "sequential" => Ok(SubtaskCompletionMode::Sequential),
        _ => Err(AppError::internal("子任务完成模式数据无效")),
    }
}

pub async fn change_impl(db: &Db, input: SubtaskChange) -> AppResult<Vec<Subtask>> {
    let mut tx = db.pool().begin().await?;
    let (series, occurrence): (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT series_id,occurrence_key FROM tasks WHERE id=?1 AND deleted_at IS NULL",
    )
    .bind(&input.task_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::not_found("任务", &input.task_id))?;
    let scope = match (series.is_some(), input.scope) {
        (true, None) => return Err(AppError::validation("请选择仅这一次或整个重复系列")),
        (false, Some(SubtaskScope::WholeSeries)) => {
            return Err(AppError::validation("普通任务没有重复系列"))
        }
        (_, scope) => scope.unwrap_or(SubtaskScope::ThisOnly),
    };
    let whole = scope == SubtaskScope::WholeSeries;
    let mut templates = if whole {
        let raw: String =
            sqlx::query_scalar("SELECT subtasks_json FROM task_series_template WHERE series_id=?1")
                .bind(&series)
                .fetch_one(&mut *tx)
                .await?;
        parse_templates(&raw)?
    } else {
        Vec::new()
    };
    let targets: Vec<String> = if whole {
        sqlx::query_scalar(
            "SELECT id FROM tasks WHERE series_id=?1 AND deleted_at IS NULL
                           AND status NOT IN ('done','archived') ORDER BY occurrence_key,id",
        )
        .bind(&series)
        .fetch_all(&mut *tx)
        .await?
    } else {
        vec![input.task_id.clone()]
    };
    let now = to_db_time(utc_now());
    let order: f64 = sqlx::query_scalar(
        "SELECT CAST(COALESCE(MAX(sort_order),0)+1 AS REAL) FROM subtasks WHERE task_id=?1",
    )
    .bind(&input.task_id)
    .fetch_one(&mut *tx)
    .await?;
    match input.action {
        SubtaskAction::Create { title } => {
            let title = validate_title(&title)?.to_owned();
            let item = SubtaskTemplate {
                id: uuid::Uuid::now_v7().to_string(),
                title,
                sort_order: order,
            };
            if whole {
                templates.push(item.clone());
                for target in &targets {
                    insert_templates(&mut tx, target, std::slice::from_ref(&item), &now).await?;
                }
            } else {
                sqlx::query("INSERT INTO subtasks(id,task_id,title,sort_order,is_done,created_at,updated_at) VALUES (?1,?2,?3,?4,0,?5,?5)")
                    .bind(uuid::Uuid::now_v7().to_string()).bind(&input.task_id).bind(&item.title)
                    .bind(order).bind(&now).execute(&mut *tx).await?;
            }
        }
        action @ (SubtaskAction::Rename { .. } | SubtaskAction::Delete { .. }) => {
            let (id, new_title) = match action {
                SubtaskAction::Rename { id, title } => {
                    (id, Some(validate_title(&title)?.to_owned()))
                }
                SubtaskAction::Delete { id } => (id, None),
                _ => unreachable!(),
            };
            let (old_title, sort_order, linked): (String, f64, Option<String>) = sqlx::query_as(
                "SELECT title,sort_order,series_template_id FROM subtasks WHERE id=?1 AND task_id=?2")
                .bind(&id).bind(&input.task_id).fetch_optional(&mut *tx).await?
                .ok_or_else(|| AppError::not_found("当前任务的子任务", &id))?;
            if whole {
                let template_id = linked
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
                if let Some(title) = &new_title {
                    if linked.is_some() {
                        let item = templates
                            .iter_mut()
                            .find(|item| item.id == template_id)
                            .ok_or_else(|| {
                                AppError::conflict("此子任务的系列模板已删除，请仅修改这一次")
                            })?;
                        item.title.clone_from(title);
                    } else {
                        let item = SubtaskTemplate {
                            id: template_id.clone(),
                            title: title.clone(),
                            sort_order,
                        };
                        for target in &targets {
                            if target == &input.task_id {
                                sqlx::query(
                                    "UPDATE subtasks SET series_template_id=?1 WHERE id=?2",
                                )
                                .bind(&template_id)
                                .bind(&id)
                                .execute(&mut *tx)
                                .await?;
                            } else {
                                insert_templates(
                                    &mut tx,
                                    target,
                                    std::slice::from_ref(&item),
                                    &now,
                                )
                                .await?;
                            }
                        }
                        templates.push(item);
                    }
                    sqlx::query("UPDATE subtasks SET title=?1,updated_at=?2 WHERE series_template_id=?3
                        AND task_id IN (SELECT id FROM tasks WHERE series_id=?4 AND deleted_at IS NULL AND status NOT IN ('done','archived'))")
                        .bind(title).bind(&now).bind(&template_id).bind(&series).execute(&mut *tx).await?;
                } else {
                    templates.retain(|item| item.id != template_id);
                    if linked.is_some() {
                        sqlx::query("DELETE FROM subtasks WHERE series_template_id=?1 AND task_id IN
                            (SELECT id FROM tasks WHERE series_id=?2 AND deleted_at IS NULL AND status NOT IN ('done','archived'))")
                            .bind(&template_id).bind(&series).execute(&mut *tx).await?;
                    } else if targets.contains(&input.task_id) {
                        sqlx::query("DELETE FROM subtasks WHERE id=?1")
                            .bind(&id)
                            .execute(&mut *tx)
                            .await?;
                    }
                }
            } else if let Some(title) = new_title {
                if title != old_title {
                    sqlx::query("UPDATE subtasks SET title=?1,updated_at=?2 WHERE id=?3")
                        .bind(title)
                        .bind(&now)
                        .bind(&id)
                        .execute(&mut *tx)
                        .await?;
                }
            } else {
                sqlx::query("DELETE FROM subtasks WHERE id=?1")
                    .bind(&id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        SubtaskAction::CopyPrevious => {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subtasks WHERE task_id=?1")
                .bind(&input.task_id)
                .fetch_one(&mut *tx)
                .await?;
            if count != 0 {
                return Err(AppError::conflict("当前已有子任务，复制已中止以免重复添加"));
            }
            let source: Option<String> = sqlx::query_scalar("SELECT id FROM tasks t WHERE series_id=?1
                AND occurrence_key < ?2 AND deleted_at IS NULL AND EXISTS (SELECT 1 FROM subtasks x WHERE x.task_id=t.id)
                ORDER BY occurrence_key DESC,id DESC LIMIT 1")
                .bind(&series).bind(&occurrence).fetch_optional(&mut *tx).await?;
            let source =
                source.ok_or_else(|| AppError::not_found("之前带子任务的发生", &input.task_id))?;
            let rows: Vec<(String, f64)> = sqlx::query_as(
                "SELECT title,sort_order FROM subtasks WHERE task_id=?1 ORDER BY sort_order,id",
            )
            .bind(source)
            .fetch_all(&mut *tx)
            .await?;
            if whole && !templates.is_empty() {
                return Err(AppError::conflict(
                    "系列已有子任务模板，请添加或编辑现有模板，避免重复复制",
                ));
            }
            let copied: Vec<SubtaskTemplate> = rows
                .into_iter()
                .map(|(title, sort_order)| SubtaskTemplate {
                    id: uuid::Uuid::now_v7().to_string(),
                    title,
                    sort_order,
                })
                .collect();
            for target in &targets {
                insert_templates(&mut tx, target, &copied, &now).await?;
                if !whole {
                    sqlx::query("UPDATE subtasks SET series_template_id=NULL WHERE task_id=?1")
                        .bind(target)
                        .execute(&mut *tx)
                        .await?;
                }
            }
            templates.extend(copied);
        }
        SubtaskAction::Reorder { ordered_ids } => {
            reorder_subtasks(
                &mut tx,
                ReorderSubtasksContext {
                    task_id: &input.task_id,
                    series_id: series.as_deref(),
                    whole,
                    targets: &targets,
                    templates: &mut templates,
                    ordered_ids: &ordered_ids,
                    now: &now,
                },
            )
            .await?;
        }
        SubtaskAction::SetCompletionMode { mode } => {
            if mode == SubtaskCompletionMode::Sequential {
                validate_sequential_state(&mut tx, &targets).await?;
            }
            if whole {
                sqlx::query(
                    "UPDATE task_series_template SET subtask_completion_mode=?1 WHERE series_id=?2",
                )
                .bind(mode.as_db())
                .bind(&series)
                .execute(&mut *tx)
                .await?;
                if !targets.is_empty() {
                    let mut update = sqlx::QueryBuilder::<Sqlite>::new(
                        "UPDATE tasks SET subtask_completion_mode = ",
                    );
                    update.push_bind(mode.as_db());
                    update.push(", updated_at = ").push_bind(&now);
                    update.push(" WHERE id IN (");
                    let mut sep = update.separated(", ");
                    for target in &targets {
                        sep.push_bind(target);
                    }
                    sep.push_unseparated(")");
                    update.build().execute(&mut *tx).await?;
                }
            } else {
                sqlx::query("UPDATE tasks SET subtask_completion_mode=?1,updated_at=?2,
                    is_user_modified=CASE WHEN series_id IS NULL THEN is_user_modified ELSE 1 END WHERE id=?3")
                    .bind(mode.as_db()).bind(&now).bind(&input.task_id).execute(&mut *tx).await?;
            }
        }
    }
    if whole {
        let raw =
            serde_json::to_string(&templates).map_err(|e| AppError::internal(e.to_string()))?;
        parse_templates(&raw)?;
        sqlx::query("UPDATE task_series_template SET subtasks_json=?1 WHERE series_id=?2")
            .bind(raw)
            .bind(&series)
            .execute(&mut *tx)
            .await?;
    } else if series.is_some() {
        sqlx::query("UPDATE tasks SET is_user_modified=1,updated_at=?1 WHERE id=?2")
            .bind(&now)
            .bind(&input.task_id)
            .execute(&mut *tx)
            .await?;
    }
    let items = sqlx::query_as("SELECT * FROM subtasks WHERE task_id=?1 ORDER BY sort_order,id")
        .bind(&input.task_id)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(items)
}

async fn validate_sequential_state(
    tx: &mut Transaction<'_, Sqlite>,
    targets: &[String],
) -> AppResult<()> {
    for task_id in targets {
        let has_out_of_order_completion: i64 = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM subtasks later
                JOIN subtasks earlier ON earlier.task_id=later.task_id
                  AND (earlier.sort_order < later.sort_order
                    OR (earlier.sort_order=later.sort_order AND earlier.id < later.id))
                WHERE later.task_id=?1 AND later.is_done=1 AND earlier.is_done=0
            )",
        )
        .bind(task_id)
        .fetch_one(&mut **tx)
        .await?;
        if has_out_of_order_completion != 0 {
            return Err(AppError::validation(
                "当前完成状态不符合顺序；请先取消后面的已完成子任务，再启用按顺序完成",
            ));
        }
    }
    Ok(())
}

struct ReorderSubtasksContext<'a> {
    task_id: &'a str,
    series_id: Option<&'a str>,
    whole: bool,
    targets: &'a [String],
    templates: &'a mut Vec<SubtaskTemplate>,
    ordered_ids: &'a [String],
    now: &'a str,
}

async fn reorder_subtasks(
    tx: &mut Transaction<'_, Sqlite>,
    context: ReorderSubtasksContext<'_>,
) -> AppResult<()> {
    let ReorderSubtasksContext {
        task_id,
        series_id,
        whole,
        targets,
        templates,
        ordered_ids,
        now,
    } = context;
    let current: Vec<(String, String, f64, Option<String>)> = sqlx::query_as(
        "SELECT id,title,sort_order,series_template_id FROM subtasks WHERE task_id=?1 ORDER BY sort_order,id",
    )
    .bind(task_id)
    .fetch_all(&mut **tx)
    .await?;
    let current_ids: std::collections::HashSet<&str> =
        current.iter().map(|row| row.0.as_str()).collect();
    let requested_ids: std::collections::HashSet<&str> =
        ordered_ids.iter().map(String::as_str).collect();
    if ordered_ids.len() != current.len()
        || requested_ids.len() != ordered_ids.len()
        || requested_ids != current_ids
    {
        return Err(AppError::validation("子任务顺序已变化，请刷新后重试"));
    }

    let mode: String = if whole {
        sqlx::query_scalar(
            "SELECT subtask_completion_mode FROM task_series_template WHERE series_id=?1",
        )
        .bind(series_id)
        .fetch_one(&mut **tx)
        .await?
    } else {
        sqlx::query_scalar("SELECT subtask_completion_mode FROM tasks WHERE id=?1")
            .bind(task_id)
            .fetch_one(&mut **tx)
            .await?
    };
    if mode == "sequential" {
        for target in targets {
            let completed: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM subtasks WHERE task_id=?1 AND is_done=1")
                    .bind(target)
                    .fetch_one(&mut **tx)
                    .await?;
            if completed > 0 {
                return Err(AppError::validation(
                    "按顺序完成的子任务已有完成记录，请先取消完成状态再调整顺序",
                ));
            }
        }
    }

    if !whole {
        for (index, id) in ordered_ids.iter().enumerate() {
            sqlx::query(
                "UPDATE subtasks SET sort_order=?1,updated_at=?2 WHERE id=?3 AND task_id=?4",
            )
            .bind(index as f64 + 1.0)
            .bind(now)
            .bind(id)
            .bind(task_id)
            .execute(&mut **tx)
            .await?;
        }
        return Ok(());
    }

    let mut reordered = Vec::with_capacity(templates.len().max(current.len()));
    let mut source_rows: std::collections::HashMap<String, String> = Default::default();
    let rows_by_id: std::collections::HashMap<&str, &(String, String, f64, Option<String>)> =
        current.iter().map(|row| (row.0.as_str(), row)).collect();
    for id in ordered_ids {
        let row = rows_by_id[id.as_str()];
        let template_id = if let Some(template_id) = &row.3 {
            if !templates.iter().any(|item| &item.id == template_id) {
                return Err(AppError::conflict(
                    "该子任务已脱离重复模板，无法安全地调整整个系列；请选择仅这一次",
                ));
            }
            template_id.clone()
        } else {
            uuid::Uuid::now_v7().to_string()
        };
        let item = if let Some(template) = templates.iter().find(|item| item.id == template_id) {
            template.clone()
        } else {
            SubtaskTemplate {
                id: template_id.clone(),
                title: row.1.clone(),
                sort_order: 0.0,
            }
        };
        source_rows.insert(template_id, row.0.clone());
        reordered.push(item);
    }
    for item in templates.iter() {
        if !source_rows.contains_key(&item.id) {
            reordered.push(item.clone());
        }
    }
    for (index, item) in reordered.iter_mut().enumerate() {
        item.sort_order = index as f64 + 1.0;
    }

    for target in targets {
        for item in &reordered {
            let existing: Option<String> = sqlx::query_scalar(
                "SELECT id FROM subtasks WHERE task_id=?1 AND series_template_id=?2",
            )
            .bind(target)
            .bind(&item.id)
            .fetch_optional(&mut **tx)
            .await?;
            if let Some(id) = existing {
                sqlx::query("UPDATE subtasks SET sort_order=?1,updated_at=?2 WHERE id=?3")
                    .bind(item.sort_order)
                    .bind(now)
                    .bind(id)
                    .execute(&mut **tx)
                    .await?;
            } else if target == task_id {
                if let Some(source_id) = source_rows.get(&item.id) {
                    sqlx::query("UPDATE subtasks SET series_template_id=?1,sort_order=?2,updated_at=?3 WHERE id=?4 AND task_id=?5")
                        .bind(&item.id).bind(item.sort_order).bind(now).bind(source_id).bind(target)
                        .execute(&mut **tx).await?;
                } else {
                    insert_templates(tx, target, std::slice::from_ref(item), now).await?;
                }
            } else {
                insert_templates(tx, target, std::slice::from_ref(item), now).await?;
            }
        }
    }
    *templates = reordered;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recurrence_service::{
        create_recurring_impl, list_instances, recurring_materialize_inner, CreateRecurringInput,
    };
    use crate::subtasks::{create_subtask_impl, update_subtask_impl, SubtaskPatch};

    async fn fixture() -> (AppState, String, Vec<String>, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("lumen-recurring-subtasks-{}", uuid::Uuid::now_v7()));
        let state = AppState::new(Db::init(&dir).await.unwrap());
        let created = create_recurring_impl(
            &state,
            CreateRecurringInput {
                title: "重复子任务验收".into(),
                description: None,
                priority: None,
                project_id: None,
                category_id: None,
                estimated_minutes: None,
                tag_ids: vec![],
                rrule: "FREQ=DAILY;COUNT=4".into(),
                tzid: Some("UTC".into()),
                dtstart_local: "2026-09-28T09:00:00".into(),
                has_start_time: Some(true),
                due_local: None,
                materialize_days: Some(4),
            },
        )
        .await
        .unwrap();
        let ids = list_instances(&state, &created.series_id)
            .await
            .unwrap()
            .into_iter()
            .map(|task| task.id)
            .collect();
        (state, created.series_id, ids, dir)
    }
    async fn items(state: &AppState, id: &str) -> Vec<Subtask> {
        sqlx::query_as("SELECT * FROM subtasks WHERE task_id=?1 ORDER BY sort_order,id")
            .bind(id)
            .fetch_all(state.db.pool())
            .await
            .unwrap()
    }
    async fn change(
        state: &AppState,
        id: &str,
        scope: SubtaskScope,
        action: SubtaskAction,
    ) -> AppResult<Vec<Subtask>> {
        change_impl(
            &state.db,
            SubtaskChange {
                task_id: id.into(),
                scope: Some(scope),
                action,
            },
        )
        .await
    }
    async fn close(state: AppState, dir: std::path::PathBuf) {
        state.db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn scope_is_required_and_local_changes_leave_other_occurrences_alone() {
        let (state, _, ids, dir) = fixture().await;
        assert!(change_impl(
            &state.db,
            SubtaskChange {
                task_id: ids[0].clone(),
                scope: None,
                action: SubtaskAction::Create {
                    title: "需要选择".into()
                }
            }
        )
        .await
        .is_err());
        assert!(require_non_recurring(&state.db, &ids[0]).await.is_err());
        assert!(items(&state, &ids[0]).await.is_empty());
        let added = change(
            &state,
            &ids[0],
            SubtaskScope::ThisOnly,
            SubtaskAction::Create {
                title: "只今天".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(added.len(), 1);
        assert!(items(&state, &ids[1]).await.is_empty());
        change(
            &state,
            &ids[0],
            SubtaskScope::ThisOnly,
            SubtaskAction::Rename {
                id: added[0].id.clone(),
                title: "今天改名".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(items(&state, &ids[0]).await[0].title, "今天改名");
        assert!(change(
            &state,
            &ids[1],
            SubtaskScope::ThisOnly,
            SubtaskAction::Delete {
                id: added[0].id.clone()
            }
        )
        .await
        .is_err());
        change(
            &state,
            &ids[0],
            SubtaskScope::ThisOnly,
            SubtaskAction::Delete {
                id: added[0].id.clone(),
            },
        )
        .await
        .unwrap();
        assert!(items(&state, &ids[0]).await.is_empty());
        close(state, dir).await;
    }

    #[tokio::test]
    async fn templates_generate_fresh_ids_and_independent_completion_times() {
        let (state, series, ids, dir) = fixture().await;
        let added = change(
            &state,
            &ids[0],
            SubtaskScope::WholeSeries,
            SubtaskAction::Create {
                title: "每日步骤".into(),
            },
        )
        .await
        .unwrap();
        let before = utc_now();
        let completed = update_subtask_impl(
            &state.db,
            &added[0].id,
            SubtaskPatch {
                is_done: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let after = utc_now();
        let time =
            chrono::DateTime::parse_from_rfc3339(completed.completed_at.as_ref().unwrap()).unwrap();
        assert!(
            time.timestamp_millis() >= before.timestamp_millis()
                && time.timestamp_millis() <= after.timestamp_millis()
        );
        let parent_time: String = sqlx::query_scalar("SELECT completed_at FROM tasks WHERE id=?1")
            .bind(&ids[0])
            .fetch_one(state.db.pool())
            .await
            .unwrap();
        assert_eq!(Some(parent_time), completed.completed_at);
        // Remove pristine future occurrences in the isolated fixture to exercise real materialization.
        sqlx::query("DELETE FROM tasks WHERE series_id=?1 AND id!=?2")
            .bind(&series)
            .bind(&ids[0])
            .execute(state.db.pool())
            .await
            .unwrap();
        assert_eq!(
            recurring_materialize_inner(
                &state,
                series.clone(),
                "2026-09-29T00:00:00.000Z".into(),
                "2026-10-02T00:00:00.000Z".into()
            )
            .await
            .unwrap(),
            3
        );
        let instances = list_instances(&state, &series).await.unwrap();
        for task in instances.iter().skip(1) {
            let children = items(&state, &task.id).await;
            assert_eq!(children.len(), 1);
            assert_ne!(children[0].id, added[0].id);
            assert_eq!(children[0].is_done, 0);
            assert!(children[0].completed_at.is_none());
            assert_eq!(task.status, "todo");
        }
        assert_eq!(
            items(&state, &ids[0]).await[0].completed_at,
            completed.completed_at
        );
        let reset = update_subtask_impl(
            &state.db,
            &added[0].id,
            SubtaskPatch {
                is_done: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(reset.completed_at.is_none());
        close(state, dir).await;
    }

    #[tokio::test]
    async fn sequence_mode_and_whole_series_reordering_apply_to_open_occurrences() {
        let (state, series, ids, dir) = fixture().await;
        let first = change(
            &state,
            &ids[0],
            SubtaskScope::WholeSeries,
            SubtaskAction::Create {
                title: "第一步".into(),
            },
        )
        .await
        .unwrap();
        let second = change(
            &state,
            &ids[0],
            SubtaskScope::WholeSeries,
            SubtaskAction::Create {
                title: "第二步".into(),
            },
        )
        .await
        .unwrap();

        let reordered = change(
            &state,
            &ids[0],
            SubtaskScope::WholeSeries,
            SubtaskAction::Reorder {
                ordered_ids: vec![second[1].id.clone(), first[0].id.clone()],
            },
        )
        .await
        .unwrap();
        assert_eq!(
            reordered
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec!["第二步", "第一步"]
        );
        assert_eq!(
            items(&state, &ids[1])
                .await
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            vec!["第二步", "第一步"]
        );

        change(
            &state,
            &ids[0],
            SubtaskScope::WholeSeries,
            SubtaskAction::SetCompletionMode {
                mode: SubtaskCompletionMode::Sequential,
            },
        )
        .await
        .unwrap();
        let modes: Vec<String> = sqlx::query_scalar(
            "SELECT subtask_completion_mode FROM tasks WHERE series_id=?1 ORDER BY occurrence_key",
        )
        .bind(&series)
        .fetch_all(state.db.pool())
        .await
        .unwrap();
        assert!(modes.iter().all(|mode| mode == "sequential"));

        let current = items(&state, &ids[0]).await;
        assert!(update_subtask_impl(
            &state.db,
            &current[1].id,
            SubtaskPatch {
                is_done: Some(true),
                ..Default::default()
            },
        )
        .await
        .is_err());
        update_subtask_impl(
            &state.db,
            &current[0].id,
            SubtaskPatch {
                is_done: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        update_subtask_impl(
            &state.db,
            &current[1].id,
            SubtaskPatch {
                is_done: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(update_subtask_impl(
            &state.db,
            &current[0].id,
            SubtaskPatch {
                is_done: Some(false),
                ..Default::default()
            },
        )
        .await
        .is_err());
        close(state, dir).await;
    }

    #[tokio::test]
    async fn whole_series_rename_and_delete_preserve_completed_history() {
        let (state, _, ids, dir) = fixture().await;
        let added = change(
            &state,
            &ids[0],
            SubtaskScope::WholeSeries,
            SubtaskAction::Create {
                title: "原步骤".into(),
            },
        )
        .await
        .unwrap();
        update_subtask_impl(
            &state.db,
            &added[0].id,
            SubtaskPatch {
                is_done: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let historical = serde_json::to_value(items(&state, &ids[0]).await).unwrap();
        let next = items(&state, &ids[1]).await.remove(0);
        change(
            &state,
            &ids[1],
            SubtaskScope::ThisOnly,
            SubtaskAction::Rename {
                id: next.id.clone(),
                title: "只改本次".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(items(&state, &ids[2]).await[0].title, "原步骤");
        change(
            &state,
            &ids[1],
            SubtaskScope::WholeSeries,
            SubtaskAction::Rename {
                id: next.id.clone(),
                title: "新模板".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(items(&state, &ids[2]).await[0].title, "新模板");
        change(
            &state,
            &ids[1],
            SubtaskScope::WholeSeries,
            SubtaskAction::Delete { id: next.id },
        )
        .await
        .unwrap();
        for id in ids.iter().skip(1) {
            assert!(items(&state, id).await.is_empty());
        }
        assert_eq!(
            serde_json::to_value(items(&state, &ids[0]).await).unwrap(),
            historical
        );
        close(state, dir).await;
    }

    #[tokio::test]
    async fn copy_previous_recovers_templates_without_changing_historical_rows() {
        let (state, _, ids, dir) = fixture().await;
        for title in ["CA", "UK"] {
            let step = create_subtask_impl(&state.db, &ids[0], title)
                .await
                .unwrap();
            update_subtask_impl(
                &state.db,
                &step.id,
                SubtaskPatch {
                    is_done: Some(true),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        let historical = serde_json::to_value(items(&state, &ids[0]).await).unwrap();
        change(
            &state,
            &ids[1],
            SubtaskScope::WholeSeries,
            SubtaskAction::CopyPrevious,
        )
        .await
        .unwrap();
        for id in ids.iter().skip(1) {
            let rows = items(&state, id).await;
            assert_eq!(rows.len(), 2);
            assert!(rows
                .iter()
                .all(|row| row.is_done == 0 && row.completed_at.is_none()));
        }
        assert_eq!(
            serde_json::to_value(items(&state, &ids[0]).await).unwrap(),
            historical
        );
        assert!(change(
            &state,
            &ids[1],
            SubtaskScope::WholeSeries,
            SubtaskAction::CopyPrevious
        )
        .await
        .is_err());
        close(state, dir).await;
    }

    #[tokio::test]
    async fn series_changes_roll_back_template_and_all_occurrences_on_write_failure() {
        let (state, series, ids, dir) = fixture().await;
        sqlx::query("CREATE TRIGGER reject_subtask BEFORE INSERT ON subtasks
            WHEN NEW.title='拒绝保存' AND NEW.task_id IN (SELECT id FROM tasks ORDER BY occurrence_key LIMIT 1 OFFSET 1)
            BEGIN SELECT RAISE(ABORT,'test'); END")
            .execute(state.db.pool()).await.unwrap();
        assert!(change(
            &state,
            &ids[0],
            SubtaskScope::WholeSeries,
            SubtaskAction::Create {
                title: "拒绝保存".into()
            }
        )
        .await
        .is_err());
        for id in &ids {
            assert!(items(&state, id).await.is_empty());
        }
        let raw: String =
            sqlx::query_scalar("SELECT subtasks_json FROM task_series_template WHERE series_id=?1")
                .bind(series)
                .fetch_one(state.db.pool())
                .await
                .unwrap();
        assert_eq!(raw, "[]");
        close(state, dir).await;
    }
}
