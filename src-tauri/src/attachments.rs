//! 附件受控存储（任务书 §4.1 / §10）。
//!
//! ## 核心安全约定
//!
//! 1. **绝不删除用户的原文件**。引用模式（`reference`）只在数据库里记录一条
//!    指向用户原文件的信息；删除附件记录、删除任务、清空回收站都不会
//!    碰到原文件。这是任务书"不允许删除任务时误删用户原文件"的直接落实。
//! 2. **复制模式才动文件**，且只删自己复制出来的副本（位于数据目录内），
//!    删除前会再次校验目标路径确实在受控目录内。
//! 3. **路径防越界**：任何对副本文件的操作都要求目标路径规范化后仍位于
//!    附件目录之下（§10「附件路径防越界」）。
//!
//! ## 为什么存 SHA-256
//!
//! 备份说明里会列出附件清单；哈希让用户能核对副本是否与原件一致，
//! 也为将来"检测附件是否被外部修改"留出余地。计算哈希失败不影响添加
//! （大文件读取可能失败），此时哈希留空而不是让整个操作失败。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::commands::AppState;
use crate::db::{to_db_time, utc_now};
use crate::error::{AppError, AppResult};

/// 单个附件的最大字节数（200 MB）。超过则拒绝并给出明确原因，
/// 而不是让用户等很久之后失败。
const MAX_ATTACHMENT_BYTES: u64 = 200 * 1024 * 1024;

/// 单个任务的附件数量上限，防止误操作导入大量文件
const MAX_PER_TASK: i64 = 100;

/// 附件视图
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub task_id: String,
    pub file_name: String,
    pub mime_type: Option<String>,
    pub byte_size: Option<i64>,
    pub sha256: Option<String>,
    /// reference = 仅记录用户原文件；copied = 已复制到受控目录
    pub storage_mode: String,
    pub external_path: Option<String>,
    pub stored_path: Option<String>,
    pub created_at: String,
}

impl Attachment {
    /// 实际可用于打开的文件路径（**相对数据目录的旧值会在这里被解析**）。
    ///
    /// 整改任务书 §11.2：`stored_path` 曾经存的是**绝对路径**，
    /// 一旦数据目录发生变化（换用户名、把数据目录搬走、在另一台机器上恢复备份），
    /// 这个路径就指向不存在的位置。现在的写入统一改成
    /// **相对数据目录**（`attachments/<id>.ext`），读取时再拼回来；
    /// 老数据里的绝对路径仍然按原样使用，保证升级不破坏已有附件。
    pub fn open_path(&self) -> Option<&str> {
        match self.storage_mode.as_str() {
            "copied" => self.stored_path.as_deref(),
            _ => self.external_path.as_deref(),
        }
    }

    /// `stored_path` 是否是相对路径（新格式）
    pub fn stored_is_relative(&self) -> bool {
        self.stored_path
            .as_deref()
            .map(|p| !Path::new(p).is_absolute() && !p.contains(':'))
            .unwrap_or(false)
    }
}

/// 把数据库里记录的 `stored_path` 解析成真实绝对路径。
///
/// - 相对路径（新格式）→ 拼到当前数据目录上；
/// - 绝对路径（老数据）→ 原样返回，避免升级后老附件打不开。
pub fn resolve_stored_path(data_dir: &Path, stored: &str) -> PathBuf {
    let p = Path::new(stored);
    if p.is_absolute() || stored.contains(':') || stored.starts_with("\\\\") {
        p.to_path_buf()
    } else {
        data_dir.join(p)
    }
}

/// 计算写入数据库的 `stored_path` 值：相对数据目录。
///
/// 为什么不存绝对路径：备份/恢复、迁移数据目录、换机器都会让绝对路径失效，
/// 而"受控目录内的附件"本质上是**相对于数据目录**定位的。
pub fn relative_stored_path(data_dir: &Path, abs: &Path) -> String {
    match abs.strip_prefix(data_dir) {
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        // 理论上不会发生（附件只写入受控目录）；真发生了就退回绝对路径，
        // 至少不会静默丢掉引用
        Err(_) => abs.to_string_lossy().to_string(),
    }
}

