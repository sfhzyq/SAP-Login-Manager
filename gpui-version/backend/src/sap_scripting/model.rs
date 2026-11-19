//! SAP 脚本原生动作模型（与 SAP 录制出的 .vbs 解耦，便于序列化存储与自有录制器产出）。
//!
//! 所有动作都映射到底层 SAP GUI Scripting 的 `findById(id).method/property` 调用。

use serde::{Deserialize, Serialize};

/// 单条脚本动作。序列化标签为 `op`，便于 JSON 存储与调试。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ScriptAction {
    /// 跳转事务码，如 "SE16N"（等价于 session.StartTransaction）
    GotoTransaction { code: String },
    /// 在控件 id 设置文本（输入框 / 下拉文本）
    SetText { id: String, value: String },
    /// 按下按钮（id 形如 `wnd[0]/usr/btnXXX`）
    PressButton { id: String },
    /// 发送虚拟键，n 见 SAP VKEY 表（0=Enter, 3=Back, 8=F8, 11=Save ...）
    SendVKey { n: i32 },
    /// 选择 Tab / TabStrip 页
    SelectTab { id: String },
    /// Combobox 按 key 选择（也可用 SetText 按显示文本）
    SelectCombo { id: String, key: String },
    /// 在命令框执行 ok-code（如 /nSE16N），自动追加 Enter
    Command { okcode: String },
    /// 等待固定毫秒（等界面加载）
    Wait { ms: u64 },
    /// 断言状态栏：msg_type 为 S/E/W/I；contains 为子串；任一不匹配则中断
    AssertStatus {
        msg_type: Option<String>,
        contains: Option<String>,
    },
}

impl ScriptAction {
    /// 单行摘要（执行日志用）：类型 + 目标 id + 参数（值超 48 字符截断）。
    pub fn describe(&self) -> String {
        let trunc = |s: &str| {
            if s.chars().count() > 48 {
                format!("{}…", s.chars().take(48).collect::<String>())
            } else {
                s.to_string()
            }
        };
        match self {
            ScriptAction::GotoTransaction { code } => format!("StartTransaction {code}"),
            ScriptAction::SetText { id, value } => {
                format!("SetText {id} = '{}'", trunc(value))
            }
            ScriptAction::PressButton { id } => format!("Press {id}"),
            ScriptAction::SendVKey { n } => format!("SendVKey {n}"),
            ScriptAction::SelectTab { id } => format!("Select {id}"),
            ScriptAction::SelectCombo { id, key } => {
                format!("SetCombo {id} key='{}'", trunc(key))
            }
            ScriptAction::Command { okcode } => format!("Command {okcode}"),
            ScriptAction::Wait { ms } => format!("Wait {ms}ms"),
            ScriptAction::AssertStatus { .. } => "AssertStatus".to_string(),
        }
    }
}

/// 一个完整脚本（对应一条连接挂载的自动化流程）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Script {
    #[serde(default)]
    pub name: String,
    /// 关联系统（connection_id），用于按系统分目录归档
    #[serde(default)]
    pub system: String,
    #[serde(default)]
    pub description: String,
    /// 默认延迟（毫秒）：登录/脚本启动后先等界面加载再动手
    #[serde(default)]
    pub default_delay_ms: u64,
    pub actions: Vec<ScriptAction>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_summarizes_actions() {
        assert_eq!(
            ScriptAction::SetText {
                id: "wnd[0]/usr/ctxtX".into(),
                value: "abc".into()
            }
            .describe(),
            "SetText wnd[0]/usr/ctxtX = 'abc'"
        );
        assert_eq!(ScriptAction::SendVKey { n: 0 }.describe(), "SendVKey 0");
        // 长值截断到 48 字符 + 省略号（值包在单引号内，行尾是 `'`）
        let long = "x".repeat(60);
        let d = ScriptAction::SetText {
            id: "wnd[0]".into(),
            value: long,
        }
        .describe();
        assert!(d.ends_with("…'"));
        assert!(d.matches('…').count() == 1);
    }
}
