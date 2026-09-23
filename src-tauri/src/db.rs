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
use sqlx::{Pool, Sqlite};

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
    pub async fn init(data_dir: impl AsRef<Path>) -> Result<Self, DbError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir)?;

        let db_path = data_dir.join("lumen.db");

        // 迁移前备份：任务书 §9「数据库写入使用事务、迁移有备份」
        if db_path.exists() {
            let backup = data_dir
                .join("backups")
                .join(format!("pre-migrate-{}.db", now_stamp()));
            if let Some(parent) = backup.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // 备份失败不应阻止启动，但要留下日志（调用方负责记录）
            if let Err(e) = std::fs::copy(&db_path, &backup) {
                eprintln!("[lumen] 迁移前备份失败（继续启动）: {e}");
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

    /// 迁移必须能在空库上跑通，且重复执行幂等。
    #[tokio::test]
    async fn migrations_apply_and_are_idempotent() {
        let dir = std::env::temp_dir().join(format!("lumen-test-{}", uuid::Uuid::now_v7()));
        let db = Db::init(&dir).await.expect("首次初始化应成功");
        let db2 = Db::init(&dir).await.expect("重复初始化应成功（迁移幂等）");

        // 关键表都应存在
        let tables: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
        )
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
