use super::{
    AppError, AppResult, Db, ErrorCode, SyncRemote, MAX_JSON, SYNC_GITHUB_BUDGET_EXCEEDED,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::time::Duration;

const API: &str = "https://api.github.com/";
const CHUNK_BYTES: usize = 45 * 1024 * 1024;
const MAX_CHUNKED_FILE: usize = 400 * 1024 * 1024;

pub(super) struct GitHub {
    client: reqwest::Client,
    owner: String,
    repository: String,
    branch: String,
    folder: String,
    token: String,
}

#[derive(Deserialize)]
struct Metadata {
    sha: String,
}

#[derive(Deserialize)]
struct RepositoryInfo {
    private: bool,
}

#[derive(Deserialize)]
struct DirectoryEntry {
    name: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChunkManifest {
    version: u8,
    size: usize,
    sha256: String,
    chunks: Vec<ChunkRef>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChunkRef {
    path: String,
    size: usize,
    sha256: String,
}

impl GitHub {
    pub(super) fn new(
        owner: &str,
        repository: &str,
        branch: &str,
        folder: &str,
        token: &str,
    ) -> AppResult<Self> {
        let valid_component = |value: &str| {
            !value.is_empty()
                && value.len() <= 100
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                && value != "."
                && value != ".."
        };
        if !valid_component(owner) || !valid_component(repository) {
            return Err(AppError::validation("GitHub 用户名和仓库名格式无效"));
        }
        if branch.trim().is_empty()
            || branch.len() > 250
            || branch.starts_with('-')
            || branch.contains("..")
            || branch.chars().any(char::is_whitespace)
        {
            return Err(AppError::validation("GitHub 分支名无效"));
        }
        if !folder.is_empty()
            && (folder.len() > 180 || folder.split('/').any(|part| !valid_component(part)))
        {
            return Err(AppError::validation("GitHub 仓库内路径格式无效"));
        }
        if token.trim().is_empty() {
            return Err(AppError::validation(
                "请输入具有该私有仓库 Contents 读写权限的 GitHub Token",
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|_| AppError::internal("无法初始化 GitHub 网络连接"))?;
        Ok(Self {
            client,
            owner: owner.trim().into(),
            repository: repository.trim().into(),
            branch: branch.trim().into(),
            folder: folder.trim_matches('/').into(),
            token: token.trim().into(),
        })
    }

    fn physical_path(&self, path: &str) -> AppResult<String> {
        if path.starts_with('/')
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || path.chars().any(char::is_control)
        {
            return Err(AppError::validation("GitHub 同步路径无效"));
        }
        Ok(if self.folder.is_empty() {
            path.into()
        } else {
            format!("{}/{path}", self.folder)
        })
    }

    fn contents_url(&self, path: &str) -> AppResult<reqwest::Url> {
        let mut url =
            reqwest::Url::parse(API).map_err(|_| AppError::internal("GitHub API 地址无效"))?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| AppError::internal("GitHub API 路径无效"))?;
            segments
                .clear()
                .push("repos")
                .push(&self.owner)
                .push(&self.repository)
                .push("contents");
            for segment in path.split('/') {
                segments.push(segment);
            }
        }
        url.query_pairs_mut().append_pair("ref", &self.branch);
        Ok(url)
    }

    fn directory_url(&self, path: &str) -> AppResult<reqwest::Url> {
        self.contents_url(path)
    }

    fn repository_url(&self) -> AppResult<reqwest::Url> {
        let mut url =
            reqwest::Url::parse(API).map_err(|_| AppError::internal("GitHub API 地址无效"))?;
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| AppError::internal("GitHub API 路径无效"))?;
        segments
            .clear()
            .push("repos")
            .push(&self.owner)
            .push(&self.repository);
        drop(segments);
        Ok(url)
    }

    async fn send(
        &self,
        db: &Db,
        method: reqwest::Method,
        url: reqwest::Url,
        body: Option<serde_json::Value>,
        accept: &str,
    ) -> AppResult<reqwest::Response> {
        let now = chrono::Utc::now().timestamp();
        super::runtime(db).await?;
        let row = sqlx::query("SELECT budget_start,request_count,device_count,retry_until,last_error FROM memo_sync_runtime WHERE singleton=1")
            .fetch_one(db.pool()).await?;
        let retry_until: i64 = row.try_get("retry_until")?;
        if retry_until > now {
            return Err(AppError::new(
                ErrorCode::RateLimited,
                format!(
                    "GitHub 同步正在等待重试，约 {} 后自动继续；本机内容仍已保存。",
                    super::retry_wait_text(retry_until - now)
                ),
            ));
        }
        let reset = now - row.try_get::<i64, _>("budget_start")? >= 1800;
        let count = if reset {
            0
        } else {
            row.try_get::<i64, _>("request_count")?
        };
        let devices = row.try_get::<i64, _>("device_count")?.max(1);
        if count >= 500 / devices {
            let retry_until = row.try_get::<i64, _>("budget_start")? + 1800;
            sqlx::query(
                "UPDATE memo_sync_runtime SET retry_until=?,last_error=? WHERE singleton=1",
            )
            .bind(retry_until)
            .bind(SYNC_GITHUB_BUDGET_EXCEEDED)
            .execute(db.pool())
            .await?;
            return Err(AppError::new(
                ErrorCode::RateLimited,
                format!(
                    "Lumen 本机 GitHub 请求额度已用完，约 {} 后自动续传；本机未上传内容仍保留。",
                    super::retry_wait_text(retry_until - now)
                ),
            ));
        }
        sqlx::query("UPDATE memo_sync_runtime SET budget_start=CASE WHEN ? THEN ? ELSE budget_start END,request_count=? WHERE singleton=1")
            .bind(reset).bind(now).bind(count + 1).execute(db.pool()).await?;
        let mut request = self
            .client
            .request(method, url)
            .bearer_auth(&self.token)
            .header("Accept", accept)
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header(
                reqwest::header::USER_AGENT,
                concat!("Lumen/", env!("CARGO_PKG_VERSION")),
            );
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|_| {
            AppError::new(
                ErrorCode::Network,
                "无法连接 GitHub；本机内容仍保留，将自动重试",
            )
        })?;
        let status = response.status().as_u16();
        if status == 401 {
            return Err(AppError::new(
                ErrorCode::Unauthorized,
                "GitHub Token 无效或已过期，请检查仓库 Contents 读写权限",
            ));
        }
        if status == 429 || status == 403 {
            let retry_after = response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<i64>().ok());
            let remaining = response
                .headers()
                .get("X-RateLimit-Remaining")
                .and_then(|v| v.to_str().ok());
            let reset_at = response
                .headers()
                .get("X-RateLimit-Reset")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<i64>().ok());
            let delay = retry_after
                .or_else(|| {
                    (remaining == Some("0"))
                        .then(|| reset_at.map(|v| v - now))
                        .flatten()
                })
                .unwrap_or(0);
            if delay > 0 {
                let delay = delay.clamp(1, 86_400);
                sqlx::query(
                    "UPDATE memo_sync_runtime SET retry_until=?,last_error=? WHERE singleton=1",
                )
                .bind(now + delay)
                .bind("GitHub API 请求受限")
                .execute(db.pool())
                .await?;
                return Err(AppError::new(
                    ErrorCode::RateLimited,
                    format!(
                        "GitHub 暂时限制 API 请求，约 {} 后自动重试；本机未上传内容仍保留。",
                        super::retry_wait_text(delay)
                    ),
                ));
            }
            return Err(AppError::new(
                ErrorCode::Unauthorized,
                "GitHub Token 缺少仓库 Contents 读写权限，或该私有仓库未授权给此 Token",
            ));
        }
        Ok(response)
    }

    async fn raw_file(&self, db: &Db, path: &str, max: usize) -> AppResult<Option<Vec<u8>>> {
        let url = self.contents_url(&self.physical_path(path)?)?;
        let response = self
            .send(
                db,
                reqwest::Method::GET,
                url,
                None,
                "application/vnd.github.raw",
            )
            .await?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Network,
                format!("GitHub 文件读取失败（HTTP {}）", response.status().as_u16()),
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| AppError::new(ErrorCode::Network, "GitHub 文件下载中断，将自动重试"))?;
        if bytes.len() > max {
            return Err(AppError::validation(
                "GitHub 云端文件超过当前内容的安全大小限制",
            ));
        }
        Ok(Some(bytes.to_vec()))
    }

    async fn metadata_sha(&self, db: &Db, physical_path: &str) -> AppResult<Option<String>> {
        let url = self.contents_url(physical_path)?;
        let response = self
            .send(
                db,
                reqwest::Method::GET,
                url,
                None,
                "application/vnd.github+json",
            )
            .await?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Network,
                format!(
                    "GitHub 文件信息读取失败（HTTP {}）",
                    response.status().as_u16()
                ),
            ));
        }
        response
            .json::<Metadata>()
            .await
            .map(|metadata| Some(metadata.sha))
            .map_err(|_| AppError::validation("GitHub 文件信息格式无效"))
    }

    async fn write_small(
        &self,
        db: &Db,
        path: &str,
        value: &[u8],
        overwrite: bool,
    ) -> AppResult<u16> {
        if value.len() > CHUNK_BYTES {
            return Err(AppError::validation("GitHub 单个同步分块不能超过 45 MiB"));
        }
        let physical = self.physical_path(path)?;
        let sha = if overwrite {
            self.metadata_sha(db, &physical).await?
        } else {
            None
        };
        let mut body = serde_json::json!({
            "message": if sha.is_some() { "Lumen encrypted sync update" } else { "Lumen encrypted sync create" },
            "content": STANDARD.encode(value),
            "branch": self.branch.clone(),
        });
        if let Some(sha) = sha {
            body["sha"] = serde_json::Value::String(sha);
        }
        let url = self.contents_url(&physical)?;
        let response = self
            .send(
                db,
                reqwest::Method::PUT,
                url,
                Some(body),
                "application/vnd.github+json",
            )
            .await?;
        let status = response.status().as_u16();
        if !matches!(status, 200 | 201 | 409 | 422) {
            return Err(AppError::new(
                ErrorCode::Network,
                format!("GitHub 文件发布失败（HTTP {status}）；本机内容仍保留"),
            ));
        }
        Ok(if matches!(status, 409 | 422) && !overwrite {
            409
        } else {
            status
        })
    }

    async fn delete_physical(&self, db: &Db, physical: &str) -> AppResult<()> {
        let Some(sha) = self.metadata_sha(db, physical).await? else {
            return Ok(());
        };
        let body = serde_json::json!({"message":"Lumen encrypted sync cleanup","sha":sha,"branch":self.branch});
        let response = self
            .send(
                db,
                reqwest::Method::DELETE,
                self.contents_url(physical)?,
                Some(body),
                "application/vnd.github+json",
            )
            .await?;
        if response.status().is_success() || response.status().as_u16() == 404 {
            Ok(())
        } else {
            Err(AppError::new(
                ErrorCode::Network,
                format!(
                    "GitHub 清理连接验证文件失败（HTTP {}）",
                    response.status().as_u16()
                ),
            ))
        }
    }

    async fn chunks_manifest(&self, db: &Db, path: &str) -> AppResult<Option<ChunkManifest>> {
        let manifest_path = format!("{path}.lumen-chunks.json");
        self.raw_file(db, &manifest_path, MAX_JSON)
            .await?
            .map(|bytes| {
                serde_json::from_slice(&bytes)
                    .map_err(|_| AppError::validation("GitHub 大文件分块清单损坏"))
            })
            .transpose()
    }

    async fn write_chunked(
        &self,
        db: &Db,
        path: &str,
        value: &[u8],
        overwrite: bool,
    ) -> AppResult<u16> {
        if value.len() > MAX_CHUNKED_FILE {
            return Err(AppError::validation(
                "GitHub 云同步单个文件目前最多 400 MiB；本机内容仍保留",
            ));
        }
        let mut chunks = Vec::new();
        for part in value.chunks(CHUNK_BYTES) {
            let hash = hex::encode(Sha256::digest(part));
            let chunk_path = format!(".lumen/chunks/{hash}");
            let result = self.write_small(db, &chunk_path, part, false).await?;
            if result == 409 {
                let existing = self
                    .raw_file(db, &chunk_path, CHUNK_BYTES)
                    .await?
                    .ok_or_else(|| AppError::conflict("GitHub 已存在的内容块无法核实"))?;
                if hex::encode(Sha256::digest(&existing)) != hash {
                    return Err(AppError::conflict(
                        "GitHub 内容块摘要不匹配，未覆盖云端内容",
                    ));
                }
            }
            chunks.push(ChunkRef {
                path: chunk_path,
                size: part.len(),
                sha256: hash,
            });
        }
        let manifest = ChunkManifest {
            version: 1,
            size: value.len(),
            sha256: hex::encode(Sha256::digest(value)),
            chunks,
        };
        let logical_path = path;
        let path = format!("{logical_path}.lumen-chunks.json");
        let status = self
            .write_small(
                db,
                &path,
                &serde_json::to_vec(&manifest)
                    .map_err(|_| AppError::internal("无法编码 GitHub 分块清单"))?,
                overwrite,
            )
            .await?;
        if status < 400 {
            // Remove an older small-file representation only after the new manifest is committed.
            self.delete_physical(db, &self.physical_path(logical_path)?)
                .await?;
        }
        Ok(status)
    }

    async fn read_chunked(
        &self,
        db: &Db,
        manifest: ChunkManifest,
        max: usize,
    ) -> AppResult<Vec<u8>> {
        if manifest.version != 1 || manifest.size > max || manifest.size > MAX_CHUNKED_FILE {
            return Err(AppError::validation("GitHub 分块文件大小或版本不兼容"));
        }
        let mut output = Vec::with_capacity(manifest.size);
        for chunk in manifest.chunks {
            if output.len().saturating_add(chunk.size) > manifest.size || chunk.size > CHUNK_BYTES {
                return Err(AppError::validation("GitHub 分块清单大小不一致"));
            }
            let bytes = self
                .raw_file(db, &chunk.path, CHUNK_BYTES)
                .await?
                .ok_or_else(|| {
                    AppError::new(ErrorCode::Network, "GitHub 同步分块缺失；本机内容未覆盖")
                })?;
            if bytes.len() != chunk.size || hex::encode(Sha256::digest(&bytes)) != chunk.sha256 {
                return Err(AppError::validation(
                    "GitHub 同步分块校验失败；本机内容未覆盖",
                ));
            }
            output.extend_from_slice(&bytes);
        }
        if output.len() != manifest.size || hex::encode(Sha256::digest(&output)) != manifest.sha256
        {
            return Err(AppError::validation(
                "GitHub 大文件整体校验失败；本机内容未覆盖",
            ));
        }
        Ok(output)
    }
}

