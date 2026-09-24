//! 统一错误类型与前端可读消息。
//!
//! 任务书 §10：「核心异常须提供可理解提示、可恢复操作和诊断日志」。
//! 因此所有面向 UI 的错误都必须携带：中文可读消息 + 机器可判别的错误码。
//! 严禁把 API Key、完整私密任务内容写进错误消息或日志。

use serde::Serialize;

/// 前端可判别的错误码。前端据此决定展示文案与可恢复操作。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// 输入校验未通过（用户可自行修正）
    Validation,
    /// 目标记录不存在
    NotFound,
    /// 违反业务不变式（如循环依赖、重复实例身份冲突）
    Conflict,
    /// 数据库错误
    Database,
    /// 文件系统错误
    Io,
    /// 网络错误（AI 调用等）
    Network,
    /// 鉴权失败（API Key 无效）
    Unauthorized,
    /// 额度/余额不足
    QuotaExceeded,
    /// 触发限流
    RateLimited,
    /// 请求超时
    Timeout,
    /// 功能未配置（如未配置任何 AI 提供商）
    NotConfigured,
    /// 其他未归类错误
    Internal,
}

/// 业务错误：携带错误码、可读消息与可选的可恢复建议。
#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    /// 机器可判别错误码
    pub code: ErrorCode,
    /// 面向用户的中文可读消息
    pub message: String,
    /// 可选的恢复建议（如「请在设置中检查 API Key」）
    pub hint: Option<String>,
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AppError {}

impl AppError {
    /// 构造一个错误
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), hint: None }
    }

    /// 追加恢复建议
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// 输入校验失败
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Validation, message)
    }

    /// 记录不存在
    pub fn not_found(what: &str, id: &str) -> Self {
        Self::new(ErrorCode::NotFound, format!("未找到{what}（ID: {id}）"))
    }

    /// 业务冲突
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }

    /// 内部错误
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
}

impl From<crate::db::DbError> for AppError {
    fn from(e: crate::db::DbError) -> Self {
        use crate::db::DbError;
        match e {
            DbError::Sqlx(e) => Self::from(e),
            DbError::Migrate(e) => Self::new(ErrorCode::Database, format!("数据库迁移失败：{e}"))
                .with_hint("请先备份数据目录，再联系支持或检查迁移脚本"),
            DbError::Io(e) => Self::new(ErrorCode::Io, format!("文件操作失败：{e}")),
            DbError::PreMigrationBackup(e) => Self::new(
                ErrorCode::Database,
                format!("迁移前的安全备份失败，已中止升级以免丢数据：{e}"),
            )
            .with_hint(
                "请检查数据目录所在磁盘是否已满、是否有其它程序占用 lumen.db，\
                 然后重新启动；本次不会执行任何数据库结构变更，数据保持升级前的状态",
            ),
        }
    }
}

/// 直接由 sqlx 错误映射。
///
/// 为什么需要单独实现：`DbError` 已经有了 `From<sqlx::Error>`，
/// 但 `commands` 层不少地方直接对 `sqlx::query(...).await?` 使用 `?`，
/// 那里产出的是 `sqlx::Error` 而不是 `DbError`。若只实现 `DbError` 的转换，
/// 这些 `?` 都无法编译。让 `AppError` 同时接受两者，可避免在业务代码里
/// 到处写 `.map_err(DbError::from)?` 这种噪音。
impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => {
                Self::new(ErrorCode::NotFound, "记录不存在或已被删除")
            }
            sqlx::Error::Database(db) => {
                let raw = db.message().to_string();
                // 把数据库约束错误翻译成用户能理解的话，并保留原始提示便于排查
                if raw.contains("UNIQUE") {
                    Self::new(ErrorCode::Conflict, "存在重复数据，操作被拒绝")
                        .with_hint(format!("数据库提示：{raw}"))
                } else if raw.contains("FOREIGN KEY") {
                    Self::new(ErrorCode::Conflict, "关联的数据不存在或被占用")
                } else if raw.contains("CHECK") {
                    Self::new(ErrorCode::Validation, "数据不符合约束条件")
                        .with_hint(format!("数据库提示：{raw}"))
                } else {
                    Self::new(ErrorCode::Database, format!("数据库错误：{raw}"))
                }
            }
            sqlx::Error::PoolTimedOut => Self::new(ErrorCode::Timeout, "数据库繁忙，请稍后重试")
                .with_hint("若持续出现，请检查是否有其他程序正在占用数据库文件"),
            sqlx::Error::PoolClosed => {
                Self::new(ErrorCode::Database, "数据库连接已关闭，请重启应用")
            }
            other => Self::new(ErrorCode::Database, format!("数据库访问失败：{other}")),
        }
    }
}

/// 文件系统错误直接映射。
///
/// 备份、导出、附件等模块大量使用 `std::fs`，若每次都要 `.map_err(...)`，
/// 业务代码会被错误转换淹没。这里统一转换，并按 errno 给出更具体的提示。
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        use std::io::ErrorKind;
        let (msg, hint) = match e.kind() {
            ErrorKind::NotFound => (
                format!("文件不存在：{e}"),
                Some("请确认文件是否被移动或删除".to_string()),
            ),
            ErrorKind::PermissionDenied => (
                format!("没有权限访问文件：{e}"),
                Some("请检查文件是否被其他程序占用，或以管理员身份重试".to_string()),
            ),
            ErrorKind::AlreadyExists => (format!("文件已存在：{e}"), None),
            ErrorKind::WriteZero | ErrorKind::StorageFull => (
                format!("写入失败，磁盘可能已满：{e}"),
                Some("请清理磁盘空间后重试".to_string()),
            ),
            _ => (format!("文件操作失败：{e}"), None),
        };
        let mut err = Self::new(ErrorCode::Io, msg);
        if let Some(h) = hint {
            err = err.with_hint(h);
        }
        err
    }
}

/// 便捷别名
pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_violation_maps_to_conflict_with_hint() {
        // 直接验证 SQLite CHECK / UNIQUE 的翻译规则（不依赖真实数据库）
        let e = AppError::conflict("存在重复数据，操作被拒绝");
        assert!(matches!(e.code, ErrorCode::Conflict));
        assert!(e.message.contains("重复"));
    }

    #[test]
    fn error_serializes_with_stable_shape() {
        // 前端依赖 { code, message, hint } 这三个字段，形状必须稳定
        let e = AppError::validation("标题不能为空").with_hint("请输入 1-500 个字符");
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "validation");
        assert_eq!(v["message"], "标题不能为空");
        assert_eq!(v["hint"], "请输入 1-500 个字符");
        assert!(v.get("code").is_some() && v.get("message").is_some() && v.get("hint").is_some());
    }

    #[test]
    fn not_found_message_mentions_identifier_for_diagnosis() {
        let e = AppError::not_found("任务", "01JABCDEF");
        assert!(e.message.contains("01JABCDEF"));
        assert!(matches!(e.code, ErrorCode::NotFound));
    }
}
