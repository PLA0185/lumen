use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;
use sqlx::{Row, SqliteConnection};
use tauri::{AppHandle, State};

use crate::{
    commands::AppState,
    db::{now_stamp, Db},
    error::{AppError, AppResult},
};

use super::{
    models::{SceneEvent, SceneMonitorStatus, SceneRule, SceneRuleInput, TemplateConfig},
    SceneRuntime,
};

const TEMPLATE_MAX_BYTES: usize = 2 * 1024 * 1024;
const BODY_MAX_CHARS: usize = 20_000;

#[derive(sqlx::FromRow)]
struct TemplateRow {
    mime: String,
    data_base64: String,
    byte_size: i64,
    sha256: String,
    width: i64,
    height: i64,
}

fn json<T: serde::Serialize>(value: &T, what: &str) -> AppResult<String> {
    serde_json::to_string(value)
        .map_err(|error| AppError::internal(format!("{what}序列化失败：{error}")))
}

fn parse_json<T: serde::de::DeserializeOwned>(raw: &str, what: &str) -> AppResult<T> {
    serde_json::from_str(raw)
        .map_err(|error| AppError::internal(format!("已保存的{what}损坏：{error}")))
}

fn normalized_list(values: Vec<String>, max: usize, what: &str) -> AppResult<Vec<String>> {
    if values.len() > max {
        return Err(AppError::validation(format!("{what}最多 {max} 项")));
    }
    let mut result = Vec::with_capacity(values.len());
    for value in values {
        let clean = value.trim();
        if clean.is_empty() || clean.chars().count() > 200 || clean.chars().any(char::is_control) {
            return Err(AppError::validation(format!(
                "{what}包含空项、控制字符或过长内容"
            )));
        }
        if !result
            .iter()
            .any(|item: &String| item.eq_ignore_ascii_case(clean))
        {
            result.push(clean.to_owned());
        }
    }
    Ok(result)
}

fn normalize_input(input: &mut SceneRuleInput) -> AppResult<()> {
    input.name = input.name.trim().to_owned();
    input.headline = input.headline.trim().to_owned();
    if input.name.is_empty() || input.name.chars().count() > 120 {
        return Err(AppError::validation("规则名称应为 1–120 个字符"));
    }
    if input.headline.is_empty() || input.headline.chars().count() > 240 {
        return Err(AppError::validation("便签标题应为 1–240 个字符"));
    }
    if input.body_md.chars().count() > BODY_MAX_CHARS {
        return Err(AppError::validation("便签正文最多 20,000 个字符"));
    }
    if !(1..=10).contains(&input.stable_hits)
        || !(1_000..=60_000).contains(&input.stable_leave_ms)
        || !(1..=1_440).contains(&input.default_snooze_minutes)
    {
        return Err(AppError::validation(
            "触发稳定次数、离开时间或稍后提醒时长超出允许范围",
        ));
    }

    input.program_allowlist = normalized_list(input.program_allowlist.clone(), 32, "允许程序")?
        .into_iter()
        .map(|name| {
            let basename = name.rsplit(['/', '\\']).next().unwrap_or("");
            if basename != name
                || !basename.to_ascii_lowercase().ends_with(".exe")
                || basename
                    .chars()
                    .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')))
            {
                return Err(AppError::validation(
                    "允许程序请填写可执行文件名，例如 chrome.exe；不能填写路径或通配符",
                ));
            }
            Ok(basename.to_ascii_lowercase())
        })
        .collect::<AppResult<Vec<_>>>()?;
    if input.program_allowlist.is_empty() {
        return Err(AppError::validation(
            "必须至少允许一个目标程序，不能监测所有程序",
        ));
    }
    input.window_title_contains =
        normalized_list(input.window_title_contains.clone(), 10, "窗口标题条件")?;
    input.required_terms = normalized_list(input.required_terms.clone(), 20, "OCR 必须词")?;
    input.excluded_terms = normalized_list(input.excluded_terms.clone(), 20, "OCR 排除词")?;
    if input.match_mode.needs_ocr()
        && (input.required_terms.is_empty()
            || input
                .required_terms
                .iter()
                .any(|term| term.chars().count() < 2))
    {
        return Err(AppError::validation(
            "OCR 规则至少需要一个长度不少于 2 个字符的必须词",
        ));
    }

    if input.match_mode.needs_image() {
        let Some(config) = &input.template_config else {
            return Err(AppError::validation("图像规则缺少已确认的局部参考图范围"));
        };
        validate_template_config(config)?;
        if input.template_image_base64.is_none() && input.existing_template_id.is_none() {
            return Err(AppError::validation("图像规则缺少已确认的参考图"));
        }
    } else {
        input.template_config = None;
        input.template_image_base64 = None;
        input.existing_template_id = None;
    }

    if input.step_id.is_some() && input.memo_id.is_none() {
        return Err(AppError::validation("关联流程步骤时必须同时选择对应流程"));
    }
    Ok(())
}

