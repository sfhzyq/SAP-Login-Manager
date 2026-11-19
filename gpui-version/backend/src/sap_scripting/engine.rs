//! SAP GUI Scripting 执行引擎：附着到正在运行的 SAP GUI 会话，按动作序列驱动界面。
//!
//! 对象路径（与 SAP 录制器一致）：
//! `SAPGUI` -> `GetScriptingEngine` -> `GuiApplication` -> `Children(0)`(连接)
//! -> `Children(0)`(会话) -> `findById("wnd[0]/...")`(控件)。

#![cfg(windows)]

use std::thread;
use std::time::Duration;

use windows::core::BSTR;
use windows::Win32::System::Com::{
    IDispatch, DISPATCH_METHOD, DISPATCH_PROPERTYGET, DISPATCH_PROPERTYPUT,
};
use windows::Win32::System::Variant::VARIANT;

use super::com::{self, ComRuntime};
use super::model::ScriptAction;
use super::ScriptError;

/// 已打开会话的摘要信息（会话感知执行用）。
#[derive(Debug, Clone)]
pub struct SessionInfo {
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

/// 已附着的 SAP GUI 脚本会话。
pub struct SapScriptEngine {
    /// 持有 STA 生命周期，drop 时反初始化 COM
    _com: ComRuntime,
    /// 当前会话（GuiSession）
    session: IDispatch,
}

/// 取 GuiApplication（SapGuiAuto.GetScriptingEngine()）。
fn get_application(sapgui: &IDispatch) -> Result<IDispatch, ScriptError> {
    let app_v = com::invoke(sapgui, "GetScriptingEngine", DISPATCH_METHOD, &[])
        .map_err(|e| ScriptError::Com(e.to_string()))?;
    extract_dispatch(&app_v)
}

/// 取「Children 集合」的元素个数（`x.Children.Count`，与 VBScript 录制一致）。
fn child_count(disp: &IDispatch) -> Result<usize, ScriptError> {
    let coll_v = com::invoke(disp, "Children", DISPATCH_PROPERTYGET, &[])
        .map_err(|e| ScriptError::Com(e.to_string()))?;
    let coll = extract_dispatch(&coll_v)?;
    let n_v = com::invoke(&coll, "Count", DISPATCH_PROPERTYGET, &[])
        .map_err(|e| ScriptError::Com(e.to_string()))?;
    Ok(variant_to_i32(&n_v).unwrap_or(0).max(0) as usize)
}

/// 取 Children(i)（与现有 attach 同姿势，参数化属性按方法调用）。
fn child_at(disp: &IDispatch, idx: usize) -> Result<IDispatch, ScriptError> {
    // COM 索引是 i32：usize 超界时显式报错而非静默回绕成垃圾下标
    let idx =
        i32::try_from(idx).map_err(|_| ScriptError::Parse(format!("Children 下标越界: {idx}")))?;
    let v = com::invoke(disp, "Children", DISPATCH_METHOD, &[com::variant_i32(idx)])
        .map_err(|e| ScriptError::Com(e.to_string()))?;
    extract_dispatch(&v)
}

impl SapScriptEngine {
    /// 附着到当前 SAP GUI 的第一个连接 / 第一个会话。
    pub fn attach() -> Result<Self, ScriptError> {
        Self::attach_to(0, 0)
    }

    /// 附着到指定连接 / 会话（会话感知执行用）。
    pub fn attach_to(conn_idx: usize, sess_idx: usize) -> Result<Self, ScriptError> {
        let com = ComRuntime::init().map_err(|e| ScriptError::Com(e.to_string()))?;
        let sapgui = com::get_sapgui_object_with_retry().map_err(ScriptError::Com)?;
        let app = get_application(&sapgui)?;
        let conn = child_at(&app, conn_idx)?;
        let session = child_at(&conn, sess_idx)?;
        Ok(SapScriptEngine { _com: com, session })
    }

