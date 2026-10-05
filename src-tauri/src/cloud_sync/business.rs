//! Table rows are indivisible conflict units. A snapshot transaction journals changes
//! since the last persisted baseline; restarting captures even changes made while offline.
use super::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
pub const TABLES: [&str; 19] = [
    "projects",
    "categories",
    "tags",
    "task_series",
    "task_series_template",
    "task_series_tags",
    "task_series_skips",
    "task_series_rebuilds",
    "tasks",
    "task_series_segments",
    "task_tags",
    "subtasks",
    "task_dependencies",
    "reminders",
    "attachments",
    "focus_sessions",
    "goals",
    "settings",
    "knowledge_sources",
];
const KNOWLEDGE_TABLE: &str = "knowledge_sources";
const ALIAS_TABLES: [&str; 7] = [
    "projects",
    "categories",
    "tags",
    "tasks",
    "subtasks",
    "knowledge_sources",
    "content_assets",
];
const NORMALIZED_SYNC_TIMESTAMP: &str = "2000-01-01T00:00:00Z";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Event {
    pub version: u32,
    pub id: String,
    pub parents: Vec<String>,
    pub record_id: String,
    pub table: String,
    pub key: Value,
    pub row: Option<Value>,
    pub created_at: String,
}
fn record_id(table: &str, key: &Value) -> AppResult<String> {
    let hash = Sha256::digest(format!("{table}:{}", encoded(key)?).as_bytes());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash[..16]);
    Ok(uuid::Uuid::from_bytes(bytes).to_string())
}
fn synchronized_setting(key: &str) -> bool {
    // Device-specific windows, shortcuts, AI endpoints/credentials and cloud connection stay local.
    matches!(key, "holiday_calendar" | "growth_config")
}
async fn primary(conn: &mut SqliteConnection, table: &str) -> AppResult<Vec<String>> {
    if !TABLES.contains(&table) {
        return Err(AppError::validation("云端包含不支持的业务类型"));
    }
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!("PRAGMA table_info({table})")))
        .fetch_all(conn)
        .await?;
    let mut cols = Vec::new();
    for row in rows {
        let pk: i64 = row.try_get("pk")?;
        if pk > 0 {
            cols.push((pk, row.try_get::<String, _>("name")?));
        }
    }
    cols.sort();
    Ok(cols.into_iter().map(|(_, n)| n).collect())
}
fn key_of(row: &Value, cols: &[String]) -> AppResult<Value> {
    let obj = row
        .as_object()
        .ok_or_else(|| AppError::validation("云端业务记录格式无效"))?;
    let mut key = serde_json::Map::new();
    for col in cols {
        key.insert(
            col.clone(),
            obj.get(col)
                .filter(|v| !v.is_null())
                .ok_or_else(|| AppError::validation("业务记录缺少编号"))?
                .clone(),
        );
    }
    Ok(Value::Object(key))
}
fn portable_attachment(row: &mut Value) -> AppResult<()> {
    let obj = row
        .as_object_mut()
        .ok_or_else(|| AppError::validation("附件记录格式无效"))?;
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::validation("附件编号缺失"))?;
    id_ok(id)?;
    let name = obj
        .get("file_name")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::validation("附件文件名缺失"))?;
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("");
    if ext.len() > 20 || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(AppError::validation("附件扩展名无法安全同步"));
    }
    let path = format!(
        "attachments/cloud-{id}{}",
        if ext.is_empty() {
            String::new()
        } else {
            format!(".{ext}")
        }
    );
    obj.insert("stored_path".into(), Value::String(path));
    obj.insert("external_path".into(), Value::Null);
    obj.insert("storage_mode".into(), Value::String("copied".into()));
    Ok(())
}
async fn heads(
    conn: &mut SqliteConnection,
    id: Option<&str>,
    uploaded_only: bool,
) -> AppResult<Vec<String>> {
    Ok(sqlx::query_scalar("SELECT e.id FROM cloud_events e WHERE (? IS NULL OR record_id=?) AND (?=0 OR uploaded=1) AND NOT EXISTS (SELECT 1 FROM cloud_parents p JOIN cloud_events child ON child.id=p.event_id WHERE p.parent_id=e.id AND (?=0 OR child.uploaded=1)) ORDER BY e.id")
        .bind(id).bind(id).bind(uploaded_only).bind(uploaded_only).fetch_all(conn).await?)
}
async fn insert(conn: &mut SqliteConnection, event: &Event, uploaded: bool) -> AppResult<()> {
    validate_event(conn, event).await?;
    insert_validated(conn, event, uploaded).await
}
async fn validate_event(conn: &mut SqliteConnection, event: &Event) -> AppResult<()> {
    id_ok(&event.id)?;
    id_ok(&event.record_id)?;
    if event.version != 1
        || event.parents.len() > MAX_HEADS
        || record_id(&event.table, &event.key)? != event.record_id
    {
        return Err(AppError::validation("业务同步版本无效"));
    }
    let pk = primary(conn, &event.table).await?;
    if key_of(&event.key, &pk)? != event.key {
        return Err(AppError::validation("业务同步编号不匹配"));
    }
    if event.table == "settings"
        && !event
            .key
            .get("key")
            .and_then(Value::as_str)
            .is_some_and(synchronized_setting)
    {
        return Err(AppError::validation("云端不能修改本机私有设置"));
    }
    if let Some(row) = &event.row {
        if key_of(row, &pk)? != event.key {
            return Err(AppError::validation("云端记录与编号不匹配"));
        }
        let columns = sqlx::query(sqlx::AssertSqlSafe(format!(
            "PRAGMA table_info({})",
            event.table
        )))
        .fetch_all(&mut *conn)
        .await?;
        let names: HashSet<String> = columns.iter().map(|r| r.get("name")).collect();
        if row.as_object().is_none_or(|o| {
            o.keys().any(|k| !names.contains(k))
                || o.values().any(|v| v.is_array() || v.is_object())
        }) {
            return Err(AppError::validation("云端业务字段不兼容"));
        }
        if event.table == "attachments" {
            let mut safe = row.clone();
            portable_attachment(&mut safe)?;
            if safe != *row {
                return Err(AppError::validation("云端附件路径无效"));
            }
        }
        if event.table == KNOWLEDGE_TABLE {
            validate_knowledge_source(row)?;
        }
    }
    Ok(())
}
fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn validate_knowledge_source(row: &Value) -> AppResult<()> {
    let title = row
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::validation("知识库来源标题无效"))?;
    let asset_id = row
        .get("asset_id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::validation("知识库原件编号缺失"))?;
    let sha256 = row
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::validation("知识库原件摘要缺失"))?;
    let extracted = row
        .get("extracted_text")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::validation("知识库解析正文无效"))?;
    let warnings = row
        .get("warnings_json")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::validation("知识库解析告警无效"))?;
    if title.trim().is_empty()
        || title.chars().count() > 255
        || extracted.chars().count() > 100_000
        || warnings.len() > 1024 * 1024
        || serde_json::from_str::<Vec<String>>(warnings).is_err()
        || !valid_sha256(sha256)
        || !matches!(
            row.get("status").and_then(Value::as_str),
            Some("ready" | "unreadable")
        )
    {
        return Err(AppError::validation("知识库同步记录超出支持范围或格式无效"));
    }
    id_ok(asset_id)?;
    Ok(())
}
async fn insert_validated(
    conn: &mut SqliteConnection,
    event: &Event,
    uploaded: bool,
) -> AppResult<()> {
    let payload = encoded(event)?;
    if let Some(existing) =
        sqlx::query_scalar::<_, String>("SELECT payload_json FROM cloud_events WHERE id=?")
            .bind(&event.id)
            .fetch_optional(&mut *conn)
            .await?
    {
        if existing != payload {
            return Err(AppError::conflict("同一业务版本编号出现不同内容"));
        }
        return Ok(());
    }
    let mut parents = HashSet::new();
    for p in &event.parents {
        if p == &event.id || !parents.insert(p) {
            return Err(AppError::validation("业务同步父版本重复或循环"));
        }
        let record: Option<String> =
            sqlx::query_scalar("SELECT record_id FROM cloud_events WHERE id=?")
                .bind(p)
                .fetch_optional(&mut *conn)
                .await?;
        if record.as_deref() != Some(&event.record_id) {
            return Err(AppError::validation("业务同步父版本缺失"));
        }
    }
    sqlx::query("INSERT INTO cloud_events (id,record_id,payload_json,uploaded) VALUES (?,?,?,?)")
        .bind(&event.id)
        .bind(&event.record_id)
        .bind(payload)
        .bind(uploaded)
        .execute(&mut *conn)
        .await?;
    for p in &event.parents {
        sqlx::query("INSERT INTO cloud_parents (event_id,parent_id) VALUES (?,?)")
            .bind(&event.id)
            .bind(p)
            .execute(&mut *conn)
            .await?;
    }
    sqlx::query("INSERT OR IGNORE INTO cloud_records (id,table_name,key_json) VALUES (?,?,?)")
        .bind(&event.record_id)
        .bind(&event.table)
        .bind(encoded(&event.key)?)
        .execute(&mut *conn)
        .await?;
    dirty_head(conn).await?;
    Ok(())
}
pub async fn capture_scope(db: &Db, include_knowledge: bool) -> AppResult<()> {
    let mut tx = db.pool().begin().await?;
    capture_from(&mut tx, db.data_dir(), include_knowledge).await?;
    tx.commit().await?;
    Ok(())
}
fn stable_id(value: &str) -> String {
    let hash = Sha256::digest(value.as_bytes());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash[..16]);
    uuid::Uuid::from_bytes(bytes).to_string()
}
type Aliases = std::collections::HashMap<(String, String), String>;
async fn aliases(conn: &mut SqliteConnection) -> AppResult<Aliases> {
    let mut out = Aliases::new();
    for table in [
        "projects",
        "categories",
        "tags",
        "tasks",
        "subtasks",
        KNOWLEDGE_TABLE,
    ] {
        let rows = crate::backup::dump_table_from(conn, table).await?;
        for row in rows {
            let id = row
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::validation("业务编号缺失"))?;
            let saved: Option<String> = sqlx::query_scalar(
                "SELECT canonical_id FROM cloud_aliases WHERE table_name=? AND local_id=?",
            )
            .bind(table)
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;
            let canonical = if let Some(saved) = saved {
                saved
            } else {
                let record = record_id(table, &serde_json::json!({"id":id}))?;
                let from_cloud: bool =
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_records WHERE id=?)")
                        .bind(record)
                        .fetch_one(&mut *conn)
                        .await?;
                let candidate = if table == KNOWLEDGE_TABLE {
                    let hash = row
                        .get("sha256")
                        .and_then(Value::as_str)
                        .ok_or_else(|| AppError::validation("知识库资料缺少内容摘要"))?;
                    stable_id(&format!("lumen-knowledge-source:{hash}"))
                } else if from_cloud {
                    id.to_owned()
                } else if matches!(table, "projects" | "categories" | "tags")
                    && row.get("deleted_at").is_some_and(Value::is_null)
                {
                    stable_id(&format!(
                        "lumen-{table}:{}",
                        row.get("name").and_then(Value::as_str).unwrap_or(id)
                    ))
                } else if table == "tasks" {
                    match (
                        row.get("series_id").and_then(Value::as_str),
                        row.get("occurrence_key").and_then(Value::as_str),
                    ) {
                        (Some(series), Some(occurrence)) => {
                            stable_id(&format!("lumen-occurrence:{series}:{occurrence}"))
                        }
                        _ => id.to_owned(),
                    }
                } else if table == "subtasks" {
                    let task = row.get("task_id").and_then(Value::as_str).unwrap_or("");
                    match row.get("series_template_id").and_then(Value::as_str) {
                        Some(template) => stable_id(&format!(
                            "lumen-subtask:{}:{template}",
                            out.get(&("tasks".into(), task.into()))
                                .map(String::as_str)
                                .unwrap_or(task)
                        )),
                        None => id.to_owned(),
                    }
                } else {
                    id.to_owned()
                };
                let reused:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_aliases WHERE table_name=? AND canonical_id=? AND local_id<>?)").bind(table).bind(&candidate).bind(id).fetch_one(&mut *conn).await?;
                // Recreating a soft-deleted named item is a distinct local record.
                let candidate = if reused && matches!(table, "projects" | "categories" | "tags") {
                    id.to_owned()
                } else {
                    candidate
                };
                sqlx::query(
                    "INSERT INTO cloud_aliases(table_name,local_id,canonical_id) VALUES(?,?,?)",
                )
                .bind(table)
                .bind(id)
                .bind(&candidate)
                .execute(&mut *conn)
                .await?;
                candidate
            };
            out.insert((table.into(), id.into()), canonical);
        }
    }
    let sources = crate::backup::dump_table_from(conn, KNOWLEDGE_TABLE).await?;
    for row in sources {
        let local_id = row
            .get("asset_id")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::validation("知识库原件编号缺失"))?;
        let hash = row
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::validation("知识库资料缺少内容摘要"))?;
        let canonical = stable_id(&format!("lumen-knowledge-asset:{hash}"));
        let saved: Option<String> = sqlx::query_scalar(
            "SELECT canonical_id FROM cloud_aliases WHERE table_name='content_assets' AND local_id=?",
        )
        .bind(local_id)
        .fetch_optional(&mut *conn)
        .await?;
        if saved.as_ref().is_some_and(|value| value != &canonical) {
            return Err(AppError::conflict("知识库原件的云端编号与内容摘要不一致"));
        }
        if saved.is_none() {
            sqlx::query("INSERT INTO cloud_aliases(table_name,local_id,canonical_id) VALUES('content_assets',?,?)")
                .bind(local_id)
                .bind(&canonical)
                .execute(&mut *conn)
                .await?;
        }
        out.insert(("content_assets".into(), local_id.into()), canonical);
    }
    Ok(out)
}
fn normalize(table: &str, row: &mut Value, aliases: &Aliases) {
    let Some(obj) = row.as_object_mut() else {
        return;
    };
    for (column, target) in [
        ("id", table),
        ("task_id", "tasks"),
        ("depends_on_id", "tasks"),
        ("project_id", "projects"),
        ("category_id", "categories"),
        ("tag_id", "tags"),
        ("asset_id", "content_assets"),
    ] {
        if column == "asset_id" && table != KNOWLEDGE_TABLE {
            continue;
        }
        if let Some(id) = obj.get(column).and_then(Value::as_str) {
            if let Some(canonical) = aliases.get(&(target.into(), id.into())) {
                obj.insert(column.into(), Value::String(canonical.clone()));
            }
        }
    }
    if table == KNOWLEDGE_TABLE {
        obj.insert(
            "created_at".into(),
            Value::String(NORMALIZED_SYNC_TIMESTAMP.into()),
        );
        obj.insert(
            "updated_at".into(),
            Value::String(NORMALIZED_SYNC_TIMESTAMP.into()),
        );
    }
}
async fn local_id(conn: &mut SqliteConnection, table: &str, canonical: &str) -> AppResult<String> {
    if !TABLES.contains(&table) && !ALIAS_TABLES.contains(&table) {
        return Err(AppError::validation("云端包含不支持的业务类型"));
    }
    // Rebuilding a generated occurrence leaves its old alias in durable history.
    // Prefer a live row; retain the historical fallback for tombstones.
    let sql = format!("SELECT a.local_id FROM cloud_aliases a LEFT JOIN {table} live ON live.id=a.local_id WHERE a.table_name=? AND a.canonical_id=? ORDER BY live.id IS NULL,a.local_id LIMIT 1");
    let local: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(table)
        .bind(canonical)
        .fetch_optional(conn)
        .await?;
    Ok(local.unwrap_or_else(|| canonical.to_owned()))
}
async fn localize(conn: &mut SqliteConnection, table: &str, row: &Value) -> AppResult<Value> {
    let mut value = row.clone();
    let Some(obj) = value.as_object_mut() else {
        return Err(AppError::validation("业务字段无效"));
    };
    for (column, target) in [
        ("id", table),
        ("task_id", "tasks"),
        ("depends_on_id", "tasks"),
        ("project_id", "projects"),
        ("category_id", "categories"),
        ("tag_id", "tags"),
        ("asset_id", "content_assets"),
    ] {
        if column == "asset_id" && table != KNOWLEDGE_TABLE {
            continue;
        }
        if let Some(id) = obj.get(column).and_then(Value::as_str).map(str::to_owned) {
            obj.insert(
                column.into(),
                Value::String(local_id(conn, target, &id).await?),
            );
        }
    }
    Ok(value)
}

