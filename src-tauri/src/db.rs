//! 数据库层：连接池、迁移、事务与错误类型。
//!
//! 设计要点（任务书 §2.4 / §5 / §9 / §10）：
//! - 使用 SQLite + WAL，保证"意外断电不可留下半条规则"（事务包裹写操作）。
//! - 迁移采用 sqlx 嵌入式迁移（`migrations/` 目录），随二进制分发，
//!   升级时自动执行，并在执行前对数据库文件做备份。
//! - 全部时间戳以 UTC ISO-8601 TEXT 存储；本地时区仅在展示层使用。

use std::path::{Path, PathBuf};
use std::str::FromStr;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Connection, Pool, Sqlite, SqliteConnection};

/// 迁移前备份的保留份数（§4.5：不无限增长，默认保留最近若干份）
pub const PRE_MIGRATE_KEEP: usize = 5;

/// 数据库层错误。
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// 底层 sqlx 错误
    #[error("数据库操作失败: {0}")]
    Sqlx(#[from] sqlx::Error),

    /// 迁移失败
    #[error("数据库迁移失败: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    /// 文件系统错误（建目录、备份等）
    #[error("文件操作失败: {0}")]
    Io(#[from] std::io::Error),

    /// 迁移前的安全备份没做成——按 §4.5 的策略，**中止升级**而不是冒险继续
    #[error("迁移前的安全备份失败，已中止升级以免丢数据：{0}")]
    PreMigrationBackup(String),
}

/// 便于在 Tauri command 中直接转成可读中文错误。
impl From<DbError> for String {
    fn from(e: DbError) -> Self {
        e.to_string()
    }
}

/// 对上层暴露的连接池句柄。
#[derive(Clone)]
pub struct Db {
    pool: Pool<Sqlite>,
    /// 数据库文件所在目录，附件与备份也放在其下
    data_dir: PathBuf,
}

impl Db {
    /// 连接池句柄（只读访问）
    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    /// 应用数据目录
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// 数据库文件完整路径
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("lumen.db")
    }

    /// 初始化：确保目录存在 → 打开连接池 → 执行迁移。
    ///
    /// `data_dir` 由 Tauri 的 `app_data_dir()` 提供（Windows 下为
    /// `%APPDATA%\com.pla0185.lumen`），刻意放在安装目录之外，
    /// 这样 NSIS 卸载程序的"删除应用数据"复选框才能统一管理（§10）。
    ///
    /// ## 迁移前备份的顺序（整改任务书 §4）
    ///
    /// 关键点：**先判断有没有待执行的迁移**，再决定要不要备份。
    ///
    /// - 有旧库 + 有待执行迁移 → 必须做一次**一致性快照**；做不出来就中止升级
    ///   （宁可这次不升，也不要在没有退路的情况下改结构）。
    /// - 没有待执行迁移 → 根本不需要备份，也就不会因为"磁盘满 / 文件被占用"
    ///   这类与迁移无关的原因把应用挡在门外。
    ///
    /// 备份与判断都在**正式的连接池建立之前**用一个临时连接完成，
    /// 且不会执行任何迁移。
    pub async fn init(data_dir: impl AsRef<Path>) -> Result<Self, DbError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir)?;

        let db_path = data_dir.join("lumen.db");

        if db_path.exists() {
            let pending = pending_migrations(&db_path).await.unwrap_or_else(|e| {
                // 读不出迁移状态时保守处理：当作"有待执行迁移"，
                // 这样至少不会在没备份的情况下贸然升级。
                log::warn!("读取迁移状态失败（将按有待执行迁移处理）：{e}");
                vec!["unknown".to_string()]
            });

            if !pending.is_empty() {
                let backup_dir = data_dir.join("backups");
                match pre_migration_snapshot(&db_path, &backup_dir).await {
                    Ok(path) => {
                        log::info!(
                            "检测到 {} 个待执行迁移，已生成迁移前一致性备份：{}",
                            pending.len(),
                            path.display()
                        );
                        // 清理旧备份：失败只记日志，绝不能因此挡住启动（§4.5）
                        if let Err(e) = prune_pre_migrate_backups(&backup_dir, PRE_MIGRATE_KEEP) {
                            log::warn!("清理旧的迁移前备份失败（忽略）：{e}");
                        }
                    }
                    Err(e) => {
                        log::error!("迁移前一致性备份失败：{e}");
                        return Err(DbError::PreMigrationBackup(e));
                    }
                }
            }
        }

        // 用 URL 形式连接，显式开启 WAL 与外键。
        // 注意：Windows 路径含反斜杠，URL 中需统一为正斜杠。
        let url_path = db_path.to_string_lossy().replace('\\', "/");
        let opts = SqliteConnectOptions::from_str(&format!("sqlite://{url_path}"))
            .map_err(DbError::Sqlx)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(std::time::Duration::from_secs(10));

        let pool = SqlitePoolOptions::new()
            // SQLite 单写入者模型：连接数不必多，避免写锁争抢
            .max_connections(5)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect_with(opts)
            .await?;

        // 嵌入式迁移：编译期把 migrations/ 打进二进制（§2.4 迁移机制）
        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { pool, data_dir })
    }

    /// 在单个事务中执行写操作。
    ///
    /// 任务书 §9 要求"不可留下半条规则"——所有跨表写入必须走这里。
    pub async fn with_tx<T, F, Fut>(&self, f: F) -> Result<T, DbError>
    where
        F: FnOnce(sqlx::Transaction<'static, Sqlite>) -> Fut,
        Fut: std::future::Future<Output = Result<(sqlx::Transaction<'static, Sqlite>, T), DbError>>,
    {
        let tx = self.pool.begin().await?;
        let (tx, out) = f(tx).await?;
        tx.commit().await?;
        Ok(out)
    }

    /// 生成数据库文件的一致性备份（供 §9 手动/自动备份复用）。
    pub async fn backup_to(&self, dest: &Path) -> Result<(), DbError> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // VACUUM INTO 产出的是完整且一致的单文件快照，
        // 比直接复制 .db 更安全（WAL 中未落盘的内容也会包含）。
        //
        // 关于 SQL 安全：`VACUUM INTO` 不接受绑定参数，路径只能内联进语句。
        // sqlx 0.9 新增了静态 SQL 审计（`SqlSafeStr`），要求动态 SQL 必须显式
        // 声明已审计——这正是任务书 §10「安全处理」想要的效果，故这里
        // 既做转义又显式标注，而不是绕过检查。
        let dest_str = dest.to_string_lossy().replace('\'', "''");
        // 纯 ASCII 且不含引号与空字节时也可安全内联（额外防御）
        debug_assert!(!dest_str.contains('\0'), "备份路径不得包含空字节");
        let sql = format!("VACUUM INTO '{dest_str}'");
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// UTC 时间戳，形如 `20260923T111530Z`，用于文件名。
pub fn now_stamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// 打开一个**不启用 WAL、不执行迁移**的临时连接。
///
/// 为什么要单独一个函数：迁移前的备份必须发生在正式连接池建立之前，
/// 而且这个连接只做"读 + VACUUM INTO"，绝不能顺手把库改了。
async fn open_probe_connection(db_path: &Path) -> Result<SqliteConnection, DbError> {
    let url_path = db_path.to_string_lossy().replace('\\', "/");
    // 注意这里**不设置** journal_mode：设置它会写库（切换日志模式），
    // 而我们要的是"只读地看一眼 + 导出一份快照"。
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{url_path}"))
        .map_err(DbError::Sqlx)?
        .create_if_missing(false)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(10));
    Ok(SqliteConnection::connect_with(&opts).await?)
}

/// 列出**尚未执行**的迁移版本号。
///
/// 读 `_sqlx_migrations` 表即可；表不存在说明这是全新库（没有"旧数据"，
/// 也就不需要备份）。用 `sqlx::migrate!()` 的同一份迁移列表做比较，
/// 避免两处维护两套版本号。
pub async fn pending_migrations(db_path: &Path) -> Result<Vec<String>, DbError> {
    let mut conn = open_probe_connection(db_path).await?;

    let applied: Vec<i64> =
        match sqlx::query_scalar::<_, i64>("SELECT version FROM _sqlx_migrations")
            .fetch_all(&mut conn)
            .await
        {
            Ok(v) => v,
            // 表不存在 = 全新库（或不是 sqlx 建的库）：当作"没有旧数据要保护"
            Err(sqlx::Error::Database(e)) if e.message().contains("no such table") => Vec::new(),
            Err(e) => {
                let _ = conn.close().await;
                return Err(DbError::Sqlx(e));
            }
        };
    let _ = conn.close().await;

    let mut pending: Vec<String> = Vec::new();
    for m in sqlx::migrate!("./migrations").iter() {
        if !applied.contains(&m.version) {
            pending.push(m.version.to_string());
        }
    }
    Ok(pending)
}

/// 生成迁移前的**一致性快照**（整改任务书 §4）。
///
/// ## 为什么不能只复制 `.db`
///
/// 库跑在 WAL 模式下，最新提交可能还在 `lumen.db-wal` 里没 checkpoint 进主库。
/// 只 copy `.db` 会得到一个"看起来是完整数据库、其实缺了最近操作"的快照——
/// 一旦迁移失败再从这份备份恢复，用户就会丢数据。这是数据安全问题。
///
/// ## 方案
///
/// 1. 首选 `VACUUM INTO`：把**包含 WAL 内容**的一致快照写成单文件，
///    且**完全不修改源库**（这是 SQLite 官方推荐的备份方式之一）。
/// 2. 万一 `VACUUM INTO` 走不通（例如目标盘空间不足、SQLite 版本限制），
///    退回到"先 `wal_checkpoint(TRUNCATE)` 再复制主库文件"：
///    checkpoint 会把 WAL 内容并入主库，之后复制就是完整的。
///    这一步会写源库，但只做持久化、不改任何用户数据，属于可接受的降级路径。
///
/// 两条路都失败才返回错误；此时调用方应当**中止迁移**。
pub async fn pre_migration_snapshot(db_path: &Path, backup_dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(backup_dir).map_err(|e| format!("创建备份目录失败：{e}"))?;
    let dest = backup_dir.join(format!("pre-migrate-{}.db", now_stamp()));

    // ---- 首选：VACUUM INTO（不改源库，天然包含 WAL 内容）----
    match vacuum_into(db_path, &dest).await {
        Ok(()) => return Ok(dest),
        Err(e) => {
            log::warn!("VACUUM INTO 备份失败，改用 checkpoint + 复制：{e}");
            let _ = std::fs::remove_file(&dest);
        }
    }

    // ---- 降级：checkpoint 后再复制 ----
    checkpoint_truncate(db_path)
        .await
        .map_err(|e| format!("checkpoint 失败：{e}"))?;
    std::fs::copy(db_path, &dest).map_err(|e| format!("复制数据库文件失败：{e}"))?;

    // 复制完成后校验一次完整性：宁可现在发现备份是坏的，
    // 也不要在用户真正需要恢复时才发现。
    verify_backup(&dest)
        .await
        .map_err(|e| format!("备份完整性校验失败：{e}"))?;
    Ok(dest)
}

/// `VACUUM INTO`：产出一份一致快照，**不修改源库**。
async fn vacuum_into(db_path: &Path, dest: &Path) -> Result<(), DbError> {
    let mut conn = open_probe_connection(db_path).await?;
    // `VACUUM INTO` 不接受绑定参数，路径只能内联；按 sqlx 0.9 的静态审计要求
    // 显式标注已审计（与 `backup_to` 同一处理）。
    let dest_str = dest.to_string_lossy().replace('\'', "''");
    let sql = format!("VACUUM INTO '{dest_str}'");
    let res = sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(&mut conn)
        .await;
    let _ = conn.close().await;
    res?;
    Ok(())
}

/// 把 WAL 内容并入主库文件（降级路径用）。
async fn checkpoint_truncate(db_path: &Path) -> Result<(), DbError> {
    let mut conn = open_probe_connection(db_path).await?;
    let res = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&mut conn)
        .await;
    let _ = conn.close().await;
    res?;
    Ok(())
}