impl SyncRemote for GitHub {
    async fn verify_repository(&self, db: &Db) -> AppResult<()> {
        let response = self
            .send(
                db,
                reqwest::Method::GET,
                self.repository_url()?,
                None,
                "application/vnd.github+json",
            )
            .await?;
        if response.status().as_u16() == 404 {
            return Err(AppError::new(
                ErrorCode::Unauthorized,
                "无法访问此 GitHub 仓库；请核对用户名、仓库名和 Token 授权范围",
            ));
        }
        if !response.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Network,
                format!("GitHub 仓库验证失败（HTTP {}）", response.status().as_u16()),
            ));
        }
        let repository = response
            .json::<RepositoryInfo>()
            .await
            .map_err(|_| AppError::validation("GitHub 仓库信息格式无效"))?;
        if !repository.private {
            return Err(AppError::validation(
                "Lumen 只允许连接 GitHub 私有仓库；请先把仓库设为 Private",
            ));
        }
        Ok(())
    }

    async fn read(&self, db: &Db, path: &str, max: usize) -> AppResult<Option<Vec<u8>>> {
        if let Some(bytes) = self.raw_file(db, path, max).await? {
            return Ok(Some(bytes));
        }
        if let Some(manifest) = self.chunks_manifest(db, path).await? {
            return self.read_chunked(db, manifest, max).await.map(Some);
        }
        Ok(None)
    }

    async fn mkdir(&self, _db: &Db, path: &str) -> AppResult<()> {
        if !path.is_empty() {
            self.physical_path(path.trim_end_matches('/'))?;
        }
        Ok(())
    }

    async fn stage(&self, db: &Db, path: &str, value: Vec<u8>, overwrite: bool) -> AppResult<u16> {
        if !overwrite
            && (self
                .metadata_sha(db, &self.physical_path(path)?)
                .await?
                .is_some()
                || self
                    .metadata_sha(
                        db,
                        &self.physical_path(&format!("{path}.lumen-chunks.json"))?,
                    )
                    .await?
                    .is_some())
        {
            return Ok(409);
        }
        if value.len() <= CHUNK_BYTES {
            let status = self.write_small(db, path, &value, overwrite).await?;
            if status < 400 {
                // A prior chunked representation is removed only after the new value is committed.
                self.delete_physical(
                    db,
                    &self.physical_path(&format!("{path}.lumen-chunks.json"))?,
                )
                .await?;
            }
            Ok(status)
        } else {
            self.write_chunked(db, path, &value, overwrite).await
        }
    }

    async fn devices(&self, db: &Db) -> AppResult<Vec<String>> {
        let path = self.physical_path("devices")?;
        let response = self
            .send(
                db,
                reqwest::Method::GET,
                self.directory_url(&path)?,
                None,
                "application/vnd.github+json",
            )
            .await?;
        if response.status().as_u16() == 404 {
            return Ok(Vec::new());
        }
        if !response.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Network,
                format!(
                    "GitHub 设备列表读取失败（HTTP {}）",
                    response.status().as_u16()
                ),
            ));
        }
        let entries: Vec<DirectoryEntry> = response
            .json()
            .await
            .map_err(|_| AppError::validation("GitHub 设备列表格式无效"))?;
        let mut ids = std::collections::BTreeSet::new();
        for entry in entries.into_iter().filter(|entry| entry.kind == "dir") {
            if super::id_ok(&entry.name).is_ok() {
                ids.insert(entry.name);
            }
        }
        Ok(ids.into_iter().collect())
    }

    async fn delete(&self, db: &Db, path: &str) -> AppResult<()> {
        self.delete_physical(
            db,
            &self.physical_path(&format!("{path}.lumen-chunks.json"))?,
        )
        .await?;
        self.delete_physical(db, &self.physical_path(path)?).await
    }
}