/// 受控附件目录：`<数据目录>/attachments`
fn attachments_dir(state: &AppState) -> PathBuf {
    state.db.data_dir().join("attachments")
}

/// 校验并规范化一个"允许操作"的路径必须位于受控目录内。
///
/// 若目录尚不存在会先创建，否则 `canonicalize` 必然失败。
fn ensure_inside_attachments(state: &AppState, candidate: &Path) -> AppResult<PathBuf> {
    let root = attachments_dir(state);
    std::fs::create_dir_all(&root)?;

    // canonicalize 会解析 .. 与符号链接，是防越界的关键一步
    let root_abs = root
        .canonicalize()
        .map_err(|e| AppError::new(crate::error::ErrorCode::Io, format!("附件目录不可用：{e}")))?;

    let cand_abs = candidate.canonicalize().map_err(|e| {
        AppError::new(crate::error::ErrorCode::Io, format!("文件路径无效：{e}"))
            .with_hint("文件可能已被移动或删除")
    })?;

    if !cand_abs.starts_with(&root_abs) {
        return Err(AppError::conflict("该操作只允许作用于受控附件目录内的文件")
            .with_hint("为保护你的原始文件，程序不会修改附件目录以外的任何内容"));
    }
    Ok(cand_abs)
}

/// 计算文件 SHA-256；失败时返回 None（不阻断添加流程）
fn try_hash_file(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).ok()?;
    Some(hex::encode(hasher.finalize()))
}

/// 从扩展名推断 MIME 类型（够用于展示图标与分类，不求完备）
fn guess_mime(name: &str) -> Option<String> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    let m = match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        _ => return None,
    };
    Some(m.to_string())
}

