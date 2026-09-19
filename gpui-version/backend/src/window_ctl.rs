//! 窗口控制（Win32）：窗口置顶切换
//!
//! gpui 未暴露 always-on-top API，通过窗口标题查找本进程窗口后
//! 调 SetWindowPos(HWND_TOPMOST / HWND_NOTOPMOST) 实现。

/// 主窗口标题（与 app/src/main.rs 中 set_window_title 保持一致）
const WINDOW_TITLE: &str = "SAP Login Manager";

/// 设置/取消窗口置顶（仅 Windows；其他平台空操作）
pub fn set_window_topmost(topmost: bool) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            FindWindowW, SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE,
        };

        unsafe {
            let title: Vec<u16> = WINDOW_TITLE
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            if hwnd.is_null() {
                return false;
            }
            let insert_after = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
            SetWindowPos(hwnd, insert_after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE) != 0
        }
    }

    #[cfg(not(windows))]
    {
        let _ = topmost;
        false
    }
}