    /// 枚举当前 SAP GUI 的所有连接 / 会话，读每个会话的
    /// `Info.SystemName / Client / User`（SAP GUI 需已启动且启用 scripting）。
    pub fn enumerate_sessions() -> Result<Vec<SessionInfo>, ScriptError> {
        // `_com` 绑定保持 STA 初始化到函数结束（仅生命周期守卫，不直接引用）
        let _com = ComRuntime::init().map_err(|e| ScriptError::Com(e.to_string()))?;
        let sapgui = com::get_sapgui_object_with_retry().map_err(ScriptError::Com)?;
        let app = get_application(&sapgui)?;

        let mut out = Vec::new();
        for ci in 0..child_count(&app)? {
            let conn = child_at(&app, ci)?;
            for si in 0..child_count(&conn)? {
                let sess = child_at(&conn, si)?;
                let info_v = com::invoke(&sess, "Info", DISPATCH_PROPERTYGET, &[])
                    .map_err(|e| ScriptError::Com(e.to_string()))?;
                let info = extract_dispatch(&info_v)?;
                let read = |name: &str| -> String {
                    com::invoke(&info, name, DISPATCH_PROPERTYGET, &[])
                        .ok()
                        .and_then(|v| variant_to_string(&v))
                        .unwrap_or_default()
                };
                out.push(SessionInfo {
                    connection: ci,
                    session: si,
                    system: read("SystemName"),
                    client: read("Client"),
                    user: read("User"),
                });
            }
        }
        Ok(out)
    }

