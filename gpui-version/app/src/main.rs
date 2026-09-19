// release 构建隐藏控制台窗口（Windows GUI 子系统）；debug 保留控制台便于查看 panic 日志
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod state;
mod ui;
mod window_state;
#[cfg(windows)]
mod native_integration;

use gpui_kit::assets::AllAssets;
use gpui_kit::component::{Root, TitleBar};
use gpui_kit::*;

use crate::state::AppState;
use crate::ui::SapApp;

/// 应用窗口尺寸：450×700，最小 450×650（最小宽 = 默认窗口宽）
const WINDOW_WIDTH: f32 = 450.0;
const WINDOW_HEIGHT: f32 = 700.0;
const MIN_WIDTH: f32 = 450.0;
const MIN_HEIGHT: f32 = 650.0;
/// 启动时的最低窗口高度（记忆尺寸或默认尺寸低于该值时按此起，屏幕放不下自动钳制）
const MIN_START_HEIGHT: f32 = 650.0;

/// 应用数据目录：exe 同目录下 data/（与 Tauri 版共享同一份数据）
pub fn data_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()))
        .join("data")
}

fn main() {
    // AllAssets 嵌入完整 Lucide 图标目录（1830 个），供 assets::IconName 全量枚举使用
    let app = gpui_kit::application().with_assets(AllAssets);

    app.run(|cx| {
        // 初始化组件库（必须最先调用）
        gpui_kit::init(cx);

        // 全局状态（存储句柄 + 主题）
        AppState::init(cx);

        // 自启动设置开启时，按当前 exe 路径重写注册表 Run 键（路径自愈：
        // exe 移动/更新位置后注册表不再指向旧路径）
        #[cfg(windows)]
        state::sync_auto_start_on_launch(&AppState::global(cx).store);

        // Windows 原生集成：系统托盘 + 全局热键（Ctrl+Shift+S 唤起命令面板）
        // 初始化失败不阻塞主窗口（如托盘图标加载失败、热键被占用）
        #[cfg(windows)]
        match native_integration::NativeIntegration::new() {
            Ok(native) => AppState::global_mut(cx).native = Some(native),
            Err(e) => log::warn!("托盘/全局热键初始化失败：{e}"),
        }

        // 恢复保存的窗口位置与尺寸（默认 450×700，上次关闭时的尺寸自动记忆；
        // 记忆高度低于 650 时按 650 起——界面元素增多后过低会频繁滚动，
        // 仍会被钳制到屏幕工作区内，锁定后也可自由缩小）
        let (saved_pos, saved_size) = window_state::load_window_bounds();

        let window_size = saved_size
            .map(|(w, h)| size(px(w as f32), px(h as f32)))
            .unwrap_or_else(|| size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)));
        let (win_w, win_h) = (
            window_size.width.as_f32(),
            window_size.height.as_f32().max(MIN_START_HEIGHT),
        );
        let bounds = match saved_pos {
            Some(pos) => Bounds {
                origin: point(px(pos.0 as f32), px(pos.1 as f32)),
                size: window_size,
            },
            None => Bounds::centered(None, window_size, cx),
        };

        // 钳制窗口位置与尺寸到主显示器工作区内：保存的位置可能越界（如 y=-74 会把
        // 标题栏和搜索框顶出屏幕外），保存的尺寸可能超出当前屏幕（更换显示器后）
        let visible = cx.primary_display().map(|d| d.visible_bounds());
        let bounds = match visible {
            Some(vb) => {
                let (vx, vy) = (vb.origin.x.as_f32(), vb.origin.y.as_f32());
                let (vw, vh) = (vb.size.width.as_f32(), vb.size.height.as_f32());
                let win_w = win_w.min(vw).max(MIN_WIDTH);
                let win_h = win_h.min(vh).max(MIN_HEIGHT);
                let max_x = (vx + vw - win_w).max(vx);
                let max_y = (vy + vh - win_h).max(vy);
                Bounds {
                    origin: point(
                        px(bounds.origin.x.as_f32().clamp(vx, max_x)),
                        px(bounds.origin.y.as_f32().clamp(vy, max_y)),
                    ),
                    size: size(px(win_w), px(win_h)),
                }
            }
            None => bounds,
        };

        let title = "SAP Login Manager".to_string();
        cx.spawn(async move |cx| {
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(gpui_kit::Size {
                    width: px(MIN_WIDTH),
                    height: px(MIN_HEIGHT),
                }),
                kind: WindowKind::Normal,
                // Win11 Mica 材质：DWM 把桌面壁纸降饱和透进窗口（22H2+，Win10 自动回落不透明）。
                // 需配合主题底色半透明（state.rs customize_theme_defaults 的 B3 alpha），
                // 标题栏/活动栏/列表空隙透出 Mica，卡片/弹层保持不透明浮起
                window_background: WindowBackgroundAppearance::MicaBackdrop,
                ..TitleBar::window_options()
            };

            let window = cx
                .open_window(options, |window, cx| {
                    let view = cx.new(|cx| SapApp::new(window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                })
                .expect("failed to open window");

            window
                .update(cx, |_, window, cx| {
                    window.activate_window();
                    window.set_window_title(&title);
                    // 关闭前保存窗口位置与尺寸（下次启动按此恢复）
                    window.on_window_should_close(cx, |window, cx| {
                        let bounds = window.bounds();
                        window_state::save_window_bounds(
                            f64::from(bounds.origin.x),
                            f64::from(bounds.origin.y),
                            f64::from(bounds.size.width),
                            f64::from(bounds.size.height),
                        );
                        // 关闭窗口时最小化到托盘（对齐 Tauri 版行为）：
                        // 设置开启则吞掉关闭请求、隐藏到托盘（点 X 不退出），
                        // 托盘左键 / Ctrl+Shift+S / 托盘菜单"显示主窗口"可唤回
                        #[cfg(windows)]
                        if AppState::global(cx).store.lock().settings.close_to_tray {
                            native_integration::hide_main_window(window);
                            return false;
                        }
                        true
                    });
                })
                .expect("failed to update window");
        })
        .detach();
    });
}
