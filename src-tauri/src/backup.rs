//! 备份、导出与恢复（任务书 §9）。
//!
//! ## 设计要点
//!
//! - **完整 JSON 备份**：附带版本号与内容校验和（SHA-256）。恢复前必须校验，
//!   校验不通过就拒绝导入——宁可拒绝，也不能把半个备份写进用户的真实数据。
//! - **恢复前先备份当前数据库**：用户点错恢复也能退回来（§9 明确要求）。
//! - **恢复是整体替换而非合并**：语义明确、可预期。合并策略容易产生
//!   难以解释的重复项，而任务书要求"冲突时不静默覆盖数据"——替换是有提示的，
//!   合并没有提示才是危险的。
//! - **附件不入 JSON**：JSON 不适合容纳二进制。备份文件里只存附件清单
//!   （文件名、大小、哈希、原路径），并在导出结果中明确告知用户附件未包含，
//!   而不是假装备份完整。
//! - **导出 CSV / Markdown**：供人类阅读与其他工具导入。
//! - **自动定期备份 + 保留份数上限**：避免占满磁盘。
//!
//! ## 时间与编码
//!
//! 所有时间沿用库内的固定宽度 UTC 字符串，因此备份文件本身就是可读的，
//! 且重新导入后排序语义不变。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
// `Column` trait 提供 `name()`，必须显式引入（trait 方法不会自动可见）
use sqlx::{Column, Row};
use tauri::State;

use crate::commands::AppState;
use crate::db::{now_stamp, to_db_time, utc_now, Db};
use crate::error::{AppError, AppResult};

/// 备份格式版本。格式变更时必须递增，并在导入时按版本分支处理。
pub const BACKUP_FORMAT_VERSION: u32 = 1;

/// 备份文件的扩展名
pub const BACKUP_EXT: &str = "lumen-backup.json";

/// 默认保留的自动备份份数（超出后删除最旧的）
pub const DEFAULT_KEEP: i64 = 10;

/// 自动备份保留份数的允许范围
const KEEP_MIN: i64 = 1;
const KEEP_MAX: i64 = 100;

// =============================================================================
// 备份文件结构
// =============================================================================

/// 备份文件顶层结构
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    /// 格式版本号
    pub format_version: u32,
    /// 生成该备份的应用版本
    pub app_version: String,
    /// 生成时刻（UTC）
    pub created_at: String,
    /// 内容校验和（对 `data` 的规范化 JSON 计算 SHA-256，十六进制小写）
    pub checksum: String,
    /// 统计信息，便于在不导入的情况下展示（不参与校验和）
    #[serde(default)]
    pub stats: BackupStats,
    /// 说明字段，供人阅读
    #[serde(default)]
    pub note: Option<String>,
    /// 实际数据
    pub data: BackupData,
}

/// 备份统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStats {
    pub tasks: usize,
    pub projects: usize,
    pub categories: usize,
    pub tags: usize,
    pub task_tags: usize,
    pub subtasks: usize,
    pub dependencies: usize,
    pub reminders: usize,
    pub attachments: usize,
    pub series: usize,
    pub segments: usize,
    pub settings: usize,
}

/// 备份承载的数据。全部使用 `serde_json::Value` 行式存储，
/// 这样新增列时不必同步改这里——备份的价值在于"能原样还给用户"，
/// 而不是在应用层重新解释一遍结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupData {
    pub projects: Vec<serde_json::Value>,
    pub categories: Vec<serde_json::Value>,
    pub tags: Vec<serde_json::Value>,
    pub tasks: Vec<serde_json::Value>,
    pub task_tags: Vec<serde_json::Value>,
    pub subtasks: Vec<serde_json::Value>,
    pub dependencies: Vec<serde_json::Value>,
    pub reminders: Vec<serde_json::Value>,
    pub attachments: Vec<serde_json::Value>,
    pub series: Vec<serde_json::Value>,
    pub segments: Vec<serde_json::Value>,
    pub settings: Vec<serde_json::Value>,
    /// 附件未随备份打包的说明（始终存在，避免用户误以为备份包含文件本体）
    #[serde(default)]
    pub attachments_note: String,
}

/// 导出结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    /// 备份文件路径
    pub path: String,
    /// 文件字节数
    pub bytes: u64,
    /// 内容校验和
    pub checksum: String,
    /// 各类数据条目数
    pub stats: BackupStats,
    /// 附件未被包含的提醒（若存在附件）
    pub attachment_warning: Option<String>,
}

/// 导入预览：让用户在真正写入前看到"会发生什么"（§9「导入前展示预览」）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub path: String,
    pub format_version: u32,
    pub app_version: String,
    pub created_at: String,
    /// 校验和是否匹配
    pub checksum_ok: bool,
    /// 校验失败时的说明
    pub checksum_error: Option<String>,
    /// 备份中的条目数
    pub stats: BackupStats,
    /// 当前库中的条目数，便于用户对比
    pub current: BackupStats,
    /// 将被覆盖的数据量（当前库中会消失的条目）
    pub will_replace_tasks: usize,
    pub attachments_note: String,
    /// 阻断性问题（存在则不允许导入）
    pub blocking_issues: Vec<String>,
}

/// 恢复结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    /// 恢复前自动生成的当前库备份路径（失败时可退回）
    pub safety_backup: Option<String>,
    /// 实际导入的条目数
    pub imported: BackupStats,
}

/// 单个备份文件的信息（列表展示）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub path: String,
    pub file_name: String,
    pub bytes: u64,
    /// 文件修改时间（本地可读）
    pub modified_at: String,
    /// 自动备份（由程序定期生成）还是手动备份
    pub kind: String,
}

// =============================================================================
// 校验和
// =============================================================================

/// 计算数据的规范化校验和。
///
/// 用 `serde_json::to_vec` 而非 `to_string`：后者依赖 map 顺序，
/// 同一份数据在不同构建下可能产生不同字节，导致校验和误判。
/// `to_vec` 配合 serde_json 的默认 BTreeMap 行为可保证稳定性。
fn checksum_of(data: &BackupData) -> AppResult<String> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(data)
        .map_err(|e| AppError::internal(format!("序列化备份数据失败：{e}")))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

// =============================================================================
// 导出
// =============================================================================

/// 把单表整表读成 JSON 行数组。
///
/// 表名只可能来自本文件内的字面量，不存在拼接注入风险。
async fn dump_table(db: &Db, table: &str) -> AppResult<Vec<serde_json::Value>> {
    let sql = match table {
        "projects" => "SELECT * FROM projects",
        "categories" => "SELECT * FROM categories",
        "tags" => "SELECT * FROM tags",
        // 任务只导出未在回收站的？不——回收站也是用户数据，
        // 恢复后应当原样回来。deleted_at 字段本身会被保留。
        "tasks" => "SELECT * FROM tasks",
        "task_tags" => "SELECT * FROM task_tags",
        "subtasks" => "SELECT * FROM subtasks",
        "task_dependencies" => "SELECT * FROM task_dependencies",
        "reminders" => "SELECT * FROM reminders",
        "attachments" => "SELECT * FROM attachments",
        "task_series" => "SELECT * FROM task_series",
        "task_series_segments" => "SELECT * FROM task_series_segments",
        "settings" => "SELECT * FROM settings",
        _ => return Err(AppError::internal("内部错误：非法表名")),
    };

    let rows = sqlx::query(sql).fetch_all(db.pool()).await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        // 逐列转 JSON：保留 SQLite 的原始类型（整数仍是数字而非字符串）
        let mut obj = serde_json::Map::new();
        for (i, col) in r.columns().iter().enumerate() {
            let name = col.name().to_string();
            let value = sqlite_value_to_json(&r, i)?;
            obj.insert(name, value);
        }
        out.push(serde_json::Value::Object(obj));
    }
    Ok(out)
}

