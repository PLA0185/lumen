//! 独立备忘和顺序业务流程；版本条件写入避免主窗口与其它入口互相覆盖。
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::commands::AppState;
use crate::db::{to_db_time, utc_now, Db};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlowStep {
    pub id: String,
    pub title: String,
    pub owner: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<FlowStepLayout>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlowStepLayout {
    pub width: f64,
    pub min_height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MemoSummary {
    pub id: String,
    pub title: String,
    pub category: String,
    pub kind: String,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoDocument {
    #[serde(flatten)]
    pub summary: MemoSummary,
    pub body_md: String,
    pub steps: Vec<FlowStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveMemoInput {
    pub id: Option<String>,
    pub expected_revision: Option<i64>,
    pub title: String,
    pub category: String,
    pub kind: String,
    pub body_md: String,
    pub steps: Vec<FlowStep>,
}

fn validate_text(value: &str, label: &str, max: usize) -> AppResult<()> {
    if value.chars().count() > max || value.contains('\0') {
        return Err(AppError::validation(format!(
            "{label}超出 {max} 字或含无效字符"
        )));
    }
    Ok(())
}

pub(crate) fn validate(input: &SaveMemoInput) -> AppResult<()> {
    if input.title.trim().is_empty() {
        return Err(AppError::validation("标题不能为空"));
    }
    validate_text(&input.title, "标题", 500)?;
    validate_text(&input.category, "分类", 100)?;
    validate_text(&input.body_md, "备忘内容", 100_000)?;
    if !matches!(input.kind.as_str(), "memo" | "flow") {
        return Err(AppError::validation("记录类型必须是备忘或流程"));
    }
    if input.steps.len() > 100 || (input.kind == "memo" && !input.steps.is_empty()) {
        return Err(AppError::validation(
            "流程最多 100 步，普通备忘不能携带流程步骤",
        ));
    }
    let mut ids = std::collections::HashSet::new();
    for step in &input.steps {
        if step.id.is_empty() || !ids.insert(&step.id) {
            return Err(AppError::validation("步骤编号不能为空且必须唯一"));
        }
        validate_text(&step.id, "步骤编号", 100)?;
        validate_text(&step.title, "步骤标题", 300)?;
        validate_text(&step.owner, "负责人", 100)?;
        validate_text(&step.detail, "步骤说明", 5_000)?;
        if let Some(layout) = &step.layout {
            if !layout.width.is_finite()
                || !(280.0..=2400.0).contains(&layout.width)
                || !layout.min_height.is_finite()
                || !(148.0..=20_000.0).contains(&layout.min_height)
                || layout.x.is_some() != layout.y.is_some()
                || [layout.x, layout.y]
                    .into_iter()
                    .flatten()
                    .any(|value| !value.is_finite() || value.abs() > 1_000_000.0)
            {
                return Err(AppError::validation("步骤布局大小或位置无效"));
            }
        }
    }
    if input.id.is_some() != input.expected_revision.is_some() {
        return Err(AppError::validation("修改已有记录必须携带原版本号"));
    }
    Ok(())
}

pub(crate) fn document(row: sqlx::sqlite::SqliteRow) -> AppResult<MemoDocument> {
    use sqlx::FromRow;
    let json: String = row.try_get("steps_json")?;
    let steps = serde_json::from_str(&json)
        .map_err(|_| AppError::internal("流程内容无法读取，请从备份恢复或联系支持"))?;
    Ok(MemoDocument {
        summary: MemoSummary::from_row(&row)?,
        body_md: row.try_get("body_md")?,
        steps,
    })
}

pub async fn get_impl(db: &Db, id: &str) -> AppResult<MemoDocument> {
    let row = sqlx::query("SELECT * FROM memo_documents WHERE id = ?")
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| AppError::not_found("备忘与流程", id))?;
    document(row)
}

pub async fn list_impl(db: &Db, query: &str, deleted_only: bool) -> AppResult<Vec<MemoSummary>> {
    validate_text(query, "搜索文字", 500)?;
    let literal = query
        .trim()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{literal}%");
    // ponytail: 全量摘要适合个人业务手册；数量达到万级时增加分页。
    Ok(sqlx::query_as::<_, MemoSummary>(
        "SELECT id,title,category,kind,revision,created_at,updated_at,deleted_at FROM memo_documents
         WHERE ((? = 0 AND deleted_at IS NULL) OR (? = 1 AND deleted_at IS NOT NULL))
         AND (title LIKE ? ESCAPE '\\' OR category LIKE ? ESCAPE '\\'
              OR body_md LIKE ? ESCAPE '\\' OR steps_json LIKE ? ESCAPE '\\')
         ORDER BY updated_at DESC, id ASC")
        .bind(deleted_only).bind(deleted_only).bind(&pattern).bind(&pattern).bind(&pattern).bind(&pattern)
        .fetch_all(db.pool()).await?)
}

pub async fn save_impl(db: &Db, input: SaveMemoInput) -> AppResult<MemoDocument> {
    validate(&input)?;
    let json =
        serde_json::to_string(&input.steps).map_err(|_| AppError::internal("流程序列化失败"))?;
    let now = to_db_time(utc_now());
    let mut tx = db.pool().begin().await?;
    let row = if let Some(id) = &input.id {
        sqlx::query("UPDATE memo_documents SET title=?,category=?,kind=?,body_md=?,steps_json=?,
            revision=revision+1,updated_at=? WHERE id=? AND revision=? AND deleted_at IS NULL RETURNING *")
            .bind(input.title.trim()).bind(input.category.trim()).bind(&input.kind)
            .bind(&input.body_md).bind(json).bind(&now).bind(id).bind(input.expected_revision)
            .fetch_optional(&mut *tx).await?
            .ok_or_else(|| AppError::conflict("记录已在其它地方修改或删除。请保留当前草稿，重新打开记录后再保存"))?
    } else {
        sqlx::query("INSERT INTO memo_documents (id,title,category,kind,body_md,steps_json,created_at,updated_at)
            VALUES (?,?,?,?,?,?,?,?) RETURNING *")
            .bind(uuid::Uuid::now_v7().to_string()).bind(input.title.trim()).bind(input.category.trim())
            .bind(&input.kind).bind(&input.body_md).bind(json).bind(&now).bind(&now)
            .fetch_one(&mut *tx).await?
    };
    let doc = document(row)?;
    crate::cloud_sync::record_event(&mut tx, &doc, None).await?;
    tx.commit().await?;
    Ok(doc)
}

pub async fn set_deleted_impl(
    db: &Db,
    id: &str,
    revision: i64,
    deleted: bool,
) -> AppResult<MemoDocument> {
    let now = to_db_time(utc_now());
    let mut tx = db.pool().begin().await?;
    let row = sqlx::query("UPDATE memo_documents SET deleted_at=?,updated_at=?,revision=revision+1
        WHERE id=? AND revision=? AND ((?=1 AND deleted_at IS NULL) OR (?=0 AND deleted_at IS NOT NULL)) RETURNING *")
        .bind(if deleted { Some(&now) } else { None }).bind(&now).bind(id).bind(revision)
        .bind(deleted).bind(deleted).fetch_optional(&mut *tx).await?
        .ok_or_else(|| AppError::conflict("记录状态已经变化，请重新读取后再操作"))?;
    let doc = document(row)?;
    crate::cloud_sync::record_event(&mut tx, &doc, None).await?;
    tx.commit().await?;
    Ok(doc)
}

#[tauri::command]
pub async fn memo_list(
    state: State<'_, AppState>,
    query: String,
    deleted_only: bool,
) -> AppResult<Vec<MemoSummary>> {
    list_impl(&state.db, &query, deleted_only).await
}
#[tauri::command]
pub async fn memo_get(state: State<'_, AppState>, id: String) -> AppResult<MemoDocument> {
    get_impl(&state.db, &id).await
}
#[tauri::command]
pub async fn memo_save(
    state: State<'_, AppState>,
    input: SaveMemoInput,
) -> AppResult<MemoDocument> {
    save_impl(&state.db, input).await
}
#[tauri::command]
pub async fn memo_set_deleted(
    state: State<'_, AppState>,
    id: String,
    revision: i64,
    deleted: bool,
) -> AppResult<MemoDocument> {
    set_deleted_impl(&state.db, &id, revision, deleted).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_rejects_invalid_sizes_and_unpaired_positions() {
        let mut value = input();
        for layout in [
            FlowStepLayout {
                width: 20.0,
                min_height: 148.0,
                x: None,
                y: None,
            },
            FlowStepLayout {
                width: 420.0,
                min_height: 20.0,
                x: None,
                y: None,
            },
            FlowStepLayout {
                width: 420.0,
                min_height: 148.0,
                x: Some(0.0),
                y: None,
            },
            FlowStepLayout {
                width: f64::NAN,
                min_height: 148.0,
                x: None,
                y: None,
            },
            FlowStepLayout {
                width: 420.0,
                min_height: 148.0,
                x: Some(1_000_001.0),
                y: Some(0.0),
            },
        ] {
            value.steps[0].layout = Some(layout);
            assert!(validate(&value).is_err(), "无效布局不能保存");
        }
        value.steps[0].layout = Some(FlowStepLayout {
            width: 620.0,
            min_height: 300.0,
            x: Some(-100.0),
            y: Some(80.0),
        });
        validate(&value).unwrap();
        let json = serde_json::to_value(&value.steps[0]).unwrap();
        assert_eq!(json["layout"]["width"], 620.0);
        value.steps[0].layout = None;
        assert!(
            serde_json::to_value(&value.steps[0])
                .unwrap()
                .get("layout")
                .is_none(),
            "旧 JSON 不新增布局字段"
        );
    }

    fn input() -> SaveMemoInput {
        SaveMemoInput {
            id: None,
            expected_revision: None,
            title: "  业务流程  ".into(),
            category: "学习".into(),
            kind: "flow".into(),
            body_md: "注意核对".into(),
            steps: vec![FlowStep {
                layout: None,
                id: "a".into(),
                title: "收集资料".into(),
                owner: "同事".into(),
                detail: "确认后提交".into(),
            }],
        }
    }

    #[tokio::test]
    async fn saves_searches_rejects_stale_writes_and_restores_without_touching_tasks() {
        let dir = std::env::temp_dir().join(format!("lumen-memos-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let first = save_impl(&db, input()).await.unwrap();
        assert_eq!(first.summary.title, "业务流程");
        assert_eq!(
            get_impl(&db, &first.summary.id).await.unwrap().steps[0].owner,
            "同事"
        );
        assert_eq!(list_impl(&db, "确认后", false).await.unwrap().len(), 1);
        assert!(list_impl(&db, "%", false).await.unwrap().is_empty());
        let mut edit = input();
        edit.id = Some(first.summary.id.clone());
        edit.expected_revision = Some(1);
        edit.title = "已修改".into();
        let saved = save_impl(&db, edit.clone()).await.unwrap();
        assert_eq!(saved.summary.revision, 2);
        edit.title = "不能覆盖".into();
        assert!(save_impl(&db, edit).await.is_err());
        assert_eq!(
            get_impl(&db, &first.summary.id)
                .await
                .unwrap()
                .summary
                .title,
            "已修改"
        );
        let deleted = set_deleted_impl(&db, &saved.summary.id, 2, true)
            .await
            .unwrap();
        assert!(list_impl(&db, "", false).await.unwrap().is_empty());
        assert_eq!(list_impl(&db, "", true).await.unwrap().len(), 1);
        assert!(set_deleted_impl(&db, &saved.summary.id, 2, false)
            .await
            .is_err());
        let restored = set_deleted_impl(&db, &deleted.summary.id, 3, false)
            .await
            .unwrap();
        assert!(restored.summary.deleted_at.is_none());
        assert_eq!(restored.steps[0].title, "收集资料");
        let tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(tasks, 0);
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn incomplete_flow_steps_are_saved_without_losing_content() {
        let dir = std::env::temp_dir().join(format!("lumen-flow-draft-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let mut draft = input();
        draft.steps[0].title.clear();
        draft.steps[0].owner.clear();
        draft.steps[0].detail = "未写完的说明和图片".into();
        let doc = save_impl(&db, draft).await.unwrap();
        assert!(doc.steps[0].title.is_empty());
        assert!(get_impl(&db, &doc.summary.id).await.unwrap().steps[0]
            .owner
            .is_empty());
        assert_eq!(
            get_impl(&db, &doc.summary.id).await.unwrap().steps[0].detail,
            "未写完的说明和图片"
        );
        let mut edit = SaveMemoInput {
            id: Some(doc.summary.id),
            expected_revision: Some(doc.summary.revision),
            ..input()
        };
        edit.steps.clear();
        assert!(save_impl(&db, edit).await.unwrap().steps.is_empty());
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    async fn invalid_flow_does_not_write_anything() {
        let dir =
            std::env::temp_dir().join(format!("lumen-memo-validation-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let mut bad = input();
        bad.steps.push(bad.steps[0].clone());
        assert!(save_impl(&db, bad).await.is_err());
        let mut bad = input();
        bad.title.clear();
        assert!(save_impl(&db, bad).await.is_err());
        assert!(list_impl(&db, "", false).await.unwrap().is_empty());
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}
