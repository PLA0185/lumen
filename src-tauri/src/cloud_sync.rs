//! Encrypted WebDAV memo history. Immutable revisions form a causal DAG;
//! each device writes only its own discovery head. Clocks never choose a winner.
use crate::{
    commands::AppState,
    content_assets::{self, ContentAsset},
    db::Db,
    error::{AppError, AppResult, ErrorCode},
    memos::{self, MemoDocument, SaveMemoInput},
};
use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteConnection};
use std::{
    collections::{BTreeSet, HashSet},
    time::Duration,
};
use tauri::{Emitter, Manager, State};
mod business;

// ponytail: one local application/account; serialize network and restore operations.
pub static ENGINE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn stamp() -> String {
    crate::db::to_db_time(crate::db::utc_now())
}
const CONFIG_KEY: &str = "memo_cloud_sync";
const MAX_JSON: usize = 4 * 1024 * 1024;
const MAX_HEADS: usize = 10_000;
const MAX_DEVICES: usize = 32;
pub const HISTORY_TABLES: [&str; 9] = [
    "memo_sync_events",
    "memo_sync_parents",
    "cloud_records",
    "cloud_events",
    "cloud_parents",
    "cloud_packets",
    "cloud_packet_parents",
    "cloud_files",
    "cloud_aliases",
];
pub(crate) async fn restore_cached_files(
    conn: &mut SqliteConnection,
    dir: &std::path::Path,
) -> AppResult<()> {
    business::restore_cached_files(conn, dir).await
}
pub async fn prepare_restore(conn: &mut SqliteConnection, format: u32) -> AppResult<()> {
    validate_history(conn).await?;
    let dangling:i64=sqlx::query_scalar("SELECT COUNT(*) FROM memo_documents d WHERE sync_event_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM memo_sync_events e WHERE e.id=d.sync_event_id AND e.memo_id=d.id)").fetch_one(&mut *conn).await?;
    if dangling > 0 && format >= 6 {
        return Err(AppError::validation("备份的备忘历史引用缺失，已中止恢复"));
    }
    if dangling > 0 {
        sqlx::query("UPDATE memo_documents SET sync_event_id=NULL")
            .execute(&mut *conn)
            .await?;
    }
    sqlx::query("UPDATE memo_sync_events SET uploaded=0")
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE cloud_events SET uploaded=0")
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE cloud_packets SET uploaded=0")
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM memo_sync_runtime")
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO memo_sync_runtime(singleton,device_id,restore_pending) VALUES(1,?,1)")
        .bind(uuid::Uuid::now_v7().to_string())
        .execute(&mut *conn)
        .await?;
    let raw: Option<String> = sqlx::query_scalar("SELECT value_json FROM settings WHERE key=?")
        .bind(CONFIG_KEY)
        .fetch_optional(&mut *conn)
        .await?;
    if let Some(raw) = raw {
        let mut c: Config = decoded(&raw)?;
        c.enabled = false;
        sqlx::query("UPDATE settings SET value_json=? WHERE key=?")
            .bind(encoded(&c)?)
            .bind(CONFIG_KEY)
            .execute(conn)
            .await?;
    }
    Ok(())
}
fn check_graph(nodes: Vec<(String, Vec<String>)>) -> AppResult<()> {
    let ids: HashSet<_> = nodes.iter().map(|(id, _)| id.clone()).collect();
    if ids.len() != nodes.len() {
        return Err(AppError::validation("同步历史编号重复"));
    }
    let mut degree = std::collections::HashMap::new();
    let mut children = std::collections::HashMap::<String, Vec<String>>::new();
    for (id, parents) in nodes {
        let unique: HashSet<_> = parents.iter().collect();
        if unique.len() != parents.len() || parents.iter().any(|p| !ids.contains(p)) {
            return Err(AppError::validation("同步历史父版本重复或缺失"));
        }
        degree.insert(id.clone(), parents.len());
        for parent in parents {
            children.entry(parent).or_default().push(id.clone());
        }
    }
    let mut queue: Vec<_> = degree
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut visited = 0;
    while let Some(id) = queue.pop() {
        visited += 1;
        for child in children.get(&id).into_iter().flatten() {
            let n = degree
                .get_mut(child)
                .ok_or_else(|| AppError::validation("同步历史关系损坏"))?;
            *n -= 1;
            if *n == 0 {
                queue.push(child.clone());
            }
        }
    }
    if visited != ids.len() {
        return Err(AppError::validation("同步历史存在循环，已中止恢复"));
    }
    Ok(())
}
async fn validate_history(conn: &mut SqliteConnection) -> AppResult<()> {
    let rows = sqlx::query("SELECT id,memo_id,payload_json,parents_json FROM memo_sync_events")
        .fetch_all(&mut *conn)
        .await?;
    let mut nodes = Vec::new();
    for row in rows {
        let event: Revision = decoded(&row.get::<String, _>("payload_json"))?;
        validate_revision(&event)?;
        let parents: Vec<String> = decoded(&row.get::<String, _>("parents_json"))?;
        let mut edges: Vec<String> = sqlx::query_scalar(
            "SELECT parent_id FROM memo_sync_parents WHERE event_id=? ORDER BY parent_id",
        )
        .bind(&event.id)
        .fetch_all(&mut *conn)
        .await?;
        let mut sorted = event.parents.clone();
        sorted.sort();
        edges.sort();
        if event.id != row.get::<String, _>("id")
            || event.document.summary.id != row.get::<String, _>("memo_id")
            || event.parents != parents
            || edges != sorted
        {
            return Err(AppError::validation("备忘同步历史与关系表不匹配"));
        }
        nodes.push((event.id, event.parents));
    }
    check_graph(nodes)?;
    business::validate_history(conn).await
}
async fn rebase_restore(db: &Db) -> AppResult<()> {
    let pending: bool =
        sqlx::query_scalar("SELECT restore_pending<>0 FROM memo_sync_runtime WHERE singleton=1")
            .fetch_one(db.pool())
            .await?;
    if !pending {
        return Ok(());
    }
    let mut tx = db.pool().begin().await?;
    let rows = sqlx::query("SELECT * FROM memo_documents")
        .fetch_all(&mut *tx)
        .await?;
    for row in rows {
        record_event(&mut tx, &memos::document(row)?, None).await?;
    }
    business::rebase_restore(&mut tx, db.data_dir()).await?;
    sqlx::query("UPDATE memo_sync_runtime SET restore_pending=0 WHERE singleton=1")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

fn encoded<T: Serialize>(value: &T) -> AppResult<String> {
    serde_json::to_string(value).map_err(|_| AppError::internal("同步内容无法编码"))
}
fn decoded<T: serde::de::DeserializeOwned>(value: &str) -> AppResult<T> {
    serde_json::from_str(value).map_err(|_| AppError::validation("同步文件格式损坏或版本不兼容"))
}
fn id_ok(id: &str) -> AppResult<()> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| AppError::validation("同步编号无效"))?;
    if parsed.to_string() != id {
        return Err(AppError::validation("同步编号必须使用标准小写 UUID"));
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Revision {
    pub version: u32,
    pub id: String,
    pub parents: Vec<String>,
    pub document: MemoDocument,
}
pub fn validate_revision(event: &Revision) -> AppResult<()> {
    if event.version != 1 || event.parents.len() > MAX_HEADS {
        return Err(AppError::validation("不支持的同步版本或父版本数量过多"));
    }
    id_ok(&event.id)?;
    id_ok(&event.document.summary.id)?;
    let mut parents = HashSet::new();
    for id in &event.parents {
        id_ok(id)?;
        if id == &event.id || !parents.insert(id) {
            return Err(AppError::validation("同步版本含循环或重复父版本"));
        }
    }
    let d = &event.document;
    memos::validate(&SaveMemoInput {
        id: None,
        expected_revision: None,
        title: d.summary.title.clone(),
        category: d.summary.category.clone(),
        kind: d.summary.kind.clone(),
        body_md: d.body_md.clone(),
        steps: d.steps.clone(),
    })?;
    for date in [&d.summary.created_at, &d.summary.updated_at]
        .into_iter()
        .chain(d.summary.deleted_at.iter())
    {
        chrono::DateTime::parse_from_rfc3339(date)
            .map_err(|_| AppError::validation("同步版本时间格式无效"))?;
    }
    if d.summary.revision < 1 {
        return Err(AppError::validation("同步版本号无效"));
    }
    Ok(())
}

async fn heads_from(conn: &mut SqliteConnection, memo_id: Option<&str>) -> AppResult<Vec<String>> {
    Ok(sqlx::query_scalar("SELECT e.id FROM memo_sync_events e WHERE (? IS NULL OR e.memo_id=?) AND NOT EXISTS (SELECT 1 FROM memo_sync_parents p WHERE p.parent_id=e.id) ORDER BY e.id")
        .bind(memo_id).bind(memo_id).fetch_all(conn).await?)
}
pub async fn record_event(
    conn: &mut SqliteConnection,
    doc: &MemoDocument,
    parents: Option<Vec<String>>,
) -> AppResult<()> {
    let parents = match parents {
        Some(p) => p,
        None => sqlx::query_scalar::<_, Option<String>>(
            "SELECT sync_event_id FROM memo_documents WHERE id=?",
        )
        .bind(&doc.summary.id)
        .fetch_one(&mut *conn)
        .await?
        .into_iter()
        .collect(),
    };
    let event = Revision {
        version: 1,
        id: uuid::Uuid::now_v7().to_string(),
        parents,
        document: doc.clone(),
    };
    insert_event(conn, &event, false).await?;
    sqlx::query("UPDATE memo_documents SET sync_event_id=? WHERE id=?")
        .bind(&event.id)
        .bind(&doc.summary.id)
        .execute(conn)
        .await?;
    Ok(())
}
async fn insert_event(
    conn: &mut SqliteConnection,
    event: &Revision,
    uploaded: bool,
) -> AppResult<()> {
    let payload = encoded(event)?;
    if let Some(existing) =
        sqlx::query_scalar::<_, String>("SELECT payload_json FROM memo_sync_events WHERE id=?")
            .bind(&event.id)
            .fetch_optional(&mut *conn)
            .await?
    {
        if existing != payload {
            return Err(AppError::conflict("相同版本编号的内容不一致，已停止同步"));
        }
        return Ok(());
    }
    for p in &event.parents {
        let memo: Option<String> =
            sqlx::query_scalar("SELECT memo_id FROM memo_sync_events WHERE id=?")
                .bind(p)
                .fetch_optional(&mut *conn)
                .await?;
        if memo.as_deref() != Some(&event.document.summary.id) {
            return Err(AppError::validation("同步父版本缺失或属于其它记录"));
        }
    }
    sqlx::query("INSERT INTO memo_sync_events (id,memo_id,parents_json,payload_json,created_at,uploaded) VALUES (?,?,?,?,?,?)")
        .bind(&event.id).bind(&event.document.summary.id).bind(encoded(&event.parents)?).bind(payload)
        .bind(&event.document.summary.updated_at).bind(uploaded).execute(&mut *conn).await?;
    for p in &event.parents {
        sqlx::query("INSERT INTO memo_sync_parents (event_id,parent_id) VALUES (?,?)")
            .bind(&event.id)
            .bind(p)
            .execute(&mut *conn)
            .await?;
    }
    dirty_head(conn).await?;
    Ok(())
}
async fn dirty_head(conn: &mut SqliteConnection) -> AppResult<()> {
    sqlx::query("INSERT INTO memo_sync_runtime (singleton,device_id) VALUES (1,?) ON CONFLICT(singleton) DO UPDATE SET head_generation=head_generation+1")
        .bind(uuid::Uuid::now_v7().to_string()).execute(conn).await?;
    Ok(())
}
pub async fn seed(conn: &mut SqliteConnection) -> AppResult<()> {
    let rows = sqlx::query("SELECT * FROM memo_documents WHERE sync_event_id IS NULL")
        .fetch_all(&mut *conn)
        .await?;
    for row in rows {
        record_event(conn, &memos::document(row)?, None).await?;
    }
    Ok(())
}
async fn runtime(db: &Db) -> AppResult<String> {
    sqlx::query("INSERT OR IGNORE INTO memo_sync_runtime (singleton,device_id) VALUES (1,?)")
        .bind(uuid::Uuid::now_v7().to_string())
        .execute(db.pool())
        .await?;
    Ok(
        sqlx::query_scalar("SELECT device_id FROM memo_sync_runtime WHERE singleton=1")
            .fetch_one(db.pool())
            .await?,
    )
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub server: String,
    pub account: String,
    pub folder: String,
    pub connection_id: String,
    pub workspace_id: String,
    pub enabled: bool,
    pub inherit_all: bool,
}
async fn config(db: &Db) -> AppResult<Option<Config>> {
    sqlx::query_scalar::<_, String>("SELECT value_json FROM settings WHERE key=?")
        .bind(CONFIG_KEY)
        .fetch_optional(db.pool())
        .await?
        .map(|v| decoded(&v))
        .transpose()
}
async fn store_config(db: &Db, config: &Config) -> AppResult<()> {
    sqlx::query("INSERT INTO settings (key,value_json,updated_at) VALUES (?,?,?) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at")
        .bind(CONFIG_KEY).bind(encoded(config)?).bind(stamp()).execute(db.pool()).await?;
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct Credentials {
    password: String,
    key: String,
}
fn credentials(id: &str, value: Option<&Credentials>) -> AppResult<Credentials> {
    let entry = keyring::Entry::new("Lumen.CloudSync", id)
        .map_err(|_| AppError::internal("无法访问 Windows 凭据管理器"))?;
    if let Some(v) = value {
        entry
            .set_password(&encoded(v)?)
            .map_err(|_| AppError::internal("无法保存同步凭据，请检查当前 Windows 账户权限"))?;
    }
    let value = entry.get_password().map_err(|_| {
        AppError::new(
            ErrorCode::NotConfigured,
            "本机缺少同步应用密码或恢复码，请重新配置连接",
        )
    })?;
    decoded(&value)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub config: Option<Config>,
    pub pending: i64,
    pub conflicts: Vec<String>,
    pub last_scan: Option<String>,
    pub last_upload: Option<String>,
    pub last_error: Option<String>,
    pub retry_until: i64,
}
pub async fn status(db: &Db) -> AppResult<Status> {
    runtime(db).await?;
    let row = sqlx::query("SELECT * FROM memo_sync_runtime WHERE singleton=1")
        .fetch_one(db.pool())
        .await?;
    let conflicts = sqlx::query_scalar("SELECT memo_id FROM memo_sync_events e WHERE NOT EXISTS (SELECT 1 FROM memo_sync_parents p WHERE p.parent_id=e.id) GROUP BY memo_id HAVING COUNT(*)>1 ORDER BY memo_id").fetch_all(db.pool()).await?;
    Ok(Status { config:config(db).await?, pending:sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM memo_sync_events WHERE uploaded=0)+(SELECT COUNT(*) FROM cloud_events WHERE uploaded=0)").fetch_one(db.pool()).await?, conflicts,
        last_scan:row.try_get("last_scan")?, last_upload:row.try_get("last_upload")?, last_error:row.try_get("last_error")?, retry_until:row.try_get("retry_until")? })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    nonce: String,
    data: String,
}
fn key_bytes(value: &str) -> AppResult<[u8; 32]> {
    URL_SAFE_NO_PAD
        .decode(value.strip_prefix("LUMEN1-").unwrap_or(value))
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| {
            AppError::validation("恢复码格式无效，请完整复制另一台电脑显示的 LUMEN1- 恢复码")
        })
}
fn encrypt<T: Serialize>(key: &[u8; 32], aad: &str, data: &T) -> AppResult<Vec<u8>> {
    let mut nonce = [0; 12];
    getrandom::fill(&mut nonce).map_err(|_| AppError::internal("无法生成加密随机数"))?;
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| AppError::internal("无法初始化加密"))?;
    let nonce_value = Nonce::from(nonce);
    let value = cipher
        .encrypt(
            &nonce_value,
            Payload {
                msg: encoded(data)?.as_bytes(),
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| AppError::internal("同步加密失败"))?;
    Ok(encoded(&Envelope {
        version: 1,
        nonce: STANDARD.encode(nonce),
        data: STANDARD.encode(value),
    })?
    .into_bytes())
}
fn decrypt<T: serde::de::DeserializeOwned>(key: &[u8; 32], aad: &str, data: &[u8]) -> AppResult<T> {
    let env: Envelope =
        serde_json::from_slice(data).map_err(|_| AppError::validation("云端加密文件损坏"))?;
    let nonce: [u8; 12] = STANDARD
        .decode(env.nonce)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| AppError::validation("云端加密参数损坏"))?;
    if env.version != 1 {
        return Err(AppError::validation("云端加密版本不兼容"));
    }
    let bytes = STANDARD
        .decode(env.data)
        .map_err(|_| AppError::validation("云端加密文件损坏"))?;
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| AppError::internal("无法初始化加密"))?;
    let value = cipher
        .decrypt(
            &Nonce::from(nonce),
            Payload {
                msg: &bytes,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| {
            AppError::new(
                ErrorCode::Unauthorized,
                "恢复码不匹配，或云端文件已损坏；没有覆盖本机内容",
            )
        })?;
    serde_json::from_slice(&value).map_err(|_| AppError::validation("云端内容格式不兼容"))
}

