//! Windows 原生集成：系统托盘 + 全局热键
//!
//! - 托盘图标：左键/「显示主窗口」激活窗口；「退出」结束进程
//! - 全局热键 Ctrl+Shift+S：任意界面唤起窗口并打开命令面板（Raycast 式入口）
//! - 事件桥接：tray-icon / global-hotkey 均通过 channel 投递事件，
//!   这里用 150ms 轮询泵回 GPUI 主循环（与自动锁定同一 spawn_in + update_in 模式）
//!
//! 图标文件：复用 Tauri 版 tauri-version/src-tauri/icons/icon.ico，编译期内嵌，首次运行释放到 data/ 目录后加载。

use std::time::Duration;

use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{
    MouseButton as TrayMouseButton, MouseButtonState as TrayMouseButtonState, TrayIcon,
    TrayIconBuilder, TrayIconEvent,
};

use gpui_kit::*;

use crate::state::AppState;
use crate::ui::SapApp;
use crate::ui::main_page::MainPage;
use crate::ui::i18n::t;

/// 内嵌应用图标（多尺寸 ico，托盘自动选用；与 build.rs 资源嵌入同源）
const ICON_BYTES: &[u8] = include_bytes!("../icons/icon.ico");

/// Windows 原生集成句柄（存 AppState 持有即生效；进程退出时自动清理）
pub struct NativeIntegration {
    /// 托盘句柄必须持有：drop 时图标会从任务栏移除
    _tray: TrayIcon,
    _hotkey_manager: GlobalHotKeyManager,
}

impl NativeIntegration {
    pub fn new() -> Result<Self, String> {
        let icon_path = ensure_icon_file()?;
        let icon = tray_icon::Icon::from_path(&icon_path, None).map_err(|e| e.to_string())?;

        let menu = build_menu();
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("SAP 登录管理器")
            .with_icon(icon)
            .build()
            .map_err(|e| e.to_string())?;

        let hotkey_manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
        // Ctrl+Shift+S：全局唤起 + 命令面板
        hotkey_manager
            .register(HotKey::new(
                Some(Modifiers::CONTROL | Modifiers::SHIFT),
                Code::KeyS,
            ))
            .map_err(|e| e.to_string())?;

        Ok(Self {
            _tray: tray,
            _hotkey_manager: hotkey_manager,
        })
    }
}

/// 首次运行释放内嵌图标到 data/tray-icon.ico（内容不一致才重写）
fn ensure_icon_file() -> Result<std::path::PathBuf, String> {
    let dir = crate::data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("tray-icon.ico");
    let need_write = std::fs::read(&path)
        .map(|cur| cur != ICON_BYTES)
        .unwrap_or(true);
    if need_write {
        std::fs::write(&path, ICON_BYTES).map_err(|e| e.to_string())?;
    }
    Ok(path)
}

/// 构建托盘菜单：显示主窗口 | 退出
///
/// 文案走 i18n（按启动时保存的 UI 语言渲染）；托盘菜单在启动时构建一次，
/// 运行中切换语言后需重启应用才会更新托盘文案
fn build_menu() -> Menu {
    let menu = Menu::new();
    let _ = menu.append(&MenuItem::with_id("show", t("显示主窗口"), true, None));
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id("quit", t("退出"), true, None));
    menu
}

/// 当前主界面（锁定时为 None）
fn main_page(cx: &App) -> Option<Entity<MainPage>> {
    AppState::global(cx)
        .main_page
        .clone()
        .and_then(|w| w.upgrade())
}