fn validate_template_config(config: &TemplateConfig) -> AppResult<()> {
    let values = [
        config.x,
        config.y,
        config.width,
        config.height,
        config.threshold,
    ];
    if values.iter().any(|value| !value.is_finite())
        || config.x < 0.0
        || config.y < 0.0
        || config.width <= 0.0
        || config.height <= 0.0
        || config.x + config.width > 1.0
        || config.y + config.height > 1.0
        || !(0.55..=0.99).contains(&config.threshold)
        || !(1..=20_000).contains(&config.source_width)
        || !(1..=20_000).contains(&config.source_height)
    {
        return Err(AppError::validation("局部参考图的范围、尺寸或匹配阈值无效"));
    }
    Ok(())
}

fn decode_template(raw: &str) -> AppResult<(String, Vec<u8>, u32, u32)> {
    if raw.len() > TEMPLATE_MAX_BYTES.div_ceil(3) * 4 {
        return Err(AppError::validation("单张场景参考图不能超过 2 MiB"));
    }
    let bytes = STANDARD
        .decode(raw)
        .map_err(|_| AppError::validation("场景参考图编码损坏"))?;
    if bytes.is_empty() || bytes.len() > TEMPLATE_MAX_BYTES {
        return Err(AppError::validation("单张场景参考图不能超过 2 MiB"));
    }
    let format = image::guess_format(&bytes)
        .map_err(|_| AppError::validation("场景参考图必须是 PNG 或 JPEG"))?;
    let mime = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        _ => return Err(AppError::validation("场景参考图必须是 PNG 或 JPEG")),
    };
    let image =
        image::load_from_memory(&bytes).map_err(|_| AppError::validation("场景参考图无法解码"))?;
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 || width > 1024 || height > 1024 {
        return Err(AppError::validation("局部参考图的宽高必须在 1–1024 像素内"));
    }
    Ok((mime.to_owned(), bytes, width, height))
}

async fn validate_memo_link(
    conn: &mut SqliteConnection,
    memo_id: Option<&str>,
    step_id: Option<&str>,
) -> AppResult<()> {
    let Some(memo_id) = memo_id else {
        return Ok(());
    };
    uuid::Uuid::parse_str(memo_id).map_err(|_| AppError::validation("关联备忘或流程编号无效"))?;
    let row = sqlx::query("SELECT kind, steps_json, deleted_at FROM memo_documents WHERE id=?")
        .bind(memo_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::not_found("关联备忘或流程", memo_id))?;
    let deleted: Option<String> = row.try_get("deleted_at")?;
    if deleted.is_some() {
        return Err(AppError::validation("不能关联已放入回收站的备忘或流程"));
    }
    let kind: String = row.try_get("kind")?;
    if let Some(step_id) = step_id {
        uuid::Uuid::parse_str(step_id).map_err(|_| AppError::validation("关联流程步骤编号无效"))?;
        if kind != "flow" {
            return Err(AppError::validation("只有流程可以关联具体步骤"));
        }
        let steps_json: String = row.try_get("steps_json")?;
        let steps: Value = serde_json::from_str(&steps_json)
            .map_err(|_| AppError::validation("关联流程的步骤数据损坏，无法保存规则"))?;
        if !steps.as_array().is_some_and(|steps| {
            steps
                .iter()
                .any(|step| step.get("id").and_then(Value::as_str) == Some(step_id))
        }) {
            return Err(AppError::validation("关联的流程步骤不存在或已被删除"));
        }
    }
    Ok(())
}

fn template_digest(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}