struct Dav {
    client: reqwest::Client,
    root: reqwest::Url,
    account: String,
    password: String,
    folders: std::sync::Mutex<HashSet<String>>,
}
impl Dav {
    fn new(server: &str, folder: &str, account: &str, password: &str) -> AppResult<Self> {
        let mut root = reqwest::Url::parse(server)
            .map_err(|_| AppError::validation("WebDAV 服务器地址无效"))?;
        if root.scheme() != "https"
            || !root.username().is_empty()
            || root.password().is_some()
            || root.query().is_some()
            || root.fragment().is_some()
        {
            return Err(AppError::validation(
                "WebDAV 地址必须是 HTTPS，不能嵌入密码、查询参数或片段",
            ));
        }
        if folder.is_empty()
            || folder.len() > 100
            || folder
                .chars()
                .any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_')))
        {
            return Err(AppError::validation(
                "同步文件夹请使用 1–100 个英文字母、数字、连字符或下划线",
            ));
        }
        if account.trim().is_empty() || password.is_empty() {
            return Err(AppError::validation("请输入账号和应用密码"));
        }
        if !root.path().ends_with('/') {
            root.set_path(&format!("{}/", root.path()));
        }
        root = root
            .join(&format!("{folder}/"))
            .map_err(|_| AppError::validation("同步文件夹无效"))?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| AppError::internal("无法初始化网络连接"))?;
        Ok(Self {
            client,
            root,
            account: account.trim().into(),
            password: password.into(),
            folders: std::sync::Mutex::new(HashSet::new()),
        })
    }
    async fn request(
        &self,
        db: &Db,
        method: &str,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> AppResult<reqwest::Response> {
        self.request_headers(db, method, path, body, &[]).await
    }
    async fn request_headers(
        &self,
        db: &Db,
        method: &str,
        path: &str,
        body: Option<Vec<u8>>,
        headers: &[(&str, &str)],
    ) -> AppResult<reqwest::Response> {
        let now = chrono::Utc::now().timestamp();
        runtime(db).await?;
        let row = sqlx::query("SELECT budget_start,request_count,device_count,retry_until FROM memo_sync_runtime WHERE singleton=1").fetch_one(db.pool()).await?;
        if row.try_get::<i64, _>("retry_until")? > now {
            return Err(AppError::new(
                ErrorCode::RateLimited,
                "云同步正在等待重试，本机修改已经保存",
            ));
        }
        let reset = now - row.try_get::<i64, _>("budget_start")? >= 1800;
        let count = if reset {
            0
        } else {
            row.try_get("request_count")?
        };
        let devices = row.try_get::<i64, _>("device_count")?.max(1);
        if count >= 500 / devices {
            sqlx::query(
                "UPDATE memo_sync_runtime SET retry_until=budget_start+1800 WHERE singleton=1",
            )
            .execute(db.pool())
            .await?;
            return Err(AppError::new(
                ErrorCode::RateLimited,
                "已达到当前共享请求预算，稍后自动续传",
            ));
        }
        sqlx::query("UPDATE memo_sync_runtime SET budget_start=CASE WHEN ? THEN ? ELSE budget_start END,request_count=? WHERE singleton=1")
            .bind(reset).bind(now).bind(count+1).execute(db.pool()).await?;
        let url = self
            .root
            .join(path)
            .map_err(|_| AppError::validation("同步路径无效"))?;
        let immutable = method == "PUT_ONCE";
        let method =
            reqwest::Method::from_bytes(if immutable { b"PUT" } else { method.as_bytes() })
                .map_err(|_| AppError::internal("请求方法无效"))?;
        let mut request = self
            .client
            .request(method.clone(), url)
            .basic_auth(&self.account, Some(&self.password));
        let seconds = if path.starts_with("files/") {
            900
        } else if path.starts_with("assets/") {
            180
        } else {
            30
        };
        request = request.timeout(Duration::from_secs(seconds));
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        if immutable {
            request = request.header("If-None-Match", "*");
        }
        if method.as_str() == "PROPFIND" {
            request = request
                .header("Depth", "1")
                .header("Content-Type", "application/xml");
        }
        if let Some(body) = body {
            request = request.body(body);
        }
        let response = request.send().await.map_err(|_| {
            AppError::new(
                ErrorCode::Network,
                "无法连接 WebDAV；本机内容仍然保留，将自动重试",
            )
        })?;
        let status = response.status().as_u16();
        if matches!(status, 401 | 403) {
            return Err(AppError::new(
                ErrorCode::Unauthorized,
                "WebDAV 授权失败，请检查账号和第三方应用密码",
            ));
        }
        if status == 429 || status == 503 {
            let delay = response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| {
                    v.parse::<i64>().ok().or_else(|| {
                        chrono::DateTime::parse_from_rfc2822(v)
                            .ok()
                            .map(|t| t.timestamp() - now)
                    })
                })
                .unwrap_or(60)
                .clamp(1, 86400);
            sqlx::query("UPDATE memo_sync_runtime SET retry_until=? WHERE singleton=1")
                .bind(now + delay)
                .execute(db.pool())
                .await?;
            return Err(AppError::new(
                ErrorCode::RateLimited,
                "网盘要求稍后重试，本机修改没有丢失",
            ));
        }
        Ok(response)
    }
    async fn read(&self, db: &Db, path: &str, max: usize) -> AppResult<Option<Vec<u8>>> {
        let mut res = self.request(db, "GET", path, None).await?;
        if res.status().as_u16() == 404 {
            return Ok(None);
        }
        if !res.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Network,
                format!("WebDAV 读取失败（HTTP {}）", res.status().as_u16()),
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = res
            .chunk()
            .await
            .map_err(|_| AppError::new(ErrorCode::Network, "下载中断，将自动重试"))?
        {
            if bytes.len() + chunk.len() > max {
                return Err(AppError::validation("云端文件超过安全大小限制"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Some(bytes))
    }
    async fn mkdir(&self, db: &Db, path: &str) -> AppResult<()> {
        let cached = self
            .folders
            .lock()
            .map_err(|_| AppError::internal("文件夹状态锁不可用"))?
            .contains(path);
        if cached {
            return Ok(());
        }
        let response = self.request(db, "MKCOL", path, None).await?;
        if response.status().is_success() || response.status().as_u16() == 405 {
            self.folders
                .lock()
                .map_err(|_| AppError::internal("文件夹状态锁不可用"))?
                .insert(path.into());
            Ok(())
        } else {
            Err(AppError::new(
                ErrorCode::Network,
                format!(
                    "WebDAV 无法建立文件夹（HTTP {}）",
                    response.status().as_u16()
                ),
            ))
        }
    }
    async fn put(&self, db: &Db, path: &str, value: Vec<u8>) -> AppResult<()> {
        let status = self.stage(db, path, value, true).await?;
        if matches!(status, 201 | 204) {
            Ok(())
        } else {
            Err(AppError::new(
                ErrorCode::Network,
                format!("上传未成功（HTTP {status}）"),
            ))
        }
    }
    async fn stage(&self, db: &Db, path: &str, value: Vec<u8>, overwrite: bool) -> AppResult<u16> {
        let folder = path.rsplit_once('/').map_or("", |(folder, _)| folder);
        let temporary = format!("{}/.lumen-upload-{}.tmp", folder, uuid::Uuid::now_v7());
        let temporary = temporary.trim_start_matches('/');
        let response = self.request(db, "PUT_ONCE", temporary, Some(value)).await?;
        if !response.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Network,
                "文件尚未完整上传，本机内容已保留，将重试",
            ));
        }
        let destination = self
            .root
            .join(path)
            .map_err(|_| AppError::validation("发布路径无效"))?
            .to_string();
        let status = self
            .request_headers(
                db,
                "MOVE",
                temporary,
                None,
                &[
                    ("Destination", &destination),
                    ("Overwrite", if overwrite { "T" } else { "F" }),
                ],
            )
            .await?
            .status()
            .as_u16();
        if !matches!(status, 201 | 204) {
            let cleanup = self.request(db, "DELETE", temporary, None).await?;
            if !cleanup.status().is_success() && cleanup.status().as_u16() != 404 {
                return Err(AppError::new(
                    ErrorCode::Network,
                    "上传未发布且暂存文件清理失败，将重试",
                ));
            }
        }
        Ok(status)
    }
    async fn devices(&self, db: &Db) -> AppResult<Vec<String>> {
        let mut response = self
            .request(
                db,
                "PROPFIND",
                "devices/",
                Some(
                    b"<d:propfind xmlns:d=\"DAV:\"><d:prop><d:resourcetype/></d:prop></d:propfind>"
                        .to_vec(),
                ),
            )
            .await?;
        if response.status().as_u16() != 207 {
            return Err(AppError::new(
                ErrorCode::Network,
                "WebDAV 不支持读取设备列表",
            ));
        }
        let mut data = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| AppError::new(ErrorCode::Network, "设备列表下载失败"))?
        {
            if data.len() + chunk.len() > MAX_JSON {
                return Err(AppError::validation("设备列表过大"));
            }
            data.extend_from_slice(&chunk);
        }
        let mut reader = quick_xml::Reader::from_reader(data.as_slice());
        let mut href = false;
        let mut ids = BTreeSet::new();
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Start(e)) => href = e.local_name().as_ref() == "href",
                Ok(quick_xml::events::Event::Text(e)) if href => {
                    let text = e.xml10_content();
                    let id = text.trim_end_matches('/').rsplit('/').next().unwrap_or("");
                    if id_ok(id).is_ok() {
                        ids.insert(id.to_string());
                    }
                    if ids.len() > MAX_DEVICES {
                        return Err(AppError::validation(
                            "首版最多支持 32 台设备，请清理已停用设备目录",
                        ));
                    }
                }
                Ok(quick_xml::events::Event::End(_)) => href = false,
                Ok(quick_xml::events::Event::Eof) => break,
                Err(_) => return Err(AppError::validation("设备目录 XML 损坏")),
                _ => {}
            }
        }
        Ok(ids.into_iter().collect())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Workspace {
    version: u32,
    id: String,
    check: serde_json::Value,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Head {
    version: u32,
    workspace_id: String,
    device_id: String,
    heads: Vec<String>,
    business_heads: Vec<String>,
}
fn bucket(kind: &str, id: &str) -> AppResult<String> {
    id_ok(id)?;
    Ok(format!("{kind}/{}/{}.json", &id[..2], id))
}
fn aad(config: &Config, path: &str) -> String {
    format!("{}:{path}", config.workspace_id)
}
fn asset_ids(doc: &MemoDocument) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"lumen-asset:([0-9a-fA-F-]{36})\)").expect("constant asset regex")
    });
    let texts =
        std::iter::once(doc.body_md.as_str()).chain(doc.steps.iter().map(|s| s.detail.as_str()));
    texts
        .flat_map(|s| re.captures_iter(s).map(|c| c[1].to_lowercase()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectInput {
    pub server: String,
    pub account: String,
    pub folder: String,
    pub password: String,
    pub recovery_code: String,
    pub inherit_all: bool,
}

async fn verify_space(db: &Db, dav: &Dav, c: &Config, key: &[u8; 32]) -> AppResult<()> {
    let bytes = dav
        .read(db, "workspace.json", MAX_JSON)
        .await?
        .ok_or_else(|| AppError::validation("云端同步空间不存在，请重新连接；本机内容仍然保留"))?;
    let w: Workspace =
        serde_json::from_slice(&bytes).map_err(|_| AppError::validation("云端空间配置损坏"))?;
    if w.version != 1 || w.id != c.workspace_id {
        return Err(AppError::conflict(
            "云端文件夹已换成其它同步空间，没有上传或覆盖内容",
        ));
    }
    let check: String = decrypt(
        key,
        &format!("{}:workspace", w.id),
        &serde_json::to_vec(&w.check).map_err(|_| AppError::validation("空间配置损坏"))?,
    )?;
    if check != w.id {
        return Err(AppError::validation("同步空间密钥验证失败"));
    }
    Ok(())
}
#[tauri::command]
pub async fn cloud_sync_connect(
    state: State<'_, AppState>,
    input: ConnectInput,
) -> AppResult<Status> {
    let _guard = ENGINE.lock().await;
    let db = &state.db;
    let dav = Dav::new(
        &input.server,
        &input.folder,
        &input.account,
        &input.password,
    )?;
    runtime(db).await?;
    let existing = dav.read(db, "workspace.json", MAX_JSON).await?;
    let create = existing.is_none();
    let previous = config(db).await?;
    let mut key = [0; 32];
    if !input.recovery_code.trim().is_empty() {
        key = key_bytes(input.recovery_code.trim())?;
    } else if let Some(previous) = previous.as_ref().filter(|p| {
        p.server == input.server && p.folder == input.folder && p.account == input.account.trim()
    }) {
        key = key_bytes(&credentials(&previous.connection_id, None)?.key)?;
    } else if !create {
        return Err(AppError::validation(
            "请填写家里电脑的同步恢复码，才能下载加密内容",
        ));
    } else {
        getrandom::fill(&mut key).map_err(|_| AppError::internal("无法生成同步恢复码"))?;
    }
    let workspace = match existing {
        Some(bytes) => {
            let w: Workspace = serde_json::from_slice(&bytes)
                .map_err(|_| AppError::validation("同步空间配置损坏"))?;
            id_ok(&w.id)?;
            if w.version != 1 {
                return Err(AppError::validation("同步空间版本不兼容"));
            }
            let check: String = decrypt(
                &key,
                &format!("{}:workspace", w.id),
                &serde_json::to_vec(&w.check).map_err(|_| AppError::validation("空间配置损坏"))?,
            )?;
            if check != w.id {
                return Err(AppError::validation("同步空间验证失败"));
            }
            w
        }
        None => {
            let id = uuid::Uuid::now_v7().to_string();
            let check = serde_json::from_slice(&encrypt(&key, &format!("{id}:workspace"), &id)?)
                .map_err(|_| AppError::internal("空间加密失败"))?;
            Workspace {
                version: 1,
                id,
                check,
            }
        }
    };
    let mut c = Config {
        server: input.server,
        account: input.account.trim().into(),
        folder: input.folder,
        connection_id: uuid::Uuid::now_v7().to_string(),
        workspace_id: workspace.id.clone(),
        enabled: false,
        inherit_all: input.inherit_all,
    };
    // Save a recoverable key before creating anything remotely. A network/disk error cannot
    // leave an encrypted workspace whose only recovery code was lost from process memory.
    credentials(
        &c.connection_id,
        Some(&Credentials {
            password: input.password,
            key: URL_SAFE_NO_PAD.encode(key),
        }),
    )?;
    store_config(db, &c).await?;
    if create {
        dav.mkdir(db, "").await?;
        let probe = format!("connection-check-{}.json", uuid::Uuid::now_v7());
        let first = dav
            .request(
                db,
                "PUT_ONCE",
                &probe,
                Some(encrypt(&key, "connection-check", &"check")?),
            )
            .await?;
        if !first.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Network,
                "网盘无法创建连接验证文件",
            ));
        }
        let second = dav
            .request(
                db,
                "PUT_ONCE",
                &probe,
                Some(encrypt(&key, "connection-check", &"second")?),
            )
            .await?;
        let conditional = second.status().as_u16() == 412;
        let cleanup = dav.request(db, "DELETE", &probe, None).await?;
        if !cleanup.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Network,
                "连接验证文件未能清理，请检查网盘权限后重试",
            ));
        }
        if !conditional {
            return Err(AppError::validation(
                "这个 WebDAV 服务没有正确支持不可覆盖创建，不能安全启用同步",
            ));
        }
        let code = dav
            .stage(
                db,
                "workspace.json",
                encoded(&workspace)?.into_bytes(),
                false,
            )
            .await?;
        if !matches!(code, 201 | 204) {
            return Err(AppError::conflict(
                "同步空间创建未成功，可能已由另一台电脑创建；请填写那台电脑的恢复码再连接",
            ));
        }
    }
    verify_space(db, &dav, &c, &key).await?;
    for dir in ["devices/", "events/", "assets/", "packets/", "files/"] {
        dav.mkdir(db, dir).await?;
    }
    c.enabled = true;
    let mut tx = db.pool().begin().await?;
    seed(&mut tx).await?;
    for table in ["memo_sync_events", "cloud_events", "cloud_packets"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE {table} SET uploaded=0"
        )))
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("UPDATE memo_sync_runtime SET device_id=?,last_scan=NULL,last_upload=NULL,last_error=NULL,head_generation=head_generation+1,published_generation=0 WHERE singleton=1").bind(uuid::Uuid::now_v7().to_string()).execute(&mut *tx).await?;
    sqlx::query("UPDATE settings SET value_json=?,updated_at=? WHERE key=?")
        .bind(encoded(&c)?)
        .bind(stamp())
        .bind(CONFIG_KEY)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    status(db).await
}