async fn capture_from(
    tx: &mut SqliteConnection,
    dir: &std::path::Path,
    include_knowledge: bool,
) -> AppResult<()> {
    // Acquire a write reservation before reading; otherwise another writer can invalidate the snapshot.
    sqlx::query("UPDATE memo_sync_runtime SET request_count=request_count WHERE singleton=1")
        .execute(&mut *tx)
        .await?;
    let mapping = aliases(tx).await?;
    for table in TABLES {
        if table == KNOWLEDGE_TABLE && !include_knowledge {
            continue;
        }
        let pk = primary(tx, table).await?;
        let rows = crate::backup::dump_table_from(tx, table).await?;
        let mut present = HashSet::new();
        for mut row in rows {
            if table == "settings"
                && !row
                    .get("key")
                    .and_then(Value::as_str)
                    .is_some_and(synchronized_setting)
            {
                continue;
            }
            if table == "attachments" {
                cache_attachment(tx, dir, &row).await?;
                portable_attachment(&mut row)?;
            }
            normalize(table, &mut row, &mapping);
            let key = key_of(&row, &pk)?;
            let id = record_id(table, &key)?;
            present.insert(id.clone());
            let base = sqlx::query("SELECT base_json,selected_event FROM cloud_records WHERE id=?")
                .bind(&id)
                .fetch_optional(&mut *tx)
                .await?;
            let value = encoded(&row)?;
            let old = base
                .as_ref()
                .and_then(|r| r.get::<Option<String>, _>("base_json"));
            if old.as_deref() == Some(&value) {
                continue;
            }
            let parents = base
                .as_ref()
                .and_then(|r| r.get::<Option<String>, _>("selected_event"))
                .into_iter()
                .collect();
            let event = Event {
                version: 1,
                id: uuid::Uuid::now_v7().to_string(),
                parents,
                record_id: id.clone(),
                table: table.into(),
                key,
                row: Some(row),
                created_at: stamp(),
            };
            insert(tx, &event, false).await?;
            sqlx::query("UPDATE cloud_records SET base_json=?,selected_event=? WHERE id=?")
                .bind(value)
                .bind(&event.id)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        let previous=sqlx::query("SELECT id,key_json,selected_event FROM cloud_records WHERE table_name=? AND base_json IS NOT NULL").bind(table).fetch_all(&mut *tx).await?;
        for row in previous {
            let id: String = row.get("id");
            if present.contains(&id) {
                continue;
            }
            let event = Event {
                version: 1,
                id: uuid::Uuid::now_v7().to_string(),
                parents: row
                    .get::<Option<String>, _>("selected_event")
                    .into_iter()
                    .collect(),
                record_id: id.clone(),
                table: table.into(),
                key: decoded(&row.get::<String, _>("key_json"))?,
                row: None,
                created_at: stamp(),
            };
            insert(tx, &event, false).await?;
            sqlx::query("UPDATE cloud_records SET base_json=NULL,selected_event=? WHERE id=?")
                .bind(&event.id)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
    }
    Ok(())
}
pub(super) async fn rebase_restore(
    conn: &mut SqliteConnection,
    dir: &std::path::Path,
    include_knowledge: bool,
) -> AppResult<()> {
    let tombstones=sqlx::query("SELECT id,table_name,key_json,selected_event FROM cloud_records WHERE base_json IS NULL AND selected_event IS NOT NULL AND (? OR table_name<>?)")
        .bind(include_knowledge)
        .bind(KNOWLEDGE_TABLE)
        .fetch_all(&mut *conn)
        .await?;
    for row in tombstones {
        let event = Event {
            version: 1,
            id: uuid::Uuid::now_v7().to_string(),
            parents: row
                .get::<Option<String>, _>("selected_event")
                .into_iter()
                .collect(),
            record_id: row.get("id"),
            table: row.get("table_name"),
            key: decoded(&row.get::<String, _>("key_json"))?,
            row: None,
            created_at: stamp(),
        };
        insert(conn, &event, false).await?;
        sqlx::query("UPDATE cloud_records SET selected_event=? WHERE id=?")
            .bind(&event.id)
            .bind(&event.record_id)
            .execute(&mut *conn)
            .await?;
    }
    sqlx::query("UPDATE cloud_records SET base_json=NULL WHERE (? OR table_name<>?)")
        .bind(include_knowledge)
        .bind(KNOWLEDGE_TABLE)
        .execute(&mut *conn)
        .await?;
    capture_from(conn, dir, include_knowledge).await
}

fn media_ids(row: &Value) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"lumen-asset:([0-9a-fA-F-]{36})\)").expect("constant regex")
    });
    row.as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .filter_map(Value::as_str)
        .flat_map(|s| re.captures_iter(s).map(|c| c[1].to_lowercase()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KnowledgeAsset {
    sha256: String,
    byte_size: i64,
    data_base64: String,
}
fn knowledge_asset_id(hash: &str) -> String {
    stable_id(&format!("lumen-knowledge-asset:{hash}"))
}
async fn upload_knowledge_asset(
    db: &Db,
    dav: &Dav,
    c: &Config,
    key: &[u8; 32],
    row: &Value,
) -> AppResult<()> {
    validate_knowledge_source(row)?;
    let canonical = row["asset_id"]
        .as_str()
        .ok_or_else(|| AppError::validation("知识库原件编号缺失"))?;
    let hash = row["sha256"]
        .as_str()
        .ok_or_else(|| AppError::validation("知识库原件摘要缺失"))?;
    if canonical != knowledge_asset_id(hash) {
        return Err(AppError::validation("知识库原件云端编号与摘要不匹配"));
    }
    let mut conn = db.pool().acquire().await?;
    let local = local_id(&mut conn, "content_assets", canonical).await?;
    let asset = content_assets::get_asset(db, &local).await?;
    if asset.sha256 != hash {
        return Err(AppError::conflict("本机知识库原件与同步摘要不一致"));
    }
    let portable = KnowledgeAsset {
        sha256: asset.sha256,
        byte_size: asset.byte_size,
        data_base64: asset.data_base64,
    };
    upload_immutable(
        dav,
        db,
        c,
        key,
        &bucket("assets", canonical)?,
        &portable,
        40 * 1024 * 1024,
    )
    .await
}
async fn ensure_asset_alias(
    conn: &mut SqliteConnection,
    local: &str,
    canonical: &str,
) -> AppResult<()> {
    let saved: Option<String> = sqlx::query_scalar(
        "SELECT canonical_id FROM cloud_aliases WHERE table_name='content_assets' AND local_id=?",
    )
    .bind(local)
    .fetch_optional(&mut *conn)
    .await?;
    if saved.as_deref().is_some_and(|value| value != canonical) {
        return Err(AppError::conflict("知识库原件已关联到另一份云端资料"));
    }
    if saved.is_none() {
        sqlx::query("INSERT INTO cloud_aliases(table_name,local_id,canonical_id) VALUES('content_assets',?,?)")
            .bind(local)
            .bind(canonical)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
async fn import_knowledge_asset(
    db: &Db,
    dav: &Dav,
    c: &Config,
    key: &[u8; 32],
    row: &Value,
) -> AppResult<()> {
    validate_knowledge_source(row)?;
    let hash = row["sha256"]
        .as_str()
        .ok_or_else(|| AppError::validation("知识库原件摘要缺失"))?;
    let canonical = row["asset_id"]
        .as_str()
        .ok_or_else(|| AppError::validation("知识库原件编号缺失"))?;
    if canonical != knowledge_asset_id(hash) {
        return Err(AppError::validation("知识库原件云端编号与摘要不匹配"));
    }
    let local: Option<String> =
        sqlx::query_scalar("SELECT id FROM content_assets WHERE sha256=? ORDER BY id LIMIT 1")
            .bind(hash)
            .fetch_optional(db.pool())
            .await?;
    if let Some(local) = local {
        content_assets::get_asset(db, &local).await?;
        let mut tx = db.pool().begin().await?;
        ensure_asset_alias(&mut tx, &local, canonical).await?;
        tx.commit().await?;
        return Ok(());
    }
    let path = bucket("assets", canonical)?;
    let bytes = dav
        .read(db, &path, 40 * 1024 * 1024)
        .await?
        .ok_or_else(|| AppError::validation("云端知识库原件缺失，没有应用该版本"))?;
    let portable: KnowledgeAsset = decrypt(key, &aad(c, &path), &bytes)?;
    if portable.sha256 != hash
        || portable.data_base64.len() > content_assets::MAX_ASSET_BYTES.div_ceil(3) * 4
    {
        return Err(AppError::validation("云端知识库原件摘要或长度无效"));
    }
    let decoded = STANDARD
        .decode(&portable.data_base64)
        .map_err(|_| AppError::validation("云端知识库原件编码损坏"))?;
    if decoded.len() > content_assets::MAX_ASSET_BYTES
        || decoded.len() as i64 != portable.byte_size
        || hex::encode(Sha256::digest(&decoded)) != hash
    {
        return Err(AppError::validation("云端知识库原件校验失败"));
    }
    let title = row["title"]
        .as_str()
        .ok_or_else(|| AppError::validation("知识库来源标题无效"))?;
    let mime = row["mime"]
        .as_str()
        .ok_or_else(|| AppError::validation("知识库来源类型无效"))?;
    let asset = ContentAsset {
        id: canonical.into(),
        name: title.into(),
        mime: mime.into(),
        data_base64: portable.data_base64,
        byte_size: portable.byte_size,
        sha256: hash.into(),
        created_at: stamp(),
    };
    content_assets::decode_asset(&asset)?;
    let mut tx = db.pool().begin().await?;
    sqlx::query("INSERT INTO content_assets (id,name,mime,data_base64,byte_size,sha256,created_at) VALUES (?,?,?,?,?,?,?)")
        .bind(&asset.id).bind(&asset.name).bind(&asset.mime).bind(&asset.data_base64)
        .bind(asset.byte_size).bind(&asset.sha256).bind(&asset.created_at)
        .execute(&mut *tx).await?;
    ensure_asset_alias(&mut tx, canonical, canonical).await?;
    tx.commit().await?;
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachmentBody {
    id: String,
    sha256: String,
    data: String,
}
async fn cache_attachment(
    conn: &mut SqliteConnection,
    dir: &std::path::Path,
    row: &Value,
) -> AppResult<()> {
    let id = row
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::validation("附件编号缺失"))?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cloud_files WHERE id=?)")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    if exists {
        return Ok(());
    }
    let copied = row.get("storage_mode").and_then(Value::as_str) == Some("copied");
    let name = row
        .get("file_name")
        .and_then(Value::as_str)
        .unwrap_or("附件");
    let path = row
        .get(if copied {
            "stored_path"
        } else {
            "external_path"
        })
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::validation("附件文件路径缺失"))?;
    let path = if copied {
        crate::attachments::resolve_stored_path(dir, path)
    } else {
        std::path::PathBuf::from(path)
    };
    let file = std::fs::File::open(path).map_err(|_| {
        AppError::validation(format!(
            "附件「{name}」原文件无法读取，请恢复文件后重试同步"
        ))
    })?;
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take(200 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 200 * 1024 * 1024 {
        return Err(AppError::validation("任务附件超过 200 MiB"));
    }
    let body = AttachmentBody {
        id: id.into(),
        sha256: hex::encode(Sha256::digest(&bytes)),
        data: STANDARD.encode(bytes),
    };
    sqlx::query("INSERT INTO cloud_files(id,body_json) VALUES(?,?)")
        .bind(id)
        .bind(encoded(&body)?)
        .execute(conn)
        .await?;
    Ok(())
}
async fn attachment_upload(
    db: &Db,
    dav: &Dav,
    c: &Config,
    key: &[u8; 32],
    event: &Event,
) -> AppResult<()> {
    let id = event
        .key
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::validation("附件编号缺失"))?;
    let raw: String = sqlx::query_scalar("SELECT body_json FROM cloud_files WHERE id=?")
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| AppError::validation("附件缺少历史副本，没有假报上传成功"))?;
    let body: AttachmentBody = decoded(&raw)?;
    validate_file(&body, id)?;
    upload_immutable(
        dav,
        db,
        c,
        key,
        &bucket("files", id)?,
        &body,
        400 * 1024 * 1024,
    )
    .await
}
fn validate_file(body: &AttachmentBody, id: &str) -> AppResult<Vec<u8>> {
    id_ok(id)?;
    if body.id != id || body.data.len() > 280 * 1024 * 1024 {
        return Err(AppError::validation("任务附件编号或长度无效"));
    }
    let bytes = STANDARD
        .decode(&body.data)
        .map_err(|_| AppError::validation("任务附件编码损坏"))?;
    if bytes.len() > 200 * 1024 * 1024 || hex::encode(Sha256::digest(&bytes)) != body.sha256 {
        return Err(AppError::validation("任务附件校验失败"));
    }
    Ok(bytes)
}