pub async fn save_rule(db: &Db, mut input: SceneRuleInput) -> AppResult<SceneRule> {
    normalize_input(&mut input)?;
    let id = match input.id.as_deref() {
        Some(id) => {
            uuid::Uuid::parse_str(id).map_err(|_| AppError::validation("场景规则编号无效"))?;
            id.to_owned()
        }
        None => uuid::Uuid::now_v7().to_string(),
    };
    if input.id.is_some() != input.expected_revision.is_some() {
        return Err(AppError::validation("编辑规则必须同时提供当前版本号"));
    }

    let new_template = input
        .template_image_base64
        .as_deref()
        .map(decode_template)
        .transpose()?;
    let now = now_stamp();
    let mut tx = db.pool().begin().await?;
    validate_memo_link(&mut tx, input.memo_id.as_deref(), input.step_id.as_deref()).await?;

    let old: Option<(Option<String>, i64, String)> = sqlx::query_as(
        "SELECT template_id, revision, created_at FROM scene_rules WHERE id=? AND deleted_at IS NULL",
    )
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?;
    let created_at = match (&input.expected_revision, &old) {
        (None, None) => now.clone(),
        (Some(_), Some((_, revision, created_at)))
            if Some(*revision) == input.expected_revision =>
        {
            created_at.clone()
        }
        (Some(_), Some(_)) => {
            return Err(AppError::conflict(
                "场景规则已被其他操作更新，请重新加载后再保存",
            ))
        }
        (Some(_), None) => return Err(AppError::not_found("场景规则", &id)),
        (None, Some(_)) => return Err(AppError::conflict("场景规则编号已存在，请重新加载后再试")),
    };

    let previous_template = old
        .as_ref()
        .and_then(|(template_id, _, _)| template_id.clone());
    let template_id = if let Some((mime, bytes, width, height)) = new_template {
        let template_id = uuid::Uuid::now_v7().to_string();
        let encoded = STANDARD.encode(&bytes);
        sqlx::query(
            "INSERT INTO scene_templates (id,mime,data_base64,byte_size,sha256,width,height,created_at)
             VALUES (?,?,?,?,?,?,?,?)",
        )
        .bind(&template_id)
        .bind(mime)
        .bind(encoded)
        .bind(i64::try_from(bytes.len()).map_err(|_| AppError::validation("场景参考图过大"))?)
        .bind(template_digest(&bytes))
        .bind(i64::from(width))
        .bind(i64::from(height))
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        Some(template_id)
    } else if input.match_mode.needs_image() {
        let selected = input
            .existing_template_id
            .clone()
            .or_else(|| previous_template.clone())
            .ok_or_else(|| AppError::validation("图像规则缺少参考图"))?;
        uuid::Uuid::parse_str(&selected).map_err(|_| AppError::validation("参考图编号无效"))?;
        let found: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scene_templates WHERE id=?")
            .bind(&selected)
            .fetch_one(&mut *tx)
            .await?;
        if found != 1 {
            return Err(AppError::not_found("场景参考图", &selected));
        }
        Some(selected)
    } else {
        None
    };

    let allowlist = json(&input.program_allowlist, "允许程序列表")?;
    let title_conditions = json(&input.window_title_contains, "窗口标题条件")?;
    let required = json(&input.required_terms, "OCR 必须词")?;
    let excluded = json(&input.excluded_terms, "OCR 排除词")?;
    let template_config = json(&input.template_config, "局部参考图条件")?;
    if let Some(expected_revision) = input.expected_revision {
        let changed = sqlx::query(
            "UPDATE scene_rules SET name=?,enabled=?,priority=?,program_allowlist_json=?,window_title_contains_json=?,
             match_mode=?,template_id=?,template_config_json=?,ocr_required_terms_json=?,ocr_excluded_terms_json=?,
             headline=?,body_md=?,memo_id=?,step_id=?,stable_hits=?,stable_leave_ms=?,default_snooze_minutes=?,
             revision=revision+1,updated_at=? WHERE id=? AND revision=? AND deleted_at IS NULL",
        )
        .bind(&input.name).bind(input.enabled).bind(input.priority).bind(allowlist).bind(title_conditions)
        .bind(input.match_mode.as_db_key()).bind(template_id).bind(template_config).bind(required).bind(excluded)
        .bind(&input.headline).bind(&input.body_md).bind(&input.memo_id).bind(&input.step_id)
        .bind(input.stable_hits).bind(input.stable_leave_ms).bind(input.default_snooze_minutes)
        .bind(&now).bind(&id).bind(expected_revision)
        .execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            return Err(AppError::conflict("场景规则版本已变化，未覆盖其他修改"));
        }
    } else {
        sqlx::query(
            "INSERT INTO scene_rules (id,name,enabled,priority,program_allowlist_json,window_title_contains_json,
             match_mode,template_id,template_config_json,ocr_required_terms_json,ocr_excluded_terms_json,headline,body_md,
             memo_id,step_id,stable_hits,stable_leave_ms,default_snooze_minutes,revision,created_at,updated_at)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,1,?,?)",
        )
        .bind(&id).bind(&input.name).bind(input.enabled).bind(input.priority).bind(allowlist).bind(title_conditions)
        .bind(input.match_mode.as_db_key()).bind(template_id).bind(template_config).bind(required).bind(excluded)
        .bind(&input.headline).bind(&input.body_md).bind(&input.memo_id).bind(&input.step_id)
        .bind(input.stable_hits).bind(input.stable_leave_ms).bind(input.default_snooze_minutes)
        .bind(&created_at).bind(&now)
        .execute(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO scene_runtime (rule_id, state, updated_at) VALUES (?, 'outside', ?)",
        )
        .bind(&id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    // Deleting unused reference frames is best-effort cleanup, never a reason
    // to report a successful rule write as failed after the transaction commits.
    if let Some(previous) = previous_template {
        if Some(previous.clone()) != input.existing_template_id {
            let still_referenced: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM scene_rules WHERE template_id=?")
                    .bind(&previous)
                    .fetch_one(db.pool())
                    .await?;
            if still_referenced == 0 {
                sqlx::query("DELETE FROM scene_templates WHERE id=?")
                    .bind(previous)
                    .execute(db.pool())
                    .await?;
            }
        }
    }
    get_rule(db, &id).await
}