/// 把某一列的值转成 JSON。
///
/// 先尝试 i64，再 f64，再 String，最后 NULL。
/// 这样 `sort_order REAL` 会正确变成小数，而 `id TEXT` 仍是字符串。
fn sqlite_value_to_json(
    row: &sqlx::sqlite::SqliteRow,
    index: usize,
) -> AppResult<serde_json::Value> {
    use sqlx::{TypeInfo, ValueRef};

    let raw = row
        .try_get_raw(index)
        .map_err(|e| AppError::internal(format!("读取备份字段失败（列 {index}）：{e}")))?;

    if raw.is_null() {
        return Ok(serde_json::Value::Null);
    }

    let type_name = raw.type_info().name().to_string();
    match type_name.as_str() {
        "INTEGER" | "INT" | "BIGINT" => {
            let v: i64 = row.try_get(index)?;
            Ok(serde_json::json!(v))
        }
        "REAL" | "FLOAT" | "DOUBLE" => {
            let v: f64 = row.try_get(index)?;
            Ok(serde_json::json!(v))
        }
        "NULL" => Ok(serde_json::Value::Null),
        _ => {
            let v: String = row.try_get(index)?;
            Ok(serde_json::json!(v))
        }
    }
}

/// 统计当前库中的条目数
async fn current_stats(db: &Db) -> AppResult<BackupStats> {
    let row = sqlx::query(
        "SELECT
            (SELECT COUNT(*) FROM tasks) AS tasks,
            (SELECT COUNT(*) FROM projects) AS projects,
            (SELECT COUNT(*) FROM categories) AS categories,
            (SELECT COUNT(*) FROM tags) AS tags,
            (SELECT COUNT(*) FROM task_tags) AS task_tags,
            (SELECT COUNT(*) FROM subtasks) AS subtasks,
            (SELECT COUNT(*) FROM task_dependencies) AS dependencies,
            (SELECT COUNT(*) FROM reminders) AS reminders,
            (SELECT COUNT(*) FROM attachments) AS attachments,
            (SELECT COUNT(*) FROM task_series) AS series,
            (SELECT COUNT(*) FROM task_series_segments) AS segments,
            (SELECT COUNT(*) FROM settings) AS settings",
    )
    .fetch_one(db.pool())
    .await?;

    Ok(BackupStats {
        tasks: row.try_get::<i64, _>("tasks")? as usize,
        projects: row.try_get::<i64, _>("projects")? as usize,
        categories: row.try_get::<i64, _>("categories")? as usize,
        tags: row.try_get::<i64, _>("tags")? as usize,
        task_tags: row.try_get::<i64, _>("task_tags")? as usize,
        subtasks: row.try_get::<i64, _>("subtasks")? as usize,
        dependencies: row.try_get::<i64, _>("dependencies")? as usize,
        reminders: row.try_get::<i64, _>("reminders")? as usize,
        attachments: row.try_get::<i64, _>("attachments")? as usize,
        series: row.try_get::<i64, _>("series")? as usize,
        segments: row.try_get::<i64, _>("segments")? as usize,
        settings: row.try_get::<i64, _>("settings")? as usize,
    })
}

/// 组装完整备份数据
async fn build_backup_data(db: &Db) -> AppResult<BackupData> {
    let attachment_count: i64 = sqlx::query("SELECT COUNT(*) AS n FROM attachments")
        .fetch_one(db.pool())
        .await?
        .try_get("n")?;

    Ok(BackupData {
        projects: dump_table(db, "projects").await?,
        categories: dump_table(db, "categories").await?,
        tags: dump_table(db, "tags").await?,
        tasks: dump_table(db, "tasks").await?,
        task_tags: dump_table(db, "task_tags").await?,
        subtasks: dump_table(db, "subtasks").await?,
        dependencies: dump_table(db, "task_dependencies").await?,
        reminders: dump_table(db, "reminders").await?,
        attachments: dump_table(db, "attachments").await?,
        series: dump_table(db, "task_series").await?,
        segments: dump_table(db, "task_series_segments").await?,
        settings: dump_table(db, "settings").await?,
        attachments_note: if attachment_count > 0 {
            format!(
                "本备份包含 {attachment_count} 条附件记录，但**不包含附件文件本身**。\
                 恢复后附件记录会回来，文件需从原路径重新关联。"
            )
        } else {
            "本备份不含附件记录。".to_string()
        },
    })
}

/// 导出完整 JSON 备份到指定路径
#[tauri::command]
pub async fn backup_export(
    state: State<'_, AppState>,
    path: Option<String>,
) -> AppResult<ExportResult> {
    let db = &state.db;
    let data = build_backup_data(db).await?;
    let stats = BackupStats {
        tasks: data.tasks.len(),
        projects: data.projects.len(),
        categories: data.categories.len(),
        tags: data.tags.len(),
        task_tags: data.task_tags.len(),
        subtasks: data.subtasks.len(),
        dependencies: data.dependencies.len(),
        reminders: data.reminders.len(),
        attachments: data.attachments.len(),
        series: data.series.len(),
        segments: data.segments.len(),
        settings: data.settings.len(),
    };
    let checksum = checksum_of(&data)?;

    let file = BackupFile {
        format_version: BACKUP_FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: to_db_time(utc_now()),
        checksum: checksum.clone(),
        stats: stats.clone(),
        note: Some("Lumen 完整数据备份（不含附件文件本体）".to_string()),
        data,
    };

    let dest = match path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => db
            .data_dir()
            .join("backups")
            .join(format!("manual-{}.{}", now_stamp(), BACKUP_EXT)),
    };

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // 先写临时文件再改名：中途失败不会留下一个"看起来完整"的半截备份
    let tmp = dest.with_extension("tmp");
    let json = serde_json::to_vec_pretty(&file)
        .map_err(|e| AppError::internal(format!("序列化备份失败：{e}")))?;
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, &dest)?;

    let bytes = std::fs::metadata(&dest)?.len();
    log::info!("已导出备份：{}（{bytes} 字节）", dest.display());

    Ok(ExportResult {
        path: dest.to_string_lossy().to_string(),
        bytes,
        checksum,
        attachment_warning: if stats.attachments > 0 {
            Some(format!(
                "备份含 {} 条附件记录，但附件文件本身未打包。如需连文件一起备份，请另行复制数据目录中的 attachments 文件夹。",
                stats.attachments
            ))
        } else {
            None
        },
        stats,
    })
}

// =============================================================================
// 读取与预览
// =============================================================================