async fn attachment_download(
    db: &Db,
    dav: &Dav,
    c: &Config,
    key: &[u8; 32],
    row: &Value,
) -> AppResult<()> {
    let id = row
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::validation("附件编号缺失"))?;
    let path = bucket("files", id)?;
    let bytes = dav
        .read(db, &path, 400 * 1024 * 1024)
        .await?
        .ok_or_else(|| AppError::validation("云端任务附件缺失，没有覆盖本机"))?;
    let body: AttachmentBody = decrypt(key, &aad(c, &path), &bytes)?;
    let cached = encoded(&body)?;
    if body.id != id || body.data.len() > 280 * 1024 * 1024 {
        return Err(AppError::validation("云端任务附件编号或长度无效"));
    }
    let bytes = STANDARD
        .decode(body.data)
        .map_err(|_| AppError::validation("云端任务附件编码损坏"))?;
    if bytes.len() > 200 * 1024 * 1024 || hex::encode(Sha256::digest(&bytes)) != body.sha256 {
        return Err(AppError::validation("云端任务附件校验失败"));
    }
    sqlx::query("INSERT INTO cloud_files(id,body_json) VALUES(?,?) ON CONFLICT(id) DO NOTHING")
        .bind(id)
        .bind(&cached)
        .execute(db.pool())
        .await?;
    let existing: String = sqlx::query_scalar("SELECT body_json FROM cloud_files WHERE id=?")
        .bind(id)
        .fetch_one(db.pool())
        .await?;
    if existing != cached {
        return Err(AppError::conflict(
            "本机与云端附件历史副本不一致，没有覆盖文件",
        ));
    }
    let stored = row
        .get("stored_path")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::validation("附件保存路径缺失"))?;
    write_cached_file(db.data_dir(), stored, &bytes)
}
fn write_cached_file(dir: &std::path::Path, stored: &str, bytes: &[u8]) -> AppResult<()> {
    let target = dir.join(stored);
    let directory = dir.join("attachments");
    std::fs::create_dir_all(&directory)?;
    if target.parent() != Some(directory.as_path()) {
        return Err(AppError::validation("附件保存路径越界"));
    }
    if target.exists() {
        if std::fs::read(&target)? != bytes {
            return Err(AppError::conflict(
                "本机附件副本与云端不一致，没有覆盖原文件",
            ));
        }
    } else {
        // Create-new prevents overwriting originals; incomplete files use a unique temporary name.
        let temp = directory.join(format!(".cloud-{}.tmp", uuid::Uuid::now_v7()));
        std::fs::write(&temp, bytes)?;
        std::fs::rename(&temp, &target)?;
    }
    Ok(())
}
pub(super) async fn restore_cached_files(
    conn: &mut SqliteConnection,
    dir: &std::path::Path,
) -> AppResult<()> {
    let rows = sqlx::query(
        "SELECT a.id,a.file_name,f.body_json FROM attachments a JOIN cloud_files f ON f.id=a.id",
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in rows {
        let id: String = row.get("id");
        let body: AttachmentBody = decoded(&row.get::<String, _>("body_json"))?;
        let bytes = validate_file(&body, &id)?;
        let mut portable =
            serde_json::json!({"id":id,"file_name":row.get::<String,_>("file_name")});
        portable_attachment(&mut portable)?;
        let stored = portable["stored_path"]
            .as_str()
            .ok_or_else(|| AppError::internal("恢复附件路径无效"))?;
        write_cached_file(dir, stored, &bytes)?;
        sqlx::query("UPDATE attachments SET stored_path=?,external_path=NULL,storage_mode='copied' WHERE id=?").bind(stored).bind(&id).execute(&mut *conn).await?;
    }
    Ok(())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Packet {
    version: u32,
    id: String,
    parents: Vec<String>,
    events: Vec<Event>,
}
fn packet_is_knowledge(packet: &Packet) -> AppResult<bool> {
    let first = packet
        .events
        .first()
        .ok_or_else(|| AppError::validation("业务批次没有事件"))?;
    let knowledge = first.table == KNOWLEDGE_TABLE;
    if packet
        .events
        .iter()
        .any(|event| (event.table == KNOWLEDGE_TABLE) != knowledge)
    {
        return Err(AppError::validation("任务与知识库不能放在同一同步批次"));
    }
    Ok(knowledge)
}
async fn packet_heads(
    conn: &mut SqliteConnection,
    published: bool,
    knowledge: bool,
) -> AppResult<Vec<String>> {
    let operator = if knowledge { "=" } else { "<>" };
    let query = format!("SELECT p.id FROM cloud_packets p WHERE (?=0 OR p.uploaded=1) AND json_extract(p.payload_json,'$.events[0].table') {operator} ? AND NOT EXISTS(SELECT 1 FROM cloud_packet_parents link JOIN cloud_packets child ON child.id=link.packet_id WHERE link.parent_id=p.id AND json_extract(child.payload_json,'$.events[0].table') {operator} ? AND (?=0 OR child.uploaded=1)) ORDER BY p.id");
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(query))
        .bind(published)
        .bind(KNOWLEDGE_TABLE)
        .bind(KNOWLEDGE_TABLE)
        .bind(published)
        .fetch_all(conn)
        .await?)
}
async fn save_packet(
    conn: &mut SqliteConnection,
    packet: &Packet,
    uploaded: bool,
) -> AppResult<()> {
    id_ok(&packet.id)?;
    if packet.version != 1
        || packet.events.is_empty()
        || packet.events.len() > 100
        || packet.parents.len() > MAX_HEADS
    {
        return Err(AppError::validation("云端业务批次格式无效"));
    }
    packet_is_knowledge(packet)?;
    let payload = encoded(packet)?;
    if payload.len() > MAX_JSON * 2 / 3 {
        return Err(AppError::validation("业务批次超过加密后 4 MiB 限制"));
    }
    if let Some(existing) =
        sqlx::query_scalar::<_, String>("SELECT payload_json FROM cloud_packets WHERE id=?")
            .bind(&packet.id)
            .fetch_optional(&mut *conn)
            .await?
    {
        if existing != payload {
            return Err(AppError::conflict("云端业务批次编号冲突"));
        }
        return Ok(());
    }
    for event in &packet.events {
        insert(conn, event, uploaded).await?;
    }
    sqlx::query("INSERT INTO cloud_packets (id,payload_json,uploaded) VALUES (?,?,?)")
        .bind(&packet.id)
        .bind(payload)
        .bind(uploaded)
        .execute(&mut *conn)
        .await?;
    let mut parents = HashSet::new();
    for parent in &packet.parents {
        id_ok(parent)?;
        if parent == &packet.id || !parents.insert(parent) {
            return Err(AppError::validation("业务批次含循环或重复父版本"));
        }
        sqlx::query("INSERT INTO cloud_packet_parents (packet_id,parent_id) VALUES (?,?)")
            .bind(&packet.id)
            .bind(parent)
            .execute(&mut *conn)
            .await?;
    }
    for event in &packet.events {
        sqlx::query("UPDATE cloud_events SET packet_id=COALESCE(packet_id,?) WHERE id=?")
            .bind(&packet.id)
            .bind(&event.id)
            .execute(&mut *conn)
            .await?;
    }
    dirty_head(conn).await?;
    Ok(())
}
async fn prune_deleted_knowledge_assets(conn: &mut SqliteConnection) -> AppResult<bool> {
    let records: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT id,selected_event FROM cloud_records
         WHERE table_name=? AND base_json IS NULL AND selected_event IS NOT NULL",
    )
    .bind(KNOWLEDGE_TABLE)
    .fetch_all(&mut *conn)
    .await?;
    let mut removed = false;
    for (record_id, selected) in records {
        let heads = heads(conn, Some(&record_id), false).await?;
        if heads.len() != 1 || selected.as_ref() != heads.first() {
            continue;
        }
        let payload: String =
            sqlx::query_scalar("SELECT payload_json FROM cloud_events WHERE id=?")
                .bind(&heads[0])
                .fetch_one(&mut *conn)
                .await?;
        let event: Event = decoded(&payload)?;
        if event.table != KNOWLEDGE_TABLE || event.row.is_some() {
            continue;
        }
        let assets: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT json_extract(payload_json,'$.row.asset_id')
             FROM cloud_events WHERE record_id=? AND json_extract(payload_json,'$.row') IS NOT NULL",
        )
        .bind(&record_id)
        .fetch_all(&mut *conn)
        .await?;
        for canonical in assets {
            let local = local_id(conn, "content_assets", &canonical).await?;
            let marker = format!("lumen-asset:{local}");
            let in_use: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM knowledge_sources WHERE asset_id=?
                    UNION ALL
                    SELECT 1 FROM memo_documents WHERE instr(body_md,?)>0 OR instr(steps_json,?)>0
                )",
            )
            .bind(&local)
            .bind(&marker)
            .bind(&marker)
            .fetch_one(&mut *conn)
            .await?;
            if !in_use {
                removed |= sqlx::query("DELETE FROM content_assets WHERE id=?")
                    .bind(local)
                    .execute(&mut *conn)
                    .await?
                    .rows_affected()
                    > 0;
            }
        }
    }
    Ok(removed)
}
async fn make_packet(db: &Db, knowledge: bool) -> AppResult<()> {
    let mut tx = db.pool().begin().await?;
    let operator = if knowledge { "=" } else { "<>" };
    let query = format!("SELECT payload_json FROM cloud_events WHERE packet_id IS NULL AND json_extract(payload_json,'$.table') {operator} ? ORDER BY rowid LIMIT 100");
    let raw: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(query))
        .bind(KNOWLEDGE_TABLE)
        .fetch_all(&mut *tx)
        .await?;
    if raw.is_empty() {
        return Ok(());
    }
    let parents = packet_heads(&mut tx, false, knowledge).await?;
    let mut packet = Packet {
        version: 1,
        id: uuid::Uuid::now_v7().to_string(),
        parents,
        events: vec![],
    };
    for raw in raw {
        let event: Event = decoded(&raw)?;
        let size = encoded(&packet)?.len() + raw.len();
        if size > MAX_JSON * 2 / 3 - 1024 {
            if packet.events.is_empty() {
                return Err(AppError::validation("单个业务版本过大，无法同步"));
            }
            break;
        }
        packet.events.push(event);
    }
    save_packet(&mut tx, &packet, false).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn upload_scope(
    db: &Db,
    dav: &Dav,
    c: &Config,
    key: &[u8; 32],
    include_knowledge: bool,
) -> AppResult<()> {
    make_packet(db, false).await?;
    if include_knowledge {
        make_packet(db, true).await?;
    }
    let knowledge_filter = if include_knowledge {
        ""
    } else {
        "AND json_extract(payload_json,'$.events[0].table') <> 'knowledge_sources'"
    };
    let query = format!("SELECT payload_json FROM cloud_packets WHERE uploaded=0 {knowledge_filter} ORDER BY rowid LIMIT 4");
    let payloads: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(query))
        .fetch_all(db.pool())
        .await?;
    for payload in payloads {
        let packet: Packet = decoded(&payload)?;
        for event in &packet.events {
            if let Some(row) = &event.row {
                if event.table == KNOWLEDGE_TABLE {
                    upload_knowledge_asset(db, dav, c, key, row).await?;
                } else {
                    for id in media_ids(row) {
                        let asset = content_assets::get_asset(db, &id).await?;
                        upload_immutable(
                            dav,
                            db,
                            c,
                            key,
                            &bucket("assets", &id)?,
                            &asset,
                            40 * 1024 * 1024,
                        )
                        .await?;
                    }
                }
                if event.table == "attachments" {
                    attachment_upload(db, dav, c, key, event).await?;
                }
            }
        }
        upload_immutable(
            dav,
            db,
            c,
            key,
            &bucket("packets", &packet.id)?,
            &packet,
            MAX_JSON,
        )
        .await?;
        let mut tx = db.pool().begin().await?;
        sqlx::query("UPDATE cloud_packets SET uploaded=1 WHERE id=?")
            .bind(&packet.id)
            .execute(&mut *tx)
            .await?;
        for event in &packet.events {
            sqlx::query("UPDATE cloud_events SET uploaded=1 WHERE id=?")
                .bind(&event.id)
                .execute(&mut *tx)
                .await?;
        }
        dirty_head(&mut tx).await?;
        tx.commit().await?;
    }
    if include_knowledge {
        let mut tx = db.pool().begin().await?;
        prune_deleted_knowledge_assets(&mut tx).await?;
        tx.commit().await?;
    }
    Ok(())
}
pub async fn published_heads(db: &Db) -> AppResult<Vec<String>> {
    let mut conn = db.pool().acquire().await?;
    packet_heads(&mut conn, true, false).await
}
pub async fn published_knowledge_heads(db: &Db) -> AppResult<Vec<String>> {
    let mut conn = db.pool().acquire().await?;
    packet_heads(&mut conn, true, true).await
}
pub async fn fetch(
    db: &Db,
    dav: &Dav,
    c: &Config,
    key: &[u8; 32],
    root: &str,
    knowledge: bool,
) -> AppResult<()> {
    let mut stack = vec![(root.to_owned(), false)];
    let mut packets = std::collections::HashMap::<String, Packet>::new();
    let mut visiting = HashSet::new();
    while let Some((id, ready)) = stack.pop() {
        let known: Option<String> =
            sqlx::query_scalar("SELECT payload_json FROM cloud_packets WHERE id=?")
                .bind(&id)
                .fetch_optional(db.pool())
                .await?;
        if let Some(payload) = known {
            let packet: Packet = decoded(&payload)?;
            if packet_is_knowledge(&packet)? != knowledge {
                return Err(AppError::validation("同步头引用了另一类业务批次"));
            }
            continue;
        }
        if ready {
            let packet = packets
                .remove(&id)
                .ok_or_else(|| AppError::validation("业务父批次尚未就绪"))?;
            for event in &packet.events {
                if let Some(row) = &event.row {
                    if event.table == KNOWLEDGE_TABLE {
                        import_knowledge_asset(db, dav, c, key, row).await?;
                    } else {
                        for asset in media_ids(row) {
                            import_asset(db, dav, c, key, &asset).await?;
                        }
                    }
                    if event.table == "attachments" {
                        let mut safe = row.clone();
                        portable_attachment(&mut safe)?;
                        if safe != *row {
                            return Err(AppError::validation("云端附件路径无效"));
                        }
                        attachment_download(db, dav, c, key, row).await?;
                    }
                }
            }
            let mut tx = db.pool().begin().await?;
            save_packet(&mut tx, &packet, true).await?;
            tx.commit().await?;
            visiting.remove(&id);
        } else {
            if !visiting.insert(id.clone()) || visiting.len() > MAX_HEADS {
                return Err(AppError::validation("业务云端历史循环或过深"));
            }
            let path = bucket("packets", &id)?;
            let data = dav
                .read(db, &path, MAX_JSON)
                .await?
                .ok_or_else(|| AppError::validation("云端业务批次缺失"))?;
            let packet: Packet = decrypt(key, &aad(c, &path), &data)?;
            if packet.version != 1
                || packet.id != id
                || packet.events.is_empty()
                || packet.events.len() > 100
                || packet.parents.len() > MAX_HEADS
            {
                return Err(AppError::validation("云端业务批次格式无效"));
            }
            if packet_is_knowledge(&packet)? != knowledge {
                return Err(AppError::validation("同步头引用了另一类业务批次"));
            }
            stack.push((id.clone(), true));
            for parent in packet.parents.iter().rev() {
                stack.push((parent.clone(), false));
            }
            packets.insert(id, packet);
        }
    }
    Ok(())
}

