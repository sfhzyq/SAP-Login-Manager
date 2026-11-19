//! SAP GUI Scripting 模块：执行内核 + 原生动作模型 + .vbs 导入 + 存储。
//!
//! - `model`：与 SAP 录制 .vbs 解耦的原生动作模型
//! - `vbs`：解析 SAP 录制器生成的 .vbs
//! - `store`：按系统分目录的 JSON 脚本存储
//! - `com` / `engine`（仅 Windows）：COM 执行内核
//!
//! 设计要点：执行内核只在 Windows + 已启用 scripting 的 SAP GUI 环境下运行；
//! 模型 / 解析 / 存储为纯 Rust，可在任意平台编译与单测。

pub mod model;
pub mod runlog;
pub mod store;
pub mod vbs;

#[cfg(windows)]
pub mod com;
#[cfg(windows)]
pub mod engine;

use std::path::Path;

pub use model::{Script, ScriptAction};
pub use runlog::RunLog;

#[cfg(windows)]
pub use engine::SapScriptEngine;

/// 默认执行延迟：等 SAP 界面加载完成（脚本可用 default_delay_ms 覆盖）
pub const DEFAULT_RUN_DELAY_MS: u64 = 3000;

/// 脚本执行错误。
#[derive(Debug)]
#[non_exhaustive]
pub enum ScriptError {
    Com(String),
    Assert(String),
    Parse(String),
    /// 保留原始 `std::io::Error` 作为 `source()`，不再压平成字符串
    Io(std::io::Error),
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptError::Com(s) => write!(f, "COM 错误: {s}"),
            ScriptError::Assert(s) => write!(f, "断言失败: {s}"),
            ScriptError::Parse(s) => write!(f, "解析错误: {s}"),
            ScriptError::Io(e) => write!(f, "IO 错误: {e}"),
        }
    }
}

impl std::error::Error for ScriptError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ScriptError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ScriptError {
    fn from(e: std::io::Error) -> Self {
        ScriptError::Io(e)
    }
}

impl From<store::StoreError> for ScriptError {
    fn from(e: store::StoreError) -> Self {
        match e {
            store::StoreError::Io(e) => ScriptError::Io(e),
            // 脚本文件 JSON 损坏本质是「解析失败」，归入 Parse 保留可读信息
            store::StoreError::Json(e) => ScriptError::Parse(e.to_string()),
            // v5 Should #6：文件名冲突必须如实传到 UI，不能压平成 IO 错误
            store::StoreError::NameCollision {
                path,
                existing,
                incoming,
            } => ScriptError::Parse(format!(
                "{path} 已存在（现有「{existing}」，待保存「{incoming}」），已中止以免静默覆盖"
            )),
        }
    }
}

/// 解析 .vbs 文件，返回动作序列。
///
/// 编码嗅探与 bat 相同（BOM/UTF-16LE/UTF-8/GBK）：中文 Windows 的 SAP
/// 录制器产出 ANSI/GBK 编码的 .vbs，硬性 UTF-8 读取会以误导性的
/// "invalid UTF-8" 失败。
pub fn parse_vbs_file(path: &Path) -> Result<Vec<ScriptAction>, ScriptError> {
    // read_bat_file 的错误消息已带文件路径，不要再包一层前缀
    let content = crate::bat_parser::read_bat_file(path).map_err(ScriptError::Parse)?;
    // v5 Should #14：跳过记录必须到达使用者（真实 logger 已接入）；
    // 一个动作都没解析出来时返回 Err，不能让残缺/空脚本报「执行成功」。
    let (actions, skipped) = vbs::parse_vbs_with_report(&content);
    for s in &skipped {
        log::warn!("{}: {s}", path.display());
    }
    if actions.is_empty() {
        let detail = if skipped.is_empty() {
            "文件不含任何可识别的脚本动作".to_string()
        } else {
            format!("无法识别的语句：{}", skipped.join("; "))
        };
        return Err(ScriptError::Parse(format!("{}: {detail}", path.display())));
    }
    Ok(actions)
}