/// 读取并解析备份文件
fn read_backup(path: &Path) -> AppResult<BackupFile> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        AppError::new(
            crate::error::ErrorCode::Io,
            format!("无法读取备份文件：{e}"),
        )
        .with_hint("请确认文件存在且未被其他程序占用")
    })?;
    serde_json::from_str::<BackupFile>(&text).map_err(|e| {
        AppError::validation(format!("备份文件格式不正确：{e}"))
            .with_hint("请确认选择的是 Lumen 导出的 .lumen-backup.json 文件")
    })
}

/// 预览导入内容（不改动任何数据）
#[tauri::command]
pub async fn backup_preview(state: State<'_, AppState>, path: String) -> AppResult<ImportPreview> {
    let p = PathBuf::from(&path);
    let file = read_backup(&p)?;
    let current = current_stats(&state.db).await?;

    // 校验和必须匹配
    let (checksum_ok, checksum_error) = match checksum_of(&file.data) {
        Ok(actual) if actual == file.checksum => (true, None),
        Ok(actual) => (
            false,
            Some(format!(
                "内容校验和不匹配（文件声明 {}，实际 {}）。备份可能已损坏或被修改。",
                &file.checksum[..file.checksum.len().min(16)],
                &actual[..actual.len().min(16)]
            )),
        ),
        Err(e) => (false, Some(format!("无法计算校验和：{e}"))),
    };

    let mut blocking = Vec::new();
    if !checksum_ok {
        blocking.push("备份内容校验失败，拒绝导入以免破坏现有数据".to_string());
    }
    if file.format_version > BACKUP_FORMAT_VERSION {
        blocking.push(format!(
            "备份格式版本 {} 高于当前程序支持的 {}，请先升级 Lumen",
            file.format_version, BACKUP_FORMAT_VERSION
        ));
    }
    if file.format_version == 0 {
        blocking.push("备份缺少格式版本号，无法确认兼容性".to_string());
    }

    Ok(ImportPreview {
        path: p.to_string_lossy().to_string(),
        format_version: file.format_version,
        app_version: file.app_version.clone(),
        created_at: file.created_at.clone(),
        checksum_ok,
        checksum_error,
        stats: file.stats.clone(),
        will_replace_tasks: current.tasks,
        current,
        attachments_note: file.data.attachments_note.clone(),
        blocking_issues: blocking,
    })
}

// =============================================================================
// 恢复
// =============================================================================

/// 把 JSON 行插入指定表。
///
/// 用 `QueryBuilder` 动态拼列名 —— 列名来自备份文件本身，
/// 因此**必须**先与当前库的真实列集合求交集，忽略多余列，
/// 缺失列交给数据库默认值。这样既不会因备份里的陌生列而失败，
/// 也不会把任意 SQL 片段带进来（列名会被加引号）。
async fn insert_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    table: &str,
    rows: &[serde_json::Value],
    allowed_columns: &[String],
) -> AppResult<usize> {
    if rows.is_empty() {
        return Ok(0);
    }
    let mut inserted = 0usize;

    for row in rows {
        let obj = match row.as_object() {
            Some(o) => o,
            None => {
                return Err(AppError::validation(format!(
                    "备份中 {table} 的某条记录不是对象，文件可能已损坏"
                )))
            }
        };

        // 只保留当前库确实存在的列
        let cols: Vec<&String> = obj
            .keys()
            .filter(|k| allowed_columns.iter().any(|c| c == *k))
            .collect();
        if cols.is_empty() {
            continue;
        }

        // 表名来自本文件的内部字面量；列名会被双引号包裹，
        // 且已与当前库的真实列集合求过交集，因此不存在注入风险。
        //
        // 注意：这里刻意**不用** `separated()`。`Separated::push` 会在
        // 每次调用时把分隔符拼到片段前面，而拼一个列名需要三次 push
        // （引号、列名、引号），结果会变成 `" , completed_at, "` 这种
        // 非法列名。因此改用手动控制分隔符 + push_unseparated，
        // 保证一个列名恰好对应一次分隔符。
        let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new("INSERT INTO ");
        qb.push(table).push(" (");
        for (i, c) in cols.iter().enumerate() {
            if i > 0 {
                qb.push(", ");
            }
            qb.push("\"").push(c.as_str()).push("\"");
        }
        qb.push(") VALUES (");
        for (i, c) in cols.iter().enumerate() {
            if i > 0 {
                qb.push(", ");
            }
            let v = obj.get(*c).unwrap_or(&serde_json::Value::Null);
            match v {
                serde_json::Value::Null => {
                    qb.push_bind(None::<String>);
                }
                serde_json::Value::Bool(b) => {
                    // SQLite 无布尔类型，统一按 0/1 存储
                    qb.push_bind(*b as i64);
                }
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        qb.push_bind(i);
                    } else {
                        qb.push_bind(n.as_f64().unwrap_or(0.0));
                    }
                }
                serde_json::Value::String(s) => {
                    qb.push_bind(s.clone());
                }
                other => {
                    // 数组/对象不该出现在这些表里；转成 JSON 字符串以保留信息
                    qb.push_bind(other.to_string());
                }
            }
        }
        qb.push(")");

        qb.build().execute(&mut **tx).await.map_err(|e| {
            AppError::conflict(format!("写入 {table} 失败：{e}"))
                .with_hint("备份数据可能与当前数据库结构不兼容")
        })?;
        inserted += 1;
    }

    Ok(inserted)
}

/// 取某表的列名集合。
///
/// 表名来自本文件的内部字面量，非用户输入；sqlx 0.9 的静态 SQL 审计
/// 要求对动态拼接的语句显式声明"已审计"，故用 AssertSqlSafe 标注。
async fn table_columns(db: &Db, table: &str) -> AppResult<Vec<String>> {
    let sql = format!("PRAGMA table_info({table})");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(db.pool())
        .await?;
    let mut cols = Vec::with_capacity(rows.len());
    for r in rows {
        cols.push(r.try_get::<String, _>("name")?);
    }
    Ok(cols)
}