    /// 执行整个脚本：先等界面加载（default_delay_ms），再依次执行动作。
    ///
    /// `log` 传入时逐动作落执行日志（成功 OK / 失败 ERR + 错误原文）。
    pub fn run_script(
        &self,
        actions: &[ScriptAction],
        default_delay_ms: u64,
        log: Option<&super::runlog::RunLog>,
    ) -> Result<(), ScriptError> {
        if default_delay_ms > 0 {
            thread::sleep(Duration::from_millis(default_delay_ms));
        }
        for (idx, action) in actions.iter().enumerate() {
            let desc = action.describe();
            match self.run_action(action, default_delay_ms) {
                Ok(()) => {
                    if let Some(l) = log {
                        l.action(idx + 1, &desc, None);
                    }
                }
                Err(e) => {
                    if let Some(l) = log {
                        l.action(idx + 1, &desc, Some(&e.to_string()));
                    }
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// 执行单条动作。
    pub fn run_action(
        &self,
        action: &ScriptAction,
        _default_delay_ms: u64,
    ) -> Result<(), ScriptError> {
        match action {
            ScriptAction::GotoTransaction { code } => self.start_transaction(code),
            ScriptAction::SetText { id, value } => self.set_text(id, value),
            ScriptAction::PressButton { id } => self.press(id),
            ScriptAction::SendVKey { n } => self.send_vkey("wnd[0]", *n),
            ScriptAction::SelectTab { id } => self.select(id),
            ScriptAction::SelectCombo { id, key } => self.set_combo_key(id, key),
            ScriptAction::Command { okcode } => {
                self.set_text("wnd[0]/tbar[0]/okcd", okcode)?;
                self.send_vkey("wnd[0]", 0)
            }
            ScriptAction::Wait { ms } => {
                thread::sleep(Duration::from_millis(*ms));
                Ok(())
            }
            ScriptAction::AssertStatus { msg_type, contains } => {
                let (mt, txt) = self.read_statusbar()?;
                if let Some(want) = msg_type {
                    if !mt.eq_ignore_ascii_case(want) {
                        return Err(ScriptError::Assert(format!(
                            "状态栏类型 {mt} 与期望 {want} 不符；消息: {txt}"
                        )));
                    }
                }
                if let Some(sub) = contains {
                    if !txt.contains(sub) {
                        return Err(ScriptError::Assert(format!(
                            "状态栏未包含 '{sub}'；消息: {txt}"
                        )));
                    }
                }
                Ok(())
            }
        }
    }

    /// 按 id 取得控件（GuiComponent）。
    pub fn find_by_id(&self, id: &str) -> Result<IDispatch, ScriptError> {
        let v = com::invoke(
            &self.session,
            "findById",
            DISPATCH_METHOD,
            &[com::variant_string(id)],
        )
        .map_err(|e| ScriptError::Com(e.to_string()))?;
        extract_dispatch(&v)
    }

    /// 设置控件文本属性（text = value）。
    pub fn set_text(&self, id: &str, value: &str) -> Result<(), ScriptError> {
        let control = self.find_by_id(id)?;
        com::invoke(
            &control,
            "text",
            DISPATCH_PROPERTYPUT,
            &[com::variant_string(value)],
        )
        .map_err(|e| ScriptError::Com(e.to_string()))?;
        Ok(())
    }

    /// 按下按钮（control.press）。
    pub fn press(&self, id: &str) -> Result<(), ScriptError> {
        let control = self.find_by_id(id)?;
        com::invoke(&control, "press", DISPATCH_METHOD, &[])
            .map_err(|e| ScriptError::Com(e.to_string()))?;
        Ok(())
    }

    /// 选择 Tab / TabStrip 页（control.select）。
    pub fn select(&self, id: &str) -> Result<(), ScriptError> {
        let control = self.find_by_id(id)?;
        com::invoke(&control, "select", DISPATCH_METHOD, &[])
            .map_err(|e| ScriptError::Com(e.to_string()))?;
        Ok(())
    }

    /// Combobox 按 key 选择（control.key = key）。
    pub fn set_combo_key(&self, id: &str, key: &str) -> Result<(), ScriptError> {
        let control = self.find_by_id(id)?;
        com::invoke(
            &control,
            "key",
            DISPATCH_PROPERTYPUT,
            &[com::variant_string(key)],
        )
        .map_err(|e| ScriptError::Com(e.to_string()))?;
        Ok(())
    }

    /// 在指定窗口上发送虚拟键（window.sendVKey n）。
    pub fn send_vkey(&self, window_id: &str, n: i32) -> Result<(), ScriptError> {
        let wnd = self.find_by_id(window_id)?;
        com::invoke(&wnd, "sendVKey", DISPATCH_METHOD, &[com::variant_i32(n)])
            .map_err(|e| ScriptError::Com(e.to_string()))?;
        Ok(())
    }

    /// 跳转到事务码（session.StartTransaction code）。
    pub fn start_transaction(&self, code: &str) -> Result<(), ScriptError> {
        com::invoke(
            &self.session,
            "StartTransaction",
            DISPATCH_METHOD,
            &[com::variant_string(code)],
        )
        .map_err(|e| ScriptError::Com(e.to_string()))?;
        Ok(())
    }

    /// 读取状态栏：返回 (MessageType, Text)。MessageType 为 S/E/W/I。
    pub fn read_statusbar(&self) -> Result<(String, String), ScriptError> {
        let sbar = self.find_by_id("wnd[0]/sbar")?;
        let text_v = com::invoke(&sbar, "text", DISPATCH_PROPERTYGET, &[])
            .map_err(|e| ScriptError::Com(e.to_string()))?;
        let msg_v = com::invoke(&sbar, "MessageType", DISPATCH_PROPERTYGET, &[])
            .map_err(|e| ScriptError::Com(e.to_string()))?;
        let text = variant_to_string(&text_v).unwrap_or_default();
        let msg = variant_to_string(&msg_v).unwrap_or_default();
        Ok((msg, text))
    }
}

/// 从 VARIANT 取出 IDispatch 接口。
fn extract_dispatch(v: &VARIANT) -> Result<IDispatch, ScriptError> {
    IDispatch::try_from(v).map_err(|e| ScriptError::Com(format!("无法从返回值取得对象: {e}")))
}

/// 从 VARIANT 取出字符串（BSTR）。
fn variant_to_string(v: &VARIANT) -> Option<String> {
    let bstr = BSTR::try_from(v).ok()?;
    String::try_from(&bstr).ok()
}

/// 从 VARIANT 取出 i32（Count 等数值属性；BSTR 数字串兜底）。
fn variant_to_i32(v: &VARIANT) -> Option<i32> {
    if let Ok(n) = i32::try_from(v) {
        return Some(n);
    }
    variant_to_string(v)?.trim().parse().ok()
}