/// 执行已保存的脚本文件：自动按后缀识别 JSON（原生）或 .vbs（SAP 录制）。
///
/// `log` 为执行日志句柄（见 [`runlog`]）：传入则逐动作落日志。
pub fn run_script_file(path: &Path, log: Option<&RunLog>) -> Result<(), ScriptError> {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("vbs"))
        .unwrap_or(false)
    {
        let actions = parse_vbs_file(path)?;
        run_actions(&actions, DEFAULT_RUN_DELAY_MS, log)
    } else {
        let script = store::load_script(path)?;
        run_script(&script, log)
    }
}

/// 已打开会话的摘要（会话感知执行用）。
#[derive(Debug, Clone)]
pub struct SessionRef {
    /// 连接下标（GuiApplication.Children(i)）
    pub connection: usize,
    /// 会话下标（GuiConnection.Children(j)）
    pub session: usize,
    /// 系统标识（如 DEV/PRD）
    pub system: String,
    /// Client（如 100）
    pub client: String,
    /// 登录账号
    pub user: String,
}

/// 枚举当前 SAP GUI 已打开的全部会话。
///
/// SAP GUI 未启动 / 未启用 scripting 时返回错误（调用方按「无会话」处理）。
pub fn list_open_sessions() -> Result<Vec<SessionRef>, ScriptError> {
    #[cfg(windows)]
    {
        Ok(engine::SapScriptEngine::enumerate_sessions()?
            .into_iter()
            .map(|i| SessionRef {
                connection: i.connection,
                session: i.session,
                system: i.system,
                client: i.client,
                user: i.user,
            })
            .collect())
    }
    #[cfg(not(windows))]
    {
        Err(ScriptError::Com(
            "会话枚举仅支持 Windows + 已启用 scripting 的 SAP GUI".into(),
        ))
    }
}

/// 在指定会话执行脚本文件（JSON 原生或 .vbs）。
///
/// 与 `run_script_file` 的区别：不附着首会话，而是按调用方给出的
/// 连接/会话下标定向附着——由上层先 `list_open_sessions` 匹配凭据。
pub fn run_script_file_in_session(
    path: &Path,
    connection: usize,
    session: usize,
    log: Option<&RunLog>,
) -> Result<(), ScriptError> {
    #[cfg(windows)]
    {
        let is_vbs = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("vbs"))
            .unwrap_or(false);
        let (actions, delay_ms) = if is_vbs {
            (parse_vbs_file(path)?, DEFAULT_RUN_DELAY_MS)
        } else {
            let script = store::load_script(path)?;
            (script.actions, script.default_delay_ms)
        };
        let engine = engine::SapScriptEngine::attach_to(connection, session)?;
        engine.run_script(&actions, delay_ms, log)
    }
    #[cfg(not(windows))]
    {
        let _ = (path, connection, session, log);
        Err(ScriptError::Com(
            "脚本执行仅支持 Windows + 已启用 scripting 的 SAP GUI".into(),
        ))
    }
}

/// 执行原生脚本结构。
pub fn run_script(script: &Script, log: Option<&RunLog>) -> Result<(), ScriptError> {
    #[cfg(windows)]
    {
        let engine = SapScriptEngine::attach()?;
        engine.run_script(&script.actions, script.default_delay_ms, log)
    }
    #[cfg(not(windows))]
    {
        let _ = (script, log);
        Err(ScriptError::Com(
            "脚本执行仅支持 Windows + 已启用 scripting 的 SAP GUI".into(),
        ))
    }
}

/// 直接执行动作序列（默认延迟 3 秒，等界面加载）。
pub fn run_actions(
    actions: &[ScriptAction],
    default_delay_ms: u64,
    log: Option<&RunLog>,
) -> Result<(), ScriptError> {
    #[cfg(windows)]
    {
        let engine = SapScriptEngine::attach()?;
        engine.run_script(actions, default_delay_ms, log)
    }
    #[cfg(not(windows))]
    {
        let _ = (actions, default_delay_ms, log);
        Err(ScriptError::Com(
            "脚本执行仅支持 Windows + 已启用 scripting 的 SAP GUI".into(),
        ))
    }
}