/// 打开备份文件跑一次 `PRAGMA integrity_check`。
pub async fn verify_backup(path: &Path) -> Result<(), DbError> {
    let mut conn = open_probe_connection(path).await?;
    let result: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut conn)
        .await?;
    let _ = conn.close().await;
    if result == "ok" {
        Ok(())
    } else {
        Err(DbError::PreMigrationBackup(format!(
            "integrity_check 返回：{result}"
        )))
    }
}

/// 清理旧的迁移前备份，只保留最近 `keep` 份。
///
/// 失败一律只记日志：清理是维护动作，不能让程序起不来。
pub fn prune_pre_migrate_backups(backup_dir: &Path, keep: usize) -> Result<usize, std::io::Error> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(backup_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("pre-migrate-") && n.ends_with(".db"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    if files.len() <= keep {
        return Ok(0);
    }
    // 文件名里的时间戳是 `%Y%m%dT%H%M%SZ`，字典序即时间序
    files.sort();
    let mut removed = 0;
    for old in &files[..files.len() - keep] {
        match std::fs::remove_file(old) {
            Ok(()) => removed += 1,
            Err(e) => log::warn!("删除旧备份 {} 失败（忽略）：{e}", old.display()),
        }
    }
    Ok(removed)
}

/// 规范化时间戳：统一转为 UTC ISO-8601 毫秒精度字符串。
///
/// 全库时间字段都经此函数落库，保证排序与比较语义一致（§4.3）。
pub fn to_db_time(dt: chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// 当前时间（UTC），统一入口便于测试替换。
pub fn utc_now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 整改任务书 §4.6：迁移前备份必须包含**还留在 WAL 里**的最新数据。
    ///
    /// 这个测试刻意做成**对照实验**：
    /// 1. 先证明"只复制 lumen.db"这种老做法确实会丢掉 WAL 中的数据；
    /// 2. 再证明新的快照流程把这条数据保住了。
    ///
    /// 只有第 1 步成立，第 2 步才有意义——否则测不出任何东西。
    #[tokio::test]
    async fn pre_migration_snapshot_includes_uncheckpointed_wal_data() {
        let dir = std::env::temp_dir().join(format!("lumen-wal-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.expect("初始化数据库");
        let db_path = db.db_path();

        // ---- 制造"数据只在 WAL 里"的局面 ----
        // 关掉自动 checkpoint，并**保持这个连接不关闭**：
        // 一旦所有连接关闭，SQLite 会把 WAL 并回主库，就模拟不出这个场景了。
        let mut writer = open_probe_connection(&db_path).await.unwrap();
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&mut writer)
            .await
            .unwrap();
        sqlx::query("PRAGMA wal_autocheckpoint=0")
            .execute(&mut writer)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO tasks (id, title, status, priority, created_at, updated_at, sort_order,
                                is_pinned, is_favorite, has_planned_time, has_due_time,
                                actual_minutes, occurrence_kind, is_exception, period_type)
             VALUES ('wal-only-task', '只在 WAL 里的任务', 'todo', 0,
                     '2026-09-23T02:00:00.000Z', '2026-09-23T02:00:00.000Z', 1,
                     0, 0, 0, 0, 0, 'single', 0, 'none')",
        )
        .execute(&mut writer)
        .await
        .expect("写入任务（数据留在 WAL）");

        // WAL 文件应当确实存在且非空
        let wal = dir.join("lumen.db-wal");
        assert!(wal.exists(), "WAL 文件应存在");
        assert!(wal.metadata().unwrap().len() > 0, "WAL 文件应非空");

        // ---- 对照组：老做法（只复制 .db）----
        let naive = dir.join("naive-copy.db");
        std::fs::copy(&db_path, &naive).unwrap();
        let mut c = open_probe_connection(&naive).await.unwrap();
        let naive_count: i64 = match sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM tasks WHERE id = 'wal-only-task'",
        )
        .fetch_one(&mut c)
        .await
        {
            Ok(n) => n,
            // 连表都读不到，说明主库文件里连迁移结果都还没有——
            // 这比"少一条数据"更严重，同样算对照组失败。
            Err(sqlx::Error::Database(e)) if e.message().contains("no such table") => {
                log::info!("对照组：只复制 .db 得到的文件里连 tasks 表都不存在");
                0
            }
            Err(e) => panic!("对照组查询失败：{e}"),
        };
        let _ = c.close().await;
        assert_eq!(
            naive_count, 0,
            "对照组说明：只复制 .db 确实丢掉了最新数据（这正是要修的问题）"
        );

        // ---- 实验组：新的迁移前快照 ----
        let backup_dir = dir.join("backups");
        let snapshot = pre_migration_snapshot(&db_path, &backup_dir)
            .await
            .expect("生成迁移前快照");

        let mut c = open_probe_connection(&snapshot).await.unwrap();
        let snap_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE id = 'wal-only-task'")
                .fetch_one(&mut c)
                .await
                .unwrap();
        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&mut c)
            .await
            .unwrap();
        let _ = c.close().await;

        assert_eq!(snap_count, 1, "快照必须包含 WAL 中的最新数据");
        assert_eq!(integrity, "ok", "快照自身必须通过完整性检查");

        // 源库不能被这次备份破坏
        let _ = writer.close().await;
        let mut c = open_probe_connection(&db_path).await.unwrap();
        let live: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE id = 'wal-only-task'")
            .fetch_one(&mut c)
            .await
            .unwrap();
        let _ = c.close().await;
        assert_eq!(live, 1, "备份过程不得破坏源库");

        let _ = db.pool().close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 迁移前备份要能保留最近 N 份、删掉更老的（§4.5）
    #[test]
    fn prune_keeps_recent_pre_migrate_backups_only() {
        let dir = std::env::temp_dir().join(format!("lumen-prune-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        // 手动备份不该被清理
        std::fs::write(dir.join("manual-20260101T000000Z.db"), b"x").unwrap();
        for stamp in [
            "20260101T000000Z",
            "20260102T000000Z",
            "20260103T000000Z",
            "20260104T000000Z",
        ] {
            std::fs::write(dir.join(format!("pre-migrate-{stamp}.db")), b"x").unwrap();
        }

        let removed = prune_pre_migrate_backups(&dir, 2).unwrap();
        assert_eq!(removed, 2, "4 份里保留 2 份，应删掉 2 份");

        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        left.sort();
        assert_eq!(
            left,
            vec![
                "manual-20260101T000000Z.db",
                "pre-migrate-20260103T000000Z.db",
                "pre-migrate-20260104T000000Z.db",
            ],
            "应保留最新的两份，且不碰手动备份"
        );

        // 目录不存在时不应报错（首次启动就会遇到）
        assert_eq!(prune_pre_migrate_backups(&dir.join("nope"), 2).unwrap(), 0);

        let _ = std::fs::remove_dir_all(dir);
    }

    /// 没有待执行迁移时不该产生备份文件（否则每次启动都写一份，纯属浪费）
    #[tokio::test]
    async fn no_pending_migrations_after_init() {
        let dir = std::env::temp_dir().join(format!("lumen-pending-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.expect("初始化");
        let db_path = db.db_path();

        let pending = pending_migrations(&db_path).await.unwrap();
        assert!(
            pending.is_empty(),
            "刚初始化完不应还有待执行迁移：{pending:?}"
        );

        // 再初始化一次：不应生成新的 pre-migrate 备份
        let _db2 = Db::init(&dir).await.expect("再次初始化");
        let backups = dir.join("backups");
        let n = if backups.exists() {
            std::fs::read_dir(&backups).unwrap().count()
        } else {
            0
        };
        assert_eq!(n, 0, "没有待执行迁移时不应写备份文件");

        let _ = db.pool().close().await;
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 迁移必须能在空库上跑通，且重复执行幂等。
    #[tokio::test]
    async fn migrations_apply_and_are_idempotent() {
        let dir = std::env::temp_dir().join(format!("lumen-test-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.expect("首次初始化应成功");
        let db2 = Db::init(&dir).await.expect("重复初始化应成功（迁移幂等）");

        // 关键表都应存在
        let tables: Vec<(String,)> =
            sqlx::query_as("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
                .fetch_all(db2.pool())
                .await
                .unwrap();
        let names: Vec<String> = tables.into_iter().map(|(n,)| n).collect();
        for t in [
            "tasks",
            "task_series",
            "task_series_segments",
            "reminders",
            "attachments",
            "subtasks",
            "task_dependencies",
            "tags",
            "projects",
            "categories",
            "settings",
            "focus_sessions",
        ] {
            assert!(names.iter().any(|n| n == t), "缺少表 {t}，实际: {names:?}");
        }

        // 外键约束应处于开启状态
        let fk: (i64,) = sqlx::query_as("PRAGMA foreign_keys")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(fk.0, 1, "外键必须开启");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// §5 核心不变式：同一系列的同一原始发生时刻不得重复插入。
    #[tokio::test]
    async fn occurrence_identity_is_unique_but_survives_reschedule() {
        let dir = std::env::temp_dir().join(format!("lumen-occ-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.expect("初始化");
        let pool = db.pool();

        let series_id = uuid::Uuid::now_v7().to_string();
        let now = to_db_time(utc_now());

        sqlx::query(
            "INSERT INTO task_series (id, rrule, tzid, dtstart_local, has_start_time, created_at, updated_at)
             VALUES (?1, 'FREQ=WEEKLY;BYDAY=MO,WE,FR', 'Asia/Shanghai', '2026-09-21T09:00:00', 1, ?2, ?2)",
        )
        .bind(&series_id)
        .bind(&now)
        .execute(pool)
        .await
        .unwrap();

        let task_id = uuid::Uuid::now_v7().to_string();
        let occ = "2026-09-23T01:00:00.000Z"; // 周三那次（UTC）
        sqlx::query(
            "INSERT INTO tasks (id, title, created_at, updated_at, series_id, occurrence_key, occurrence_kind)
             VALUES (?1, '周三那一次', ?2, ?2, ?3, ?4, 'generated')",
        )
        .bind(&task_id)
        .bind(&now)
        .bind(&series_id)
        .bind(occ)
        .execute(pool)
        .await
        .unwrap();

        // 同系列同 occurrence_key 再插一次必须失败——防止"旧日期又生成一个副本"
        let dup = sqlx::query(
            "INSERT INTO tasks (id, title, created_at, updated_at, series_id, occurrence_key, occurrence_kind)
             VALUES (?1, '重复副本', ?2, ?2, ?3, ?4, 'generated')",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(&now)
        .bind(&series_id)
        .bind(occ)
        .execute(pool)
        .await;
        assert!(dup.is_err(), "同系列同 occurrence_key 必须被唯一索引拒绝");

        // 但改期（改 planned_at）不改变 occurrence_key，仍是"同一次"
        sqlx::query("UPDATE tasks SET planned_at = ?1, has_planned_time = 1 WHERE id = ?2")
            .bind("2026-09-24T02:00:00.000Z")
            .bind(&task_id)
            .execute(pool)
            .await
            .unwrap();
        let kept: (String,) = sqlx::query_as("SELECT occurrence_key FROM tasks WHERE id = ?1")
            .bind(&task_id)
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(kept.0, occ, "改期后 occurrence_key 不得改变");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 循环依赖必须由应用层拒绝；这里验证自依赖被数据库 CHECK 拦住。
    #[tokio::test]
    async fn self_dependency_is_rejected() {
        let dir = std::env::temp_dir().join(format!("lumen-dep-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.expect("初始化");
        let now = to_db_time(utc_now());
        let id = uuid::Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO tasks (id, title, created_at, updated_at) VALUES (?1, 'A', ?2, ?2)",
        )
        .bind(&id)
        .bind(&now)
        .execute(db.pool())
        .await
        .unwrap();

        let r = sqlx::query(
            "INSERT INTO task_dependencies (task_id, depends_on_id, created_at) VALUES (?1, ?1, ?2)",
        )
        .bind(&id)
        .bind(&now)
        .execute(db.pool())
        .await;
        assert!(r.is_err(), "自依赖必须被 CHECK 约束拒绝");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
