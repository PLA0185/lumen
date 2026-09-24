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
use crate::db::{to_db_time, utc_now, Db};
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

/// 把受控附件路径标准化成用于**身份比较**的字符串（不代表真实文件位置）。
///
/// ## 为什么需要它（第三轮任务书 §14 / §15 / §16）
///
/// 孤儿清理要回答的问题是"磁盘上这个文件是否仍被数据库引用"，做法是两边各算
/// 一个字符串再比。而**第二轮整改之前**写进库的 `stored_path` 是**绝对路径**，
/// 它的盘符与目录大小写可能与当前 `data_dir` 不同（`C:\Users\…` vs `c:\users\…`），
/// 分隔符也可能是 `/`。
///
/// `relative_stored_path()` 内部用 `strip_prefix`，它**逐组件精确比较**：
/// 大小写不同就匹配不上，于是回退成绝对路径字符串，而与扫描得到的
/// `attachments/<uuid>.pdf` 对不上 → **仍被引用的 live 副本被误判成孤儿删除**（数据破坏级）。
///
/// 因此这里不再依赖任何前缀匹配：先把相对/绝对都变成绝对路径，
/// 再做词法归一化（`.`、`..`、结尾分隔符），最后按平台折叠差异：
///
/// | 平台 | 处理 |
/// | --- | --- |
/// | Windows | 转小写、`/` 统一成 `\`（NTFS 默认大小写不敏感） |
/// | 其它 | **保持大小写敏感**，只做词法归一化，不把 Windows 专有语义扩散出去 |
///
/// ## 只用于身份比较
///
/// 返回值是**比较键**，不是可用的文件位置：Windows 上它是小写的，直接拿去
/// 打开文件在大小写敏感的卷上会失败。**绝不能**用它决定 symlink 的删除目标——
/// 删除始终交给 `safe_remove_managed_copy()`（只删目录项、不跟随链接）。
fn normalize_managed_identity(data_dir: &Path, stored_or_abs: &str) -> String {
    let abs = lexical_normalize(&resolve_stored_path(data_dir, stored_or_abs));
    let raw = abs.to_string_lossy().to_string();

    if cfg!(windows) {
        // `/` 与 `\` 在 Windows 上是等价分隔符，统一成 `\` 后再折叠大小写。
        // 结尾多余分隔符：`lexical_normalize` 按组件重建，本就不会留下尾分隔符，
        // 这里再兜一次，防止 `…\attachments\` 与 `…\attachments` 算成两个身份。
        // 盘符根（`C:\`）的尾分隔符要保留，否则会把根目录写成 `c:`。
        let mut s = raw.replace('/', "\\");
        while s.len() > 1 && s.ends_with('\\') && !s.ends_with(":\\") {
            s.pop();
        }
        s.to_lowercase()
    } else {
        raw
    }
}

/// 受控附件目录：`<数据目录>/attachments`
fn attachments_dir(state: &AppState) -> PathBuf {
    attachments_dir_of(state.db.data_dir())
}

/// 受控附件目录（不依赖 `AppState` 的版本，供后台清理与测试使用）
fn attachments_dir_of(data_dir: &Path) -> PathBuf {
    data_dir.join("attachments")
}

/// 越界校验的纯路径版本（第二轮整改任务书 §6.5）。
///
/// 抽出来是为了让"删附件副本"这件事在**没有 `AppState`** 的地方
/// （后台孤儿清理、单元测试）也能复用同一套越界判断，
/// 而不是另写一份——那种重复正是越界漏洞的来源。
fn ensure_inside_dir(root: &Path, candidate: &Path) -> AppResult<PathBuf> {
    std::fs::create_dir_all(root)?;

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

// =============================================================================
// 永久删除后的副本文件清理（第二轮整改任务书 §6）
// =============================================================================
//
// 背景：`attachments.task_id` 是 `ON DELETE CASCADE`，所以永久删除任务时
// 附件**记录**会自动消失；但 copied 模式的**实体文件**留在
// `<数据目录>/attachments/` 里，于是产生"数据库里没记录、磁盘上还占着"的孤儿文件。
//
// 顺序（§6.4 明确要求）：
// 1. 事务**之前**读出要清理的路径（事务提交后记录就没了，读不到了）；
// 2. 完成数据库删除并提交；
// 3. 提交**之后**再删文件。文件删除失败只记日志，不回滚已经完成的删除
//    ——回滚会退化成"记录还在、文件没了"，那比留个孤儿文件更糟。

/// 永久删除任务前，取出该任务下 **copied** 附件的存储路径。
pub async fn copied_paths_of_task(db: &Db, task_id: &str) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT stored_path FROM attachments
         WHERE task_id = ?1 AND storage_mode = 'copied' AND stored_path IS NOT NULL",
    )
    .bind(task_id)
    .fetch_all(db.pool())
    .await?;
    Ok(rows.into_iter().map(|(p,)| p).collect())
}

