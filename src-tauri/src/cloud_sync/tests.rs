use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
struct Mock {
    url: String,
    files: Arc<Mutex<std::collections::HashMap<String, Vec<u8>>>>,
    fail: Arc<AtomicBool>,
    partial_put: Arc<AtomicBool>,
    folder_exists: Arc<AtomicBool>,
    read_conflict: Arc<AtomicBool>,
    ignore_put_condition: Arc<AtomicBool>,
    ignore_move_condition: Arc<AtomicBool>,
    requests: Arc<AtomicUsize>,
}
impl Mock {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/Lumen/", listener.local_addr().unwrap());
        let files = Arc::new(Mutex::new(
            std::collections::HashMap::<String, Vec<u8>>::new(),
        ));
        let fail = Arc::new(AtomicBool::new(false));
        let data = files.clone();
        let failure = fail.clone();
        let partial_put = Arc::new(AtomicBool::new(false));
        let partial = partial_put.clone();
        let folder_exists = Arc::new(AtomicBool::new(true));
        let folder = folder_exists.clone();
        let read_conflict = Arc::new(AtomicBool::new(false));
        let conflict = read_conflict.clone();
        let ignore_put_condition = Arc::new(AtomicBool::new(false));
        let ignore_put = ignore_put_condition.clone();
        let ignore_move_condition = Arc::new(AtomicBool::new(false));
        let ignore_move = ignore_move_condition.clone();
        let requests = Arc::new(AtomicUsize::new(0));
        let request_count = requests.clone();
        std::thread::spawn(move || {
            for socket in listener.incoming() {
                let Ok(mut socket) = socket else { break };
                socket
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0; 8192];
                let end = loop {
                    let n = socket.read(&mut buf).unwrap();
                    if n == 0 {
                        break 0;
                    }
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(p) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        break p + 4;
                    }
                };
                if end == 0 {
                    continue;
                }
                request_count.fetch_add(1, Ordering::SeqCst);
                let headers = String::from_utf8_lossy(&bytes[..end]).to_string();
                let length = headers
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while bytes.len() < end + length {
                    let n = socket.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buf[..n]);
                }
                let mut parts = headers.split_whitespace();
                let method = parts.next().unwrap();
                let path = parts.next().unwrap().to_string();
                let mut files = data.lock().unwrap();
                let mut body = Vec::new();
                let status = if failure.load(Ordering::SeqCst) {
                    500
                } else {
                    match method {
                        "MKCOL" => {
                            if path == "/Lumen/" && folder.swap(true, Ordering::SeqCst) {
                                405
                            } else {
                                201
                            }
                        }
                        "PUT" => {
                            if partial.swap(false, Ordering::SeqCst) {
                                files.insert(path, b"broken partial upload".to_vec());
                                500
                            } else if headers.to_lowercase().contains("if-none-match: *")
                                && !ignore_put.load(Ordering::SeqCst)
                                && files.contains_key(&path)
                            {
                                412
                            } else {
                                files.insert(path, bytes[end..].to_vec());
                                201
                            }
                        }
                        "GET" => {
                            if !folder.load(Ordering::SeqCst) || conflict.load(Ordering::SeqCst) {
                                409
                            } else if let Some(v) = files.get(&path) {
                                body = v.clone();
                                200
                            } else {
                                404
                            }
                        }
                        "DELETE" => {
                            if files.remove(&path).is_some() {
                                204
                            } else {
                                404
                            }
                        }
                        "MOVE" => {
                            let destination = headers
                                .lines()
                                .find_map(|l| {
                                    l.to_lowercase()
                                        .starts_with("destination:")
                                        .then(|| l.split_once(':').unwrap().1.trim())
                                })
                                .and_then(|s| reqwest::Url::parse(s).ok())
                                .unwrap()
                                .path()
                                .to_owned();
                            if headers.to_lowercase().contains("overwrite: f")
                                && !ignore_move.load(Ordering::SeqCst)
                                && files.contains_key(&destination)
                            {
                                if ignore_put.load(Ordering::SeqCst) {
                                    409
                                } else {
                                    412
                                }
                            } else if let Some(value) = files.remove(&path) {
                                files.insert(destination, value);
                                201
                            } else {
                                404
                            }
                        }
                        "PROPFIND" => {
                            let devices = files
                                .keys()
                                .filter_map(|p| {
                                    p.strip_prefix("/Lumen/devices/")
                                        .and_then(|p| p.split('/').next())
                                })
                                .collect::<BTreeSet<_>>();
                            body=format!("<d:multistatus xmlns:d=\"DAV:\">{}</d:multistatus>",devices.iter().map(|d|format!("<d:response><d:href>/Lumen/devices/{d}/</d:href></d:response>")).collect::<String>()).into_bytes();
                            207
                        }
                        _ => 405,
                    }
                };
                let response = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(response.as_bytes()).unwrap();
                socket.write_all(&body).unwrap();
            }
        });
        Self {
            url,
            files,
            fail,
            partial_put,
            folder_exists,
            read_conflict,
            ignore_put_condition,
            ignore_move_condition,
            requests,
        }
    }
    fn dav(&self) -> Dav {
        Dav {
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            root: reqwest::Url::parse(&self.url).unwrap(),
            account: "test".into(),
            password: "not-a-real-secret".into(),
            folders: std::sync::Mutex::new(HashSet::new()),
        }
    }
    fn request_count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}
