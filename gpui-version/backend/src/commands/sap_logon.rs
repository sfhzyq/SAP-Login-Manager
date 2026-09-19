//! SAP Logon 集成命令（Windows）：检测进程 / 唤起到前台 / 启动
//!
//! 供标题栏「打开 SAP Logon」按钮调用：
//! - 已在运行 → 找到 SAP Logon 主窗口，最小化则还原，然后置前台
//! - 未运行 → 依次探测用户设置路径与常见安装路径后启动
//!
//! 窗口定位策略：EnumWindows 找「可见 + 标题含 SAP Logon」的首个窗口
//! （SAP Logon 主窗口标题形如 "SAP Logon 770"）。

use std::path::PathBuf;

/// saplogon.exe 进程名（比较不区分大小写）
const SAPLOGON_PROC: &str = "saplogon.exe";
/// SAP Logon 主窗口标题关键字
const SAPLOGON_WINDOW_KEYWORD: &str = "SAP Logon";

/// 常见安装路径兜底（用户在设置中指定的路径优先）
const DEFAULT_PATHS: [&str; 2] = [
    r"C:\Program Files (x86)\SAP\FrontEnd\SAPgui\saplogon.exe",
    r"C:\Program Files\SAP\FrontEnd\SAPgui\saplogon.exe",
];

/// 打开或唤起 SAP Logon。
///
/// 返回动作结果（供 UI 提示）：
/// - `Ok("focused")`：已在运行且已置前台
/// - `Ok("running")`：在运行但未找到可激活的主窗口（如缩在托盘/加载中）
/// - `Ok("opened")`：新启动
/// - `Err(..)`：未找到可执行文件或启动失败
pub fn open_or_focus(settings_path: Option<&str>) -> Result<&'static str, String> {
    #[cfg(windows)]
    {
        if win::sap_logon_running() {
            return Ok(if win::bring_to_front() {
                "focused"
            } else {
                "running"
            });
        }
    }
    let path = resolve_path(settings_path).ok_or_else(|| {
        "未找到 saplogon.exe，请在 设置 → SAP Logon 路径 中指定".to_string()
    })?;
    std::process::Command::new(&path)
        .spawn()
        .map_err(|e| format!("启动 SAP Logon 失败：{e}"))?;
    Ok("opened")
}

/// 解析 saplogon.exe 路径：用户设置优先，其次常见安装路径
fn resolve_path(settings_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = settings_path {
        let p = p.trim();
        if !p.is_empty() {
            let pb = PathBuf::from(p);
            if pb.is_file() {
                return Some(pb);
            }
        }
    }
    DEFAULT_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

#[cfg(windows)]
mod win {
    use std::mem::zeroed;

    use windows_sys::Win32::Foundation::{
        CloseHandle, INVALID_HANDLE_VALUE, BOOL, HWND, LPARAM,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsIconic, IsWindowVisible,
        SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
    };

    /// saplogon.exe 是否正在运行（按进程名枚举系统进程）
    pub(super) fn sap_logon_running() -> bool {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return false;
            }
            let mut entry: PROCESSENTRY32W = zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut running = false;
            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    let len = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                    if name.eq_ignore_ascii_case(super::SAPLOGON_PROC) {
                        running = true;
                        break;
                    }
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
            running
        }
    }

    /// 找到 SAP Logon 主窗口，最小化则还原，然后置前台
    pub(super) fn bring_to_front() -> bool {
        unsafe {
            let mut found: HWND = std::ptr::null_mut();
            let lparam = &mut found as *mut HWND as LPARAM;
            EnumWindows(Some(enum_find_sap_window), lparam);
            if found.is_null() {
                return false;
            }
            // 最小化 → 还原；否则确保可见，再置前台
            if IsIconic(found) != 0 {
                ShowWindow(found, SW_RESTORE);
            } else {
                ShowWindow(found, SW_SHOW);
            }
            SetForegroundWindow(found);
            true
        }
    }

    /// EnumWindows 回调：可见 + 标题含关键字即命中，写入 lparam 指向的 HWND
    unsafe extern "system" fn enum_find_sap_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd) == 0 {
            return 1; // 继续枚举
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 || len > 256 {
            return 1;
        }
        let mut buf = [0u16; 257];
        let copied = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if copied <= 0 {
            return 1;
        }
        let title = String::from_utf16_lossy(&buf[..copied as usize]);
        if title.contains(super::SAPLOGON_WINDOW_KEYWORD) {
            let out = lparam as *mut HWND;
            *out = hwnd;
            return 0; // 已找到，停止枚举
        }
        1
    }
}

#[cfg(not(windows))]
pub fn open_or_focus(_settings_path: Option<&str>) -> Result<&'static str, String> {
    Err("仅支持 Windows".to_string())
}
