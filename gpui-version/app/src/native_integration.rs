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

/// 内嵌 Tauri 版图标（多尺寸 ico，托盘自动选用）
const ICON_BYTES: &[u8] = include_bytes!("../../../tauri-version/src-tauri/icons/icon.ico");

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
fn build_menu() -> Menu {
    let menu = Menu::new();
    let _ = menu.append(&MenuItem::with_id("show", "显示主窗口", true, None));
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&MenuItem::with_id("quit", "退出", true, None));
    menu
}

/// 当前主界面（锁定时为 None）
fn main_page(cx: &App) -> Option<Entity<MainPage>> {
    AppState::global(cx)
        .main_page
        .clone()
        .and_then(|w| w.upgrade())
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
    // 1) 托盘图标事件：左键松开 → 激活窗口
    for event in TrayIconEvent::receiver().try_iter() {
        if matches!(
            event,
            TrayIconEvent::Click {
                button: TrayMouseButton::Left,
                button_state: TrayMouseButtonState::Up,
                ..
            }
        ) {
            window.activate_window();
        }
    }

    // 2) 全局热键：唤起窗口 + 命令面板（锁定时仅唤起，引导解锁）
    for event in GlobalHotKeyEvent::receiver().try_iter() {
        if event.state == HotKeyState::Pressed {
            window.activate_window();
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
            "show" => window.activate_window(),
            "quit" => cx.quit(),
            _ => {}
        }
    }
}