#[tokio::test]
async fn first_connection_creates_missing_folder_before_reading_workspace() {
    let mock = Mock::new();
    mock.folder_exists.store(false, Ordering::SeqCst);
    let dav = mock.dav();
    let a = db().await;
    let result = dav.workspace(&a).await.unwrap();
    assert!(result.is_none());
    assert!(mock.folder_exists.load(Ordering::SeqCst));
    close(a).await;
}
#[tokio::test]
async fn workspace_open_preserves_existing_data_and_real_errors() {
    let mock = Mock::new();
    let dav = mock.dav();
    let a = db().await;
    mock.files.lock().unwrap().insert(
        "/Lumen/workspace.json".into(),
        b"existing encrypted space".to_vec(),
    );
    assert_eq!(
        dav.workspace(&a).await.unwrap().unwrap(),
        b"existing encrypted space"
    );
    mock.read_conflict.store(true, Ordering::SeqCst);
    let error = dav.workspace(&a).await.unwrap_err();
    assert!(error.message.contains("409"));
    assert_eq!(
        mock.files
            .lock()
            .unwrap()
            .get("/Lumen/workspace.json")
            .unwrap(),
        b"existing encrypted space"
    );
    close(a).await;
}
#[tokio::test]
async fn failed_folder_creation_does_not_read_or_create_workspace() {
    let mock = Mock::new();
    mock.folder_exists.store(false, Ordering::SeqCst);
    mock.fail.store(true, Ordering::SeqCst);
    let a = db().await;
    let error = mock.dav().workspace(&a).await.unwrap_err();
    assert!(error.message.contains("建立文件夹"));
    assert!(!mock.folder_exists.load(Ordering::SeqCst));
    assert!(mock.files.lock().unwrap().is_empty());
    close(a).await;
}
#[tokio::test]
async fn publication_probe_uses_move_protection_when_put_condition_is_ignored() {
    let mock = Mock::new();
    mock.ignore_put_condition.store(true, Ordering::SeqCst);
    let a = db().await;
    mock.dav().verify_publication(&a, &[9; 32]).await.unwrap();
    assert!(mock.files.lock().unwrap().is_empty());
    close(a).await;
}
#[tokio::test]
async fn publication_probe_rejects_overwriting_server_and_cleans_probe() {
    let mock = Mock::new();
    mock.ignore_put_condition.store(true, Ordering::SeqCst);
    mock.ignore_move_condition.store(true, Ordering::SeqCst);
    let a = db().await;
    assert!(mock.dav().verify_publication(&a, &[9; 32]).await.is_err());
    assert!(mock.files.lock().unwrap().is_empty());
    close(a).await;
}
#[tokio::test]
async fn immutable_retry_verifies_content_after_move_conflict_409() {
    let mock = Mock::new();
    mock.ignore_put_condition.store(true, Ordering::SeqCst);
    let a = db().await;
    let dav = mock.dav();
    let c = cfg();
    let key = [4; 32];
    let path = bucket("events", &uuid::Uuid::now_v7().to_string()).unwrap();
    let value = "existing immutable revision".to_owned();
    upload_immutable(&dav, &a, &c, &key, &path, &value, MAX_JSON)
        .await
        .unwrap();
    upload_immutable(&dav, &a, &c, &key, &path, &value, MAX_JSON)
        .await
        .unwrap();
    let different = "different revision".to_owned();
    assert!(
        upload_immutable(&dav, &a, &c, &key, &path, &different, MAX_JSON)
            .await
            .is_err()
    );
    let bytes = dav.read(&a, &path, MAX_JSON).await.unwrap().unwrap();
    assert_eq!(
        decrypt::<String>(&key, &aad(&c, &path), &bytes).unwrap(),
        value
    );
    close(a).await;
}
#[tokio::test]
async fn concurrent_workspace_publication_never_overwrites_winning_key() {
    let mock = Mock::new();
    let a = db().await;
    let b = db().await;
    let first = mock.dav();
    let second = mock.dav();
    let (left, right) = tokio::join!(
        first.stage(&a, "workspace.json", b"first space and key".to_vec(), false),
        second.stage(
            &b,
            "workspace.json",
            b"second space and key".to_vec(),
            false
        )
    );
    let (left, right) = (left.unwrap(), right.unwrap());
    assert!(matches!((left, right), (201, 412) | (412, 201)));
    let expected = if left == 201 {
        b"first space and key".as_slice()
    } else {
        b"second space and key".as_slice()
    };
    assert_eq!(
        first
            .read(&a, "workspace.json", MAX_JSON)
            .await
            .unwrap()
            .unwrap(),
        expected
    );
    assert_eq!(mock.files.lock().unwrap().len(), 1);
    close(a).await;
    close(b).await;
}
#[tokio::test]
async fn interrupted_put_keeps_published_head_and_can_retry() {
    let mock = Mock::new();
    let dav = mock.dav();
    let a = db().await;
    mock.files
        .lock()
        .unwrap()
        .insert("/Lumen/head.json".into(), b"published old head".to_vec());
    mock.partial_put.store(true, Ordering::SeqCst);
    assert!(dav
        .put(&a, "head.json", b"new complete head".to_vec())
        .await
        .is_err());
    assert_eq!(
        mock.files.lock().unwrap().get("/Lumen/head.json").unwrap(),
        b"published old head"
    );
    dav.put(&a, "head.json", b"new complete head".to_vec())
        .await
        .unwrap();
    assert_eq!(
        mock.files.lock().unwrap().get("/Lumen/head.json").unwrap(),
        b"new complete head"
    );
    let dir = a.data_dir().to_path_buf();
    a.pool().close().await;
    std::fs::remove_dir_all(dir).unwrap();
}
fn input(title: &str) -> SaveMemoInput {
    SaveMemoInput {
        id: None,
        expected_revision: None,
        title: title.into(),
        category: "业务".into(),
        kind: "memo".into(),
        body_md: "重要流程".into(),
        steps: vec![],
    }
}
async fn db() -> Db {
    Db::init(std::env::temp_dir().join(format!("lumen-cloud-{}", uuid::Uuid::now_v7())))
        .await
        .unwrap()
}
async fn add_knowledge_source(db: &Db, title: &str, original: &[u8], extracted: &str) -> String {
    let asset = content_assets::store_bytes(db, title, original.to_vec())
        .await
        .unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let now = stamp();
    sqlx::query("INSERT INTO knowledge_sources (id,asset_id,title,mime,sha256,extracted_text,warnings_json,status,error,created_at,updated_at) VALUES (?,?,?,?,?,?,?,'ready',NULL,?,?)")
        .bind(&id).bind(&asset.id).bind(title).bind(&asset.mime).bind(&asset.sha256)
        .bind(extracted).bind("[]").bind(&now).bind(&now)
        .execute(db.pool()).await.unwrap();
    sqlx::query("INSERT INTO knowledge_chunks (source_id,locator,heading,content,start_offset,end_offset) VALUES (?, '第 1 页', '', ?, 0, ?)")
        .bind(&id).bind(extracted).bind(extracted.chars().count() as i64)
        .execute(db.pool()).await.unwrap();
    id
}
fn cfg() -> Config {
    Config {
        server: "https://example.test/dav/".into(),
        account: "test".into(),
        folder: "Lumen".into(),
        connection_id: uuid::Uuid::now_v7().to_string(),
        workspace_id: uuid::Uuid::now_v7().to_string(),
        enabled: true,
        inherit_all: true,
        default_sync: None,
    }
}
#[test]
fn sync_selection_is_serializable_and_old_connection_fields_survive() {
    let original = cfg();
    let mut json = serde_json::to_value(&original).unwrap();
    json["defaultSync"] = serde_json::json!({"scope":"tasks","direction":"download"});
    let configured: Config =
        serde_json::from_value(json).expect("new sync selection must be accepted");
    assert_eq!(configured.connection_id, original.connection_id);
    assert_eq!(configured.workspace_id, original.workspace_id);
    assert_eq!(
        serde_json::to_value(configured).unwrap()["defaultSync"]["direction"],
        "download"
    );
}
#[tokio::test]
async fn waiting_rate_limit_does_not_extend_retry_or_hide_original_error() {
    let db = db().await;
    runtime(&db).await.unwrap();
    let retry_until = chrono::Utc::now().timestamp() + 12;
    let cause = "已达到当前共享请求预算，稍后自动续传";
    sqlx::query("UPDATE memo_sync_runtime SET retry_until=?,last_error=? WHERE singleton=1")
        .bind(retry_until)
        .bind(cause)
        .execute(db.pool())
        .await
        .unwrap();

    save_error(
        &db,
        &AppError::new(
            crate::error::ErrorCode::RateLimited,
            "云同步正在等待重试，本机修改已经保存",
        ),
    )
    .await;

    let row = sqlx::query("SELECT retry_until,last_error FROM memo_sync_runtime WHERE singleton=1")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(row.get::<i64, _>("retry_until"), retry_until);
    assert_eq!(row.get::<String, _>("last_error"), cause);
    close(db).await;
}