#[tauri::command]
pub async fn cloud_sync_status(state: State<'_, AppState>) -> AppResult<Status> {
    status(&state.db).await
}
#[tauri::command]
pub async fn cloud_sync_recovery_code(state: State<'_, AppState>) -> AppResult<String> {
    let c = config(&state.db)
        .await?
        .ok_or_else(|| AppError::new(ErrorCode::NotConfigured, "请先连接同步空间"))?;
    Ok(format!(
        "LUMEN1-{}",
        credentials(&c.connection_id, None)?.key
    ))
}
#[tauri::command]
pub async fn cloud_sync_set_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<Status> {
    let _guard = ENGINE.lock().await;
    let mut c = config(&state.db)
        .await?
        .ok_or_else(|| AppError::new(ErrorCode::NotConfigured, "请先连接同步空间"))?;
    if enabled {
        let cred = credentials(&c.connection_id, None)?;
        let dav = Dav::new(&c.server, &c.folder, &c.account, &cred.password)?;
        verify_space(&state.db, &dav, &c, &key_bytes(&cred.key)?).await?;
    }
    c.enabled = enabled;
    store_config(&state.db, &c).await?;
    status(&state.db).await
}

async fn upload_immutable<T: Serialize + serde::de::DeserializeOwned>(
    dav: &Dav,
    db: &Db,
    c: &Config,
    key: &[u8; 32],
    path: &str,
    value: &T,
    max: usize,
) -> AppResult<()> {
    let folder = path
        .rsplit_once('/')
        .ok_or_else(|| AppError::internal("资源路径无效"))?
        .0;
    dav.mkdir(db, &format!("{folder}/")).await?;
    let status = dav
        .stage(db, path, encrypt(key, &aad(c, path), value)?, false)
        .await?;
    if matches!(status, 201 | 204) {
        return Ok(());
    }
    if status == 412 {
        let bytes = dav
            .read(db, path, max)
            .await?
            .ok_or_else(|| AppError::conflict("云端不可变资源无法核实"))?;
        let existing: T = decrypt(key, &aad(c, path), &bytes)?;
        if encoded(&existing)? == encoded(value)? {
            return Ok(());
        }
        return Err(AppError::conflict(
            "云端相同资源编号的内容不一致，没有覆盖原文件",
        ));
    }
    Err(AppError::new(
        ErrorCode::Network,
        format!("云端资源上传失败（HTTP {status}）"),
    ))
}
async fn upload(db: &Db, dav: &Dav, c: &Config, key: &[u8; 32], device: &str) -> AppResult<()> {
    let mut tx = db.pool().begin().await?;
    seed(&mut tx).await?;
    tx.commit().await?;
    let values: Vec<String> = sqlx::query_scalar(
        "SELECT payload_json FROM memo_sync_events WHERE uploaded=0 ORDER BY rowid LIMIT 100",
    )
    .fetch_all(db.pool())
    .await?;
    for value in values {
        let event: Revision = decoded(&value)?;
        validate_revision(&event)?;
        for id in asset_ids(&event.document) {
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
        upload_immutable(
            dav,
            db,
            c,
            key,
            &bucket("events", &event.id)?,
            &event,
            MAX_JSON,
        )
        .await?;
        sqlx::query("UPDATE memo_sync_events SET uploaded=1 WHERE id=?")
            .bind(&event.id)
            .execute(db.pool())
            .await?;
        let mut conn = db.pool().acquire().await?;
        dirty_head(&mut conn).await?;
    }
    let _ = device;
    Ok(())
}
async fn publish_head(
    db: &Db,
    dav: &Dav,
    c: &Config,
    key: &[u8; 32],
    device: &str,
) -> AppResult<()> {
    let mut conn = db.pool().acquire().await?;
    let generation: i64 =
        sqlx::query_scalar("SELECT head_generation FROM memo_sync_runtime WHERE singleton=1")
            .fetch_one(&mut *conn)
            .await?;
    let heads=sqlx::query_scalar("SELECT e.id FROM memo_sync_events e WHERE uploaded=1 AND NOT EXISTS(SELECT 1 FROM memo_sync_parents p JOIN memo_sync_events child ON child.id=p.event_id WHERE p.parent_id=e.id AND child.uploaded=1) ORDER BY e.id").fetch_all(&mut *conn).await?;
    let business_heads = business::published_heads(db).await?;
    if heads.len() + business_heads.len() > MAX_HEADS {
        return Err(AppError::validation(
            "同步空间超过 10000 条当前版本，请拆分空间",
        ));
    }
    let path = format!("devices/{device}/head.json");
    dav.mkdir(db, &format!("devices/{device}/")).await?;
    dav.put(
        db,
        &path,
        encrypt(
            key,
            &aad(c, &path),
            &Head {
                version: 1,
                workspace_id: c.workspace_id.clone(),
                device_id: device.into(),
                heads,
                business_heads,
            },
        )?,
    )
    .await?;
    sqlx::query("UPDATE memo_sync_runtime SET last_upload=?,last_error=NULL,published_generation=? WHERE singleton=1 AND head_generation=?").bind(stamp()).bind(generation).bind(generation).execute(db.pool()).await?;
    Ok(())
}

async fn import_asset(db: &Db, dav: &Dav, c: &Config, key: &[u8; 32], id: &str) -> AppResult<()> {
    let existing: Option<String> = sqlx::query_scalar("SELECT id FROM content_assets WHERE id=?")
        .bind(id)
        .fetch_optional(db.pool())
        .await?;
    if existing.is_some() {
        content_assets::get_asset(db, id).await?;
        return Ok(());
    }
    let path = bucket("assets", id)?;
    let bytes = dav
        .read(db, &path, 40 * 1024 * 1024)
        .await?
        .ok_or_else(|| AppError::validation("云端图片或文件缺失，没有应用该版本"))?;
    let asset: ContentAsset = decrypt(key, &aad(c, &path), &bytes)?;
    if asset.id != id {
        return Err(AppError::validation("云端资源编号不匹配"));
    }
    content_assets::decode_asset(&asset)?;
    if asset.name.is_empty()
        || asset.name.chars().count() > 255
        || asset.name.chars().any(char::is_control)
    {
        return Err(AppError::validation("云端文件名无效"));
    }
    sqlx::query("INSERT OR IGNORE INTO content_assets (id,name,mime,data_base64,byte_size,sha256,created_at) VALUES (?,?,?,?,?,?,?)")
        .bind(&asset.id).bind(&asset.name).bind(&asset.mime).bind(&asset.data_base64).bind(asset.byte_size).bind(&asset.sha256).bind(&asset.created_at).execute(db.pool()).await?;
    // Another local writer cannot silently replace an immutable asset during the download.
    if encoded(&content_assets::get_asset(db, id).await?)? != encoded(&asset)? {
        return Err(AppError::conflict("本机与云端的资源编号冲突"));
    }
    Ok(())
}
async fn fetch_branch(db: &Db, dav: &Dav, c: &Config, key: &[u8; 32], root: &str) -> AppResult<()> {
    let mut stack = vec![(root.to_owned(), false)];
    let mut visiting = HashSet::new();
    let mut events = std::collections::HashMap::<String, Revision>::new();
    while let Some((id, ready)) = stack.pop() {
        let known: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM memo_sync_events WHERE id=?)")
                .bind(&id)
                .fetch_one(db.pool())
                .await?;
        if known {
            continue;
        }
        if ready {
            let event = events
                .remove(&id)
                .ok_or_else(|| AppError::validation("同步父版本未就绪"))?;
            for asset in asset_ids(&event.document) {
                import_asset(db, dav, c, key, &asset).await?;
            }
            let mut tx = db.pool().begin().await?;
            insert_event(&mut tx, &event, true).await?;
            tx.commit().await?;
            visiting.remove(&id);
        } else {
            if !visiting.insert(id.clone()) {
                return Err(AppError::validation("云端历史含循环，没有覆盖本机"));
            }
            if visiting.len() > MAX_HEADS {
                return Err(AppError::validation("单条记录历史过深，请联系支持"));
            }
            let path = bucket("events", &id)?;
            let bytes = dav
                .read(db, &path, MAX_JSON)
                .await?
                .ok_or_else(|| AppError::validation("云端历史版本缺失，没有覆盖本机"))?;
            let event: Revision = decrypt(key, &aad(c, &path), &bytes)?;
            validate_revision(&event)?;
            if event.id != id {
                return Err(AppError::validation("云端版本编号不匹配"));
            }
            stack.push((id.clone(), true));
            for parent in event.parents.iter().rev() {
                stack.push((parent.clone(), false));
            }
            events.insert(id, event);
        }
    }
    Ok(())
}
async fn apply_document(conn: &mut SqliteConnection, event: &Revision) -> AppResult<MemoDocument> {
    let d = &event.document;
    let row=sqlx::query("INSERT INTO memo_documents (id,title,category,kind,body_md,steps_json,revision,created_at,updated_at,deleted_at,sync_event_id) VALUES (?,?,?,?,?,?,1,?,?,?,?) ON CONFLICT(id) DO UPDATE SET title=excluded.title,category=excluded.category,kind=excluded.kind,body_md=excluded.body_md,steps_json=excluded.steps_json,revision=memo_documents.revision+1,updated_at=excluded.updated_at,deleted_at=excluded.deleted_at,sync_event_id=excluded.sync_event_id RETURNING *")
        .bind(&d.summary.id).bind(&d.summary.title).bind(&d.summary.category).bind(&d.summary.kind).bind(&d.body_md).bind(encoded(&d.steps)?)
        .bind(&d.summary.created_at).bind(&d.summary.updated_at).bind(&d.summary.deleted_at).bind(&event.id).fetch_one(conn).await?;
    memos::document(row)
}
async fn apply_heads(db: &Db) -> AppResult<bool> {
    let mut tx = db.pool().begin().await?;
    let ids: Vec<String> = sqlx::query_scalar("SELECT DISTINCT memo_id FROM memo_sync_events")
        .fetch_all(&mut *tx)
        .await?;
    let mut changed = false;
    for id in ids {
        let heads = heads_from(&mut tx, Some(&id)).await?;
        let selected: Option<Option<String>> =
            sqlx::query_scalar("SELECT sync_event_id FROM memo_documents WHERE id=?")
                .bind(&id)
                .fetch_optional(&mut *tx)
                .await?;
        if heads.len() == 1 && selected.as_ref().and_then(|s| s.as_ref()) != heads.first() {
            let payload: String =
                sqlx::query_scalar("SELECT payload_json FROM memo_sync_events WHERE id=?")
                    .bind(&heads[0])
                    .fetch_one(&mut *tx)
                    .await?;
            apply_document(&mut tx, &decoded(&payload)?).await?;
            changed = true;
        } else if selected.is_none() && !heads.is_empty() {
            // On first download there is no local choice. Show one candidate, retaining all heads.
            let payload: String =
                sqlx::query_scalar("SELECT payload_json FROM memo_sync_events WHERE id=?")
                    .bind(&heads[0])
                    .fetch_one(&mut *tx)
                    .await?;
            apply_document(&mut tx, &decoded(&payload)?).await?;
            changed = true;
        }
    }
    tx.commit().await?;
    Ok(changed)
}
async fn download(db: &Db, dav: &Dav, c: &Config, key: &[u8; 32]) -> AppResult<bool> {
    let devices = dav.devices(db).await?;
    sqlx::query("UPDATE memo_sync_runtime SET device_count=? WHERE singleton=1")
        .bind(devices.len().max(1) as i64)
        .execute(db.pool())
        .await?;
    for device in devices {
        let path = format!("devices/{device}/head.json");
        if let Some(bytes) = dav.read(db, &path, MAX_JSON).await? {
            let head: Head = decrypt(key, &aad(c, &path), &bytes)?;
            if head.version != 1
                || head.workspace_id != c.workspace_id
                || head.device_id != device
                || head.heads.len() + head.business_heads.len() > MAX_HEADS
            {
                return Err(AppError::validation("云端设备头不兼容"));
            }
            for root in head.heads {
                fetch_branch(db, dav, c, key, &root).await?;
            }
            if c.inherit_all {
                for root in head.business_heads {
                    business::fetch(db, dav, c, key, &root).await?;
                }
            }
        }
    }
    let memo_changed = apply_heads(db).await?;
    let changed = if c.inherit_all {
        business::apply(db).await? || memo_changed
    } else {
        memo_changed
    };
    sqlx::query("UPDATE memo_sync_runtime SET last_scan=?,last_error=NULL WHERE singleton=1")
        .bind(stamp())
        .execute(db.pool())
        .await?;
    Ok(changed)
}
async fn sync(db: &Db, scan: bool) -> AppResult<bool> {
    let c = config(db)
        .await?
        .ok_or_else(|| AppError::new(ErrorCode::NotConfigured, "请先连接 WebDAV 同步空间"))?;
    if !c.enabled {
        return Ok(false);
    }
    let cred = credentials(&c.connection_id, None)?;
    let key = key_bytes(&cred.key)?;
    let dav = Dav::new(&c.server, &c.folder, &c.account, &cred.password)?;
    if scan {
        verify_space(db, &dav, &c, &key).await?;
    }
    let device = runtime(db).await?;
    sync_exchange(db, &dav, &c, &key, &device, scan).await
}
async fn sync_exchange(
    db: &Db,
    dav: &Dav,
    c: &Config,
    key: &[u8; 32],
    device: &str,
    scan: bool,
) -> AppResult<bool> {
    rebase_restore(db).await?;
    upload(db, dav, c, key, device).await?;
    // Publish memos immediately; a large first task upload must not hold them back.
    let dirty: bool = sqlx::query_scalar(
        "SELECT head_generation<>published_generation FROM memo_sync_runtime WHERE singleton=1",
    )
    .fetch_one(db.pool())
    .await?;
    if dirty {
        publish_head(db, dav, c, key, device).await?;
    }
    let business_result: AppResult<()> = async {
        business::capture(db).await?;
        business::upload(db, dav, c, key).await?;
        Ok(())
    }
    .await;
    if let Err(error) = business_result {
        // Uncaptured local task changes must not be overwritten. Memo history is
        // independent and can still arrive when an old attachment is missing.
        if scan {
            let mut memos_only = c.clone();
            memos_only.inherit_all = false;
            download(db, dav, &memos_only, key).await?;
        }
        return Err(AppError {
            message: format!("备忘和流程仍会同步；其它业务同步失败：{}", error.message),
            ..error
        });
    }
    let dirty: bool = sqlx::query_scalar(
        "SELECT head_generation<>published_generation FROM memo_sync_runtime WHERE singleton=1",
    )
    .fetch_one(db.pool())
    .await?;
    if dirty {
        publish_head(db, dav, c, key, device).await?;
    }
    if scan {
        download(db, dav, c, key).await
    } else {
        Ok(false)
    }
}
fn notify(app: &tauri::AppHandle) {
    let _ = app.emit(
        "lumen-data-changed",
        serde_json::json!({"from":"cloud-sync","domains":["all"]}),
    );
}
async fn save_error(db: &Db, error: &AppError) {
    // Status is auxiliary; pending revisions remain durable even if status cannot be refreshed.
    let _=sqlx::query("UPDATE memo_sync_runtime SET last_error=?,retry_until=MAX(retry_until,?) WHERE singleton=1")
        .bind(&error.message).bind(chrono::Utc::now().timestamp()+30).execute(db.pool()).await;
}
#[tauri::command]
pub async fn cloud_sync_now(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Status> {
    let _guard = ENGINE.lock().await;
    match sync(&state.db, true).await {
        Ok(changed) => {
            if changed {
                notify(&app);
            }
        }
        Err(e) => {
            save_error(&state.db, &e).await;
            notify(&app);
            return Err(e);
        }
    }
    status(&state.db).await
}
pub fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_scan = std::time::Instant::now() - Duration::from_secs(3600);
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let _guard = ENGINE.lock().await;
            let db = &app.state::<AppState>().db;
            if !matches!(config(db).await, Ok(Some(Config { enabled: true, .. }))) {
                continue;
            }
            let devices: i64 =
                sqlx::query_scalar("SELECT device_count FROM memo_sync_runtime WHERE singleton=1")
                    .fetch_optional(db.pool())
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or(1);
            let scan = last_scan.elapsed().as_secs() >= (6 * devices * devices).max(30) as u64;
            if scan {
                last_scan = std::time::Instant::now();
            }
            match sync(db, scan).await {
                Ok(changed) => {
                    if changed {
                        notify(&app);
                    }
                }
                Err(e) => {
                    save_error(db, &e).await;
                    notify(&app);
                }
            }
        }
    });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub heads: Vec<String>,
    pub versions: Vec<Revision>,
}
#[tauri::command]
pub async fn cloud_sync_history(state: State<'_, AppState>, id: String) -> AppResult<History> {
    let mut tx = state.db.pool().begin().await?;
    seed(&mut tx).await?;
    let heads = heads_from(&mut tx, Some(&id)).await?;
    let payloads:Vec<String>=sqlx::query_scalar("SELECT payload_json FROM memo_sync_events WHERE memo_id=? ORDER BY created_at DESC,id DESC").bind(id).fetch_all(&mut *tx).await?;
    let versions = payloads
        .iter()
        .map(|v| decoded(v))
        .collect::<AppResult<Vec<_>>>()?;
    tx.commit().await?;
    Ok(History { heads, versions })
}
pub async fn resolve(
    db: &Db,
    id: &str,
    event_id: &str,
    expected_heads: Vec<String>,
    keep_both: bool,
) -> AppResult<MemoDocument> {
    let mut tx = db.pool().begin().await?;
    let heads = heads_from(&mut tx, Some(id)).await?;
    let expected: BTreeSet<_> = expected_heads.into_iter().collect();
    if heads.iter().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(AppError::conflict(
            "冲突版本已经变化，请刷新后选择；没有覆盖任何内容",
        ));
    }
    let payload: String =
        sqlx::query_scalar("SELECT payload_json FROM memo_sync_events WHERE id=? AND memo_id=?")
            .bind(event_id)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| AppError::not_found("历史版本", event_id))?;
    let mut event: Revision = decoded(&payload)?;
    if keep_both {
        for other in heads.iter().filter(|h| h.as_str() != event_id) {
            let payload: String =
                sqlx::query_scalar("SELECT payload_json FROM memo_sync_events WHERE id=?")
                    .bind(other)
                    .fetch_one(&mut *tx)
                    .await?;
            let mut copy: Revision = decoded(&payload)?;
            copy.id = uuid::Uuid::now_v7().to_string();
            copy.parents.clear();
            copy.document.summary.id = uuid::Uuid::now_v7().to_string();
            // The copy includes a concurrent deletion marker in its title, but remains accessible.
            let suffix = if copy.document.summary.deleted_at.is_some() {
                "（冲突副本·删除版本）"
            } else {
                "（冲突副本）"
            };
            copy.document.summary.title = format!(
                "{}{}",
                copy.document
                    .summary
                    .title
                    .chars()
                    .take(470)
                    .collect::<String>(),
                suffix
            );
            copy.document.summary.deleted_at = None;
            copy.document.summary.created_at = stamp();
            copy.document.summary.updated_at = stamp();
            insert_event(&mut tx, &copy, false).await?;
            apply_document(&mut tx, &copy).await?;
        }
    }
    event.document.summary.updated_at = stamp();
    event.document.summary.revision += 1;
    let doc = apply_document(&mut tx, &event).await?;
    record_event(&mut tx, &doc, Some(heads)).await?;
    tx.commit().await?;
    Ok(doc)
}
#[tauri::command]
pub async fn cloud_sync_restore(
    state: State<'_, AppState>,
    id: String,
    event_id: String,
    expected_heads: Vec<String>,
    keep_both: bool,
) -> AppResult<MemoDocument> {
    let _guard = ENGINE.lock().await;
    resolve(&state.db, &id, &event_id, expected_heads, keep_both).await
}
#[tauri::command]
pub async fn cloud_sync_set_inheritance(
    state: State<'_, AppState>,
    inherit_all: bool,
) -> AppResult<Status> {
    let _guard = ENGINE.lock().await;
    let mut c = config(&state.db)
        .await?
        .ok_or_else(|| AppError::new(ErrorCode::NotConfigured, "请先配置同步"))?;
    c.inherit_all = inherit_all;
    store_config(&state.db, &c).await?;
    status(&state.db).await
}
#[tauri::command]
pub async fn cloud_sync_business_conflicts(
    state: State<'_, AppState>,
) -> AppResult<Vec<business::Conflict>> {
    business::conflicts(&state.db).await
}
#[tauri::command]
pub async fn cloud_sync_business_resolve(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    event_id: String,
    expected_heads: Vec<String>,
) -> AppResult<()> {
    let _guard = ENGINE.lock().await;
    business::resolve(&state.db, &id, &event_id, expected_heads).await?;
    notify(&app);
    Ok(())
}

#[cfg(test)]
mod tests;