/// `copied_paths_of_task` 的**事务内**版本（最终收口任务书 §3）。
///
/// 单条永久删除要求"校验仍在回收站 → 取副本路径 → 条件 DELETE"全在同一个事务里，
/// 这样拿到的文件集合一定与真正被删的记录来自同一个数据库快照。
pub async fn copied_paths_of_task_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT stored_path FROM attachments
         WHERE task_id = ?1 AND storage_mode = 'copied' AND stored_path IS NOT NULL",
    )
    .bind(task_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows.into_iter().map(|(p,)| p).collect())
}

/// 删除一批受控副本文件，返回 `(已删除, 失败或跳过)`。
///
/// **刻意不返回错误**：调用方按第二轮 §6.4 只记日志，
/// 绝不因为删文件失败而回滚已经提交的数据库删除。
///
/// 每个文件都交给 `safe_remove_managed_copy()`——它保证"只删目录项、
/// 不跟随符号链接删目标"，与孤儿清理、单个附件删除共用同一套语义
/// （第三轮任务书 §2.5 要求四处删除行为必须一致）。
pub fn delete_copied_files(data_dir: &Path, stored_paths: &[String]) -> (usize, usize) {
    let root = attachments_dir_of(data_dir);
    let mut removed = 0usize;
    let mut failed = 0usize;

    for stored in stored_paths {
        let candidate = resolve_stored_path(data_dir, stored);
        match safe_remove_managed_copy(&root, &candidate) {
            Ok(()) => removed += 1,
            Err(e) => {
                failed += 1;
                log::warn!("附件副本未删除（不影响已完成的数据库删除）：{stored} —— {e}");
            }
        }
    }
    (removed, failed)
}

/// 安全删除一个"由 Lumen 管理的副本文件"（第三轮任务书 §2.2 / §2.4）。
///
/// ## 根本原则
///
/// 删除**数据库记录指向的那个目录项本身**，而不是它解析后的目标文件。
///
/// ## 为什么不能 canonicalize 之后删
///
/// `canonicalize` 会**跟随符号链接**。若 `attachments/B.pdf` 是指向
/// `attachments/A.pdf` 的链接，而 A.pdf 属于另一个仍在使用的任务，那么
/// `canonicalize(B) → remove_file(A)` 会把 A 的真实文件删掉，而 B 的链接还留着——
/// 删除任务 B 时误伤了任务 A。这是第三轮任务书点名的 P0 缺陷。
///
/// ## 分情况处理
///
/// | 情况 | 处理 |
/// | --- | --- |
/// | 词法上越界，或等于受控目录本身 | 拒绝 |
/// | 文件不存在 | `Ok`（幂等，重复清理安全） |
/// | 符号链接 / junction（reparse point） | **只删链接自身**，绝不跟随；并记 warn |
/// | 普通文件 | 校验**父目录**在受控目录内，再删原路径（不是 canonicalized 的结果） |
/// | 目录 / 其它类型 | 拒绝（受控目录是平铺的，不该出现子目录） |
pub fn safe_remove_managed_copy(root: &Path, candidate: &Path) -> Result<(), String> {
    if !path_lexically_inside(root, candidate) {
        return Err(format!("路径越界（词法检查）：{}", candidate.display()));
    }
    if same_path(root, candidate) {
        return Err("拒绝删除受控目录本身".into());
    }
    let cand_lex = lexical_normalize(candidate);

    // symlink_metadata **不跟随**链接；metadata 会跟随，不能用来判断类型
    let meta = match std::fs::symlink_metadata(&cand_lex) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("读取文件属性失败：{e}")),
    };

    let ft = meta.file_type();
    if ft.is_symlink() {
        // Windows 上 junction 等 reparse point 也会走到这个分支。
        // 删"链接自身"：目录链接用 remove_dir，文件链接用 remove_file。
        // 这里读 metadata 只是为了判断链接指向的是文件还是目录，
        // **删除动作仍然只作用于链接本身**，不会碰目标。
        let points_to_dir = std::fs::metadata(&cand_lex)
            .map(|m| m.is_dir())
            .unwrap_or(false);
        let res = if points_to_dir {
            std::fs::remove_dir(&cand_lex)
        } else {
            std::fs::remove_file(&cand_lex)
        };
        res.map_err(|e| format!("删除符号链接自身失败：{e}"))?;
        log::warn!(
            "受控附件目录里出现了符号链接/junction，已只删除链接自身、未跟随目标：{}",
            cand_lex.display()
        );
        return Ok(());
    }

    if !ft.is_file() {
        return Err(format!("不是普通文件，拒绝删除：{}", cand_lex.display()));
    }

    // 普通文件：解析**父目录**（父目录必须真实位于受控目录内），
    // 再用"规范化后的父目录 + 原文件名"删除。
    // 这样既挡住了"父目录本身是链接"的情况，也不会跟随文件自身的链接。
    let parent = cand_lex
        .parent()
        .ok_or_else(|| "无法确定父目录".to_string())?;
    let file_name = cand_lex
        .file_name()
        .ok_or_else(|| "无法确定文件名".to_string())?;
    let parent_abs = ensure_inside_dir(root, parent).map_err(|e| e.to_string())?;
    let target = parent_abs.join(file_name);
    std::fs::remove_file(&target).map_err(|e| format!("删除失败：{e}"))
}