#[tokio::test]
async fn rate_limit_cause_does_not_extend_provider_retry_after() {
    let db = db().await;
    runtime(&db).await.unwrap();
    let retry_until = chrono::Utc::now().timestamp() + 12;
    let cause = "网盘要求稍后重试，本机修改没有丢失";
    sqlx::query("UPDATE memo_sync_runtime SET retry_until=?,last_error=NULL WHERE singleton=1")
        .bind(retry_until)
        .execute(db.pool())
        .await
        .unwrap();

    save_error(
        &db,
        &AppError::new(crate::error::ErrorCode::RateLimited, cause),
    )
    .await;

    let row = sqlx::query("SELECT retry_until,last_error FROM memo_sync_runtime WHERE singleton=1")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(row.get::<i64, _>("retry_until"), retry_until);
    assert_eq!(row.get::<String, _>("last_error"), cause);
    close(db).await;
}

#[tokio::test]
async fn local_request_budget_wait_says_request_was_not_sent() {
    let db = db().await;
    runtime(&db).await.unwrap();
    let retry_until = chrono::Utc::now().timestamp() + 50;
    sqlx::query("UPDATE memo_sync_runtime SET request_count=250,device_count=2,retry_until=?,last_error=? WHERE singleton=1")
        .bind(retry_until)
        .bind("已达到当前共享请求预算，稍后自动续传")
        .execute(db.pool())
        .await
        .unwrap();

    let mock = Mock::new();
    let error = mock.dav().workspace(&db).await.unwrap_err();
    assert!(matches!(error.code, crate::error::ErrorCode::RateLimited));
    assert!(error.message.contains("本机 WebDAV 请求额度已用完"));
    assert!(error.message.contains("当前 WebDAV 请求尚未发出"));
    assert_eq!(mock.request_count(), 0);
    close(db).await;
}