pub async fn list_rules(db: &Db, include_deleted: bool) -> AppResult<Vec<SceneRule>> {
    let rows = if include_deleted {
        sqlx::query(
            "SELECT * FROM scene_rules ORDER BY enabled DESC, priority DESC, updated_at DESC, id",
        )
        .fetch_all(db.pool())
        .await?
    } else {
        sqlx::query("SELECT * FROM scene_rules WHERE deleted_at IS NULL ORDER BY enabled DESC, priority DESC, updated_at DESC, id")
            .fetch_all(db.pool()).await?
    };
    rows.iter().map(parse_rule).collect()
}

fn parse_rule(row: &sqlx::sqlite::SqliteRow) -> AppResult<SceneRule> {
    let match_mode: String = row.try_get("match_mode")?;
    let match_mode = serde_json::from_str(&format!("\"{match_mode}\""))
        .map_err(|_| AppError::internal("已保存的场景匹配方式损坏"))?;
    let config: String = row.try_get("template_config_json")?;
    Ok(SceneRule {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        enabled: row.try_get("enabled")?,
        priority: row.try_get("priority")?,
        program_allowlist: parse_json(
            &row.try_get::<String, _>("program_allowlist_json")?,
            "允许程序列表",
        )?,
        window_title_contains: parse_json(
            &row.try_get::<String, _>("window_title_contains_json")?,
            "窗口标题条件",
        )?,
        match_mode,
        template_id: row.try_get("template_id")?,
        template_config: parse_json(&config, "局部参考图条件")?,
        required_terms: parse_json(
            &row.try_get::<String, _>("ocr_required_terms_json")?,
            "OCR 必须词",
        )?,
        excluded_terms: parse_json(
            &row.try_get::<String, _>("ocr_excluded_terms_json")?,
            "OCR 排除词",
        )?,
        headline: row.try_get("headline")?,
        body_md: row.try_get("body_md")?,
        memo_id: row.try_get("memo_id")?,
        step_id: row.try_get("step_id")?,
        stable_hits: row.try_get("stable_hits")?,
        stable_leave_ms: row.try_get("stable_leave_ms")?,
        default_snooze_minutes: row.try_get("default_snooze_minutes")?,
        revision: row.try_get("revision")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        deleted_at: row.try_get("deleted_at")?,
    })
}

