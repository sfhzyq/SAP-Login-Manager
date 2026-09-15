//! 全局应用状态

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::component::theme::ThemeMode;
use gpui_kit::{App, WeakEntity};
use sap_backend::Store;

use crate::ui::main_page::MainPage;
use crate::ui::SapApp;

/// 应用全局状态（GPUI Global）
pub struct AppState {
    /// 存储句柄（跨线程共享）
    pub store: Store,
    /// 主界面弱引用（设置变更后通知主页重渲染）
    pub main_page: Option<WeakEntity<MainPage>>,
    /// 根视图弱引用（子视图绕过 action 焦点链直接打开面板，如分组管理）
    pub weak_app: Option<WeakEntity<SapApp>>,
    /// 最近一次键鼠活动时间（自动锁定计时基准）
    pub last_activity: Rc<RefCell<Instant>>,
    /// Windows 原生集成（系统托盘 + 全局热键），持有即生效
    #[cfg(windows)]
    pub native: Option<crate::native_integration::NativeIntegration>,
}

impl gpui_kit::Global for AppState {}

impl AppState {
    pub fn init(cx: &mut App) {
        let store = Store::init();
        cx.set_global::<AppState>(Self {
            store: store.clone(),
            main_page: None,
            weak_app: None,
            last_activity: Rc::new(RefCell::new(Instant::now())),
            #[cfg(windows)]
            native: None,
        });

        // 根据保存的设置应用主题
        let theme = {
            let s = store.lock();
            s.settings.theme.clone()
        };
        apply_theme(&theme, None, cx);
    }

    pub fn global(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    pub fn global_mut(cx: &mut App) -> &mut Self {
        cx.global_mut::<Self>()
    }
}

/// 记录一次用户活动（重置自动锁定闲置计时）
pub fn touch_activity(cx: &App) {
    *AppState::global(cx).last_activity.borrow_mut() = Instant::now();
}

/// 应用主题："light" | "dark" | "system"
pub fn apply_theme(theme: &str, window: Option<&mut gpui_kit::Window>, cx: &mut App) {
    let mode = match theme {
        "dark" => ThemeMode::Dark,
        "light" => ThemeMode::Light,
        _ => {
            // 跟随系统
            let is_dark = window
                .as_ref()
                .map(|w| w.appearance() == gpui_kit::WindowAppearance::Dark)
                .unwrap_or_else(|| cx.window_appearance() == gpui_kit::WindowAppearance::Dark);
            if is_dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            }
        }
    };
    customize_theme_defaults(cx);
    gpui_kit::component::Theme::change(mode, window, cx);
}