async fn close(db: Db) {
    let dir = db.data_dir().to_owned();
    db.pool().close().await;
    std::fs::remove_dir_all(dir).unwrap();
}
async fn push(db: &Db, dav: &Dav, c: &Config, key: &[u8; 32]) {
    let device = runtime(db).await.unwrap();
    upload(db, dav, c, key, &device).await.unwrap();
    publish_head(db, dav, c, key, &device, true).await.unwrap();
}
#[test]
fn encryption_rejects_tampering_wrong_key_and_wrong_path() {
    let bytes = encrypt(&[7; 32], "workspace:path", &"商业流程").unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("商业流程"));
    assert_eq!(
        decrypt::<String>(&[7; 32], "workspace:path", &bytes).unwrap(),
        "商业流程"
    );
    assert!(decrypt::<String>(&[8; 32], "workspace:path", &bytes).is_err());
    assert!(decrypt::<String>(&[7; 32], "workspace:other", &bytes).is_err());
    let mut env: Envelope = serde_json::from_slice(&bytes).unwrap();
    let mut payload = STANDARD.decode(&env.data).unwrap();
    payload[0] ^= 1;
    env.data = STANDARD.encode(payload);
    assert!(decrypt::<String>(
        &[7; 32],
        "workspace:path",
        encoded(&env).unwrap().as_bytes()
    )
    .is_err());
    assert!(Dav::new("http://example.test/", "Lumen", "a", "p").is_err());
}
#[tokio::test]
async fn real_http_two_databases_preserve_concurrent_memos_assets_and_restore_history() {
    let mock = Mock::new();
    let dav = mock.dav();
    let a = db().await;
    let b = db().await;
    let c = cfg();
    let key = [9; 32];
    let asset = content_assets::store_bytes(&a, "说明.txt", b"actual attached text".to_vec())
        .await
        .unwrap();
    let mut create = input("流程");
    create.body_md = format!("[附件](lumen-asset:{})", asset.id);
    create.kind = "flow".into();
    create.steps = vec![crate::memos::FlowStep {
        id: "original-small-step".into(),
        title: "原文小操作".into(),
        owner: String::new(),
        detail: create.body_md.clone(),
        layout: None,
        group: Some(crate::memos::FlowGroup {
            id: "source-1".into(),
            title: "原文章节".into(),
            path: vec!["原文阶段".into()],
        }),
    }];
    let first = memos::save_impl(&a, create).await.unwrap();
    push(&a, &dav, &c, &key).await;
    assert!(download(&b, &dav, &c, &key).await.unwrap());
    assert_eq!(
        memos::get_impl(&b, &first.summary.id)
            .await
            .unwrap()
            .body_md,
        first.body_md
    );
    assert_eq!(
        content_assets::decode_asset(&content_assets::get_asset(&b, &asset.id).await.unwrap())
            .unwrap(),
        b"actual attached text"
    );
    assert_eq!(
        memos::get_impl(&b, &first.summary.id).await.unwrap().steps[0].group,
        first.steps[0].group
    );
    let initial_id: String =
        sqlx::query_scalar("SELECT sync_event_id FROM memo_documents WHERE id=?")
            .bind(&first.summary.id)
            .fetch_one(a.pool())
            .await
            .unwrap();
    let mut edit_a = input("家里改动");
    edit_a.id = Some(first.summary.id.clone());
    edit_a.expected_revision = Some(first.summary.revision);
    memos::save_impl(&a, edit_a).await.unwrap();
    let current_b = memos::get_impl(&b, &first.summary.id).await.unwrap();
    let mut edit_b = input("公司改动");
    edit_b.id = Some(first.summary.id.clone());
    edit_b.expected_revision = Some(current_b.summary.revision);
    memos::save_impl(&b, edit_b).await.unwrap();
    push(&a, &dav, &c, &key).await;
    push(&b, &dav, &c, &key).await;
    download(&a, &dav, &c, &key).await.unwrap();
    download(&b, &dav, &c, &key).await.unwrap();
    assert_eq!(
        memos::get_impl(&a, &first.summary.id)
            .await
            .unwrap()
            .summary
            .title,
        "家里改动"
    );
    assert_eq!(
        memos::get_impl(&b, &first.summary.id)
            .await
            .unwrap()
            .summary
            .title,
        "公司改动"
    );
    let mut conn = a.pool().acquire().await.unwrap();
    let heads = heads_from(&mut conn, Some(&first.summary.id))
        .await
        .unwrap();
    drop(conn);
    assert_eq!(heads.len(), 2);
    assert!(resolve(&a, &first.summary.id, &initial_id, vec![], true)
        .await
        .is_err());
    let restored = resolve(&a, &first.summary.id, &initial_id, heads, true)
        .await
        .unwrap();
    assert_eq!(restored.summary.title, "流程");
    assert_eq!(restored.steps[0].group, first.steps[0].group);
    assert_eq!(restored.steps[0].detail, first.steps[0].detail);
    assert_eq!(
        memos::list_impl(&a, "冲突副本", false).await.unwrap().len(),
        2
    );
    push(&a, &dav, &c, &key).await;
    download(&b, &dav, &c, &key).await.unwrap();
    assert_eq!(
        memos::get_impl(&b, &first.summary.id)
            .await
            .unwrap()
            .summary
            .title,
        "流程"
    );
    assert_eq!(
        memos::list_impl(&b, "冲突副本", false).await.unwrap().len(),
        2
    );
    assert!(mock
        .files
        .lock()
        .unwrap()
        .values()
        .all(|b| !String::from_utf8_lossy(b).contains("重要流程")));
    close(a).await;
    close(b).await;
}
#[tokio::test]
async fn failed_upload_is_durable_and_journal_failure_rolls_back_memo() {
    let mock = Mock::new();
    let dav = mock.dav();
    let a = db().await;
    let c = cfg();
    let key = [2; 32];
    let first = memos::save_impl(&a, input("离线修改")).await.unwrap();
    let device = runtime(&a).await.unwrap();
    mock.fail.store(true, Ordering::SeqCst);
    assert!(upload(&a, &dav, &c, &key, &device).await.is_err());
    assert_eq!(status(&a).await.unwrap().pending, 1);
    mock.fail.store(false, Ordering::SeqCst);
    push(&a, &dav, &c, &key).await;
    assert_eq!(status(&a).await.unwrap().pending, 0);
    sqlx::query("CREATE TRIGGER fail_journal BEFORE INSERT ON memo_sync_events BEGIN SELECT RAISE(ABORT,'test journal failure'); END").execute(a.pool()).await.unwrap();
    let mut edit = input("不应该写入");
    edit.id = Some(first.summary.id.clone());
    edit.expected_revision = Some(1);
    assert!(memos::save_impl(&a, edit).await.is_err());
    assert_eq!(
        memos::get_impl(&a, &first.summary.id)
            .await
            .unwrap()
            .summary
            .title,
        "离线修改"
    );
    close(a).await;
}
async fn push_all(db: &Db, dav: &Dav, c: &Config, key: &[u8; 32]) {
    runtime(db).await.unwrap();
    business::capture_scope(db, true).await.unwrap();
    business::upload_scope(db, dav, c, key, true).await.unwrap();
    push(db, dav, c, key).await;
}
async fn add_task(db: &Db, id: &str, title: &str) {
    sqlx::query("INSERT INTO tasks(id,title,created_at,updated_at) VALUES(?,?,?,?)")
        .bind(id)
        .bind(title)
        .bind(stamp())
        .bind(stamp())
        .execute(db.pool())
        .await
        .unwrap();
}
#[tokio::test]
async fn missing_task_attachment_does_not_block_memo_upload_or_download() {
    let mock = Mock::new();
    let dav = mock.dav();
    let home = db().await;
    let company = db().await;
    let c = cfg();
    let key = [3; 32];
    let memo = memos::save_impl(&home, input("家里写的流程"))
        .await
        .unwrap();
    push(&home, &dav, &c, &key).await;
    add_task(&company, "local-task", "公司已有任务").await;
    sqlx::query("INSERT INTO attachments(id,task_id,file_name,byte_size,storage_mode,external_path,created_at) VALUES('missing-file','local-task','找不到的附件.txt',1,'reference',?,?)")
        .bind(company.data_dir().join("absent.txt").to_string_lossy().as_ref()).bind(stamp()).execute(company.pool()).await.unwrap();
    let local = memos::save_impl(&company, input("公司写的说明"))
        .await
        .unwrap();
    let device = runtime(&company).await.unwrap();
    assert!(sync_exchange(&company, &dav, &c, &key, &device, true)
        .await
        .is_err());
    assert_eq!(
        memos::get_impl(&company, &memo.summary.id)
            .await
            .unwrap()
            .summary
            .title,
        "家里写的流程"
    );
    download(&home, &dav, &c, &key).await.unwrap();
    assert_eq!(
        memos::get_impl(&home, &local.summary.id)
            .await
            .unwrap()
            .summary
            .title,
        "公司写的说明"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks WHERE id='local-task'")
            .fetch_one(company.pool())
            .await
            .unwrap(),
        1
    );
    close(home).await;
    close(company).await;
}
#[tokio::test]
async fn all_business_sync_merges_separate_records_and_selective_download_keeps_tasks_in_cloud() {
    let mock = Mock::new();
    let dav = mock.dav();
    let a = db().await;
    let b = db().await;
    let only_memos = db().await;
    let c = cfg();
    let key = [4; 32];
    add_task(&a, "task-1", "完整任务").await;
    add_task(&a, "task-2", "第二条").await;
    sqlx::query("INSERT INTO subtasks(id,task_id,title,created_at,updated_at) VALUES('sub-1','task-1','检查子任务',?,?)").bind(stamp()).bind(stamp()).execute(a.pool()).await.unwrap();
    let memo = memos::save_impl(&a, input("家里的业务流程")).await.unwrap();
    push_all(&a, &dav, &c, &key).await;
    download(&b, &dav, &c, &key).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(b.pool())
        .await
        .unwrap();
    assert_eq!(count, 2);
    let mut selective = c.clone();
    selective.inherit_all = false;
    download(&only_memos, &dav, &selective, &key).await.unwrap();
    assert_eq!(
        memos::get_impl(&only_memos, &memo.summary.id)
            .await
            .unwrap()
            .summary
            .title,
        "家里的业务流程"
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(only_memos.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
    sqlx::query("UPDATE tasks SET title='家里改任务' WHERE id='task-1'")
        .execute(a.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE subtasks SET is_done=1 WHERE id='sub-1'")
        .execute(b.pool())
        .await
        .unwrap();
    push_all(&a, &dav, &c, &key).await;
    push_all(&b, &dav, &c, &key).await;
    download(&a, &dav, &c, &key).await.unwrap();
    download(&b, &dav, &c, &key).await.unwrap();
    let done: i64 = sqlx::query_scalar("SELECT is_done FROM subtasks WHERE id='sub-1'")
        .fetch_one(a.pool())
        .await
        .unwrap();
    assert_eq!(done, 1);
    let title: String = sqlx::query_scalar("SELECT title FROM tasks WHERE id='task-1'")
        .fetch_one(b.pool())
        .await
        .unwrap();
    assert_eq!(title, "家里改任务");
    download(&only_memos, &dav, &c, &key).await.unwrap(); // Later enabling all inherits the retained task data.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(only_memos.pool())
        .await
        .unwrap();
    assert_eq!(count, 2);
    close(a).await;
    close(b).await;
    close(only_memos).await;
}
#[tokio::test]
async fn concurrent_task_versions_and_parent_delete_child_edit_never_cascade_away_local_work() {
    let mock = Mock::new();
    let dav = mock.dav();
    let a = db().await;
    let b = db().await;
    let c = cfg();
    let key = [5; 32];
    add_task(&a, "parent", "父任务").await;
    sqlx::query("INSERT INTO subtasks(id,task_id,title,created_at,updated_at) VALUES('child','parent','子任务',?,?)").bind(stamp()).bind(stamp()).execute(a.pool()).await.unwrap();
    push_all(&a, &dav, &c, &key).await;
    download(&b, &dav, &c, &key).await.unwrap();
    sqlx::query("UPDATE tasks SET title='A改动' WHERE id='parent'")
        .execute(a.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE tasks SET title='B改动' WHERE id='parent'")
        .execute(b.pool())
        .await
        .unwrap();
    push_all(&a, &dav, &c, &key).await;
    push_all(&b, &dav, &c, &key).await;
    download(&b, &dav, &c, &key).await.unwrap();
    let conflict = business::conflicts(&b).await.unwrap().remove(0);
    assert_eq!(conflict.heads.len(), 2);
    assert!(
        business::resolve(&b, &conflict.id, &conflict.heads[0], vec![])
            .await
            .is_err()
    );
    business::resolve(&b, &conflict.id, &conflict.heads[0], conflict.heads.clone())
        .await
        .unwrap();
    push_all(&b, &dav, &c, &key).await;
    download(&a, &dav, &c, &key).await.unwrap();
    sqlx::query("DELETE FROM tasks WHERE id='parent'")
        .execute(a.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE subtasks SET title='必须保留的公司编辑' WHERE id='child'")
        .execute(b.pool())
        .await
        .unwrap();
    push_all(&a, &dav, &c, &key).await;
    push_all(&b, &dav, &c, &key).await;
    assert!(download(&b, &dav, &c, &key).await.is_err());
    let title: String = sqlx::query_scalar("SELECT title FROM subtasks WHERE id='child'")
        .fetch_one(b.pool())
        .await
        .unwrap();
    assert_eq!(title, "必须保留的公司编辑");
    let conflicts = business::conflicts(&b).await.unwrap();
    assert!(conflicts.iter().any(|c| c.table == "tasks"));
    assert!(conflicts.iter().any(|c| c.table == "subtasks"));
    let parent = conflicts.iter().find(|c| c.table == "tasks").unwrap();
    let earlier = parent.versions.iter().find(|v| v.row.is_some()).unwrap();
    business::resolve(&b, &parent.id, &earlier.id, parent.heads.clone())
        .await
        .unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM tasks WHERE id='parent')")
            .fetch_one(b.pool())
            .await
            .unwrap()
    );
    close(a).await;
    close(b).await;
}
#[tokio::test]
async fn legacy_occurrence_ids_and_same_named_tags_remain_editable_without_duplicate_tasks() {
    let mock = Mock::new();
    let dav = mock.dav();
    let a = db().await;
    let b = db().await;
    let c = cfg();
    let key = [6; 32];
    for (db, task, tag) in [
        (&a, "home-random-id", "home-tag"),
        (&b, "company-random-id", "company-tag"),
    ] {
        sqlx::query("INSERT INTO task_series(id,rrule,dtstart_local,created_at,updated_at) VALUES('shared-series','FREQ=DAILY','2026-09-30T09:00:00',?,?)").bind(stamp()).bind(stamp()).execute(db.pool()).await.unwrap();
        add_task(db, task, "关键词更新").await;
        sqlx::query("UPDATE tasks SET series_id='shared-series',occurrence_key='2026-09-30',occurrence_kind='generated' WHERE id=?").bind(task).execute(db.pool()).await.unwrap();
        sqlx::query("INSERT INTO tags(id,name,created_at,updated_at) VALUES(?,'工作',?,?)")
            .bind(tag)
            .bind(stamp())
            .bind(stamp())
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO task_tags(task_id,tag_id) VALUES(?,?)")
            .bind(task)
            .bind(tag)
            .execute(db.pool())
            .await
            .unwrap();
    }
    push_all(&a, &dav, &c, &key).await;
    push_all(&b, &dav, &c, &key).await;
    download(&b, &dav, &c, &key).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(b.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tags")
        .fetch_one(b.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    let conflicts = business::conflicts(&b).await.unwrap();
    let task = conflicts.iter().find(|c| c.table == "tasks").unwrap();
    business::resolve(&b, &task.id, &task.heads[0], task.heads.clone())
        .await
        .unwrap();
    assert!(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE id='company-random-id')"
    )
    .fetch_one(b.pool())
    .await
    .unwrap());
    sqlx::query("UPDATE tasks SET title='公司编辑已继承的任务' WHERE id='company-random-id'")
        .execute(b.pool())
        .await
        .unwrap();
    push_all(&b, &dav, &c, &key).await;
    download(&a, &dav, &c, &key).await.unwrap();
    let title: String = sqlx::query_scalar("SELECT title FROM tasks WHERE id='home-random-id'")
        .fetch_one(a.pool())
        .await
        .unwrap();
    assert_eq!(title, "公司编辑已继承的任务");
    close(a).await;
    close(b).await;
}
#[tokio::test]
async fn initial_400_tasks_use_batches_and_missing_old_attachment_does_not_destroy_history() {
    let mock = Mock::new();
    let dav = mock.dav();
    let a = db().await;
    let b = db().await;
    let c = cfg();
    let key = [3; 32];
    for n in 0..400 {
        add_task(&a, &format!("t-{n}"), "批量业务任务").await;
    }
    for _ in 0..4 {
        push_all(&a, &dav, &c, &key).await;
    }
    download(&b, &dav, &c, &key).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(b.pool())
        .await
        .unwrap();
    assert_eq!(count, 400);
    let requests: i64 =
        sqlx::query_scalar("SELECT request_count FROM memo_sync_runtime WHERE singleton=1")
            .fetch_one(a.pool())
            .await
            .unwrap();
    assert!(
        requests < 30,
        "must batch rows rather than make 400 requests: {requests}"
    );
    let id = uuid::Uuid::now_v7().to_string();
    let original = a.data_dir().join("原文件.txt");
    std::fs::write(&original, b"actual business attachment").unwrap();
    sqlx::query("INSERT INTO attachments(id,task_id,file_name,storage_mode,external_path,created_at) VALUES(?,'t-0','原文件.txt','reference',?,?)").bind(&id).bind(original.to_string_lossy().as_ref()).bind(stamp()).execute(a.pool()).await.unwrap();
    business::capture_scope(&a, true).await.unwrap();
    std::fs::remove_file(original).unwrap();
    push_all(&a, &dav, &c, &key).await;
    download(&b, &dav, &c, &key).await.unwrap();
    let stored: String = sqlx::query_scalar("SELECT stored_path FROM attachments WHERE id=?")
        .bind(&id)
        .fetch_one(b.pool())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(b.data_dir().join(stored)).unwrap(),
        b"actual business attachment"
    );
    close(a).await;
    close(b).await;
}

fn selection(c: &Config, scope: SyncScope, direction: SyncDirection) -> Config {
    let mut c = c.clone();
    c.default_sync = Some(SyncOptions { scope, direction });
    c
}
#[tokio::test]
async fn selected_download_never_writes_cloud_and_keeps_local_pending_memos() {
    let mock = Mock::new();
    let dav = mock.dav();
    let remote = db().await;
    let local = db().await;
    let c = cfg();
    let key = [8; 32];
    let from_cloud = memos::save_impl(&remote, input("云端流程")).await.unwrap();
    push_all(&remote, &dav, &c, &key).await;
    let mine = memos::save_impl(&local, input("本地未上传流程"))
        .await
        .unwrap();
    let before = mock.files.lock().unwrap().clone();
    let selected = selection(&c, SyncScope::Memos, SyncDirection::Download);
    let device = runtime(&local).await.unwrap();
    assert!(sync_exchange(&local, &dav, &selected, &key, &device, true)
        .await
        .unwrap());
    assert_eq!(
        *mock.files.lock().unwrap(),
        before,
        "download-only must not write any remote file"
    );
    assert!(memos::get_impl(&local, &from_cloud.summary.id)
        .await
        .is_ok());
    assert!(memos::get_impl(&local, &mine.summary.id).await.is_ok());
    let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memo_sync_events WHERE uploaded=0")
        .fetch_one(local.pool())
        .await
        .unwrap();
    assert_eq!(pending, 1);
    close(remote).await;
    close(local).await;
}
#[tokio::test]
async fn selected_task_upload_leaves_memo_pending_and_does_not_download() {
    let mock = Mock::new();
    let dav = mock.dav();
    let remote = db().await;
    let local = db().await;
    let c = cfg();
    let key = [7; 32];
    let from_cloud = memos::save_impl(&remote, input("不该下载的流程"))
        .await
        .unwrap();
    push_all(&remote, &dav, &c, &key).await;
    memos::save_impl(&local, input("不该上传的流程"))
        .await
        .unwrap();
    add_task(&local, "selected-task", "应上传的任务").await;
    let selected = selection(&c, SyncScope::Tasks, SyncDirection::Upload);
    let device = runtime(&local).await.unwrap();
    assert!(!sync_exchange(&local, &dav, &selected, &key, &device, true)
        .await
        .unwrap());
    assert!(memos::get_impl(&local, &from_cloud.summary.id)
        .await
        .is_err());
    let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memo_sync_events WHERE uploaded=0")
        .fetch_one(local.pool())
        .await
        .unwrap();
    assert_eq!(pending, 1);
    download(&remote, &dav, &c, &key).await.unwrap();
    let title: String = sqlx::query_scalar("SELECT title FROM tasks WHERE id='selected-task'")
        .fetch_one(remote.pool())
        .await
        .unwrap();
    assert_eq!(title, "应上传的任务");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memo_documents")
        .fetch_one(remote.pool())
        .await
        .unwrap();
    assert_eq!(count, 1);
    close(remote).await;
    close(local).await;
}
#[tokio::test]
async fn selected_task_download_ignores_memos_and_preserves_local_task_changes() {
    let mock = Mock::new();
    let dav = mock.dav();
    let remote = db().await;
    let local = db().await;
    let c = cfg();
    let key = [6; 32];
    let memo = memos::save_impl(&remote, input("不下载的云端流程"))
        .await
        .unwrap();
    add_task(&remote, "cloud-task", "云端任务").await;
    push_all(&remote, &dav, &c, &key).await;
    add_task(&local, "local-task", "保留本地任务").await;
    let selected = selection(&c, SyncScope::Tasks, SyncDirection::Download);
    let device = runtime(&local).await.unwrap();
    let before = mock.files.lock().unwrap().clone();
    assert!(sync_exchange(&local, &dav, &selected, &key, &device, true)
        .await
        .unwrap());
    assert_eq!(*mock.files.lock().unwrap(), before);
    assert!(memos::get_impl(&local, &memo.summary.id).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(local.pool())
        .await
        .unwrap();
    assert_eq!(count, 2);
    close(remote).await;
    close(local).await;
}

#[tokio::test]
async fn all_nine_scope_direction_combinations_only_exchange_selected_content() {
    for scope in [SyncScope::All, SyncScope::Tasks, SyncScope::Memos] {
        for direction in [
            SyncDirection::Both,
            SyncDirection::Upload,
            SyncDirection::Download,
        ] {
            let mock = Mock::new();
            let dav = mock.dav();
            let remote = db().await;
            let local = db().await;
            let c = cfg();
            let key = [3; 32];
            let remote_memo = memos::save_impl(&remote, input("远端备忘")).await.unwrap();
            add_task(&remote, "remote-task", "远端任务").await;
            add_knowledge_source(
                &remote,
                "远端资料.txt",
                b"remote knowledge bytes",
                "远端知识库正文",
            )
            .await;
            push_all(&remote, &dav, &c, &key).await;
            let local_memo = memos::save_impl(&local, input("本地备忘")).await.unwrap();
            add_task(&local, "local-task", "本地任务").await;
            add_knowledge_source(
                &local,
                "本地资料.txt",
                b"local knowledge bytes",
                "本地知识库正文",
            )
            .await;
            let before = mock.files.lock().unwrap().clone();
            let device = runtime(&local).await.unwrap();
            let selected = selection(&c, scope, direction);
            sync_exchange(&local, &dav, &selected, &key, &device, true)
                .await
                .unwrap();
            let downloads = direction != SyncDirection::Upload;
            let uploads = direction != SyncDirection::Download;
            let memos = scope != SyncScope::Tasks;
            let tasks = scope != SyncScope::Memos;
            assert_eq!(
                memos::get_impl(&local, &remote_memo.summary.id)
                    .await
                    .is_ok(),
                downloads && memos,
                "{scope:?}/{direction:?}: memo download"
            );
            let downloaded_task: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tasks WHERE id='remote-task')")
                    .fetch_one(local.pool())
                    .await
                    .unwrap();
            assert_eq!(
                downloaded_task,
                downloads && tasks,
                "{scope:?}/{direction:?}: task download"
            );
            let downloaded_knowledge: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM knowledge_sources WHERE title='远端资料.txt')",
            )
            .fetch_one(local.pool())
            .await
            .unwrap();
            assert_eq!(
                downloaded_knowledge,
                downloads && scope == SyncScope::All,
                "{scope:?}/{direction:?}: knowledge download"
            );
            let local_knowledge_head = format!("/Lumen/devices/{device}/knowledge-head.json");
            assert_eq!(
                mock.files
                    .lock()
                    .unwrap()
                    .contains_key(&local_knowledge_head),
                uploads && scope == SyncScope::All,
                "{scope:?}/{direction:?}: knowledge head publication"
            );
            if !uploads {
                assert!(
                    *mock.files.lock().unwrap() == before,
                    "download-only cannot change remote files"
                );
            }
            download(&remote, &dav, &c, &key).await.unwrap();
            assert_eq!(
                memos::get_impl(&remote, &local_memo.summary.id)
                    .await
                    .is_ok(),
                uploads && memos,
                "{scope:?}/{direction:?}: memo upload"
            );
            let uploaded_task: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tasks WHERE id='local-task')")
                    .fetch_one(remote.pool())
                    .await
                    .unwrap();
            assert_eq!(
                uploaded_task,
                uploads && tasks,
                "{scope:?}/{direction:?}: task upload"
            );
            let uploaded_knowledge: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM knowledge_sources WHERE title='本地资料.txt')",
            )
            .fetch_one(remote.pool())
            .await
            .unwrap();
            assert_eq!(
                uploaded_knowledge,
                uploads && scope == SyncScope::All,
                "{scope:?}/{direction:?}: knowledge upload"
            );
            close(remote).await;
            close(local).await;
        }
    }
}