pub async fn get_rule(db: &Db, id: &str) -> AppResult<SceneRule> {
    uuid::Uuid::parse_str(id).map_err(|_| AppError::validation("场景规则编号无效"))?;
    let row = sqlx::query("SELECT * FROM scene_rules WHERE id=? AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| AppError::not_found("场景规则", id))?;
    parse_rule(&row)
}

pub async fn set_rule_enabled(db: &Db, id: &str, enabled: bool) -> AppResult<SceneRule> {
    uuid::Uuid::parse_str(id).map_err(|_| AppError::validation("场景规则编号无效"))?;
    let now = now_stamp();
    let changed = sqlx::query("UPDATE scene_rules SET enabled=?,revision=revision+1,updated_at=? WHERE id=? AND deleted_at IS NULL")
        .bind(enabled).bind(&now).bind(id).execute(db.pool()).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::not_found("场景规则", id));
    }
    if !enabled {
        sqlx::query("UPDATE scene_runtime SET state='outside',window_identity_hash=NULL,last_seen_at=NULL,snooze_until=NULL,updated_at=? WHERE rule_id=?")
            .bind(&now).bind(id).execute(db.pool()).await?;
    }
    get_rule(db, id).await
}

pub async fn delete_rule(db: &Db, id: &str) -> AppResult<()> {
    uuid::Uuid::parse_str(id).map_err(|_| AppError::validation("场景规则编号无效"))?;
    let now = now_stamp();
    let changed = sqlx::query("UPDATE scene_rules SET enabled=0,deleted_at=?,updated_at=?,revision=revision+1 WHERE id=? AND deleted_at IS NULL")
        .bind(&now).bind(&now).bind(id).execute(db.pool()).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::not_found("场景规则", id));
    }
    sqlx::query("UPDATE scene_runtime SET state='outside',window_identity_hash=NULL,snooze_until=NULL,updated_at=? WHERE rule_id=?")
        .bind(now).bind(id).execute(db.pool()).await?;
    Ok(())
}

pub async fn template_bytes(db: &Db, id: &str) -> AppResult<Vec<u8>> {
    uuid::Uuid::parse_str(id).map_err(|_| AppError::validation("场景参考图编号无效"))?;
    let row: TemplateRow = sqlx::query_as(
        "SELECT mime,data_base64,byte_size,sha256,width,height FROM scene_templates WHERE id=?",
    )
    .bind(id)
    .fetch_optional(db.pool())
    .await?
    .ok_or_else(|| AppError::not_found("场景参考图", id))?;
    if row.byte_size <= 0
        || row.byte_size as usize > TEMPLATE_MAX_BYTES
        || row.width <= 0
        || row.height <= 0
    {
        return Err(AppError::validation("已保存的场景参考图尺寸或大小无效"));
    }
    let bytes = STANDARD
        .decode(&row.data_base64)
        .map_err(|_| AppError::validation("已保存的场景参考图编码损坏"))?;
    if bytes.len() != row.byte_size as usize || template_digest(&bytes) != row.sha256 {
        return Err(AppError::validation("已保存的场景参考图校验失败"));
    }
    let (mime, decoded, width, height) = decode_template(&STANDARD.encode(&bytes))?;
    if mime != row.mime
        || decoded != bytes
        || i64::from(width) != row.width
        || i64::from(height) != row.height
    {
        return Err(AppError::validation(
            "已保存的场景参考图内容与校验信息不一致",
        ));
    }
    Ok(bytes)
}

pub async fn monitor_status(db: &Db, capture_armed: bool) -> AppResult<SceneMonitorStatus> {
    let row = sqlx::query("SELECT enabled,paused FROM scene_monitor_settings WHERE id='default'")
        .fetch_one(db.pool())
        .await?;
    let enabled: bool = row.try_get("enabled")?;
    let paused: bool = row.try_get("paused")?;
    let allowed_rule_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scene_rules WHERE deleted_at IS NULL AND enabled=1",
    )
    .fetch_one(db.pool())
    .await?;
    let platform_supported = super::capture::platform_supported();
    let reason = if !platform_supported {
        Some("智能场景便签只支持 Windows 10 1903（18362）或更高版本；Android 保持原功能，不执行画面监测。".to_owned())
    } else if !enabled {
        Some("监测默认关闭；开启前会显示本地画面采样说明。".to_owned())
    } else if paused {
        Some("智能场景便签目前已暂停。".to_owned())
    } else if allowed_rule_count == 0 {
        Some("没有已启用的规则，或规则未填写允许程序。".to_owned())
    } else {
        None
    };
    Ok(SceneMonitorStatus {
        platform_supported,
        enabled,
        paused,
        capture_armed,
        allowed_rule_count: allowed_rule_count as usize,
        reason,
    })
}