fn bind(qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>, value: &Value) -> AppResult<()> {
    match value {
        Value::Null => {
            qb.push_bind(None::<String>);
        }
        Value::String(v) => {
            qb.push_bind(v.clone());
        }
        Value::Bool(v) => {
            qb.push_bind(i64::from(*v));
        }
        Value::Number(v) => {
            if let Some(n) = v.as_i64() {
                qb.push_bind(n);
            } else {
                qb.push_bind(
                    v.as_f64()
                        .ok_or_else(|| AppError::validation("数值字段无效"))?,
                );
            }
        }
        _ => return Err(AppError::validation("业务字段必须是单个值")),
    };
    Ok(())
}
async fn write(conn: &mut SqliteConnection, event: &Event) -> AppResult<()> {
    validate_event(conn, event).await?;
    if !TABLES.contains(&event.table.as_str()) {
        return Err(AppError::validation("业务类型无效"));
    }
    if let Some(row) = &event.row {
        let row = localize(conn, &event.table, row).await?;
        let obj = row
            .as_object()
            .ok_or_else(|| AppError::validation("业务记录无效"))?;
        let source_id = (event.table == KNOWLEDGE_TABLE)
            .then(|| obj.get("id").and_then(Value::as_str))
            .flatten()
            .map(str::to_owned);
        let pk = primary(conn, &event.table).await?;
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new("INSERT INTO ");
        qb.push(&event.table).push(" (");
        for (i, col) in obj.keys().enumerate() {
            if i > 0 {
                qb.push(",");
            }
            qb.push("\"").push(col).push("\"");
        }
        qb.push(") VALUES (");
        for (i, val) in obj.values().enumerate() {
            if i > 0 {
                qb.push(",");
            }
            bind(&mut qb, val)?;
        }
        qb.push(") ON CONFLICT (");
        for (i, col) in pk.iter().enumerate() {
            if i > 0 {
                qb.push(",");
            }
            qb.push("\"").push(col).push("\"");
        }
        qb.push(") DO UPDATE SET ");
        for (i, col) in obj.keys().enumerate() {
            if i > 0 {
                qb.push(",");
            }
            qb.push("\"")
                .push(col)
                .push("\"=excluded.\"")
                .push(col)
                .push("\"");
        }
        qb.build().execute(&mut *conn).await?;
        if let Some(source_id) = source_id {
            crate::knowledge_base::rebuild_synced_source(conn, &source_id).await?;
        }
    } else {
        let key = localize(conn, &event.table, &event.key).await?;
        let obj = key
            .as_object()
            .ok_or_else(|| AppError::validation("业务编号无效"))?;
        let source_asset = if event.table == KNOWLEDGE_TABLE {
            let source_id = obj
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| AppError::validation("知识库来源编号缺失"))?;
            sqlx::query_scalar::<_, String>("SELECT asset_id FROM knowledge_sources WHERE id=?")
                .bind(source_id)
                .fetch_optional(&mut *conn)
                .await?
        } else {
            None
        };
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new("DELETE FROM ");
        qb.push(&event.table).push(" WHERE ");
        for (i, (col, val)) in obj.iter().enumerate() {
            if i > 0 {
                qb.push(" AND ");
            }
            qb.push("\"").push(col).push("\"=");
            bind(&mut qb, val)?;
        }
        qb.build().execute(&mut *conn).await?;
        if let Some(asset_id) = source_asset {
            let marker = format!("lumen-asset:{asset_id}");
            let referenced: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM memo_documents WHERE instr(body_md,?)>0 OR instr(steps_json,?)>0)",
            )
            .bind(&marker)
            .bind(&marker)
            .fetch_one(&mut *conn)
            .await?;
            if !referenced {
                sqlx::query("DELETE FROM content_assets WHERE id=?")
                    .bind(asset_id)
                    .execute(&mut *conn)
                    .await?;
            }
        }
    }
    sqlx::query("UPDATE cloud_records SET base_json=?,selected_event=? WHERE id=?")
        .bind(event.row.as_ref().map(encoded).transpose()?)
        .bind(&event.id)
        .bind(&event.record_id)
        .execute(conn)
        .await?;
    Ok(())
}
pub async fn apply_scope(db: &Db, include_knowledge: bool) -> AppResult<bool> {
    let mut tx = db.pool().begin().await?;
    let changed = apply_from(&mut tx, db.data_dir(), include_knowledge).await?;
    tx.commit().await?;
    Ok(changed)
}
async fn apply_from(
    tx: &mut SqliteConnection,
    dir: &std::path::Path,
    include_knowledge: bool,
) -> AppResult<bool> {
    capture_from(tx, dir, include_knowledge).await?;
    let mapping = aliases(tx).await?;
    sqlx::query("PRAGMA defer_foreign_keys=ON")
        .execute(&mut *tx)
        .await?;
    let records = sqlx::query("SELECT id,selected_event,table_name FROM cloud_records")
        .fetch_all(&mut *tx)
        .await?;
    let mut candidates = Vec::new();
    for row in records {
        let id: String = row.get("id");
        let table: String = row.get("table_name");
        if table == KNOWLEDGE_TABLE && !include_knowledge {
            continue;
        }
        let heads = heads(tx, Some(&id), false).await?;
        let selected: Option<String> = row.get("selected_event");
        if heads.is_empty() || (heads.len() == 1 && selected.as_ref() == heads.first()) {
            continue;
        }
        if heads.len() > 1 && table == KNOWLEDGE_TABLE {
            let mut identical: Option<Event> = None;
            let mut all_identical = true;
            for head in &heads {
                let payload: String =
                    sqlx::query_scalar("SELECT payload_json FROM cloud_events WHERE id=?")
                        .bind(head)
                        .fetch_one(&mut *tx)
                        .await?;
                let event: Event = decoded(&payload)?;
                if let Some(first) = &identical {
                    all_identical &= first.row == event.row;
                } else {
                    identical = Some(event);
                }
            }
            if all_identical {
                let mut merged =
                    identical.ok_or_else(|| AppError::internal("知识库合并版本缺失"))?;
                merged.id = uuid::Uuid::now_v7().to_string();
                merged.parents = heads;
                merged.created_at = stamp();
                insert(tx, &merged, false).await?;
                candidates.push(merged);
            }
            continue;
        }
        if heads.len() != 1 {
            continue;
        }
        let payload: String =
            sqlx::query_scalar("SELECT payload_json FROM cloud_events WHERE id=?")
                .bind(&heads[0])
                .fetch_one(&mut *tx)
                .await?;
        candidates.push(decoded::<Event>(&payload)?);
    }
    // Deferred FK checks do not defer cascading deletes. Detect cross-record delete/edit conflicts
    // before touching business rows; otherwise a remote parent deletion could erase local children.
    for event in candidates.iter().filter(|e| e.row.is_none()) {
        for table in TABLES {
            if table == KNOWLEDGE_TABLE && !include_knowledge {
                continue;
            }
            let sql = format!("PRAGMA foreign_key_list({table})");
            let fks = sqlx::query(sqlx::AssertSqlSafe(sql))
                .fetch_all(&mut *tx)
                .await?;
            for fk in fks {
                if fk.get::<String, _>("table") != event.table {
                    continue;
                }
                let from: String = fk.get("from");
                let to: String = fk.get("to");
                let Some(value) = event.key.get(&to) else {
                    continue;
                };
                let pk = primary(tx, table).await?;
                let mut children = crate::backup::dump_table_from(tx, table).await?;
                for child in &mut children {
                    normalize(table, child, &mapping);
                }
                for child in children.iter().filter(|r| r.get(&from) == Some(value)) {
                    let child_key = key_of(child, &pk)?;
                    if !candidates.iter().any(|e| {
                        e.table == table
                            && e.key == child_key
                            && e.row.as_ref().is_none_or(|r| r.get(&from) != Some(value))
                    }) {
                        return Err(AppError::conflict("云端删除与本机关联记录冲突；为避免级联删除，本机内容全部保留。请先处理关联任务或恢复云端删除版本"));
                    }
                }
            }
        }
    }
    candidates.sort_by_key(|e| {
        if e.row.is_some() {
            TABLES.iter().position(|t| *t == e.table).unwrap_or(99)
        } else {
            100 + TABLES.len() - TABLES.iter().position(|t| *t == e.table).unwrap_or(0)
        }
    });
    for event in &candidates {
        write(tx, event).await?;
    }
    let pruned_assets = prune_deleted_knowledge_assets(tx).await?;
    Ok(!candidates.is_empty() || pruned_assets)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub id: String,
    pub table: String,
    pub heads: Vec<String>,
    pub versions: Vec<Event>,
}
pub async fn conflicts(db: &Db) -> AppResult<Vec<Conflict>> {
    let mut conn = db.pool().acquire().await?;
    let ids:Vec<String>=sqlx::query_scalar("SELECT record_id FROM cloud_events e JOIN cloud_records r ON r.id=e.record_id WHERE NOT EXISTS(SELECT 1 FROM cloud_parents p WHERE p.parent_id=e.id) GROUP BY record_id HAVING COUNT(*)>1 OR (COUNT(*)=1 AND json_extract(payload_json,'$.row') IS NULL AND r.base_json IS NOT NULL AND r.selected_event<>e.id)").fetch_all(&mut *conn).await?;
    let mut out = Vec::new();
    for id in ids {
        let heads = heads(&mut conn, Some(&id), false).await?;
        let table = sqlx::query_scalar("SELECT table_name FROM cloud_records WHERE id=?")
            .bind(&id)
            .fetch_one(&mut *conn)
            .await?;
        let payloads: Vec<String> = sqlx::query_scalar(
            "SELECT payload_json FROM cloud_events WHERE record_id=? ORDER BY rowid DESC",
        )
        .bind(&id)
        .fetch_all(&mut *conn)
        .await?;
        let versions = payloads
            .iter()
            .map(|p| decoded(p))
            .collect::<AppResult<Vec<_>>>()?;
        out.push(Conflict {
            id,
            table,
            heads,
            versions,
        });
    }
    Ok(out)
}
pub async fn resolve(db: &Db, id: &str, event_id: &str, expected: Vec<String>) -> AppResult<()> {
    let mut tx = db.pool().begin().await?;
    let heads = heads(&mut tx, Some(id), false).await?;
    if heads.iter().cloned().collect::<BTreeSet<_>>() != expected.into_iter().collect() {
        return Err(AppError::conflict("业务冲突已经变化，请刷新后再选择"));
    }
    let payload: String =
        sqlx::query_scalar("SELECT payload_json FROM cloud_events WHERE id=? AND record_id=?")
            .bind(event_id)
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    let mut event: Event = decoded(&payload)?;
    event.id = uuid::Uuid::now_v7().to_string();
    event.parents = heads;
    event.created_at = stamp();
    insert(&mut tx, &event, false).await?;
    apply_from(&mut tx, db.data_dir(), true).await?;
    tx.commit().await?;
    Ok(())
}
pub(super) async fn validate_history(conn: &mut SqliteConnection) -> AppResult<()> {
    let rows = sqlx::query("SELECT id,record_id,payload_json,packet_id FROM cloud_events")
        .fetch_all(&mut *conn)
        .await?;
    let mut nodes = Vec::new();
    let mut all = std::collections::HashMap::new();
    for row in rows {
        let event: Event = decoded(&row.get::<String, _>("payload_json"))?;
        validate_event(conn, &event).await?;
        let edges: Vec<String> = sqlx::query_scalar(
            "SELECT parent_id FROM cloud_parents WHERE event_id=? ORDER BY parent_id",
        )
        .bind(&event.id)
        .fetch_all(&mut *conn)
        .await?;
        let mut sorted = event.parents.clone();
        sorted.sort();
        if event.id != row.get::<String, _>("id")
            || event.record_id != row.get::<String, _>("record_id")
            || edges != sorted
        {
            return Err(AppError::validation("业务同步历史与关系表不匹配"));
        }
        all.insert(event.id.clone(), encoded(&event)?);
        nodes.push((event.id, event.parents));
    }
    check_graph(nodes)?;
    let records = sqlx::query("SELECT id,table_name,key_json,selected_event FROM cloud_records")
        .fetch_all(&mut *conn)
        .await?;
    for row in records {
        let id: String = row.get("id");
        let table: String = row.get("table_name");
        let key: Value = decoded(&row.get::<String, _>("key_json"))?;
        if !TABLES.contains(&table.as_str()) || record_id(&table, &key)? != id {
            return Err(AppError::validation("业务同步索引损坏"));
        }
        if let Some(selected) = row.get::<Option<String>, _>("selected_event") {
            let event: Event = decoded(
                all.get(&selected)
                    .ok_or_else(|| AppError::validation("业务历史引用缺失"))?,
            )?;
            if event.record_id != id {
                return Err(AppError::validation("业务历史引用了其它记录"));
            }
        }
    }
    let packets: Vec<String> = sqlx::query_scalar("SELECT payload_json FROM cloud_packets")
        .fetch_all(&mut *conn)
        .await?;
    let mut nodes = Vec::new();
    for raw in packets {
        let packet: Packet = decoded(&raw)?;
        let edges: Vec<String> = sqlx::query_scalar(
            "SELECT parent_id FROM cloud_packet_parents WHERE packet_id=? ORDER BY parent_id",
        )
        .bind(&packet.id)
        .fetch_all(&mut *conn)
        .await?;
        let mut sorted = packet.parents.clone();
        sorted.sort();
        if packet.version != 1
            || edges != sorted
            || packet
                .events
                .iter()
                .any(|e| all.get(&e.id) != encoded(e).ok().as_ref())
        {
            return Err(AppError::validation("业务批次与历史版本不匹配"));
        }
        nodes.push((packet.id, packet.parents));
    }
    check_graph(nodes)?;
    let files = sqlx::query("SELECT id,body_json FROM cloud_files")
        .fetch_all(&mut *conn)
        .await?;
    for row in files {
        let body: AttachmentBody = decoded(&row.get::<String, _>("body_json"))?;
        validate_file(&body, &row.get::<String, _>("id"))?;
    }
    Ok(())
}