#[tokio::test]
async fn all_scope_syncs_knowledge_original_and_rebuilds_the_local_search_index() {
    let mock = Mock::new();
    let dav = mock.dav();
    let home = db().await;
    let company = db().await;
    let c = cfg();
    let key = [4; 32];
    let title = "仓库标签打印说明.txt";
    let original = b"warehouse label procedure original";
    let extracted = "标贴请在仓库标签打印机打印，打印完核对箱号";
    add_knowledge_source(&home, title, original, extracted).await;

    let device = runtime(&home).await.unwrap();
    add_task(&home, "task-side-by-side", "任务仍走原同步流").await;
    sync_exchange(
        &home,
        &dav,
        &selection(&c, SyncScope::All, SyncDirection::Both),
        &key,
        &device,
        true,
    )
    .await
    .unwrap();
    let ordinary_bytes = mock
        .files
        .lock()
        .unwrap()
        .get(&format!("/Lumen/devices/{device}/head.json"))
        .unwrap()
        .clone();
    let ordinary: Head = decrypt(
        &key,
        &aad(&c, &format!("devices/{device}/head.json")),
        &ordinary_bytes,
    )
    .unwrap();
    for head in ordinary.business_heads {
        let packet_json: String =
            sqlx::query_scalar("SELECT payload_json FROM cloud_packets WHERE id=?")
                .bind(head)
                .fetch_one(home.pool())
                .await
                .unwrap();
        let packet: serde_json::Value = serde_json::from_str(&packet_json).unwrap();
        assert!(packet["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|event| { event["table"] != "knowledge_sources" }));
    }
    let knowledge_path = format!("devices/{device}/knowledge-head.json");
    let knowledge_bytes = mock
        .files
        .lock()
        .unwrap()
        .get(&format!("/Lumen/{knowledge_path}"))
        .unwrap()
        .clone();
    let knowledge: KnowledgeHead =
        decrypt(&key, &aad(&c, &knowledge_path), &knowledge_bytes).unwrap();
    assert!(
        !knowledge.heads.is_empty(),
        "knowledge data uses its own head file"
    );
    let other_device = runtime(&company).await.unwrap();
    sync_exchange(
        &company,
        &dav,
        &selection(&c, SyncScope::All, SyncDirection::Both),
        &key,
        &other_device,
        true,
    )
    .await
    .unwrap();

    let row = sqlx::query("SELECT s.title,s.extracted_text,s.sha256,a.data_base64 FROM knowledge_sources s JOIN content_assets a ON a.id=s.asset_id")
        .fetch_one(company.pool()).await.unwrap();
    assert_eq!(row.get::<String, _>("title"), title);
    assert_eq!(row.get::<String, _>("extracted_text"), extracted);
    let asset: String = row.get("data_base64");
    assert_eq!(STANDARD.decode(asset).unwrap(), original);
    let indexed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_chunks_fts")
        .fetch_one(company.pool())
        .await
        .unwrap();
    assert!(
        indexed > 0,
        "the receiving computer must rebuild its local FTS index"
    );

    let cloud_bytes: Vec<Vec<u8>> = mock.files.lock().unwrap().values().cloned().collect();
    assert!(cloud_bytes.iter().all(|blob| !blob
        .windows(extracted.len())
        .any(|w| w == extracted.as_bytes())));
    assert!(cloud_bytes
        .iter()
        .all(|blob| !blob.windows(title.len()).any(|w| w == title.as_bytes())));
    close(home).await;
    close(company).await;
}

#[tokio::test]
async fn same_knowledge_file_imported_on_both_devices_does_not_create_a_conflict() {
    let mock = Mock::new();
    let dav = mock.dav();
    let home = db().await;
    let company = db().await;
    let c = cfg();
    let key = [5; 32];
    let original = b"same source bytes";
    let extracted = "同一份资料的解析正文";
    add_knowledge_source(&home, "相同资料.txt", original, extracted).await;
    add_knowledge_source(&company, "相同资料.txt", original, extracted).await;

    for local in [&home, &company] {
        let device = runtime(local).await.unwrap();
        sync_exchange(
            local,
            &dav,
            &selection(&c, SyncScope::All, SyncDirection::Both),
            &key,
            &device,
            true,
        )
        .await
        .unwrap();
    }

    let conflicts = business::conflicts(&company).await.unwrap();
    assert!(conflicts
        .iter()
        .all(|conflict| conflict.table != "knowledge_sources"));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_sources")
        .fetch_one(company.pool())
        .await
        .unwrap();
    assert_eq!(
        count, 1,
        "matching content hashes represent one knowledge source"
    );
    close(home).await;
    close(company).await;
}

#[tokio::test]
async fn deleting_a_synced_knowledge_source_keeps_assets_referenced_by_a_flow() {
    let mock = Mock::new();
    let dav = mock.dav();
    let home = db().await;
    let company = db().await;
    let c = cfg();
    let key = [9; 32];
    let source_id = add_knowledge_source(
        &home,
        "装箱步骤.txt",
        b"packing instructions",
        "先贴标签，再装箱",
    )
    .await;
    let asset_id: String = sqlx::query_scalar("SELECT asset_id FROM knowledge_sources WHERE id=?")
        .bind(&source_id)
        .fetch_one(home.pool())
        .await
        .unwrap();
    let mut flow = input("装箱流程");
    flow.body_md = format!("请核对原件：[装箱说明](lumen-asset:{asset_id})");
    memos::save_impl(&home, flow).await.unwrap();
    push_all(&home, &dav, &c, &key).await;

    let device = runtime(&company).await.unwrap();
    sync_exchange(
        &company,
        &dav,
        &selection(&c, SyncScope::All, SyncDirection::Both),
        &key,
        &device,
        true,
    )
    .await
    .unwrap();
    let received_asset: String =
        sqlx::query_scalar("SELECT asset_id FROM knowledge_sources WHERE title='装箱步骤.txt'")
            .fetch_one(company.pool())
            .await
            .unwrap();
    let flow_body: String =
        sqlx::query_scalar("SELECT body_md FROM memo_documents WHERE title='装箱流程'")
            .fetch_one(company.pool())
            .await
            .unwrap();
    assert!(flow_body.contains(&format!("lumen-asset:{received_asset}")));

    sqlx::query("DELETE FROM knowledge_sources WHERE title='装箱步骤.txt'")
        .execute(company.pool())
        .await
        .unwrap();
    let device = runtime(&company).await.unwrap();
    sync_exchange(
        &company,
        &dav,
        &selection(&c, SyncScope::All, SyncDirection::Both),
        &key,
        &device,
        true,
    )
    .await
    .unwrap();
    let home_device = runtime(&home).await.unwrap();
    sync_exchange(
        &home,
        &dav,
        &selection(&c, SyncScope::All, SyncDirection::Both),
        &key,
        &home_device,
        true,
    )
    .await
    .unwrap();
    let home_source: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_sources WHERE title='装箱步骤.txt')",
    )
    .fetch_one(home.pool())
    .await
    .unwrap();
    assert!(!home_source, "source tombstone must reach the other device");
    let preserved_asset: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content_assets WHERE id=?)")
            .bind(&received_asset)
            .fetch_one(company.pool())
            .await
            .unwrap();
    assert!(preserved_asset, "flow references keep the original asset");
    close(home).await;
    close(company).await;
}