pub async fn set_monitor_enabled(
    db: &Db,
    enabled: bool,
    consent: bool,
) -> AppResult<SceneMonitorStatus> {
    if enabled && !super::capture::platform_supported() {
        return Err(AppError::validation(
            "当前 Windows 版本不支持按窗口安全采集；智能场景监测没有启动。",
        ));
    }
    if enabled && !consent {
        return Err(AppError::validation("首次开启必须明确确认：仅采样允许程序的前台窗口，图像留在内存和本机规则库，不上传屏幕。"));
    }
    let now = now_stamp();
    sqlx::query(
        "UPDATE scene_monitor_settings SET enabled=?,paused=0,consented_at=CASE WHEN ? THEN COALESCE(consented_at,?) ELSE consented_at END,updated_at=? WHERE id='default'",
    )
    .bind(enabled).bind(enabled).bind(&now).bind(&now).execute(db.pool()).await?;
    if !enabled {
        sqlx::query("UPDATE scene_runtime SET state='outside',window_identity_hash=NULL,last_seen_at=NULL,snooze_until=NULL,updated_at=?")
            .bind(&now).execute(db.pool()).await?;
    }
    monitor_status(db, false).await
}

pub async fn set_monitor_paused(db: &Db, paused: bool) -> AppResult<SceneMonitorStatus> {
    let now = now_stamp();
    sqlx::query("UPDATE scene_monitor_settings SET paused=?,updated_at=? WHERE id='default'")
        .bind(paused)
        .bind(&now)
        .execute(db.pool())
        .await?;
    if paused {
        sqlx::query("UPDATE scene_runtime SET state='outside',window_identity_hash=NULL,last_seen_at=NULL,snooze_until=NULL,updated_at=?")
            .bind(&now).execute(db.pool()).await?;
    }
    monitor_status(db, false).await
}

pub async fn record_event(
    db: &Db,
    rule_id: Option<&str>,
    event_type: &str,
    result_code: &str,
    image_score: Option<f64>,
    elapsed_ms: u64,
) -> AppResult<()> {
    if event_type.len() > 40 || result_code.len() > 60 {
        return Err(AppError::validation("场景诊断事件字段超长"));
    }
    sqlx::query("INSERT INTO scene_events (id,rule_id,occurred_at,event_type,result_code,image_score,elapsed_ms) VALUES (?,?,?,?,?,?,?)")
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(rule_id)
        .bind(now_stamp())
        .bind(event_type)
        .bind(result_code)
        .bind(image_score)
        .bind(i64::try_from(elapsed_ms).unwrap_or(i64::MAX))
        .execute(db.pool()).await?;
    sqlx::query("DELETE FROM scene_events WHERE id IN (SELECT id FROM scene_events ORDER BY occurred_at DESC,id DESC LIMIT -1 OFFSET 1000)")
        .execute(db.pool()).await?;
    Ok(())
}

pub async fn list_events(db: &Db, limit: i64) -> AppResult<Vec<SceneEvent>> {
    let limit = limit.clamp(1, 200);
    let rows = sqlx::query("SELECT id,rule_id,occurred_at,event_type,result_code,image_score,elapsed_ms FROM scene_events ORDER BY occurred_at DESC,id DESC LIMIT ?")
        .bind(limit).fetch_all(db.pool()).await?;
    rows.iter()
        .map(|row| {
            Ok(SceneEvent {
                id: row.try_get("id")?,
                rule_id: row.try_get("rule_id")?,
                occurred_at: row.try_get("occurred_at")?,
                event_type: row.try_get("event_type")?,
                result_code: row.try_get("result_code")?,
                image_score: row.try_get("image_score")?,
                elapsed_ms: row.try_get("elapsed_ms")?,
            })
        })
        .collect()
}

#[tauri::command]
pub async fn scene_rules_list(
    state: State<'_, AppState>,
    include_deleted: Option<bool>,
) -> AppResult<Vec<SceneRule>> {
    list_rules(&state.db, include_deleted.unwrap_or(false)).await
}

#[tauri::command]
pub async fn scene_rule_get(state: State<'_, AppState>, id: String) -> AppResult<SceneRule> {
    get_rule(&state.db, &id).await
}

#[tauri::command]
pub async fn scene_rule_save(
    state: State<'_, AppState>,
    input: SceneRuleInput,
) -> AppResult<SceneRule> {
    save_rule(&state.db, input).await
}

#[tauri::command]
pub async fn scene_rule_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, SceneRuntime>,
    id: String,
) -> AppResult<()> {
    delete_rule(&state.db, &id).await?;
    runtime.reset_rule(&app, &id);
    Ok(())
}