/// 覆盖默认主题配置
///
/// gpui_kit 默认浅色主题下 background 与 popover 都接近纯白，弹窗 / 卡片 / 批量栏
/// 与主界面背景"撞色"导致层次缺失（弹窗浮不起来、卡片在列表里看不出区分）。
/// 这里对齐 Tauri 版 `src/styles/global.css` 的三层背景色板：
///
/// - 浅色：bg-primary `#eef0f6` → bg-secondary `#ffffff` → bg-tertiary `#e4e7f0`
/// - 深色：bg-primary `#0a0b14` → bg-secondary `#14161f` → bg-tertiary `#1e2130`
///
/// 同时统一前景 / 弱化文本 / 边框 / 危险色，与 Tauri 版语义一致；primary 改为系统
/// 蓝（默认 gpui_kit 的 primary 是黑白单色，深色下自定义 chip 会白底白字）。
/// 圆角 radius=8、radius_lg=12（弹窗圆角加大）。
fn customize_theme_defaults(cx: &mut App) {
    use gpui_kit::component::Theme;
    if !cx.has_global::<Theme>() {
        Theme::change(ThemeMode::Light, None, cx);
    }
    let theme = Theme::global_mut(cx);

    // === 浅色主题（对齐 Tauri 版 Light — Refined Blue） ===
    let light = std::rc::Rc::make_mut(&mut theme.light_theme);
    // 三层背景：主界面灰 / 弹窗+卡片+批量栏 白 / 子容器+chip 浅灰
    light.colors.background = Some("#eef0f6".into());
    light.colors.popover = Some("#ffffff".into());
    light.colors.secondary = Some("#e4e7f0".into());
    light.colors.sidebar = Some("#eef0f6".into());
    // 文本：主文本深 / 弱化文本中灰
    light.colors.foreground = Some("#1a1d2e".into());
    light.colors.muted_foreground = Some("#8b92a8".into());
    // 边框与危险色（与 Tauri 版 border-color / danger 一致）
    light.colors.border = Some("#dde1ec".into());
    light.colors.danger = Some("#e53e3e".into());
    // 语义色：成功 / 警告（与 Tauri 版 success / warning 一致）
    light.colors.success = Some("#16a34a".into());
    light.colors.warning = Some("#d97706".into());
    // 主色：系统蓝
    light.colors.primary = Some("#007AFF".into());
    light.colors.primary_hover = Some("#3395FF".into());
    light.colors.primary_active = Some("#0066D6".into());
    light.colors.primary_foreground = Some("#FFFFFF".into());
    // 字体：微软雅黑（对齐 Tauri 版 --font-stack 首选）
    light.font_family = Some("Microsoft YaHei".into());
    // 圆角：常规 10 / 弹窗 14（对齐 Tauri 版 --radius / --radius-lg）
    light.radius = Some(10);
    light.radius_lg = Some(14);

    // === 深色主题（对齐 Tauri 版 Dark — Refined Blue） ===
    let dark = std::rc::Rc::make_mut(&mut theme.dark_theme);
    // 三层背景：主界面深蓝黑 / 弹窗+卡片+批量栏 深灰 / 子容器+chip 更深灰
    dark.colors.background = Some("#0a0b14".into());
    dark.colors.popover = Some("#14161f".into());
    dark.colors.secondary = Some("#1e2130".into());
    dark.colors.sidebar = Some("#0a0b14".into());
    // 文本：主文本近白 / 弱化文本中灰
    dark.colors.foreground = Some("#edeff5".into());
    dark.colors.muted_foreground = Some("#737b91".into());
    // 边框（不透明近似 Tauri 的 rgba(255,255,255,0.09) 叠加效果）/ 危险色
    dark.colors.border = Some("#262a3a".into());
    dark.colors.danger = Some("#f06a6a".into());
    // 语义色：成功 / 警告（深色下提亮保证对比度）
    dark.colors.success = Some("#22c55e".into());
    dark.colors.warning = Some("#f59e0b".into());
    // 主色：系统蓝（深色变体）
    dark.colors.primary = Some("#0A84FF".into());
    dark.colors.primary_hover = Some("#339CFF".into());
    dark.colors.primary_active = Some("#0071E3".into());
    dark.colors.primary_foreground = Some("#FFFFFF".into());
    // 字体与圆角（同浅色）
    dark.font_family = Some("Microsoft YaHei".into());
    dark.radius = Some(10);
    dark.radius_lg = Some(14);
}

/// 开机自启：写 Windows 注册表 Run 键（HKCU）
#[cfg(windows)]
pub fn set_auto_start(enabled: bool) {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu
        .open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            KEY_SET_VALUE,
        )
        .ok();
    let Some(run) = run else {
        log::warn!("开机自启设置失败：无法打开注册表 Run 键");
        return;
    };
    if enabled {
        match std::env::current_exe() {
            Ok(exe) => {
                if let Err(e) = run.set_value(
                    "SAPLoginManager",
                    &std::ffi::OsString::from(exe.to_string_lossy().to_string()),
                ) {
                    log::warn!("开机自启设置失败：写入注册表出错：{e}");
                }
            }
            Err(e) => log::warn!("开机自启设置失败：无法获取当前 exe 路径：{e}"),
        }
    } else if let Err(e) = run.delete_value("SAPLoginManager") {
        // 值不存在（ERROR_FILE_NOT_FOUND）属正常，其余记日志
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("开机自启取消失败：删除注册表值出错：{e}");
        }
    }
}

/// 启动时按设置同步自启动注册表项：开启状态下重写 exe 路径，
/// 修复 exe 位置变更后注册表指向旧路径导致自启失效的问题
#[cfg(windows)]
pub fn sync_auto_start_on_launch(store: &Store) {
    let enabled = {
        let s = store.lock();
        s.settings.auto_start
    };
    if enabled {
        set_auto_start(true);
    }
}