#[tokio::test]
async fn missing_knowledge_original_aborts_import_without_a_partial_source() {
    let mock = Mock::new();
    let dav = mock.dav();
    let home = db().await;
    let company = db().await;
    let c = cfg();
    let key = [10; 32];
    add_knowledge_source(&home, "缺失原件.txt", b"original bytes", "可检索正文").await;
    push_all(&home, &dav, &c, &key).await;
    let canonical: String = sqlx::query_scalar(
        "SELECT canonical_id FROM cloud_aliases WHERE table_name='content_assets'",
    )
    .fetch_one(home.pool())
    .await
    .unwrap();
    let asset_path = format!("/Lumen/assets/{}/{}.json", &canonical[..2], canonical);
    mock.files.lock().unwrap().remove(&asset_path);

    let device = runtime(&company).await.unwrap();
    let result = sync_exchange(
        &company,
        &dav,
        &selection(&c, SyncScope::All, SyncDirection::Download),
        &key,
        &device,
        true,
    )
    .await;
    assert!(result.is_err(), "missing original must fail the sync");
    let sources: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_sources")
        .fetch_one(company.pool())
        .await
        .unwrap();
    assert_eq!(sources, 0, "a partial source must not be visible");
    let packets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cloud_packets")
        .fetch_one(company.pool())
        .await
        .unwrap();
    assert_eq!(
        packets, 0,
        "the packet must not be acknowledged before assets validate"
    );
    close(home).await;
    close(company).await;
}