#[tauri::command]
pub async fn scene_rule_set_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, SceneRuntime>,
    id: String,
    enabled: bool,
) -> AppResult<SceneRule> {
    let rule = set_rule_enabled(&state.db, &id, enabled).await?;
    if !enabled {
        runtime.reset_rule(&app, &id);
    }
    Ok(rule)
}

#[tauri::command]
pub async fn scene_monitor_status(
    state: State<'_, AppState>,
    runtime: State<'_, SceneRuntime>,
) -> AppResult<SceneMonitorStatus> {
    monitor_status(&state.db, runtime.capture_armed()).await
}

#[tauri::command]
pub async fn scene_monitor_set_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, SceneRuntime>,
    enabled: bool,
    consent: bool,
) -> AppResult<SceneMonitorStatus> {
    let status = set_monitor_enabled(&state.db, enabled, consent).await?;
    if !enabled {
        runtime.reset_all(&app);
    }
    Ok(status)
}

#[tauri::command]
pub async fn scene_monitor_set_paused(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, SceneRuntime>,
    paused: bool,
) -> AppResult<SceneMonitorStatus> {
    let status = set_monitor_paused(&state.db, paused).await?;
    if paused {
        runtime.reset_all(&app);
    }
    Ok(status)
}

#[tauri::command]
pub async fn scene_events_list(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> AppResult<Vec<SceneEvent>> {
    list_events(&state.db, limit.unwrap_or(50)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::scene_notes::models::SceneMatchMode;

    #[tokio::test]
    async fn rule_write_checks_allowlist_and_revision_in_isolated_database() {
        let dir = std::env::temp_dir().join(format!("lumen-scene-rule-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.unwrap();
        let invalid = SceneRuleInput {
            id: None,
            expected_revision: None,
            name: "无白名单".into(),
            enabled: false,
            priority: 0,
            program_allowlist: vec![],
            window_title_contains: vec![],
            match_mode: SceneMatchMode::Ocr,
            template_config: None,
            template_image_base64: None,
            existing_template_id: None,
            required_terms: vec!["FNSKU".into()],
            excluded_terms: vec![],
            headline: "只测试当前已授权应用".into(),
            body_md: "".into(),
            memo_id: None,
            step_id: None,
            stable_hits: 2,
            stable_leave_ms: 6000,
            default_snooze_minutes: 10,
        };
        assert!(save_rule(&db, invalid).await.is_err());

        let input = SceneRuleInput {
            id: None,
            expected_revision: None,
            name: "标签编辑".into(),
            enabled: false,
            priority: 0,
            program_allowlist: vec!["Editor.EXE".into()],
            window_title_contains: vec![],
            match_mode: SceneMatchMode::Ocr,
            template_config: None,
            template_image_base64: None,
            existing_template_id: None,
            required_terms: vec!["FNSKU".into()],
            excluded_terms: vec![],
            headline: "FNSKU 要扫码试一下！".into(),
            body_md: "".into(),
            memo_id: None,
            step_id: None,
            stable_hits: 2,
            stable_leave_ms: 6000,
            default_snooze_minutes: 10,
        };
        let saved = save_rule(&db, input).await.unwrap();
        assert_eq!(saved.program_allowlist, ["editor.exe"]);
        assert!(
            !saved.enabled,
            "new rules start disabled until the user enables them"
        );
        let mut stale = SceneRuleInput {
            id: Some(saved.id.clone()),
            expected_revision: Some(0),
            name: "覆盖旧版本".into(),
            enabled: false,
            priority: 0,
            program_allowlist: vec!["editor.exe".into()],
            window_title_contains: vec![],
            match_mode: SceneMatchMode::Ocr,
            template_config: None,
            template_image_base64: None,
            existing_template_id: None,
            required_terms: vec!["FNSKU".into()],
            excluded_terms: vec![],
            headline: "不应覆盖".into(),
            body_md: "".into(),
            memo_id: None,
            step_id: None,
            stable_hits: 2,
            stable_leave_ms: 6000,
            default_snooze_minutes: 10,
        };
        stale.expected_revision = Some(0);
        assert!(save_rule(&db, stale).await.is_err());
        let stored = get_rule(&db, &saved.id).await.unwrap();
        assert_eq!(stored.revision, 1);
        assert_eq!(stored.headline, "FNSKU 要扫码试一下！");
        db.pool().close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }
}