/// 取主窗口 HWND：经 gpui Window 的 HasWindowHandle 公开 API（Win32 变体）
///
/// 注意：Window 有固有方法 window_handle() -> AnyWindowHandle（遮蔽 trait
/// 同名方法），必须用完全限定语法调用 trait 方法
fn main_hwnd(window: &Window) -> Option<isize> {
    use raw_window_handle::HasWindowHandle;
    let raw = HasWindowHandle::window_handle(window).ok()?.as_raw().clone();
    match raw {
        raw_window_handle::RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

/// 显示主窗口：隐藏到托盘态先 SW_SHOW（保持原尺寸/位置），再 activate 置前台
///
/// vendor 的 activate 只处理最小化（IsIconic → SW_RESTORE），对 SW_HIDE
/// 隐藏的窗口不会 ShowWindow，SetForegroundWindow 对隐藏窗口无效——
/// 所以这里必须先补 SW_SHOW 再走 activate
pub fn show_main_window(window: &mut Window) {
    use windows_sys::Win32::Foundation::HWND as WinHWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindowVisible, ShowWindow, SW_SHOW};
    if let Some(hwnd) = main_hwnd(window) {
        unsafe {
            let h = hwnd as WinHWND;
            if IsWindowVisible(h) == 0 {
                ShowWindow(h, SW_SHOW);
            }
        }
    }
    window.activate_window();
}

/// 隐藏主窗口到托盘（SW_HIDE；任务栏图标随之消失，托盘仍驻留）
pub fn hide_main_window(window: &Window) {
    use windows_sys::Win32::Foundation::HWND as WinHWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    if let Some(hwnd) = main_hwnd(window) {
        unsafe { ShowWindow(hwnd as WinHWND, SW_HIDE) };
    }
}

/// 最小化到托盘轮询（150ms 泵内调用）：设置开启且窗口被最小化时，
/// 先 SW_RESTORE 还原到正常尺寸（下次唤回不是最小化态），再 SW_HIDE 到托盘。
/// 已在托盘隐藏态（不可见）则跳过，保证幂等
pub fn poll_minimize_to_tray(window: &Window, cx: &App) {
    let enabled = AppState::global(cx).store.lock().settings.minimize_to_tray;
    if !enabled {
        return;
    }
    let Some(hwnd) = main_hwnd(window) else {
        return;
    };
    unsafe {
        use windows_sys::Win32::Foundation::HWND as WinHWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            IsIconic, IsWindowVisible, ShowWindow, SW_HIDE, SW_RESTORE,
        };
        let h = hwnd as WinHWND;
        if IsIconic(h) != 0 && IsWindowVisible(h) != 0 {
            ShowWindow(h, SW_RESTORE);
            ShowWindow(h, SW_HIDE);
        }
    }
}

/// 启动事件轮询泵（150ms）：托盘点击 / 菜单命令 / 全局热键 → GPUI 主循环
///
/// 与自动锁定同一模式：挂在 SapApp 实体上，窗口关闭后 update_in 失败即退出循环
pub fn start_event_pump(window: &mut Window, cx: &mut Context<SapApp>) {
    cx.spawn_in(window, async move |this, window| {
        loop {
            window
                .background_executor()
                .timer(Duration::from_millis(150))
                .await;
            let result = this.update_in(window, |_, window, cx| {
                poll_events(window, cx);
            });
            if result.is_err() {
                break;
            }
        }
    })
    .detach();
}

/// 处理一批原生事件 + 按需刷新托盘菜单（GPUI 主线程执行）
fn poll_events(window: &mut Window, cx: &mut App) {
    // 1) 托盘图标事件：左键松开 → 从托盘唤回窗口（隐藏态先 SW_SHOW）
    for event in TrayIconEvent::receiver().try_iter() {
        if matches!(
            event,
            TrayIconEvent::Click {
                button: TrayMouseButton::Left,
                button_state: TrayMouseButtonState::Up,
                ..
            }
        ) {
            show_main_window(window);
        }
    }

    // 2) 全局热键：唤起窗口 + 命令面板（锁定时仅唤起，引导解锁）
    for event in GlobalHotKeyEvent::receiver().try_iter() {
        if event.state == HotKeyState::Pressed {
            show_main_window(window);
            if let Some(page) = main_page(cx) {
                crate::ui::command_palette::open_command_palette_dialog(
                    page.downgrade(),
                    window,
                    cx,
                );
            }
        }
    }

    // 3) 托盘菜单命令
    for event in MenuEvent::receiver().try_iter() {
        let id = event.id.0.clone();
        match id.as_str() {
            "show" => show_main_window(window),
            "quit" => cx.quit(),
            _ => {}
        }
    }

    // 4) 最小化到托盘：设置开启时把最小化窗口转入托盘隐藏
    poll_minimize_to_tray(window, cx);
}