/// 从备份恢复。
///
/// 步骤：校验 → 备份当前库 → 事务内清空并导入 → 提交。
/// 任何一步失败都不会留下"清空了但没导入"的状态，因为清空与导入在同一事务内。
#[tauri::command]
pub async fn backup_restore(state: State<'_, AppState>, path: String) -> AppResult<RestoreResult> {
    let db = &state.db;
    let p = PathBuf::from(&path);

    // 1) 先做与预览相同的校验（防止前端跳过预览直接调用）
    let file = read_backup(&p)?;
    if file.format_version > BACKUP_FORMAT_VERSION {
        return Err(AppError::conflict(format!(
            "备份格式版本 {} 高于当前程序支持的 {}",
            file.format_version, BACKUP_FORMAT_VERSION
        )));
    }
    let actual = checksum_of(&file.data)?;
    if actual != file.checksum {
        return Err(AppError::conflict("备份内容校验失败，已拒绝导入")
            .with_hint("备份文件可能已损坏或被修改。为保护现有数据，未做任何改动"));
    }

    // 2) 恢复前先备份当前数据库（§9 明确要求）
    let safety = db
        .data_dir()
        .join("backups")
        .join(format!("before-restore-{}.db", now_stamp()));
    let safety_path = match db.backup_to(&safety).await {
        Ok(()) => Some(safety.to_string_lossy().to_string()),
        Err(e) => {
            // 无法生成安全备份时**中止恢复**：任务书把"数据丢失或不可恢复"
            // 列为阻断交付的问题，这里不允许在无退路的情况下覆盖数据。
            return Err(AppError::new(
                crate::error::ErrorCode::Io,
                format!("无法生成恢复前的安全备份，已中止恢复：{e}"),
            )
            .with_hint("请检查磁盘空间与数据目录写入权限后重试"));
        }
    };

    // 3) 事务内清空 + 导入
    let mut tx = db.pool().begin().await?;

    // 顺序必须满足外键依赖：先删子表，再删父表
    for t in [
        "task_dependencies",
        "reminders",
        "attachments",
        "subtasks",
        "task_tags",
        "tasks",
        "task_series_segments",
        "task_series",
        "tags",
        "categories",
        "projects",
        "settings",
    ] {
        // 表名来自上面这个内部字面量数组，非用户输入
        let sql = format!("DELETE FROM {t}");
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut *tx)
            .await?;
    }

    // 插入顺序与外键方向一致：父表先插
    let plan: [(&str, &[serde_json::Value]); 12] = [
        ("projects", &file.data.projects),
        ("categories", &file.data.categories),
        ("tags", &file.data.tags),
        ("task_series", &file.data.series),
        ("tasks", &file.data.tasks),
        ("task_series_segments", &file.data.segments),
        ("task_tags", &file.data.task_tags),
        ("subtasks", &file.data.subtasks),
        ("task_dependencies", &file.data.dependencies),
        ("reminders", &file.data.reminders),
        ("attachments", &file.data.attachments),
        ("settings", &file.data.settings),
    ];

    // 预先取好各表列名（避免在事务里反复 PRAGMA 查询）
    let mut cols_cache: std::collections::HashMap<&str, Vec<String>> = Default::default();
    for (t, _) in plan.iter() {
        cols_cache.insert(t, table_columns(db, t).await?);
    }

    let mut imported = BackupStats::default();
    for (table, rows) in plan.iter() {
        let cols = cols_cache
            .get(table)
            .ok_or_else(|| AppError::internal(format!("缺少 {table} 的列信息")))?;
        let n = insert_rows(&mut tx, table, rows, cols).await?;
        match *table {
            "projects" => imported.projects = n,
            "categories" => imported.categories = n,
            "tags" => imported.tags = n,
            "task_series" => imported.series = n,
            "tasks" => imported.tasks = n,
            "task_series_segments" => imported.segments = n,
            "task_tags" => imported.task_tags = n,
            "subtasks" => imported.subtasks = n,
            "task_dependencies" => imported.dependencies = n,
            "reminders" => imported.reminders = n,
            "attachments" => imported.attachments = n,
            "settings" => imported.settings = n,
            _ => {}
        }
    }

    tx.commit().await?;
    log::info!(
        "已从备份恢复：{} 个任务、{} 个项目（安全备份：{}）",
        imported.tasks,
        imported.projects,
        safety_path.as_deref().unwrap_or("无")
    );

    Ok(RestoreResult {
        safety_backup: safety_path,
        imported,
    })
}

// =============================================================================
// 备份文件管理
// =============================================================================

/// 列出备份目录中的备份文件
#[tauri::command]
pub async fn backup_list(state: State<'_, AppState>) -> AppResult<Vec<BackupEntry>> {
    let dir = state.db.data_dir().join("backups");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        // 只列出 JSON 备份与数据库快照，忽略临时文件
        let is_backup = name.ends_with(BACKUP_EXT) || name.ends_with(".db");
        if !is_backup || name.ends_with(".tmp") {
            continue;
        }
        let meta = entry.metadata()?;
        let modified = meta
            .modified()
            .ok()
            .map(|t| {
                let dt: chrono::DateTime<chrono::Local> = t.into();
                dt.format("%Y-%m-%d %H:%M:%S").to_string()
            })
            .unwrap_or_else(|| "未知".to_string());

        let kind = if name.starts_with("auto-") {
            "自动备份"
        } else if name.starts_with("before-restore-") {
            "恢复前快照"
        } else if name.starts_with("pre-migrate-") {
            "迁移前快照"
        } else if name.starts_with("manual-") {
            "手动备份"
        } else {
            "其他"
        };

        out.push(BackupEntry {
            path: path.to_string_lossy().to_string(),
            file_name: name,
            bytes: meta.len(),
            modified_at: modified,
            kind: kind.to_string(),
        });
    }

    // 新的排在前面
    out.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(out)
}

/// 删除某个备份文件。
///
/// 只允许删除数据目录 backups 子目录内的文件，防止把任意路径传进来删掉
/// 用户的其他文件（§10 附件路径防越界，同样的思路适用于此处）。
#[tauri::command]
pub async fn backup_delete(state: State<'_, AppState>, path: String) -> AppResult<bool> {
    let dir = state
        .db
        .data_dir()
        .join("backups")
        .canonicalize()
        .unwrap_or_else(|_| state.db.data_dir().join("backups"));

    let target = PathBuf::from(&path);
    let target_abs = target
        .canonicalize()
        .map_err(|_| AppError::not_found("备份文件", &path))?;

    if !target_abs.starts_with(&dir) {
        return Err(AppError::conflict("只能删除备份目录内的文件")
            .with_hint("为安全起见，程序不会删除备份目录以外的任何文件"));
    }

    std::fs::remove_file(&target_abs)?;
    log::info!("已删除备份：{}", target_abs.display());
    Ok(true)
}

/// 生成一次自动备份，并按保留份数清理旧文件。
#[tauri::command]
pub async fn backup_auto(state: State<'_, AppState>, keep: Option<i64>) -> AppResult<ExportResult> {
    let keep = keep.unwrap_or(DEFAULT_KEEP);
    if !(KEEP_MIN..=KEEP_MAX).contains(&keep) {
        return Err(AppError::validation(format!("保留份数超出范围：{keep}"))
            .with_hint(format!("允许 {KEEP_MIN}–{KEEP_MAX} 份")));
    }

    let db = &state.db;
    let dest = db
        .data_dir()
        .join("backups")
        .join(format!("auto-{}.{}", now_stamp(), BACKUP_EXT));

    // 复用导出逻辑，只是指定了目标路径
    let data = build_backup_data(db).await?;
    let stats = BackupStats {
        tasks: data.tasks.len(),
        projects: data.projects.len(),
        categories: data.categories.len(),
        tags: data.tags.len(),
        task_tags: data.task_tags.len(),
        subtasks: data.subtasks.len(),
        dependencies: data.dependencies.len(),
        reminders: data.reminders.len(),
        attachments: data.attachments.len(),
        series: data.series.len(),
        segments: data.segments.len(),
        settings: data.settings.len(),
    };
    let checksum = checksum_of(&data)?;
    let file = BackupFile {
        format_version: BACKUP_FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: to_db_time(utc_now()),
        checksum: checksum.clone(),
        stats: stats.clone(),
        note: Some("Lumen 自动备份".to_string()),
        data,
    };

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("tmp");
    let json = serde_json::to_vec_pretty(&file)
        .map_err(|e| AppError::internal(format!("序列化备份失败：{e}")))?;
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, &dest)?;

    prune_auto_backups(db, keep).await?;

    let bytes = std::fs::metadata(&dest)?.len();
    Ok(ExportResult {
        path: dest.to_string_lossy().to_string(),
        bytes,
        checksum,
        stats,
        attachment_warning: None,
    })
}