#[tokio::test]
async fn restore_rebase_outside_all_scope_preserves_knowledge_sync_baseline() {
    let local = db().await;
    add_knowledge_source(&local, "仅本机资料.txt", b"local only", "本机解析正文").await;
    business::capture_scope(&local, true).await.unwrap();
    let before: Option<String> = sqlx::query_scalar(
        "SELECT base_json FROM cloud_records WHERE table_name='knowledge_sources'",
    )
    .fetch_one(local.pool())
    .await
    .unwrap();
    assert!(before.is_some());

    let mut tx = local.pool().begin().await.unwrap();
    business::rebase_restore(&mut tx, local.data_dir(), false)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let after: Option<String> = sqlx::query_scalar(
        "SELECT base_json FROM cloud_records WHERE table_name='knowledge_sources'",
    )
    .fetch_one(local.pool())
    .await
    .unwrap();
    assert_eq!(
        after, before,
        "non-All restore must not rebase local KB data"
    );
    close(local).await;
}

#[tokio::test]
async fn pending_knowledge_source_delete_keeps_original_until_tombstone_uploads() {
    let mock = Mock::new();
    let dav = mock.dav();
    let local = db().await;
    let remote = db().await;
    let c = cfg();
    let key = [11; 32];
    let source_id =
        add_knowledge_source(&local, "刚导入的资料.txt", b"pending source", "待同步正文").await;
    business::capture_scope(&local, true).await.unwrap();
    let asset_id: String = sqlx::query_scalar("SELECT asset_id FROM knowledge_sources WHERE id=?")
        .bind(&source_id)
        .fetch_one(local.pool())
        .await
        .unwrap();

    let retained = crate::knowledge_base::delete_impl(&local, &source_id)
        .await
        .unwrap();
    assert!(
        retained,
        "pending source history still needs the local original"
    );
    let asset_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content_assets WHERE id=?)")
            .bind(&asset_id)
            .fetch_one(local.pool())
            .await
            .unwrap();
    assert!(asset_exists);

    let device = runtime(&local).await.unwrap();
    sync_exchange(
        &local,
        &dav,
        &selection(&c, SyncScope::All, SyncDirection::Both),
        &key,
        &device,
        true,
    )
    .await
    .unwrap();
    let asset_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content_assets WHERE id=?)")
            .bind(&asset_id)
            .fetch_one(local.pool())
            .await
            .unwrap();
    assert!(
        !asset_exists,
        "after its tombstone uploads, an unreferenced original is cleaned up"
    );
    let remote_device = runtime(&remote).await.unwrap();
    sync_exchange(
        &remote,
        &dav,
        &selection(&c, SyncScope::All, SyncDirection::Both),
        &key,
        &remote_device,
        true,
    )
    .await
    .unwrap();
    let remote_sources: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_sources")
        .fetch_one(remote.pool())
        .await
        .unwrap();
    let remote_assets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM content_assets")
        .fetch_one(remote.pool())
        .await
        .unwrap();
    assert_eq!(remote_sources, 0);
    assert_eq!(
        remote_assets, 0,
        "remote import prunes an orphaned original after a final tombstone"
    );
    close(local).await;
    close(remote).await;
}

