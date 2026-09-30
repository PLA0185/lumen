use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
struct Mock {
    url: String,
    files: Arc<Mutex<std::collections::HashMap<String, Vec<u8>>>>,
    fail: Arc<AtomicBool>,
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
                        "MKCOL" => 201,
                        "PUT" => {
                            if headers.to_lowercase().contains("if-none-match: *")
                                && files.contains_key(&path)
                            {
                                412
                            } else {
                                files.insert(path, bytes[end..].to_vec());
                                201
                            }
                        }
                        "GET" => {
                            if let Some(v) = files.get(&path) {
                                body = v.clone();
                                200
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
        Self { url, files, fail }
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
fn cfg() -> Config {
    Config {
        server: "https://example.test/dav/".into(),
        account: "test".into(),
        folder: "Lumen".into(),
        connection_id: uuid::Uuid::now_v7().to_string(),
        workspace_id: uuid::Uuid::now_v7().to_string(),
        enabled: true,
        inherit_all: true,
    }
}
async fn close(db: Db) {
    let dir = db.data_dir().to_owned();
    db.pool().close().await;
    std::fs::remove_dir_all(dir).unwrap();
}
async fn push(db: &Db, dav: &Dav, c: &Config, key: &[u8; 32]) {
    let device = runtime(db).await.unwrap();
    upload(db, dav, c, key, &device).await.unwrap();
    publish_head(db, dav, c, key, &device).await.unwrap();
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
    business::capture(db).await.unwrap();
    business::upload(db, dav, c, key).await.unwrap();
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
    business::capture(&a).await.unwrap();
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