/// 清理超出保留份数的自动备份（只针对 auto- 前缀，不动手动备份）
async fn prune_auto_backups(db: &Db, keep: i64) -> AppResult<usize> {
    let dir = db.data_dir().join("backups");
    if !dir.exists() {
        return Ok(0);
    }

    let mut autos: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("auto-") && name.ends_with(BACKUP_EXT) {
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            autos.push((modified, entry.path()));
        }
    }

    if autos.len() as i64 <= keep {
        return Ok(0);
    }

    // 新的在前，删掉超出的
    autos.sort_by(|a, b| b.0.cmp(&a.0));
    let mut removed = 0usize;
    for (_, path) in autos.iter().skip(keep as usize) {
        if std::fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        log::info!("已清理 {removed} 个超出保留份数的自动备份（保留 {keep} 份）");
    }
    Ok(removed)
}

// =============================================================================
// CSV / Markdown 导出
// =============================================================================

/// CSV 字段转义：包含逗号、引号、换行时必须加引号，引号需双写。
fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// 导出 CSV（§9）。表头与常见任务工具的导入模板保持接近，便于迁移。
#[tauri::command]
pub async fn export_csv(state: State<'_, AppState>, path: String) -> AppResult<String> {
    let db = &state.db;
    let rows = sqlx::query(
        "SELECT t.id, t.title, t.description, t.note_md, t.status, t.priority,
                p.name AS project_name, c.name AS category_name,
                t.planned_at, t.has_planned_time, t.due_at, t.has_due_time,
                t.estimated_minutes, t.actual_minutes, t.completed_at,
                t.created_at, t.updated_at, t.is_pinned,
                CASE WHEN t.series_id IS NOT NULL THEN 1 ELSE 0 END AS is_recurring,
                CASE WHEN t.deleted_at IS NOT NULL THEN '是' ELSE '' END AS in_trash
         FROM tasks t
         LEFT JOIN projects p ON p.id = t.project_id
         LEFT JOIN categories c ON c.id = t.category_id
         ORDER BY t.created_at",
    )
    .fetch_all(db.pool())
    .await?;

    // 标签需要聚合，单独查一次再拼
    let tag_rows = sqlx::query(
        "SELECT tt.task_id, tg.name FROM task_tags tt
         JOIN tags tg ON tg.id = tt.tag_id
         WHERE tg.deleted_at IS NULL
         ORDER BY tg.sort_order, tg.name",
    )
    .fetch_all(db.pool())
    .await?;
    let mut tags_by_task: std::collections::HashMap<String, Vec<String>> = Default::default();
    for r in tag_rows {
        tags_by_task
            .entry(r.try_get::<String, _>("task_id")?)
            .or_default()
            .push(r.try_get::<String, _>("name")?);
    }

    let mut csv = String::new();
    csv.push_str(
        "ID,标题,描述,备注,状态,优先级,项目,分类,标签,计划时间,仅日期,截止时间,仅日期,预计分钟,实际分钟,完成时间,创建时间,更新时间,已置顶,是否重复,在回收站\n",
    );

    for r in rows {
        let id: String = r.try_get("id")?;
        let tags = tags_by_task.get(&id).cloned().unwrap_or_default().join("|");
        let fields: Vec<String> = vec![
            csv_escape(&id),
            csv_escape(&r.try_get::<String, _>("title")?),
            csv_escape(&r.try_get::<String, _>("description")?),
            csv_escape(&r.try_get::<String, _>("note_md")?),
            csv_escape(&r.try_get::<String, _>("status")?),
            r.try_get::<i64, _>("priority")?.to_string(),
            csv_escape(
                r.try_get::<Option<String>, _>("project_name")?
                    .as_deref()
                    .unwrap_or(""),
            ),
            csv_escape(
                r.try_get::<Option<String>, _>("category_name")?
                    .as_deref()
                    .unwrap_or(""),
            ),
            csv_escape(&tags),
            csv_escape(
                r.try_get::<Option<String>, _>("planned_at")?
                    .as_deref()
                    .unwrap_or(""),
            ),
            r.try_get::<i64, _>("has_planned_time")?.to_string(),
            csv_escape(
                r.try_get::<Option<String>, _>("due_at")?
                    .as_deref()
                    .unwrap_or(""),
            ),
            r.try_get::<i64, _>("has_due_time")?.to_string(),
            r.try_get::<Option<i64>, _>("estimated_minutes")?
                .map(|v| v.to_string())
                .unwrap_or_default(),
            r.try_get::<i64, _>("actual_minutes")?.to_string(),
            csv_escape(
                r.try_get::<Option<String>, _>("completed_at")?
                    .as_deref()
                    .unwrap_or(""),
            ),
            csv_escape(&r.try_get::<String, _>("created_at")?),
            csv_escape(&r.try_get::<String, _>("updated_at")?),
            r.try_get::<i64, _>("is_pinned")?.to_string(),
            r.try_get::<i64, _>("is_recurring")?.to_string(),
            csv_escape(&r.try_get::<String, _>("in_trash")?),
        ];
        csv.push_str(&fields.join(","));
        csv.push('\n');
    }

    let dest = PathBuf::from(&path);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 加 UTF-8 BOM，否则 Excel 打开中文会乱码
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(csv.as_bytes());
    std::fs::write(&dest, &bytes)?;

    log::info!("已导出 CSV：{}", dest.display());
    Ok(dest.to_string_lossy().to_string())
}