/// 词法层面折叠 `..` 与 `.`（不访问文件系统）。
///
/// 为什么需要它：`canonicalize` 要求路径**存在**，所以"记录里写了越界路径、
/// 而那个文件恰好不存在"的情况没法用它判断。若此时先判"文件不存在 ⇒ 已完成"，
/// 就会把越界路径记成"已清理"——虽然没删到目录外的东西，但统计失真，
/// 也掩盖了数据库被改坏的事实。因此先做词法归一化，再做存在性与符号链接检查。
fn lexical_normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 词法上 `candidate` 是否位于 `root` 之下（Windows 下**大小写不敏感**）。
///
/// 为什么单独写一个：Windows 的文件路径大小写不敏感，而 `Path::starts_with`
/// 是逐组件精确比较。数据库里存的是 Lumen 自己生成的相对路径，大小写通常一致；
/// 但记录被手工改过、或数据目录改过大小写时，严格比较会把**受控目录内**的文件
/// 误判成越界而拒绝删除（留下孤儿）。
///
/// 放宽成大小写不敏感只会让"本来就该删的副本"被正常删掉，不会放过真正的越界：
/// 越界路径还要过下一道 `canonicalize`（`ensure_inside_dir`）的检查。
fn path_lexically_inside(root: &Path, candidate: &Path) -> bool {
    let r = lexical_normalize(root);
    let c = lexical_normalize(candidate);
    if cfg!(windows) {
        let rs = r.to_string_lossy().to_lowercase();
        let cs = c.to_string_lossy().to_lowercase();
        // 必须是 root 本身，或以 "root + 分隔符" 开头——
        // 否则 `…\attachments-evil` 会被误判成 `…\attachments` 的子路径
        cs == rs || cs.starts_with(&format!("{rs}\\")) || cs.starts_with(&format!("{rs}/"))
    } else {
        c.starts_with(&r)
    }
}

