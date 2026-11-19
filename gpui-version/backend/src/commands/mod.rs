pub mod auth;
pub mod credentials;
pub mod sap;
pub mod sap_logon;
pub mod settings;
pub mod transfer;

use crate::storage::StorageError;

/// 命令层统一错误类型。
///
/// `sap-backend` 是库 crate：公共 API 返回结构化错误，调用方（app crate）
/// 可按类别编程处理；显示文本由 `Display` 提供。
#[derive(Debug)]
#[non_exhaustive]
pub enum CommandError {
    /// 实体不存在（entity = "凭据" / "分组" 等）
    NotFound { entity: &'static str, id: String },
    /// 主密码错误
    BadMasterPassword,
    /// 主密码尚未设置
    MasterPasswordNotSet,
    /// 输入无效（语言代码 / 环境分类 / 删除模式等）
    InvalidInput(String),
    /// 文件系统错误
    Io(std::io::Error),
    /// JSON 序列化/反序列化错误
    Json(serde_json::Error),
    /// 存储层错误（含加密错误）
    Storage(StorageError),
    /// 外部进程 / SAP 自动化失败（消息已含上下文）
    External(String),
    /// 其它带上下文的错误
    Other(String),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::NotFound { entity, id } => write!(f, "{entity}不存在: {id}"),
            // v5 Should #2：改主密码后旧会话密码会一律失败，给出可恢复的引导
            CommandError::BadMasterPassword => {
                write!(f, "主密码错误（若刚修改过主密码，请锁定后重新解锁）")
            }
            CommandError::MasterPasswordNotSet => write!(f, "未设置主密码"),
            CommandError::InvalidInput(msg) => write!(f, "{msg}"),
            CommandError::Io(e) => write!(f, "IO 错误: {e}"),
            CommandError::Json(e) => write!(f, "JSON 错误: {e}"),
            CommandError::Storage(e) => write!(f, "{e}"),
            CommandError::External(msg) => write!(f, "{msg}"),
            CommandError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for CommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CommandError::Io(e) => Some(e),
            CommandError::Json(e) => Some(e),
            CommandError::Storage(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for CommandError {
    fn from(e: std::io::Error) -> Self {
        CommandError::Io(e)
    }
}

impl From<serde_json::Error> for CommandError {
    fn from(e: serde_json::Error) -> Self {
        CommandError::Json(e)
    }
}

impl From<StorageError> for CommandError {
    fn from(e: StorageError) -> Self {
        // 统一在此映射，避免各命令用 `?` 传播时被兜进 Storage(..)
        // 而丢失结构化语义（app 层未来按变体匹配时才会生效）。
        match e {
            StorageError::WrongMasterPassword => CommandError::BadMasterPassword,
            StorageError::MasterPasswordNotSet => CommandError::MasterPasswordNotSet,
            other => CommandError::Storage(other),
        }
    }
}

impl From<crate::crypto::CryptoError> for CommandError {
    fn from(e: crate::crypto::CryptoError) -> Self {
        CommandError::Storage(e.into())
    }
}

/// 桥接 app 层（应用代码统一用 `Result<_, String>` 呈现错误）：
/// 让 `backend_fn()?` 在 app 的 `Result<_, String>` 函数里直接传播。
impl From<CommandError> for String {
    fn from(e: CommandError) -> String {
        e.to_string()
    }
}

/// 快捷别名：命令层函数统一返回本类型。
pub type CmdResult<T> = Result<T, CommandError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// v5 Should #2：改主密码后旧会话密码会一律失败，错误文案必须给出
    /// 「重新解锁」这条可恢复路径，而不是只说「密码错误」。
    #[test]
    fn bad_master_password_error_includes_recovery_hint() {
        let msg = CommandError::BadMasterPassword.to_string();
        assert!(msg.contains("重新解锁"), "实际文案: {msg}");
        assert!(msg.contains("主密码错误"));
    }
}