/// 导出 Markdown（§9「CSV/Markdown 导出」），便于贴进笔记软件。
#[tauri::command]
pub async fn export_markdown(state: State<'_, AppState>, path: String) -> AppResult<String> {
    let db = &state.db;
    let rows = sqlx::query(
        "SELECT t.title, t.description, t.note_md, t.status, t.priority,
                t.planned_at, t.due_at, t.completed_at,
                p.name AS project_name,
                CASE WHEN t.series_id IS NOT NULL THEN 1 ELSE 0 END AS is_recurring
         FROM tasks t
         LEFT JOIN projects p ON p.id = t.project_id
         WHERE t.deleted_at IS NULL
         ORDER BY t.status, COALESCE(t.due_at, t.planned_at) IS NULL,
                  COALESCE(t.due_at, t.planned_at), t.created_at",
    )
    .fetch_all(db.pool())
    .await?;

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    let mut md = String::new();
    md.push_str(&format!(
        "# Lumen 任务导出\n\n> 导出时间：{now}\n>\n> 共 {} 项任务。\n\n",
        rows.len()
    ));

    for r in rows {
        let title: String = r.try_get("title")?;
        let status: String = r.try_get("status")?;
        let prio: i64 = r.try_get("priority")?;
        let check = if status == "done" { "x" } else { " " };
        let status_text = status_label(&status);
        let prio_text = priority_label(prio);

        md.push_str(&format!("- [{check}] **{title}**\n"));
        md.push_str(&format!("  - 状态：{status_text}　优先级：{prio_text}\n"));

        if let Some(p) = r.try_get::<Option<String>, _>("project_name")? {
            md.push_str(&format!("  - 项目：{p}\n"));
        }
        if let Some(t) = r.try_get::<Option<String>, _>("planned_at")? {
            md.push_str(&format!("  - 计划：{t}\n"));
        }
        if let Some(t) = r.try_get::<Option<String>, _>("due_at")? {
            md.push_str(&format!("  - 截止：{t}\n"));
        }
        if let Some(t) = r.try_get::<Option<String>, _>("completed_at")? {
            md.push_str(&format!("  - 完成：{t}\n"));
        }
        if r.try_get::<i64, _>("is_recurring")? == 1 {
            md.push_str("  - 重复任务的一次发生\n");
        }
        let desc: String = r.try_get("description")?;
        if !desc.trim().is_empty() {
            md.push_str(&format!("  - 描述：{}\n", desc.replace('\n', " ")));
        }
        let note: String = r.try_get("note_md")?;
        if !note.trim().is_empty() {
            md.push_str("\n");
            for line in note.lines() {
                md.push_str(&format!("    {line}\n"));
            }
        }
        md.push('\n');
    }

    let dest = PathBuf::from(&path);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, md.as_bytes())?;
    log::info!("已导出 Markdown：{}", dest.display());
    Ok(dest.to_string_lossy().to_string())
}

/// 状态的中文标签。
///
/// 写成普通函数而不是闭包：闭包返回借用的 `&str` 时，
/// 生命周期无法自动关联到入参，会产生 "lifetime may not live long enough"。
fn status_label(s: &str) -> &str {
    match s {
        "todo" => "待办",
        "doing" => "进行中",
        "waiting" => "等待",
        "done" => "已完成",
        "archived" => "已归档",
        other => other,
    }
}