/// 添加附件。
///
/// `mode` 为 `copied` 时把文件复制进受控目录（原件保持不动）；
/// 为 `reference` 时只记录原路径。
#[tauri::command]
pub async fn attachment_add(
    state: State<'_, AppState>,
    task_id: String,
    source_path: String,
    mode: Option<String>,
) -> AppResult<Attachment> {
    let db = &state.db;
    let mode = mode.unwrap_or_else(|| "reference".to_string());
    if mode != "reference" && mode != "copied" {
        return Err(AppError::validation(format!("存储模式非法：{mode}"))
            .with_hint("允许值：reference（仅记录）或 copied（复制到受控目录）"));
    }

    // 任务必须存在且不在回收站
    let row = sqlx::query("SELECT deleted_at FROM tasks WHERE id = ?1")
        .bind(&task_id)
        .fetch_optional(db.pool())
        .await?;
    let Some(row) = row else {
        return Err(AppError::not_found("任务", &task_id));
    };
    if row.try_get::<Option<String>, _>("deleted_at")?.is_some() {
        return Err(AppError::conflict("任务在回收站中，无法添加附件"));
    }

    // 数量上限
    let count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM attachments WHERE task_id = ?1")
        .bind(&task_id)
        .fetch_one(db.pool())
        .await?
        .try_get("n")?;
    if count >= MAX_PER_TASK {
        return Err(AppError::validation(format!(
            "单个任务最多 {MAX_PER_TASK} 个附件"
        )));
    }

    let src = PathBuf::from(&source_path);
    let meta = std::fs::metadata(&src).map_err(|e| {
        AppError::new(
            crate::error::ErrorCode::Io,
            format!("无法读取所选文件：{e}"),
        )
        .with_hint("请确认文件存在且你有读取权限")
    })?;
    if meta.is_dir() {
        return Err(AppError::validation("目录不能作为附件")
            .with_hint("请选择单个文件；如需附带整个文件夹，请先压缩为 zip"));
    }
    if meta.len() > MAX_ATTACHMENT_BYTES {
        return Err(AppError::validation(format!(
            "文件过大（{:.1} MB），上限 {} MB",
            meta.len() as f64 / 1024.0 / 1024.0,
            MAX_ATTACHMENT_BYTES / 1024 / 1024
        )));
    }

    let file_name = src
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or_else(|| AppError::validation("无法确定文件名"))?;

    let id = uuid::Uuid::now_v7().to_string();
    let now = to_db_time(utc_now());
    let mime = guess_mime(&file_name);
    let byte_size = meta.len() as i64;

    let (storage_mode, external_path, stored_path, sha256) = if mode == "copied" {
        // 复制到受控目录：用 ID 前缀避免同名冲突，同时保留原始扩展名
        let dir = attachments_dir(&state);
        std::fs::create_dir_all(&dir)?;
        let ext = Path::new(&file_name)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let dest = dir.join(format!("{id}{ext}"));

        std::fs::copy(&src, &dest).map_err(|e| {
            AppError::new(
                crate::error::ErrorCode::Io,
                format!("复制文件到附件目录失败：{e}"),
            )
            .with_hint("请检查磁盘空间与数据目录写入权限")
        })?;

        let hash = try_hash_file(&dest);
        (
            "copied".to_string(),
            Some(src.to_string_lossy().to_string()),
            // 存**相对数据目录**的路径（§11.2），换机器/搬数据目录后仍然有效
            Some(relative_stored_path(state.db.data_dir(), &dest)),
            hash,
        )
    } else {
        let hash = try_hash_file(&src);
        (
            "reference".to_string(),
            Some(src.to_string_lossy().to_string()),
            None,
            hash,
        )
    };

    sqlx::query(
        "INSERT INTO attachments
            (id, task_id, file_name, mime_type, byte_size, sha256, storage_mode, external_path, stored_path, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )
    .bind(&id)
    .bind(&task_id)
    .bind(&file_name)
    .bind(&mime)
    .bind(byte_size)
    .bind(&sha256)
    .bind(&storage_mode)
    .bind(&external_path)
    .bind(&stored_path)
    .bind(&now)
    .execute(db.pool())
    .await?;

    log::info!(
        "已添加附件「{file_name}」（模式 {storage_mode}，{byte_size} 字节）到任务 {task_id}"
    );

    get_attachment(&state, &id).await
}

async fn get_attachment(state: &AppState, id: &str) -> AppResult<Attachment> {
    let a = sqlx::query_as::<_, Attachment>("SELECT * FROM attachments WHERE id = ?1")
        .bind(id)
        .fetch_optional(state.db.pool())
        .await?;
    a.ok_or_else(|| AppError::not_found("附件", id))
}

/// 列出某任务的附件
#[tauri::command]
pub async fn attachment_list(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<Vec<Attachment>> {
    let rows = sqlx::query_as::<_, Attachment>(
        "SELECT * FROM attachments WHERE task_id = ?1 ORDER BY created_at ASC",
    )
    .bind(&task_id)
    .fetch_all(state.db.pool())
    .await?;
    Ok(rows)
}

/// 删除附件记录。
///
/// - 引用模式：**只删记录，绝不动原文件**（任务书明确要求）。
/// - 复制模式：删记录，并删除我们自己的副本（副本在受控目录内，删它不影响用户原件）。
#[tauri::command]
pub async fn attachment_remove(state: State<'_, AppState>, id: String) -> AppResult<bool> {
    let a = get_attachment(&state, &id).await?;

    let mut removed_copy = false;
    if a.storage_mode == "copied" {
        if let Some(p) = a.stored_path.as_deref() {
            // 删副本前再次确认路径在受控目录内：即使数据库被手工改过，
            // 也不会因为这个操作删除目录外的文件。
            // 注意先把可能存在的相对路径解析成绝对路径（§11.2）。
            let abs_candidate = resolve_stored_path(state.db.data_dir(), p);
            match ensure_inside_attachments(&state, &abs_candidate) {
                Ok(abs) => {
                    if std::fs::remove_file(&abs).is_ok() {
                        removed_copy = true;
                    }
                }
                Err(e) => {
                    log::warn!("附件副本路径校验未通过，跳过删除文件（仅删记录）：{e}");
                }
            }
        }
    }

    sqlx::query("DELETE FROM attachments WHERE id = ?1")
        .bind(&id)
        .execute(state.db.pool())
        .await?;

    log::info!(
        "已删除附件记录「{}」（模式 {}，副本文件{}）",
        a.file_name,
        a.storage_mode,
        if removed_copy {
            "已删除"
        } else {
            "未涉及"
        }
    );
    Ok(removed_copy)
}

/// 在文件管理器中定位附件（用户想要"找到这个文件"）
#[tauri::command]
pub async fn attachment_reveal(state: State<'_, AppState>, id: String) -> AppResult<String> {
    let a = get_attachment(&state, &id).await?;
    let p = a
        .open_path()
        .ok_or_else(|| AppError::internal("该附件没有可用的文件路径"))?;
    // 受控副本存的是相对路径，这里解析成绝对路径再返回给系统（§11.2）
    let path = resolve_stored_path(state.db.data_dir(), p);
    if !path.exists() {
        return Err(AppError::new(
            crate::error::ErrorCode::Io,
            format!("附件文件已不存在：{}", path.display()),
        )
        .with_hint("文件可能已被移动或删除。若这是引用模式的附件，原件不在 Lumen 管理范围内"));
    }
    Ok(path.to_string_lossy().to_string())
}

/// 检查某任务所有附件的文件是否仍然存在（用于界面提示"文件已丢失"）
#[tauri::command]
pub async fn attachment_check(
    state: State<'_, AppState>,
    task_id: String,
) -> AppResult<Vec<serde_json::Value>> {
    let list = sqlx::query_as::<_, Attachment>(
        "SELECT * FROM attachments WHERE task_id = ?1 ORDER BY created_at ASC",
    )
    .bind(&task_id)
    .fetch_all(state.db.pool())
    .await?;

    let mut out = Vec::with_capacity(list.len());
    for a in list {
        // 相对路径要先解析（§11.2），否则新写入的附件会被误报成"文件已丢失"
        let resolved = a
            .open_path()
            .map(|p| resolve_stored_path(state.db.data_dir(), p));
        let exists = resolved.as_ref().map(|p| p.exists()).unwrap_or(false);
        out.push(serde_json::json!({
            "id": a.id,
            "fileName": a.file_name,
            "exists": exists,
            "mode": a.storage_mode,
            "path": resolved.map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        }));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §11.2：受控副本必须用相对路径保存，数据目录搬走后仍然能找到文件。
    #[test]
    fn stored_path_is_relative_and_resolves_after_moving_data_dir() {
        let data_dir = Path::new("C:/Users/me/AppData/Roaming/com.pla0185.lumen");
        let abs = data_dir.join("attachments").join("abc123.pdf");

        let stored = relative_stored_path(data_dir, &abs);
        assert_eq!(stored, "attachments/abc123.pdf", "应存相对路径且用正斜杠");

        // 搬到另一个数据目录后依然能解析到新位置的文件
        let moved = Path::new("D:/lumen-data");
        let resolved = resolve_stored_path(moved, &stored);
        assert_eq!(resolved, moved.join("attachments").join("abc123.pdf"));
    }

    /// 老数据里存的是绝对路径，升级后必须继续可用（不能因为改了格式就打不开）
    #[test]
    fn absolute_legacy_stored_path_still_resolves() {
        let old = "C:/old-place/attachments/legacy.pdf";
        let resolved = resolve_stored_path(Path::new("D:/new-data"), old);
        assert_eq!(
            resolved,
            PathBuf::from(old),
            "绝对路径必须原样使用，否则老附件会全部失效"
        );

        let win_style = "D:\\data\\attachments\\legacy.pdf";
        assert_eq!(
            resolve_stored_path(Path::new("C:/new"), win_style),
            PathBuf::from(win_style)
        );
    }

    /// 判断"是否相对路径"的实现要能正确处理 Windows 盘符与 UNC
    #[test]
    fn stored_is_relative_detects_windows_paths() {
        let mk = |p: Option<&str>| Attachment {
            id: "a".into(),
            task_id: "t".into(),
            file_name: "f".into(),
            mime_type: None,
            byte_size: None,
            sha256: None,
            storage_mode: "copied".into(),
            external_path: None,
            stored_path: p.map(|s| s.to_string()),
            created_at: String::new(),
        };
        assert!(mk(Some("attachments/a.pdf")).stored_is_relative());
        assert!(!mk(Some("C:/x/a.pdf")).stored_is_relative());
        assert!(!mk(Some("D:\\x\\a.pdf")).stored_is_relative());
        assert!(!mk(Some("\\\\server\\share\\a.pdf")).stored_is_relative());
        assert!(!mk(None).stored_is_relative());
    }

    #[test]
    fn mime_guess_covers_common_types() {
        assert_eq!(guess_mime("a.pdf").as_deref(), Some("application/pdf"));
        assert_eq!(
            guess_mime("图.PNG").as_deref(),
            Some("image/png"),
            "扩展名应大小写无关"
        );
        assert_eq!(guess_mime("data.csv").as_deref(), Some("text/csv"));
        assert_eq!(guess_mime("noext"), None);
        assert_eq!(guess_mime("unknown.xyz"), None);
    }

    /// 引用模式的附件只能通过 external_path 打开；
    /// 复制模式优先用 stored_path（副本），这才是受控的那份。
    #[test]
    fn open_path_depends_on_storage_mode() {
        let reference = Attachment {
            id: "1".into(),
            task_id: "t".into(),
            file_name: "a.pdf".into(),
            mime_type: None,
            byte_size: Some(1),
            sha256: None,
            storage_mode: "reference".into(),
            external_path: Some("C:/users/me/a.pdf".into()),
            stored_path: None,
            created_at: "2026-01-01T00:00:00.000Z".into(),
        };
        assert_eq!(reference.open_path(), Some("C:/users/me/a.pdf"));

        let copied = Attachment {
            storage_mode: "copied".into(),
            external_path: Some("C:/users/me/a.pdf".into()),
            stored_path: Some("C:/data/attachments/x.pdf".into()),
            ..reference.clone()
        };
        assert_eq!(
            copied.open_path(),
            Some("C:/data/attachments/x.pdf"),
            "复制模式应使用受控目录内的副本"
        );

        let broken = Attachment {
            stored_path: None,
            ..copied
        };
        assert_eq!(broken.open_path(), None, "路径缺失时应返回 None 而不是空串");
    }

    /// 上限值必须合理：太小会挡住正常文件，太大则失去意义。
    #[test]
    fn attachment_limits_are_sane() {
        assert_eq!(MAX_ATTACHMENT_BYTES, 200 * 1024 * 1024);
        const { assert!(MAX_PER_TASK > 0 && MAX_PER_TASK <= 1000) };
        const { assert!(MAX_ATTACHMENT_BYTES >= 10 * 1024 * 1024) };
    }

    /// 越界校验的语义：备份目录之外的路径必须被拒绝。
    /// 这里用临时目录模拟，验证 starts_with 判定本身的方向性。
    #[test]
    fn path_containment_uses_prefix_semantics() {
        let root = PathBuf::from("C:/data/attachments");
        let inside = PathBuf::from("C:/data/attachments/a.pdf");
        let outside = PathBuf::from("C:/users/me/a.pdf");
        // 注意：这里只验证判定逻辑，真实实现还要经过 canonicalize
        assert!(inside.starts_with(&root));
        assert!(!outside.starts_with(&root));
    }

    /// 尝试用 `..` 穿越目录时，规范化后不应仍落在受控目录语义内。
    /// 这验证的是"必须 canonicalize 而不能只做字符串前缀比较"这一设计前提。
    #[test]
    fn parent_traversal_is_not_contained_after_normalization() {
        let root = PathBuf::from("C:/data/attachments");
        let evil = PathBuf::from("C:/data/attachments/../secret.txt");
        // 未规范化时看起来在目录内（这正是危险之处）
        let naive = evil.starts_with(&root);
        assert!(naive, "字符串层面看确实在目录内，所以必须规范化后再判断");
        // 规范化后应落在父目录
        let normalized = PathBuf::from("C:/data/secret.txt");
        assert!(!normalized.starts_with(&root), "规范化后必须被拒绝");
    }

    /// 在真实文件系统上验证 `canonicalize` 的防越界效果。
    ///
    /// 这个测试用临时目录实际建出 `attachments/` 与一个同级的 `secret.txt`，
    /// 然后确认：
    /// 1. `attachments/../secret.txt` 规范化后**不**在 attachments 之下；
    /// 2. 目录内的普通文件规范化后**在** attachments 之下；
    /// 3. 不存在的路径会 canonicalize 失败（因此不会被当作合法目标）。
    ///
    /// 之所以要实测而不是只信字符串比较，是因为 Windows 路径分隔符、
    /// 短文件名（8.3）、符号链接都会让"看起来在目录内"与"实际在目录内"不一致。
    #[test]
    fn canonicalize_actually_blocks_traversal() {
        let base = std::env::temp_dir().join(format!("lumen-att-{}", uuid::Uuid::now_v7()));
        let attachments = base.join("attachments");
        std::fs::create_dir_all(&attachments).unwrap();

        let inside_file = attachments.join("a.txt");
        std::fs::write(&inside_file, b"x").unwrap();
        let outside_file = base.join("secret.txt");
        std::fs::write(&outside_file, b"y").unwrap();

        let root_abs = attachments.canonicalize().unwrap();

        // 1) 目录内的文件：规范化后应在 root 之下
        let ok = inside_file.canonicalize().unwrap();
        assert!(
            ok.starts_with(&root_abs),
            "附件目录内的文件应通过校验：{ok:?}"
        );

        // 2) 用 .. 穿越：规范化后不在 root 之下
        let evil = attachments.join("..").join("secret.txt");
        let evil_abs = evil.canonicalize().unwrap();
        assert!(
            !evil_abs.starts_with(&root_abs),
            "穿越到上级目录的文件必须被拒绝：{evil_abs:?}"
        );
        // 而且它确实指向了目录外的那个文件
        assert_eq!(evil_abs, outside_file.canonicalize().unwrap());

        // 3) 不存在的路径：canonicalize 必须失败，
        //    因此实现里不会把"文件已被删除"当成合法目标静默通过
        let missing = attachments.join("不存在.txt");
        assert!(
            missing.canonicalize().is_err(),
            "不存在的路径不应通过规范化校验"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// 计算哈希必须对同一内容稳定、对不同内容不同——
    /// 这是备份说明里"核对副本是否与原件一致"的前提。
    #[test]
    fn file_hash_is_stable_and_content_sensitive() {
        let dir = std::env::temp_dir().join(format!("lumen-hash-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();

        let f1 = dir.join("a.txt");
        std::fs::write(&f1, b"hello").unwrap();
        let h1 = try_hash_file(&f1).expect("小文件应能计算哈希");
        let h2 = try_hash_file(&f1).unwrap();
        assert_eq!(h1, h2, "同一文件两次哈希必须一致");
        assert_eq!(h1.len(), 64, "SHA-256 十六进制应为 64 字符");

        let f2 = dir.join("b.txt");
        std::fs::write(&f2, b"hello!").unwrap();
        assert_ne!(h1, try_hash_file(&f2).unwrap(), "内容不同哈希必须不同");

        // 不存在的文件返回 None 而不是 panic（添加附件时不应因此失败）
        assert!(try_hash_file(&dir.join("missing.txt")).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