#[tokio::test]
async fn alias_audit_rebuilt_occurrence_accepts_peer_edit_without_duplicate() {
    let mock = Mock::new();
    let dav = mock.dav();
    let a = db().await;
    let b = db().await;
    let c = cfg();
    let key = [6; 32];
    sqlx::query("INSERT INTO task_series(id,rrule,tzid,dtstart_local,created_at,updated_at) VALUES('shared-series','FREQ=DAILY','UTC','2026-09-30T09:00:00',?,?)").bind(stamp()).bind(stamp()).execute(a.pool()).await.unwrap();
    add_task(&a, "0199-old-task", "重建前").await;
    sqlx::query("UPDATE tasks SET series_id='shared-series',occurrence_key='2026-09-30T09:00:00.000Z',planned_at='2026-09-30T09:00:00.000Z',occurrence_kind='generated' WHERE id='0199-old-task'").execute(a.pool()).await.unwrap();
    push_all(&a, &dav, &c, &key).await;
    download(&b, &dav, &c, &key).await.unwrap();
    // A recurrence rebuild replaces a disposable cache row while retaining its natural occurrence identity.
    sqlx::query("DELETE FROM tasks WHERE id='0199-old-task'")
        .execute(a.pool())
        .await
        .unwrap();
    add_task(&a, "019a-new-task", "重建后").await;
    sqlx::query("UPDATE tasks SET series_id='shared-series',occurrence_key='2026-09-30T09:00:00.000Z',planned_at='2026-09-30T09:00:00.000Z',occurrence_kind='generated' WHERE id='019a-new-task'").execute(a.pool()).await.unwrap();
    push_all(&a, &dav, &c, &key).await;
    download(&b, &dav, &c, &key).await.unwrap();
    sqlx::query("UPDATE tasks SET title='另一台设备的编辑' WHERE series_id='shared-series'")
        .execute(b.pool())
        .await
        .unwrap();
    push_all(&b, &dav, &c, &key).await;
    download(&a, &dav, &c, &key)
        .await
        .expect("peer edit must target the existing rebuilt occurrence");
    let rows: Vec<(String,String)> = sqlx::query_as("SELECT id,title FROM tasks WHERE series_id='shared-series' AND occurrence_key='2026-09-30T09:00:00.000Z'").fetch_all(a.pool()).await.unwrap();
    assert_eq!(
        rows,
        vec![("019a-new-task".into(), "另一台设备的编辑".into())]
    );
    close(a).await;
    close(b).await;
}