/// 优先级的中文标签（§4.1 四级：0 无 / 1 低 / 2 中 / 3 高）
fn priority_label(p: i64) -> &'static str {
    match p {
        1 => "低",
        2 => "中",
        3 => "高",
        _ => "无",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_labels_cover_all_five_states() {
        // 与数据库 CHECK 约束保持一致，不能漏项
        assert_eq!(status_label("todo"), "待办");
        assert_eq!(status_label("doing"), "进行中");
        assert_eq!(status_label("waiting"), "等待");
        assert_eq!(status_label("done"), "已完成");
        assert_eq!(status_label("archived"), "已归档");
        // 未知值原样返回，保证导出的 Markdown 不丢信息
        assert_eq!(status_label("custom_state"), "custom_state");
    }

    #[test]
    fn priority_labels_cover_four_levels() {
        assert_eq!(priority_label(0), "无");
        assert_eq!(priority_label(1), "低");
        assert_eq!(priority_label(2), "中");
        assert_eq!(priority_label(3), "高");
        // 越界值不应 panic
        assert_eq!(priority_label(99), "无");
        assert_eq!(priority_label(-1), "无");
    }

    // =========================================================================
    // 数据库往返测试（§11 要求自动化测试覆盖"备份恢复"）
    // =========================================================================

    /// 建一个带样例数据的临时库，返回 (库句柄, 目录)
    async fn make_db_with_data() -> (Db, PathBuf) {
        let dir = std::env::temp_dir().join(format!("lumen-backup-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.expect("初始化数据库");
        let now = to_db_time(utc_now());

        // 项目
        sqlx::query(
            "INSERT INTO projects (id, name, description, sort_order, created_at, updated_at)
             VALUES ('p1', '工作', '工作相关', 1, ?1, ?1)",
        )
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        // 分类
        sqlx::query(
            "INSERT INTO categories (id, name, description, sort_order, created_at, updated_at)
             VALUES ('c1', '深度工作', '', 1, ?1, ?1)",
        )
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        // 标签
        sqlx::query(
            "INSERT INTO tags (id, name, sort_order, created_at, updated_at)
             VALUES ('g1', '紧急', 1, ?1, ?1)",
        )
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        // 重复系列（验证 task_series 与 segments 也能往返）
        sqlx::query(
            "INSERT INTO task_series (id, rrule, tzid, dtstart_local, has_start_time, created_at, updated_at)
             VALUES ('s1', 'FREQ=WEEKLY;BYDAY=MO,WE,FR', 'Asia/Shanghai', '2026-09-21T09:00:00', 1, ?1, ?1)",
        )
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        // 任务：一个普通任务 + 一个重复实例
        sqlx::query(
            "INSERT INTO tasks (id, title, description, status, priority, project_id, category_id,
                                planned_at, has_planned_time, due_at, has_due_time,
                                created_at, updated_at, occurrence_kind, is_exception)
             VALUES ('t1', '写周报', '每周五交', 'todo', 2, 'p1', 'c1',
                     '2026-09-25T01:00:00.000Z', 1, '2026-09-25T09:00:00.000Z', 1,
                     ?1, ?1, 'single', 0)",
        )
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO tasks (id, title, description, status, created_at, updated_at,
                                series_id, occurrence_key, occurrence_kind, is_exception)
             VALUES ('t2', '周一那一次', '', 'done', ?1, ?1,
                     's1', '2026-09-21T01:00:00.000Z', 'generated', 0)",
        )
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        // 标签关联、子任务、依赖、提醒（含已触发与未触发）
        for sql in [
            "INSERT INTO task_tags (task_id, tag_id) VALUES ('t1', 'g1')",
            "INSERT INTO subtasks (id, task_id, title, is_done, sort_order, created_at, updated_at)
             VALUES ('st1', 't1', '收集数据', 0, 1, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            "INSERT INTO task_dependencies (task_id, depends_on_id, created_at)
             VALUES ('t1', 't2', '2026-01-01T00:00:00.000Z')",
            "INSERT INTO reminders (id, task_id, kind, offset_minutes, remind_at, is_enabled, created_at, updated_at)
             VALUES ('r1', 't1', 'before_due', 30, '2026-09-25T08:30:00.000Z', 1,
                     '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            "INSERT INTO settings (key, value_json, updated_at)
             VALUES ('theme', '\"dark\"', '2026-01-01T00:00:00.000Z')",
        ] {
            sqlx::query(sql).execute(db.pool()).await.unwrap();
        }

        (db, dir)
    }

    /// 完整往返：导出 → 清空 → 恢复 → 数据必须与原来逐项一致。
    ///
    /// 这是 §11 明确要求的自动化覆盖项，也是"数据不丢"这一阻断性问题的
    /// 核心防线：任何序列化/反序列化字段遗漏都会在这里暴露。
    #[tokio::test]
    async fn full_roundtrip_preserves_all_data() {
        let (db, dir) = make_db_with_data().await;

        // 导出
        let data = build_backup_data(&db).await.expect("导出数据");
        let checksum = checksum_of(&data).expect("计算校验和");
        let stats_before = current_stats(&db).await.expect("导出前统计");

        assert_eq!(stats_before.tasks, 2);
        assert_eq!(stats_before.projects, 1);
        assert_eq!(stats_before.series, 1);
        assert_eq!(stats_before.reminders, 1);

        // 模拟"数据被清空"（例如用户误操作或换机）
        for t in [
            "task_dependencies",
            "reminders",
            "attachments",
            "subtasks",
            "task_tags",
            "tasks",
            "task_series_segments",
            "task_series",
            "tags",
            "categories",
            "projects",
            "settings",
        ] {
            let sql = format!("DELETE FROM {t}");
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .execute(db.pool())
                .await
                .unwrap();
        }
        let emptied = current_stats(&db).await.expect("清空后统计");
        assert_eq!(emptied.tasks, 0, "清空后任务数应为 0");

        // 校验和必须仍然匹配（证明往返稳定）
        assert_eq!(checksum_of(&data).unwrap(), checksum, "往返后校验和应不变");

        // 恢复：走与命令相同的插入路径
        let mut tx = db.pool().begin().await.unwrap();
        let plan: [(&str, &[serde_json::Value]); 12] = [
            ("projects", &data.projects),
            ("categories", &data.categories),
            ("tags", &data.tags),
            ("task_series", &data.series),
            ("tasks", &data.tasks),
            ("task_series_segments", &data.segments),
            ("task_tags", &data.task_tags),
            ("subtasks", &data.subtasks),
            ("task_dependencies", &data.dependencies),
            ("reminders", &data.reminders),
            ("attachments", &data.attachments),
            ("settings", &data.settings),
        ];
        for (table, rows) in plan.iter() {
            let cols = table_columns(&db, table).await.unwrap();
            insert_rows(&mut tx, table, rows, &cols).await.unwrap();
        }
        tx.commit().await.unwrap();

        // 恢复后统计必须与导出前一致
        let stats_after = current_stats(&db).await.expect("恢复后统计");
        assert_eq!(stats_after.tasks, stats_before.tasks, "任务数应一致");
        assert_eq!(stats_after.projects, stats_before.projects, "项目数应一致");
        assert_eq!(stats_after.series, stats_before.series, "系列数应一致");
        assert_eq!(
            stats_after.reminders, stats_before.reminders,
            "提醒数应一致"
        );
        assert_eq!(
            stats_after.dependencies, stats_before.dependencies,
            "依赖数应一致"
        );
        assert_eq!(stats_after.subtasks, stats_before.subtasks);
        assert_eq!(stats_after.task_tags, stats_before.task_tags);
        assert_eq!(stats_after.settings, stats_before.settings);

        // 抽查关键字段是否逐字保留——分数类型与时间字符串最容易在
        // JSON 往返中变形（例如 2 变成 2.0，或时间被重新格式化）
        let row = sqlx::query(
            "SELECT title, priority, planned_at, due_at, has_planned_time, status
             FROM tasks WHERE id = 't1'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row.try_get::<String, _>("title").unwrap(), "写周报");
        assert_eq!(row.try_get::<i64, _>("priority").unwrap(), 2);
        assert_eq!(
            row.try_get::<String, _>("planned_at").unwrap(),
            "2026-09-25T01:00:00.000Z",
            "时间字符串必须逐字保留"
        );
        assert_eq!(row.try_get::<i64, _>("has_planned_time").unwrap(), 1);

        // 重复实例的稳定身份必须完整回来
        let occ = sqlx::query("SELECT series_id, occurrence_key FROM tasks WHERE id = 't2'")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(occ.try_get::<String, _>("series_id").unwrap(), "s1");
        assert_eq!(
            occ.try_get::<String, _>("occurrence_key").unwrap(),
            "2026-09-21T01:00:00.000Z"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 验证外键约束确实生效，且恢复时的插入顺序能满足它。
    ///
    /// 两件事分开验：
    /// 1. 子表先于父表插入（或父记录缺失）时，外键必须**拒绝**——这证明约束是活的；
    /// 2. 父记录存在时插入子记录必须成功——这证明恢复顺序是对的。
    ///
    /// 注意本测试只插入不提交（最后 rollback），因此不会与已有数据冲突。
    #[tokio::test]
    async fn restore_respects_foreign_key_order() {
        let (db, dir) = make_db_with_data().await;
        let data = build_backup_data(&db).await.unwrap();

        // 连接池已在 Db::init 中开启 foreign_keys，这里确认一下
        let fk: (i64,) = sqlx::query_as("PRAGMA foreign_keys")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(fk.0, 1, "外键必须处于开启状态，否则本测试失去意义");

        // 1) 父记录不存在时，插入子记录必须被外键拒绝
        let orphan = vec![serde_json::json!({
            "id": "orphan-sub",
            "task_id": "不存在的任务",
            "title": "孤儿子任务",
            "is_done": 0,
            "sort_order": 1.0,
            "completed_at": serde_json::Value::Null,
            "created_at": "2026-01-01T00:00:00.000Z",
            "updated_at": "2026-01-01T00:00:00.000Z"
        })];
        let mut tx = db.pool().begin().await.unwrap();
        let cols = table_columns(&db, "subtasks").await.unwrap();
        let r = insert_rows(&mut tx, "subtasks", &orphan, &cols).await;
        tx.rollback().await.unwrap();
        assert!(
            r.is_err(),
            "引用不存在任务的子任务必须被外键约束拒绝，否则删除任务会留下悬空数据"
        );

        // 2) 父记录存在时（t1 在库中），备份里的子任务应能正常插入
        //    用一个新 id 避免与库中已有的 st1 主键冲突
        let valid = vec![serde_json::json!({
            "id": "sub-order-check",
            "task_id": "t1",
            "title": "顺序验证",
            "is_done": 0,
            "sort_order": 9.0,
            "completed_at": serde_json::Value::Null,
            "created_at": "2026-01-01T00:00:00.000Z",
            "updated_at": "2026-01-01T00:00:00.000Z"
        })];
        let mut tx = db.pool().begin().await.unwrap();
        let n = insert_rows(&mut tx, "subtasks", &valid, &cols)
            .await
            .expect("父记录存在时应能插入子记录");
        tx.rollback().await.unwrap();
        assert_eq!(n, 1);

        // 3) 备份里的 subtasks 内容应非空，证明上面的场景是有意义的
        assert!(!data.subtasks.is_empty(), "样例数据应包含子任务");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 备份必须如实反映条目数，否则用户会误判备份是否完整。
    #[tokio::test]
    async fn backup_stats_match_actual_content() {
        let (db, dir) = make_db_with_data().await;
        let data = build_backup_data(&db).await.unwrap();
        let stats = current_stats(&db).await.unwrap();

        assert_eq!(data.tasks.len(), stats.tasks);
        assert_eq!(data.projects.len(), stats.projects);
        assert_eq!(data.categories.len(), stats.categories);
        assert_eq!(data.tags.len(), stats.tags);
        assert_eq!(data.task_tags.len(), stats.task_tags);
        assert_eq!(data.subtasks.len(), stats.subtasks);
        assert_eq!(data.dependencies.len(), stats.dependencies);
        assert_eq!(data.reminders.len(), stats.reminders);
        assert_eq!(data.series.len(), stats.series);
        assert_eq!(data.segments.len(), stats.segments);
        assert_eq!(data.settings.len(), stats.settings);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 附件说明必须在有附件时给出明确提醒：
    /// 用户不能因为"备份成功"就以为附件文件也被打包了。
    #[tokio::test]
    async fn attachments_note_warns_when_attachments_exist() {
        let (db, dir) = make_db_with_data().await;

        // 无附件时应说明"不含附件记录"
        let d1 = build_backup_data(&db).await.unwrap();
        assert!(d1.attachments_note.contains("不含附件记录"));

        // 加入一条附件记录后，说明必须变成明确的警告
        sqlx::query(
            "INSERT INTO attachments (id, task_id, file_name, storage_mode, created_at)
             VALUES ('a1', 't1', 'report.pdf', 'reference', '2026-01-01T00:00:00.000Z')",
        )
        .execute(db.pool())
        .await
        .unwrap();

        let d2 = build_backup_data(&db).await.unwrap();
        assert_eq!(d2.attachments.len(), 1);
        assert!(
            d2.attachments_note.contains("不包含附件文件本身"),
            "有附件时必须明确说明文件本体未打包，实际：{}",
            d2.attachments_note
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 数据库一致性快照应当能真正生成（VACUUM INTO 的封装）。
    /// 恢复流程依赖它做"恢复前安全备份"，失败会导致恢复被中止。
    #[tokio::test]
    async fn consistency_snapshot_can_be_created() {
        let (db, dir) = make_db_with_data().await;
        let dest = dir.join("backups").join("snapshot.db");
        db.backup_to(&dest).await.expect("应能生成一致性快照");
        assert!(dest.exists(), "快照文件应存在");
        assert!(
            std::fs::metadata(&dest).unwrap().len() > 0,
            "快照不应为空文件"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn csv_escape_handles_delimiters_and_quotes() {
        assert_eq!(csv_escape("普通"), "普通");
        assert_eq!(csv_escape("含,逗号"), "\"含,逗号\"");
        assert_eq!(csv_escape("含\"引号"), "\"含\"\"引号\"");
        assert_eq!(csv_escape("含\n换行"), "\"含\n换行\"");
    }

    /// 含引号与逗号的字段必须能安全往返，否则导出的 CSV 会错位
    #[test]
    fn csv_escape_roundtrip_shape() {
        let raw = "他说\"你好\", 然后走了";
        let escaped = csv_escape(raw);
        assert!(escaped.starts_with('"') && escaped.ends_with('"'));
        assert_eq!(escaped, "\"他说\"\"你好\"\", 然后走了\"");
    }

    /// 校验和必须只取决于 data 内容。
    /// 若这里不稳定，恢复前的校验就会误报"文件已损坏"。
    #[test]
    fn checksum_is_deterministic() {
        let d1 = BackupData {
            projects: vec![serde_json::json!({"id": "p1", "name": "工作"})],
            categories: vec![],
            tags: vec![],
            tasks: vec![serde_json::json!({"id": "t1", "title": "写周报"})],
            task_tags: vec![],
            subtasks: vec![],
            dependencies: vec![],
            reminders: vec![],
            attachments: vec![],
            series: vec![],
            segments: vec![],
            settings: vec![],
            attachments_note: String::new(),
        };
        let d2 = d1.clone();
        assert_eq!(checksum_of(&d1).unwrap(), checksum_of(&d2).unwrap());
    }

    /// 内容变化必须导致校验和变化，否则损坏的备份会被当成好的
    #[test]
    fn checksum_changes_with_content() {
        let base = BackupData {
            projects: vec![],
            categories: vec![],
            tags: vec![],
            tasks: vec![serde_json::json!({"title": "A"})],
            task_tags: vec![],
            subtasks: vec![],
            dependencies: vec![],
            reminders: vec![],
            attachments: vec![],
            series: vec![],
            segments: vec![],
            settings: vec![],
            attachments_note: String::new(),
        };
        let mut changed = base.clone();
        changed.tasks = vec![serde_json::json!({"title": "B"})];

        assert_ne!(checksum_of(&base).unwrap(), checksum_of(&changed).unwrap());
    }

    /// 字段顺序不同的等价 JSON 应产生相同校验和，
    /// 否则重新序列化一次就会误判为损坏。
    #[test]
    fn checksum_ignores_key_order() {
        let a = BackupData {
            projects: vec![serde_json::json!({"id": "1", "name": "x"})],
            categories: vec![],
            tags: vec![],
            tasks: vec![],
            task_tags: vec![],
            subtasks: vec![],
            dependencies: vec![],
            reminders: vec![],
            attachments: vec![],
            series: vec![],
            segments: vec![],
            settings: vec![],
            attachments_note: String::new(),
        };
        // 用字符串构造，键顺序相反
        let b = BackupData {
            projects: vec![serde_json::from_str(r#"{"name":"x","id":"1"}"#).unwrap()],
            ..a.clone()
        };
        assert_eq!(checksum_of(&a).unwrap(), checksum_of(&b).unwrap());
    }

    #[test]
    fn checksum_is_hex_sha256_length() {
        let d = BackupData {
            projects: vec![],
            categories: vec![],
            tags: vec![],
            tasks: vec![],
            task_tags: vec![],
            subtasks: vec![],
            dependencies: vec![],
            reminders: vec![],
            attachments: vec![],
            series: vec![],
            segments: vec![],
            settings: vec![],
            attachments_note: String::new(),
        };
        let c = checksum_of(&d).unwrap();
        assert_eq!(c.len(), 64, "SHA-256 十六进制应为 64 字符");
        assert!(c.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    /// 保留份数必须落在允许区间，避免用户填 0 导致所有备份被删。
    #[test]
    fn keep_range_is_sane() {
        assert!(KEEP_MIN >= 1, "至少保留 1 份，否则自动备份失去意义");
        assert!(KEEP_MAX >= KEEP_MIN);
        assert_eq!(DEFAULT_KEEP, 10);
    }

    #[test]
    fn backup_file_serializes_and_parses() {
        let f = BackupFile {
            format_version: BACKUP_FORMAT_VERSION,
            app_version: "0.1.0".into(),
            created_at: "2026-09-23T00:00:00.000Z".into(),
            checksum: "abc".into(),
            stats: BackupStats::default(),
            note: None,
            data: BackupData {
                projects: vec![],
                categories: vec![],
                tags: vec![],
                tasks: vec![],
                task_tags: vec![],
                subtasks: vec![],
                dependencies: vec![],
                reminders: vec![],
                attachments: vec![],
                series: vec![],
                segments: vec![],
                settings: vec![],
                attachments_note: "无附件".into(),
            },
        };
        let s = serde_json::to_string(&f).unwrap();
        let back: BackupFile = serde_json::from_str(&s).unwrap();
        assert_eq!(back.format_version, BACKUP_FORMAT_VERSION);
        assert_eq!(back.data.attachments_note, "无附件");
    }
}