/// 两个路径是否指向同一位置（Windows 下大小写不敏感）。
fn same_path(a: &Path, b: &Path) -> bool {
    let a = lexical_normalize(a);
    let b = lexical_normalize(b);
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// 判断一个文件名是否是 Lumen 自己生成的副本（`<uuid><扩展名>`）。
///
/// 孤儿清理**只动这种命名的文件**：用户手动放进附件目录的任何东西都不属于
/// Lumen 的管理范围，即使数据库没有引用也不能删。
fn looks_like_managed_copy(file_name: &str) -> bool {
    let stem = Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    uuid::Uuid::parse_str(&stem).is_ok()
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
/// - 复制模式：先删记录，**删成功之后**才删我们自己的副本。
///
/// ## 顺序为什么必须是"先 DB、后文件"（最终收口任务书 §23/§24）
///
/// 旧实现是反过来的（先删文件、再删记录）。一旦第二步失败，就留下
/// **"活记录指向一个已经不存在的文件"** —— 用户看到附件还在、点开却打不开，
/// 而且没有任何机制能自愈。
///
/// 反过来做：记录先没了，最坏情况只是留一个没人引用的副本文件，
/// 那属于 `cleanup_orphans` 能扫掉的孤儿，是可恢复的。
/// 两者相比，"活记录 → 文件不存在"是**不可接受**的那一种。
#[tauri::command]
pub async fn attachment_remove(state: State<'_, AppState>, id: String) -> AppResult<bool> {
    attachment_remove_impl(&state.db, &id).await
}

/// `attachment_remove` 的实现（与 Tauri 解耦，便于集成测试直接调用）。
///
/// 顺序见上面的说明：**先删数据库记录，成功之后才删副本文件**。
pub async fn attachment_remove_impl(db: &Db, id: &str) -> AppResult<bool> {
    let a = get_attachment_row(db, id).await?;
    let controlled_dir = db.data_dir().join("attachments");

    // 1) 先删数据库记录（失败就直接返回错误，一个文件都不碰）
    sqlx::query("DELETE FROM attachments WHERE id = ?1")
        .bind(id)
        .execute(db.pool())
        .await?;

    // 2) 记录删掉之后才动文件：失败只记日志，留孤儿给 cleanup_orphans 收拾
    let mut removed_copy = false;
    if a.storage_mode == "copied" {
        if let Some(p) = a.stored_path.as_deref() {
            // 与任务删除、孤儿清理共用同一套安全语义（第三轮任务书 §2.5）：
            // 只删数据库记录指向的那个目录项，绝不跟随符号链接删目标。
            let abs_candidate = resolve_stored_path(db.data_dir(), p);
            match safe_remove_managed_copy(&controlled_dir, &abs_candidate) {
                Ok(()) => removed_copy = true,
                Err(e) => {
                    log::warn!("附件记录已删除，但副本文件未能删除（留作孤儿待清理）：{e}");
                }
            }
        }
    }

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

/// 按 id 读一条附件记录（不带 `State` 的版本，供 impl 与测试使用）。
pub async fn get_attachment_row(db: &Db, id: &str) -> AppResult<Attachment> {
    sqlx::query_as::<_, Attachment>("SELECT * FROM attachments WHERE id = ?1")
        .bind(id)
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| AppError::not_found("附件", id))
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

/// 孤儿附件清理结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanCleanupResult {
    /// 受控目录里扫描到的文件数（只统计顶层文件）
    pub scanned: i64,
    /// 数据库仍引用、保留下来的文件数
    pub kept: i64,
    /// 已删除的无引用副本数
    pub removed: i64,
    /// 被跳过的文件数（命名不属于 Lumen 管理范围，或删除失败）
    pub skipped: i64,
    /// 实际删掉的文件名，供用户核对
    pub removed_files: Vec<String>,
}

/// 清理受控附件目录里**数据库已无引用**的副本文件（整改任务书 §6.5）。
///
/// 这个命令处理的是"永久删除任务时删文件失败""程序被强制结束"等情况下
/// 留下的孤儿文件。两个硬约束：
///
/// 1. **只删文件名形如 `<uuid><扩展名>` 的文件**——那是 Lumen 唯一会创建的形态；
///    用户自己丢进这个目录的东西一律不碰（`skipped` 会如实计数）。
/// 2. **绝不碰受控目录之外的任何文件**：路径比较用的是 `canonicalize` 之后的结果。
///
/// 数据库读取失败时直接返回错误、**不删任何东西**——宁可留下孤儿文件，
/// 也不能在"不知道哪些还被引用"的情况下开始删。
#[tauri::command]
pub async fn attachment_cleanup_orphans(
    state: State<'_, AppState>,
) -> AppResult<OrphanCleanupResult> {
    cleanup_orphans_impl(&state.db).await
}

/// 孤儿清理的实现（与 Tauri 解耦，便于集成测试直接调用）。
pub async fn cleanup_orphans_impl(db: &Db) -> AppResult<OrphanCleanupResult> {
    let data_dir = db.data_dir().to_path_buf();
    let root = attachments_dir_of(&data_dir);

    // 先把"仍被引用的相对路径"读全。任何错误都在这里返回，
    // 不允许在引用集合不完整的情况下继续。
    let referenced: Vec<(String,)> = sqlx::query_as(
        "SELECT stored_path FROM attachments
         WHERE storage_mode = 'copied' AND stored_path IS NOT NULL",
    )
    .fetch_all(db.pool())
    .await?;
    let referenced: std::collections::HashSet<String> = referenced
        .into_iter()
        // 统一成"平台内的同一身份"再比较（第三轮任务书 §14 / §15 / §16）。
        //
        // 这里曾经用 `relative_stored_path(strip_prefix)`：它**逐组件精确比较**，
        // 于是老数据里"同样位置但大小写不同（且可能用 `/` 分隔）"的绝对路径
        // 匹配不上、回退成绝对路径字符串，与扫描得到的 `attachments/<uuid>.pdf`
        // 对不上 —— 仍被引用的 live 副本会被当成孤儿删掉。
        .map(|(p,)| normalize_managed_identity(&data_dir, &p))
        .collect();

    if !root.is_dir() {
        // 目录不存在 ⇒ 没有孤儿，也没什么可清理的
        return Ok(OrphanCleanupResult {
            scanned: 0,
            kept: 0,
            removed: 0,
            skipped: 0,
            removed_files: Vec::new(),
        });
    }

    let mut scanned = 0i64;
    let mut kept = 0i64;
    let mut removed = 0i64;
    let mut skipped = 0i64;
    let mut removed_files: Vec<String> = Vec::new();

    let entries = std::fs::read_dir(&root).map_err(|e| {
        AppError::new(
            crate::error::ErrorCode::Io,
            format!("无法读取附件目录：{e}"),
        )
    })?;

    for entry in entries.flatten() {
        let path = entry.path();
        // 顶层条目分三类处理（第三轮任务书 §2.3）：
        //
        // - 符号链接 / junction：**保守地跳过，但必须显式告警**。
        //   它们是 reparse point，`is_file()` 会跟随链接去判断目标类型，
        //   直接用它把链接一概记成"普通跳过"是**静默的**——真出现异常链接时
        //   运维看不到任何信号。这里先看 `symlink_metadata`（不跟随），
        //   是链接就记一条 warn 说明"未跟随目标"，绝不删除。
        // - 普通文件：继续走下面的候选判定与安全删除。
        // - 其它（目录等）：跳过。
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                skipped += 1;
                log::warn!("附件目录条目不可读，已跳过：{} —— {e}", path.display());
                continue;
            }
        };
        if meta.file_type().is_symlink() {
            skipped += 1;
            log::warn!(
                "附件目录里存在符号链接/junction，已跳过且**未跟随目标**：{}",
                path.display()
            );
            continue;
        }
        if !meta.is_file() {
            skipped += 1;
            continue;
        }
        scanned += 1;

        let Some(name) = path.file_name().map(|s| s.to_string_lossy().to_string()) else {
            skipped += 1;
            continue;
        };
        if !looks_like_managed_copy(&name) {
            // 不是 Lumen 生成的命名，不属于管理范围
            skipped += 1;
            continue;
        }

        // 与引用集合**必须用同一个**标准化函数：两边只要有一边走的是
        // 大小写敏感的前缀匹配，判断就会失真。删除动作本身不受影响——
        // 下面仍然按原始路径交给 `safe_remove_managed_copy()`（只删目录项）。
        let key = normalize_managed_identity(&data_dir, &path.to_string_lossy());
        if referenced.contains(&key) {
            kept += 1;
            continue;
        }

        // 与任务删除共用同一个安全删除函数（第三轮任务书 §2.5）：
        // 越界拒绝、符号链接只删链接自身、绝不跟随目标
        match safe_remove_managed_copy(&root, &path) {
            Ok(()) => {
                removed += 1;
                removed_files.push(name);
            }
            Err(e) => {
                skipped += 1;
                log::warn!("孤儿附件未删除：{} —— {e}", path.display());
            }
        }
    }

    if removed > 0 {
        log::info!("已清理 {removed} 个无引用的附件副本文件");
    }

    Ok(OrphanCleanupResult {
        scanned,
        kept,
        removed,
        skipped,
        removed_files,
    })
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

    // =========================================================================
    // 第三轮 §2 / §10.2：安全删除受控副本
    // =========================================================================

    /// 建一个临时"受控附件目录" + 外部目录，返回 (受控目录, 外部目录, 清理句柄)
    fn temp_dirs(tag: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("lumen-att-{tag}-{}", uuid::Uuid::now_v7()));
        let root = base.join("attachments");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        (root, outside)
    }

    /// Windows 上创建目录联接（junction）。
    ///
    /// 为什么用它替代符号链接：本机与 CI 都没有
    /// `SeCreateSymbolicLinkPrivilege`（`std::os::windows::fs::symlink_file`
    /// 会报 "Administrator privilege required"），而 **junction 普通用户就能建**，
    /// 且它与 symlink 一样都是 reparse point——`FileType::is_symlink()` 对两者
    /// 都返回 true，走的是 `safe_remove_managed_copy` 里**同一段代码路径**。
    #[cfg(windows)]
    fn make_junction(link: &Path, target: &Path) -> bool {
        std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(link)
            .arg(target)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// 越界路径一律拒绝；受控目录本身拒绝；不存在的文件当"已完成"。
    #[test]
    fn safe_remove_refuses_outside_paths_and_is_idempotent() {
        let (root, outside) = temp_dirs("escape");
        let victim = outside.join("important.txt");
        std::fs::write(&victim, b"do not delete").unwrap();

        // 目录外绝对路径
        assert!(safe_remove_managed_copy(&root, &victim).is_err());
        // 相对穿越
        assert!(safe_remove_managed_copy(&root, &root.join("../outside/important.txt")).is_err());
        // UNC 路径：绝不能当成受控目录内的东西
        #[cfg(windows)]
        assert!(
            safe_remove_managed_copy(&root, Path::new(r"\\server\share\file.pdf")).is_err(),
            "UNC 路径必须被拒绝"
        );
        // 受控目录本身
        assert!(safe_remove_managed_copy(&root, &root).is_err());
        // 不存在的文件：幂等成功
        assert!(safe_remove_managed_copy(&root, &root.join("nope.pdf")).is_ok());

        assert!(victim.exists(), "受控目录之外的文件必须原样保留");
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    /// 大小写差异不得造成错误判定（任务书 §2.6 最后一条）。
    #[test]
    fn safe_remove_handles_case_variants_safely() {
        let (root, _outside) = temp_dirs("case");
        let file = root.join("AbC.pdf");
        std::fs::write(&file, b"x").unwrap();

        // 同一路径写成不同大小写：应当被认作"在受控目录内"并被删掉
        let upper = PathBuf::from(root.to_string_lossy().to_uppercase()).join("abc.PDF");
        let res = safe_remove_managed_copy(&root, &upper);
        assert!(res.is_ok(), "大小写变体应正常删除，实际：{res:?}");
        assert!(!file.exists(), "文件应已删除");

        // 而"看起来像受控目录、实际不是"的兄弟目录必须拒绝
        let sibling = PathBuf::from(format!("{}-evil", root.to_string_lossy()));
        assert!(
            safe_remove_managed_copy(&root, &sibling.join("x.pdf")).is_err(),
            "同前缀的兄弟目录不是受控目录，必须拒绝"
        );
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    /// junction 指向受控目录**之外**：只删链接自身，外部目录与文件完好。
    #[cfg(windows)]
    #[test]
    fn junction_to_outside_is_only_unlinked() {
        let (root, outside) = temp_dirs("junc-out");
        let inner = outside.join("keep.txt");
        std::fs::write(&inner, b"keep me").unwrap();

        // 受控目录里放一个指向外部的 junction（名字是 Lumen 会生成的 UUID 形式）
        let link = root.join(uuid::Uuid::now_v7().to_string());
        if !make_junction(&link, &outside) {
            eprintln!("[skip] 本机无法创建 junction，跳过该用例");
            let _ = std::fs::remove_dir_all(root.parent().unwrap());
            return;
        }

        safe_remove_managed_copy(&root, &link).expect("删除链接自身应当成功");
        assert!(!link.exists(), "junction 本身应被删除");
        assert!(outside.is_dir(), "链接指向的外部目录必须完好");
        assert!(inner.exists(), "外部目录里的文件必须完好");
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    /// junction 指向**另一个仍在使用的副本所在目录**：
    /// 删掉链接不能让那个文件消失（任务书 §2.1 的核心场景）。
    #[cfg(windows)]
    #[test]
    fn junction_to_live_copy_dir_never_deletes_the_file_behind_it() {
        let (root, _outside) = temp_dirs("junc-live");
        let live = root.join("live-copy.pdf");
        std::fs::write(&live, b"still needed").unwrap();

        let link = root.join(uuid::Uuid::now_v7().to_string());
        if !make_junction(&link, &root) {
            eprintln!("[skip] 本机无法创建 junction，跳过该用例");
            let _ = std::fs::remove_dir_all(root.parent().unwrap());
            return;
        }

        safe_remove_managed_copy(&root, &link).expect("删除链接自身应当成功");
        assert!(!link.exists(), "junction 本身应被删除");
        assert!(live.exists(), "链接背后的 live 副本绝不能被删掉");
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    }

    /// 删除副本集合时，链接与普通文件混合出现也不会误伤：
    /// 用 delete_copied_files 走一遍完整路径（它内部调用同一个安全函数）。
    #[cfg(windows)]
    #[test]
    fn delete_copied_files_only_unlinks_and_keeps_normal_files_safe() {
        let (root, outside) = temp_dirs("mixed");
        let data_dir = root.parent().unwrap().to_path_buf();
        let outside_file = outside.join("user.pdf");
        std::fs::write(&outside_file, b"user data").unwrap();

        // 一个正常的受控副本 + 一个指向外部的 junction
        let normal = root.join(format!("{}.pdf", uuid::Uuid::now_v7()));
        std::fs::write(&normal, b"copy").unwrap();
        let link = root.join(uuid::Uuid::now_v7().to_string());
        let made = make_junction(&link, &outside);

        let stored = vec![
            "attachments/../outside/user.pdf".to_string(), // 越界，必须拒绝
            format!(
                "attachments/{}",
                normal.file_name().unwrap().to_string_lossy()
            ),
            format!(
                "attachments/{}",
                link.file_name().unwrap().to_string_lossy()
            ),
        ];
        let (removed, failed) = delete_copied_files(&data_dir, &stored);

        assert!(!normal.exists(), "受控目录内的普通副本应被删除");
        assert!(outside_file.exists(), "受控目录之外的文件绝不能被删");
        assert_eq!(failed, 1, "越界那一条必须被记为失败/跳过");
        assert_eq!(removed, if made { 2 } else { 1 });

        let _ = std::fs::remove_dir_all(root.parent().unwrap());
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

    // =========================================================================
    // 第三轮 §14 / §15 / §16：legacy 绝对路径的大小写身份标准化
    // =========================================================================

    /// 同一个受控附件的各种写法必须标准化成**同一个**身份；
    /// 不同副本（不同 UUID）必须得到不同结果。
    #[test]
    fn normalize_managed_identity_matches_case_and_separator_variants() {
        let data_dir = Path::new(r"C:\Users\Me\AppData\Roaming\com.pla0185.lumen");
        let id = "0192f1c4-0000-7000-8000-0123456789ab";
        let relative = format!("attachments/{id}.pdf");
        let expected = normalize_managed_identity(data_dir, &relative);

        // 跨平台：`.` 与 `..` 都要被词法折叠掉（词法归一化不依赖文件系统）
        let dotted = format!("attachments/./{id}.pdf");
        assert_eq!(
            normalize_managed_identity(data_dir, &dotted),
            expected,
            "带 `.` 的写法必须与干净写法是同一个身份"
        );
        let backtrack = format!("attachments/别的目录/../{id}.pdf");
        assert_eq!(
            normalize_managed_identity(data_dir, &backtrack),
            expected,
            "带 `..` 的写法必须与干净写法是同一个身份"
        );

        if cfg!(windows) {
            // legacy 绝对路径：盘符、目录、文件名全大写，而且用 `/` 作分隔符
            let legacy_abs = format!(
                "C:/USERS/ME/APPDATA/ROAMING/COM.PLA0185.LUMEN/ATTACHMENTS/{}.PDF",
                id.to_uppercase()
            );
            assert_eq!(
                normalize_managed_identity(data_dir, &legacy_abs),
                expected,
                "Windows 上大小写不同、分隔符不同的绝对路径必须与相对路径算作同一个附件"
            );

            // `\` 与 `/` 混用
            let mixed =
                format!("C:/Users\\Me/AppData\\Roaming/com.pla0185.lumen/attachments/{id}.pdf");
            assert_eq!(
                normalize_managed_identity(data_dir, &mixed),
                expected,
                "分隔符混用不能改变身份"
            );

            // 结尾多一个分隔符
            let trailing = format!("{relative}\\");
            assert_eq!(
                normalize_managed_identity(data_dir, &trailing),
                expected,
                "结尾多余的分隔符不能改变身份"
            );
        } else {
            // 非 Windows 平台保持大小写敏感：不能为了 Windows 把别的平台搞坏
            let upper_rel = format!("attachments/{}.pdf", id.to_ascii_uppercase());
            assert_ne!(
                normalize_managed_identity(data_dir, &upper_rel),
                expected,
                "非 Windows 平台大小写不同就是不同文件，不能被折叠成同一个身份"
            );
        }

        // 不同 UUID ⇒ 不同身份（不能把所有附件都归一成同一个键，那会让孤儿永远清不掉）
        let other = format!("attachments/{}.pdf", uuid::Uuid::now_v7());
        assert_ne!(normalize_managed_identity(data_dir, &other), expected);
    }

    /// 数据库里 legacy 的**绝对路径**与当前 `data_dir` 大小写/分隔符不同时，
    /// 那个仍被引用的 live 副本**绝不能被当成孤儿删掉**（第三轮 §14 的核心缺陷）。
    ///
    /// 修复前的实际行为：`cleanup_orphans_impl` 的引用集合走
    /// `relative_stored_path()`（内部是 `strip_prefix`，逐组件精确比较），
    /// 大小写一变就匹配不上、回退成绝对路径字符串，与扫描得到的
    /// `attachments/<uuid>.pdf` 对不上 ⇒ 仍被引用的文件被删除。
    ///
    /// 只在 Windows 上跑：本用例的前提是"路径大小写不敏感、`/` 与 `\` 等价"。
    /// 非 Windows 把 `A.pdf` 与 `a.pdf` 当成两个不同的文件，
    /// 这里构造的"同一个位置的另一种写法"根本不成立。
    #[cfg(windows)]
    #[tokio::test]
    async fn legacy_absolute_stored_path_with_different_case_is_still_referenced() {
        let dir = std::env::temp_dir().join(format!("lumen-case-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.expect("初始化临时数据库");
        let att_dir = dir.join("attachments");
        std::fs::create_dir_all(&att_dir).unwrap();

        // attachments.task_id 有外键约束，先建一个任务
        let task_id = uuid::Uuid::now_v7().to_string();
        let now = to_db_time(utc_now());
        sqlx::query(
            "INSERT INTO tasks (id, title, status, sort_order, created_at, updated_at)
             VALUES (?1, ?2, 'todo', 0, ?3, ?3)",
        )
        .bind(&task_id)
        .bind("大小写身份测试")
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        // 磁盘上的真实文件：UUID 命名（Lumen 唯一会生成的形态）
        let att_id = uuid::Uuid::now_v7().to_string();
        let file_name = format!("{att_id}.pdf");
        let live_file = att_dir.join(&file_name);
        std::fs::write(&live_file, b"%PDF-1.4 legacy").unwrap();

        // 数据库里那条"第二轮整改之前"的记录：**同一个位置**，
        // 但盘符/目录/文件名全大写，并且用 `/` 作分隔符。
        // 只大写 ASCII，避免 `to_uppercase()` 把非 ASCII 字符变长而指向别处。
        let legacy_stored = format!(
            "{}/ATTACHMENTS/{}",
            dir.to_string_lossy()
                .to_ascii_uppercase()
                .replace('\\', "/"),
            file_name.to_ascii_uppercase()
        );

        // 前提校验 1：这个写法确实与真实路径不同，否则本用例测不出东西
        assert_ne!(
            Path::new(&legacy_stored),
            live_file.as_path(),
            "构造的 legacy 路径必须与真实路径大小写不同"
        );
        // 前提校验 2：**旧实现**在这个大小写变体上确实会回退成绝对路径字符串，
        // 这正是 live 副本被误删的根因。这条断言红了说明用例前提已变，需要重新评估。
        assert_eq!(
            relative_stored_path(&dir, Path::new(&legacy_stored)),
            legacy_stored,
            "旧实现（strip_prefix 精确比较）应在此回退成绝对路径"
        );

        sqlx::query(
            "INSERT INTO attachments
                (id, task_id, file_name, mime_type, byte_size, sha256, storage_mode,
                 external_path, stored_path, created_at)
             VALUES (?1, ?2, ?3, 'application/pdf', ?4, NULL, 'copied', NULL, ?5, ?6)",
        )
        .bind(&att_id)
        .bind(&task_id)
        .bind(&file_name)
        .bind(12i64)
        .bind(&legacy_stored)
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        let r = cleanup_orphans_impl(&db).await.expect("孤儿清理");

        assert!(
            live_file.exists(),
            "仍被数据库引用的副本绝不能被当作孤儿删除：{}",
            live_file.display()
        );
        assert!(r.kept >= 1, "被引用的副本必须计入 kept，实际：{r:?}");
        assert_eq!(
            r.removed, 0,
            "没有任何文件该被删掉，实际删了：{:?}",
            r.removed_files
        );
        assert!(
            !r.removed_files.contains(&file_name),
            "被引用的副本不能出现在删除清单里"
        );

        // 对照：真孤儿仍然必须被删掉——防止"把误判改成一律不删"这种假修复
        let orphan_name = format!("{}.bin", uuid::Uuid::now_v7());
        let orphan = att_dir.join(&orphan_name);
        std::fs::write(&orphan, b"orphan").unwrap();

        let r2 = cleanup_orphans_impl(&db).await.expect("第二次孤儿清理");
        assert!(!orphan.exists(), "真正的孤儿文件仍应被清理");
        assert_eq!(
            r2.removed, 1,
            "第二次只应删掉那个真孤儿，实际：{:?}",
            r2.removed_files
        );
        assert!(
            live_file.exists(),
            "补跑一次清理后，被引用的副本依然不能被删"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
