//! SAP Login Manager - GPUI 版 UI
//!
//! 根视图：解锁页 / 主界面路由 + 顶栏 + Dialog / Notification 层

pub mod command_palette;
pub mod cred_filter;
pub mod credential_card;
pub mod credential_form;
pub mod group_manager;
pub mod i18n;
pub mod main_page;
mod panels;
pub mod sap_import;
pub mod script_library;
pub mod settings_page;
pub mod tokens;
pub mod unlock_page;

pub(crate) use panels::export::segmented_item;
pub use panels::{AboutPanel, BackupImportPanel, ExportPanel};

/// 功能开关：脚本库（📜 活动栏入口）。
/// 临时隐藏入口时置 `false`（代码保留，可随时恢复）；重新启用改回 `true`。
const FEATURE_SCRIPT_LIBRARY: bool = false;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    v_flex, ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use i18n::{t, tf};

use crate::state::AppState;
use sap_backend::commands::{auth, sap_logon};
use secrecy::{ExposeSecret as _, SecretBox, SecretString};
use zeroize::Zeroizing;

/// 主密码/会话密码在 app 内传递的受保护类型：析构自动清零。
/// 全链路（SapApp → MainPage/CredentialForm/SettingsPage/UnlockPage）统一使用，
/// 避免任何裸 `String` 副本滞留堆内存。
pub type MasterSecret = Zeroizing<String>;

actions!(
    sap_app,
    [
        /// 打开设置
        OpenSettings,
        /// 导出凭据
        ExportCredentials,
        /// 打开 SAP 配置导入
        ImportFromSap,
        /// 打开分组管理
        OpenGroupManager,
        /// 锁定应用
        LockApp,
        /// 关于
        ShowAbout,
        /// 命令面板（Ctrl+K）
        OpenCommandPalette,
        /// 打开脚本库（P4：左侧活动栏入口，管理 SAP GUI 脚本）
        OpenScriptLibrary
    ]
);

/// 从任意子视图打开分组管理面板
///
/// 不用 dispatch_action：action 沿焦点链冒泡，窗口无焦点时会被静默丢弃
/// （表现为点「管理分组」没反应）。改为经 AppState 的根视图弱引用直接调用。
pub(crate) fn request_open_group_manager(window: &mut Window, cx: &mut App) {
    if let Some(app) = AppState::global(cx)
        .weak_app
        .clone()
        .and_then(|w| w.upgrade())
    {
        app.update(cx, |app, cx| {
            app.open_group_manager(&OpenGroupManager, window, cx)
        });
    }
}

/// 分组管理穿透（v1.5.3）：关闭面板并让主列表筛选到指定分组
///
/// 与 request_open_group_manager 同款路径：不走 dispatch_action（窗口无
/// 焦点时 action 会被静默丢弃），经 AppState 弱引用直接调根视图。
pub(crate) fn request_drill_to_group(group_id: String, cx: &mut App) {
    if let Some(app) = AppState::global(cx)
        .weak_app
        .clone()
        .and_then(|w| w.upgrade())
    {
        app.update(cx, |app, cx| {
            app.close_panel_impl(cx);
            if let Some(page) = app.main_page.clone() {
                page.update(cx, |page, cx| page.drill_to_group(group_id, cx));
            }
        });
    }
}

/// 关闭面板宿主中的命令面板（Esc / 登录后）
///
/// 面板宿主上 `window.close_dialog` 是 no-op（同 sap_import.rs:467 记录的
/// 坑），必须经根视图 close_panel_impl；调用方以 panel_mode 把关，仅在
/// 面板确为命令面板时进入本函数
pub(crate) fn request_close_command_palette(cx: &mut App) {
    if let Some(app) = AppState::global(cx)
        .weak_app
        .clone()
        .and_then(|w| w.upgrade())
    {
        app.update(cx, |app, cx| {
            if app.active_panel.is_some() {
                app.close_panel_impl(cx);
            }
        });
    }
}

/// 语言切换后重烘焙构造期写入的静态文案（v1.5.5）
///
/// InputState 的 placeholder 在实体构造时一次性写入，window.refresh()
/// 重渲染不会更新它；设置页切换语言后经此路径让 MainPage 重写搜索框
/// 占位符（其余输入框均为打开时重建，无需处理）。
pub(crate) fn request_language_refresh(window: &mut Window, cx: &mut App) {
    if let Some(app) = AppState::global(cx)
        .weak_app
        .clone()
        .and_then(|w| w.upgrade())
    {
        app.update(cx, |app, cx| {
            if let Some(page) = app.main_page.clone() {
                page.update(cx, |page, cx| page.on_language_changed(window, cx));
            }
        });
    }
}

/// 从任意子视图打开凭据表单右侧面板（新增/编辑通用）——薄封装：
/// 经根视图 update 转调 open_credential_inplace（表单构建唯一实现，
/// 消除两份逐行重复的表单创建逻辑）
pub fn open_credential_panel(cred_id: Option<String>, window: &mut Window, cx: &mut App) {
    let Some(app) = AppState::global(cx)
        .weak_app
        .clone()
        .and_then(|w| w.upgrade())
    else {
        return;
    };
    app.update(cx, |app, cx| {
        app.open_credential_inplace(cred_id, window, cx)
    });
}

/// 应用根视图
pub struct SapApp {
    focus_handle: FocusHandle,
    /// 运行时主密码（解锁后驻留内存；SecretString Drop 时自动清零，Debug 打码）
    master_password: Option<SecretString>,
    /// 免密模式会话密码（本次运行内锁定后可快速解锁；应用关闭即失效）
    session_password: Option<SecretString>,
    /// 解锁页（锁定时显示）
    unlock_page: Entity<unlock_page::UnlockPage>,
    /// 主界面（解锁后创建）
    main_page: Option<Entity<main_page::MainPage>>,
    /// 主界面搜索框句柄：标题栏渲染同一 `Entity<InputState>`（搜索框上移自绘标题栏；
    /// 解锁成功创建 MainPage 后由 search_input_handle() 交出）
    search_input: Option<Entity<InputState>>,
    /// 当前激活的右侧面板（None = 显示 MainPage；Some = 显示面板）
    active_panel: Option<AnyView>,
    /// 面板标题（用于面板标题栏）
    panel_title: Option<String>,
    /// 面板种类（活动栏按钮激活高亮用："add"/"groups"/"import"/"export"/"settings"/"cmd"/"about"）
    panel_kind: Option<&'static str>,
    /// 窗口置顶状态（自绘标题栏置顶按钮用）
    pinned: bool,
    /// 系统深浅色变化订阅（theme=system 时跟随系统切换，需保活防取消）
    _appearance_sub: Subscription,
}

impl SapApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = AppState::global(cx).store.clone();
        // 载入界面语言（zh/en）
        i18n::set_lang(&store.lock().settings.ui_language.clone());
        // 首次使用判定：存储中尚未设置主密码。
        // fail-secure：读取失败当作"已设置"处理（走解锁页而非设置向导），
        // 防止 store 读取抖动时用户重设主密码覆盖已有加密数据。
        let is_setup = !auth::is_password_set(&store).unwrap_or(true);

        let weak_app = cx.entity().downgrade();
        let unlock_page = cx.new(|cx| {
            // 冷启动无会话密码：免密模式仅本次运行内有效（对应 Tauri 版 sessionPwRef）
            unlock_page::UnlockPage::new(is_setup, None, weak_app, window, cx)
        });

        // 自动锁定：后台循环按设置检查闲置时长（无操作 N 分钟后自动锁定）
        Self::start_auto_lock_timer(window, cx);

        // 托盘 / 全局热键事件轮询泵（150ms，桥接原生 channel 事件到 GPUI 主循环）
        #[cfg(windows)]
        crate::native_integration::start_event_pump(window, cx);

        // 根视图弱引用：供子视图直接打开面板（绕过 action 焦点链）
        AppState::global_mut(cx).weak_app = Some(cx.entity().downgrade());

        // 系统深浅色变化监听：主题=跟随系统时自动切换（gpui 平台层收 WM_SETTINGCHANGE）
        let _appearance_sub = window.observe_window_appearance({
            let store = store.clone();
            move |window, cx| {
                let mode = store.lock().settings.theme.clone();
                if mode == "system" {
                    crate::state::apply_theme("system", Some(window), cx);
                }
            }
        });

        Self {
            focus_handle: cx.focus_handle(),
            master_password: None,
            session_password: None,
            unlock_page,
            main_page: None,
            search_input: None,
            active_panel: None,
            panel_title: None,
            panel_kind: None,
            pinned: false,
            _appearance_sub,
        }
    }

    /// 启动自动锁定后台循环：每 5 秒检查一次闲置时长（窗口存活期内运行一次）
    fn start_auto_lock_timer(window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, window| {
            loop {
                window
                    .background_executor()
                    .timer(std::time::Duration::from_secs(5))
                    .await;
                let result = this.update_in(window, |app: &mut Self, window, cx| {
                    app.check_auto_lock(window, cx);
                });
                // 窗口已关闭：退出循环
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// 闲置超时检查：已锁定 / 设置关闭 / 登录进行中则跳过；超时自动锁定
    fn check_auto_lock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(main_page) = self.main_page.clone() else {
            return;
        };
        let minutes = {
            let store = AppState::global(cx).store.clone();
            let s = store.lock();
            s.settings.auto_lock_minutes
        };
        if minutes == 0 {
            return;
        }
        // 批量登录可能长达数分钟，期间不锁定以免打断后台任务
        if main_page.read(cx).is_logging_in() {
            return;
        }
        let idle = AppState::global(cx).last_activity.borrow().elapsed();
        if idle >= std::time::Duration::from_secs(minutes as u64 * 60) {
            window.push_notification(t("已闲置超时，自动锁定"), cx);
            self.lock(window, cx);
        }
    }

    /// 解锁成功：记录主密码并切换到主界面。
    /// 主密码以受保护类型（MasterSecret/SecretBox）传递：副本各自析构自动清零，
    /// 全链路不再保留裸 `String`。
    pub fn unlock(&mut self, password: MasterSecret, window: &mut Window, cx: &mut Context<Self>) {
        let store = AppState::global(cx).store.clone();
        let password_free = store.lock().settings.password_free;
        if password_free {
            self.session_password = Some(SecretBox::new(password.as_str().into()));
        }
        self.master_password = Some(SecretBox::new(password.as_str().into()));
        let page = cx.new(|cx| main_page::MainPage::new(password, window, cx));
        AppState::global_mut(cx).main_page = Some(page.downgrade());
        self.search_input = Some(page.read(cx).search_input_handle());
        self.main_page = Some(page);
        cx.notify();
    }

    /// 打开凭据表单面板（SapApp 固有方法，供自身 listener 调用）
    ///
    /// 注意：活动栏按钮的 on_click 处于 SapApp 自身的可变借用中，
    /// 这里必须直接操作 self；若走 open_credential_panel（weak_app.upgrade
    /// + app.update）会对同一实体嵌套 update，GPUI 直接 panic 闪退。
    pub fn open_credential_inplace(
        &mut self,
        cred_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_edit = cred_id.is_some();
        let editing = cred_id.and_then(|id| {
            let s = AppState::global(cx).store.lock();
            s.credentials.iter().find(|c| c.id == id).cloned()
        });
        let Some(main_page) = self.main_page.clone() else {
            return;
        };
        let mp = self.master_password();
        let form = cx.new(|cx| {
            credential_form::CredentialForm::new(editing, mp, main_page.downgrade(), window, cx)
        });
        let title = if is_edit {
            t("编辑凭据")
        } else {
            t("新增凭据")
        };
        self.open_panel(title, "add", form.into(), window, cx);
    }

    /// 锁定：清除主密码并回到解锁页；免密模式保留会话密码供快速解锁
    pub fn lock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // SecretString Drop 时自动把缓冲区覆写为 0（显式置 None 触发）
        self.master_password = None;
        self.main_page = None;
        // v4 Must #4：锁定必须一次复位全部关联状态。否则 active_panel 仍强引用
        // 含 MasterSecret 的表单/导出面板，主密码副本在整个锁定期驻留；
        // 解锁后 main_page 重建，旧面板会被重新渲染出来。
        self.active_panel = None;
        self.panel_title = None;
        self.panel_kind = None;
        self.search_input = None;
        AppState::global_mut(cx).main_page = None;

        let store = AppState::global(cx).store.clone();
        let password_free = store.lock().settings.password_free;
        if !password_free {
            self.session_password = None;
        }

        // 重建解锁页（清空上次输入）；副本用 Zeroizing 包裹，析构自动清零
        let weak_app = cx.entity().downgrade();
        let session_password = self
            .session_password
            .as_ref()
            .map(|s| MasterSecret::new(s.expose_secret().to_string()));
        self.unlock_page = cx
            .new(|cx| unlock_page::UnlockPage::new(false, session_password, weak_app, window, cx));
        cx.notify();
    }

    /// v5 Should #2：改主密码成功后强制重新认证。
    ///
    /// 会话里 `master_password` / `session_password` / `MainPage.master_password`
    /// / 已打开表单的 `MasterSecret` 都是旧密码副本，逐个刷新会漏；统一回到
    /// 解锁页重建，避免此后所有写入一律 `BadMasterPassword` 却只提示「密码错误」。
    pub fn relock_after_password_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 旧会话密码在新主密码下同样失效，免密模式也不得保留
        self.session_password = None;
        self.lock(window, cx);
    }

    /// 主密码受保护副本：`Zeroizing` 析构自动清零，调用方持有的副本不再裸留堆上
    fn master_password(&self) -> MasterSecret {
        self.master_password
            .as_ref()
            .map(|s| MasterSecret::new(s.expose_secret().to_string()))
            .unwrap_or_else(|| MasterSecret::new(String::new()))
    }

    /// 切换窗口置顶（Win32 SetWindowPos；状态存内存，应用重启后复位）
    fn toggle_pin(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let next = !self.pinned;
        if sap_backend::window_ctl::set_window_topmost(next) {
            self.pinned = next;
            cx.notify();
        }
    }

    /// 标题栏右侧窗口控制按钮（最小化/最大化/关闭）：原生 caption 行为
    ///
    /// 仅声明 window_control_area，由系统处理点击（gpui-kit ControlIcon 同款做法）。
    /// Div 层没有文本 tooltip 扩展（ManagedTooltipExt 为 crate 私有），故不加提示。
    /// `danger`：关闭钮走 Win11 规范——悬停 #C42B1C 红底白图标（其余为浅灰）。
    fn title_control_btn(
        &self,
        cx: &Context<Self>,
        id: &'static str,
        icon: IconName,
        area: gpui::WindowControlArea,
        danger: bool,
    ) -> AnyElement {
        let (hover_bg, hover_fg): (gpui_kit::Hsla, gpui_kit::Hsla) = if danger {
            (rgba(0xC42B1CFF).into(), rgba(0xFFFFFFFF).into())
        } else {
            (cx.theme().secondary, cx.theme().foreground)
        };
        // v1.5.9 修复（关闭钮右上角红色缺失）：此前 island 态给关闭钮加了
        // rounded_tr(radius_lg=8px)，那是旧「顶岛」设计的残留——当时红底需要
        // 避开顶岛圆角。v1.5.7 起 island 改为「直角 chrome + 浮起内容岛」：
        // 关闭钮贴在**窗口**直角上，而内容岛边框圆角内缩 ISLAND_GAP(6px) 自成
        // 一角，红底圆角与边框圆角两角不重合，红底右上角缺一小块（既不被红底
        // 覆盖、也不被边框覆盖）。现统一方形贴角（与 classic 一致），红底完整
        // 铺满右上角；Win11 DWM 对顶层窗口的圆角裁切仍由系统负责。
        h_flex()
            .id(id)
            .w(px(44.))
            .h_full()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(cx.theme().muted_foreground)
            .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
            .window_control_area(area)
            .child(Icon::new(icon).size_4())
            .into_any_element()
    }

    /// 标题栏 chrome 行（classic 标题栏与 island 顶部细行共用）：
    /// `[返回按钮?] [搜索框?] [拖拽区] [窗控三键]`
    ///
    /// 不用组件库 TitleBar：其窗口控制按钮渲染在最右且不可插入自定义按钮。
    /// 拖拽区与窗口控制按钮声明 window_control_area，交给系统按原生
    /// 标题栏处理（拖动、双击最大化、Snap、Alt+F4 等行为全部保留）。
    ///
    /// `locked`：锁定态精简模式——无搜索框/返回按钮，只保留拖拽区 + 三键。
    /// `show_back`：仅 classic 面板态为 true（island 面板态的返回按钮在
    /// 内容岛 header，见 render_panel_header）。
    /// 主界面搜索框上移自绘标题栏（2026-09-29）：仅凭据列表态显示
    ///（v1.5.7 回退：面板页常驻搜索+即打即搜实测误触跳走风险高、空顶岛
    /// 显眼，全局搜索语义归还 Ctrl+K 命令面板——面板态 Ctrl+F 转投）。
    /// 搜索框是可交互元素，必须渲染在 Drag 命中区之外，否则点击会被拖拽
    /// 吞掉；宽度取标题栏 60%（上限 420px），窄窗不挤窗口控制键，宽窗不至
    /// 于拉得过长。自身不铺 bg：island 顶部细行透明（canvas 透出），
    /// classic 由 render_title_bar 外层 v_flex 的 bg 兜底
    fn title_chrome_row(&self, cx: &Context<Self>, locked: bool, show_back: bool) -> Div {
        let search = (!locked && self.active_panel.is_none())
            .then(|| self.search_input.clone())
            .flatten();

        h_flex()
            .flex_none()
            .h(px(36.))
            .items_center()
            // 返回按钮纯图标（语义由 tooltip + Esc 兜底，箭头即通用返回符号）
            .children(show_back.then(|| {
                Button::new("panel-back")
                    .secondary()
                    .small()
                    .icon(IconName::ArrowLeft)
                    .tooltip(t("返回主界面 (Esc)"))
                    .on_click(cx.listener(Self::close_panel))
            }))
            // 搜索框（与 MainPage 共享同一 Entity<InputState>，状态/搜索逻辑不变）
            .children(search.map(|input| {
                div()
                    .flex_none()
                    .w(relative(0.6))
                    .max_w(px(420.))
                    .h_full()
                    .flex()
                    .items_center()
                    // pl_3(12px)：island 态行本身再缩进 ISLAND_GAP(6px)，
                    // 搜索框左缘 = 6+12 = 18px，与岛内 px_3 内容 spine 共线
                    .pl_3()
                    .child({
                        // 标题栏统一画布色后（v1.4.0），浅色 Input 默认底
                        // =background 会融入灰条 → 浮起输入面用 popover 表面
                        // token + input 边框 token（v1.5.3：去裸色值 white()，
                        // 对齐 gpui-kit「语义 token 优先、禁裸色值」规范）；
                        // 深色 Input 自有层次保持默认
                        let input = Input::new(&input)
                            .small()
                            .prefix(Icon::new(IconName::Search).size_4())
                            .cleanable(true);
                        if cx.theme().is_dark() {
                            input
                        } else {
                            input.bg(cx.theme().popover).border_color(cx.theme().input)
                        }
                    })
            }))
            // 拖拽区（应用名已移除：450px 小窗里装饰性文字纯占空间，活动栏顶部
            // S logo 兼身份标识；锁定态占满整行）
            .child(
                div()
                    .id("titlebar-drag")
                    .flex_1()
                    .min_w_6()
                    .h_full()
                    .window_control_area(gpui::WindowControlArea::Drag),
            )
            // 右侧：最小化 | 最大化 | 关闭（锁定/置顶/SAP Logon 已移入活动栏）
            .child(
                h_flex()
                    .flex_none()
                    .h_full()
                    .items_center()
                    .child(self.title_control_btn(
                        cx,
                        "tb-min",
                        IconName::Minus,
                        gpui::WindowControlArea::Min,
                        false,
                    ))
                    .child(self.title_control_btn(
                        cx,
                        "tb-max",
                        IconName::Square,
                        gpui::WindowControlArea::Max,
                        false,
                    ))
                    .child(self.title_control_btn(
                        cx,
                        "tb-close",
                        IconName::X,
                        gpui::WindowControlArea::Close,
                        true,
                    )),
            )
    }

    /// classic 自绘标题栏：chrome 行 + 面板态 34px 居中标题行（两行改版 v1.2.0）
    ///
    /// island 模式不走此函数——顶岛直接用 title_chrome_row，返回/标题由
    /// render_panel_header 渲染在内容岛内（v1.5.7 方案 1）
    fn render_title_bar(
        &self,
        cx: &Context<Self>,
        locked: bool,
        panel_title: Option<String>,
    ) -> AnyElement {
        let row1 = self.title_chrome_row(cx, locked, panel_title.is_some());

        // 第二行：面板标题行（整行宽度为基准居中，整行挂 Drag 热区）
        let title_row = panel_title.map(|title| {
            div()
                .id("panel-title-row")
                .flex()
                .flex_none()
                .h(px(34.))
                .w_full()
                .items_center()
                .justify_center()
                .window_control_area(gpui::WindowControlArea::Drag)
                .border_b_1()
                .border_color(cx.theme().title_bar_border)
                .bg(cx.theme().title_bar)
                .child(
                    div()
                        .max_w(px(320.))
                        .truncate()
                        .text_size(px(tokens::TS_MD))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.theme().foreground)
                        .child(title),
                )
        });

        // 无面板：单行 + 底部 hairline；面板态：两行，hairline 挂标题行底
        v_flex()
            .flex_none()
            .bg(cx.theme().title_bar)
            .child(row1.when(title_row.is_none(), |r| {
                r.border_b_1().border_color(cx.theme().title_bar_border)
            }))
            .children(title_row)
            .into_any_element()
    }

    /// island 面板态内容岛 header：返回按钮 + 左对齐标题（替代 classic 居中标题行）。
    /// 仅面板态渲染在内容岛顶部；Esc 返回主界面行为不变
    fn render_panel_header(&self, cx: &Context<Self>, title: String) -> AnyElement {
        h_flex()
            .flex_none()
            .h(px(34.))
            .items_center()
            .gap(px(4.))
            .px(px(10.))
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .child(
                Button::new("panel-back")
                    .secondary()
                    .small()
                    .icon(IconName::ArrowLeft)
                    .tooltip(t("返回主界面 (Esc)"))
                    .on_click(cx.listener(Self::close_panel)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(tokens::TS_MD))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().foreground)
                    .child(title),
            )
            .into_any_element()
    }

    /// 打开设置面板（替代弹窗）
    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.new(|cx| settings_page::SettingsPage::new(window, cx));
        self.open_panel(t("设置"), "settings", page.into(), window, cx);
    }

    /// 打开 SAP 配置导入面板
    fn open_sap_import(&mut self, _: &ImportFromSap, window: &mut Window, cx: &mut Context<Self>) {
        let Some(main_page) = self.main_page.clone() else {
            return;
        };
        let main_page = main_page.downgrade();
        let weak_app = cx.entity().downgrade();
        let page = cx.new(|cx| sap_import::SapImportPage::new(main_page, weak_app, window, cx));
        self.open_panel(t("导入"), "import", page.into(), window, cx);
    }

    /// 导出凭据面板
    fn open_export(&mut self, _: &ExportCredentials, window: &mut Window, cx: &mut Context<Self>) {
        let pw_state = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t("输入主密码以验证"))
        });
        let pw_for_click = pw_state.clone();
        let weak_app = cx.entity().downgrade();
        let cred_count = {
            let s = AppState::global(cx).store.lock();
            s.credentials.len()
        };
        let panel = cx.new(|cx| ExportPanel::new(pw_state, pw_for_click, cred_count, weak_app, cx));
        self.open_panel(t("导出凭据"), "export", panel.into(), window, cx);
    }

    /// 打开分组管理面板
    pub(crate) fn open_group_manager(
        &mut self,
        _: &OpenGroupManager,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(main_page) = self.main_page.clone() else {
            return;
        };
        let main_page = main_page.downgrade();
        let page = cx.new(|cx| group_manager::GroupManagerPage::new(main_page, window, cx));
        self.open_panel(t("分组管理"), "groups", page.into(), window, cx);
    }

    /// 打开脚本库面板（P4：SAP GUI 脚本管理——导入 .vbs / 运行 / 绑定凭据 / 会话感知执行）
    pub(crate) fn open_script_library(
        &mut self,
        _: &OpenScriptLibrary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let page = cx.new(script_library::ScriptLibraryPanel::new);
        self.open_panel(
            t("脚本库"),
            script_library::SCRIPT_PANEL_KIND,
            page.into(),
            window,
            cx,
        );
    }

    /// 锁定应用（快捷键 Ctrl+L 触发）
    fn lock_app_action(&mut self, _: &LockApp, window: &mut Window, cx: &mut Context<Self>) {
        self.lock(window, cx);
    }

    /// 关于面板
    fn show_about(&mut self, _: &ShowAbout, window: &mut Window, cx: &mut Context<Self>) {
        let version = env!("CARGO_PKG_VERSION").to_string();
        let panel = cx.new(|_cx| AboutPanel::new(version));
        self.open_panel(t("关于"), "about", panel.into(), window, cx);
    }

    /// 命令面板（Ctrl+K / 全局热键唤起）
    pub(crate) fn open_command_palette(
        &mut self,
        _: &OpenCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(main_page) = self.main_page.clone() else {
            return;
        };
        let palette = cx.new(|cx| {
            command_palette::CommandPalette::new(main_page.downgrade(), true, window, cx)
        });
        self.open_panel(t("命令面板"), "cmd", palette.into(), window, cx);
    }

    /// 打开右侧面板（替代弹窗）：存储 AnyView + 标题 + 种类（活动栏高亮）
    fn open_panel(
        &mut self,
        title: &str,
        kind: &'static str,
        view: AnyView,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.active_panel = Some(view);
        self.panel_title = Some(title.to_string());
        self.panel_kind = Some(kind);
        cx.notify();
    }

    /// 关闭面板，回到主界面
    fn close_panel(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.close_panel_impl(cx);
    }

    /// 关闭面板的共享实现（返回按钮 / Esc 共用）
    fn close_panel_impl(&mut self, cx: &mut Context<Self>) {
        self.active_panel = None;
        self.panel_title = None;
        self.panel_kind = None;
        cx.notify();
    }

    /// 全局即打即搜（v1.5.7 方案 A）：**仅凭据列表（主界面）态**生效——焦点
    /// 不在任何输入框时，直接敲字母/数字/空格即写入搜索框并执行搜索——首字符
    /// 经此劫持（焦点自动落入搜索框），后续字符由 Input 正常接收。面板态不
    /// 劫持（v1.5.7 回退：面板页任务单一，打字劫持会误触跳走丢失上下文；
    /// 全局搜索语义归还 Ctrl+K 命令面板）。
    ///
    /// 边界：
    /// - 锁定态不劫持（lock() 已清 main_page）；面板态不劫持（上方判断）
    /// - 焦点在任何输入框（搜索框/凭据表单/命令面板/解锁页密码框等）
    ///   时不劫持：文本经 InputHandler 提交而非 key_down 冒泡，这里再写
    ///   会双写——focused_input 覆盖 Input/Textarea/Editor/Otp 全部形态
    /// - 仅劫持字母/数字/空格：数字行上排符号无法从 keystroke 还原
    ///   （shift+2 键面报 "2" 实际是 "@"），符号请点击搜索框后输入
    /// - IME 组合中的按键不产生普通 keydown，中文输入法不受影响
    /// - 带 Ctrl/Alt/平台键的组合键放行（其余快捷键不受影响）
    fn type_to_search(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(page), Some(search)) = (self.main_page.clone(), self.search_input.clone()) else {
            return;
        };
        if self.active_panel.is_some() {
            return;
        }
        let mods = &event.keystroke.modifiers;
        if mods.control || mods.alt || mods.platform {
            return;
        }
        let typed: String = if event.keystroke.key == "space" {
            " ".to_string()
        } else {
            let key: &str = &event.keystroke.key;
            let Some(ch) = key.chars().next() else {
                return;
            };
            if key.len() != 1 || !ch.is_ascii_alphanumeric() {
                return;
            }
            if mods.shift {
                // gpui 键面值不反映 shift 映射（注释先例：shift+2 报 "2" 实际
                // 是 "@"）——字母可安全大写还原，数字/符号无法还原出真实
                // 键面字符，劫持会把 "2" 当 "@" 写入搜索词（脏数据），放行
                if !ch.is_ascii_alphabetic() {
                    return;
                }
                ch.to_ascii_uppercase().to_string()
            } else {
                ch.to_string()
            }
        };
        if window.focused_input(cx).is_some() {
            return;
        }
        cx.stop_propagation();
        // set_value 自带聚焦，但 emit_events=false 不发 Change——
        // 手动执行与订阅一致的搜索语义（劫持即开启新搜索，覆盖旧词）
        search.update(cx, |s, cx| {
            s.set_value(typed.as_str(), window, cx);
        });
        if self.active_panel.is_some() {
            self.close_panel_impl(cx);
        }
        page.update(cx, |p, cx| p.apply_search(&typed, cx));
    }

    /// 渲染左侧活动栏（48px 宽，全高，垂直排列图标按钮）
    ///
    /// 布局（分隔线分三区）：S logo → ＋新增 · 📁分组管理 ─ ⬇导入 · ⬆导出
    ///       ─ ⚙设置 · ℹ关于 → (弹性)
    /// 打开的面板对应按钮高亮（左竖条 + 白图标）；导出下方为深浅模式快捷切换
    ///
    /// 配色（2026-09-15 方案 C"暗壳亮芯"）：活动栏固定深蓝灰 #1E293B，
    /// 与浅/深主题的内容区都形成层次锚点；浅色主题下尤其修正"整窗发淡"。
    fn render_activity_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // 双主题自适应配色（浅色浅壳 / 深色深壳），色值集中在 tokens.rs。
        // v1.5.7 方案 Y：rail 在 island 模式下回归贴边直角通高（与 classic
        // 同形态）——「岛」只保留给内容区，层级 = rail(贴边) < canvas < 内容岛
        let abc = tokens::activity_bar_colors(cx);
        v_flex()
            .flex_none()
            .w_12()
            .h_full()
            .items_center()
            .gap(px(2.))
            .py_1()
            .bg(abc.bar_bg)
            // 右侧分割线已删除（v1.2.0）：白壳/深壳与内容区靠明度差分界，去线更通透
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            // S logo：兼窗口拖拽标识 + 点击回主界面（相当于"返回"）
            .child(
                div()
                    .id("activity-logo")
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(36.))
                    .mb_1()
                    .rounded(px(tokens::R_ICON_BOX))
                    .bg(cx.theme().primary)
                    .text_size(px(tokens::TS_XL))
                    .font_weight(FontWeight::BOLD)
                    .text_color(cx.theme().primary_foreground)
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.85))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_panel_impl(cx);
                        // 回到默认分组（历史行为）：收藏/最近登录等临时视图一键还原
                        if let Some(mp) = this.main_page.clone() {
                            mp.update(cx, |p, cx| p.reset_to_default_group(cx));
                        }
                    }))
                    .child("S"),
            )
            // ＋ 新增凭据（高频，置顶）
            .child(self.activity_btn(
                "act-add",
                IconName::Plus,
                t("新增凭据 (Ctrl+N)"),
                "add",
                cx.listener(|this, _, window, cx| {
                    // 直接调用固有方法：处于自身可变借用中，不能 weak_app.upgrade + app.update
                    //（嵌套可变借用会 panic → 应用闪退）
                    this.open_credential_inplace(None, window, cx);
                }),
                cx,
            ))
            // 🏷 分组管理（tags 比 folder 更贴合"分组"语义）
            .child(self.activity_btn(
                "act-group-manager",
                IconName::Tags,
                t("分组管理"),
                "groups",
                |_, window, cx| request_open_group_manager(window, cx),
                cx,
            ))
            // ⌘ 命令面板（Ctrl+K 高频，紧跟编辑组）
            .child(self.activity_btn(
                "act-cmd",
                IconName::Command,
                t("命令面板 (Ctrl+K)"),
                "cmd",
                cx.listener(|this, _, window, cx| {
                    this.open_command_palette(&OpenCommandPalette, window, cx);
                }),
                cx,
            ))
            // ⭐ 收藏 / ⏱ 最近使用（用户要求提前到编辑组末尾）：点击直接把
            // 分组下拉切到对应虚拟分组（跨自建分组全局视角），再点一次回「全部」
            .child(self.activity_quick_btn(
                "act-fav",
                IconName::Star,
                // 语义精确化：这是快速切换到收藏视图的按钮，非「收藏」动作
                t("收藏视图"),
                "favorites",
                cx,
            ))
            .child(self.activity_quick_btn(
                "act-recent",
                IconName::Clock,
                t("最近使用"),
                "recent",
                cx,
            ))
            // ─ 分区：编辑/命令 ↑ ─ 数据（导入/导出）↓
            .child(Self::activity_divider(cx))
            // ⬇ 从 SAP 配置导入
            .child(self.activity_btn(
                "act-import",
                IconName::Import,
                t("从 SAP 配置导入"),
                "import",
                cx.listener(|this, _, window, cx| {
                    this.open_sap_import(&ImportFromSap, window, cx);
                }),
                cx,
            ))
            // ⬆ 导出凭据（Upload 向上：数据出去，与导入方向可辨）
            .child(self.activity_btn(
                "act-export",
                IconName::Upload,
                t("导出凭据"),
                "export",
                cx.listener(|this, _, window, cx| {
                    this.open_export(&ExportCredentials, window, cx);
                }),
                cx,
            ))
            // 📜 脚本库（P4：按系统管理 SAP GUI 脚本，导入 .vbs / 运行 / 绑定凭据）
            // 入口由 FEATURE_SCRIPT_LIBRARY 控制；当前临时隐藏（代码保留，可恢复）
            .when(FEATURE_SCRIPT_LIBRARY, |el| {
                el.child(self.activity_btn(
                    "act-script-lib",
                    IconName::Code,
                    t("脚本库"),
                    script_library::SCRIPT_PANEL_KIND,
                    cx.listener(|this, _, window, cx| {
                        this.open_script_library(&OpenScriptLibrary, window, cx);
                    }),
                    cx,
                ))
            })
            // ☀/☾ 深浅模式快捷切换（用户要求：导出下方 + 分隔符）。kind="theme"
            // 永不命中 panel_kind，不会误激活面板高亮；点击翻转主题并持久化
            .child(Self::activity_divider(cx))
            .child(self.activity_btn(
                "act-theme",
                if cx.theme().is_dark() {
                    IconName::Sun
                } else {
                    IconName::Moon
                },
                t("切换深浅模式"),
                "theme",
                cx.listener(|_, _, window, cx| {
                    let next = if cx.theme().is_dark() {
                        "light"
                    } else {
                        "dark"
                    };
                    apply_theme_and_save(next, window, cx);
                }),
                cx,
            ))
            // ─ 分区：主题切换 ↓ 窗口操作（原自绘标题栏三键，2026-09-29 移入：
            //   低频操作离最小化/关闭远，消除误触；与功能入口统一到活动栏单栏）
            .child(Self::activity_divider(cx))
            // 打开 SAP Logon（运行中 → 还原并置前台；未运行 → 启动）
            .child(self.activity_btn(
                "act-saplogon",
                IconName::AppWindow,
                t("打开 SAP Logon"),
                "saplogon",
                cx.listener(|_, _, window, cx| {
                    let path = {
                        let s = AppState::global(cx).store.lock();
                        s.settings.sap_logon_path.clone()
                    };
                    match sap_logon::open_or_focus(path.as_deref()) {
                        Ok("opened") => window.push_notification(t("SAP Logon 已启动"), cx),
                        Ok("focused") => window.push_notification(t("SAP Logon 已置于前台"), cx),
                        Ok("running") => window.push_notification(t("SAP Logon 正在运行"), cx),
                        Ok(_) => {}
                        Err(e) => window.push_notification(
                            tf("打开 SAP Logon 失败：{e}", &[&e.to_string()]),
                            cx,
                        ),
                    }
                }),
                cx,
            ))
            // 锁定（锁定分支不渲染活动栏，按钮自然消失；Ctrl+L 快捷键不受影响）
            .child(self.activity_btn(
                "act-lock",
                IconName::Lock,
                t("锁定 (Ctrl+L)"),
                "lock",
                cx.listener(|this, _, window, cx| this.lock(window, cx)),
                cx,
            ))
            // 置顶（开关型：置顶中走激活视觉，非面板高亮）
            .child(self.activity_pin_btn(cx))
            // 弹性间距（把设置+关于推到底部；锁定已移至活动栏）
            .child(div().flex_1())
            // ⚙ 设置（移到关于上方）
            .child(self.activity_btn(
                "act-settings",
                IconName::Settings2,
                t("设置"),
                "settings",
                cx.listener(|this, _, window, cx| {
                    this.open_settings(&OpenSettings, window, cx);
                }),
                cx,
            ))
            // ℹ 关于
            .child(self.activity_btn(
                "act-about",
                IconName::Info,
                t("关于"),
                "about",
                cx.listener(|this, _, window, cx| {
                    this.show_about(&ShowAbout, window, cx);
                }),
                cx,
            ))
    }

    /// 活动栏图标按钮（方案 C「实心容器」，2026-10-22）：
    /// - 激活态 = primary 实底圆角方盒（R8）+ 白图标（原 2px 竖条 + 12% 胶囊退场）
    /// - 常态 = 裸露线图标（idle 文字色），hover/按压走叠加微反馈
    /// - 配色双主题自适应，色值见 tokens::activity_bar_colors
    fn activity_btn(
        &self,
        id: &'static str,
        icon: IconName,
        tooltip: impl Into<SharedString>,
        kind: &'static str,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let active = self.panel_kind == Some(kind);
        let abc = tokens::activity_bar_colors(cx);
        h_flex()
            .flex_none()
            .relative()
            .size_8()
            .items_center()
            .justify_center()
            .child(
                Button::new(id)
                    .icon(icon)
                    .small()
                    // 方案 C：激活态实底方盒（R_ICON_BOX）；常态裸露线图标
                    .rounded(px(tokens::R_ICON_BOX))
                    .tooltip(tooltip)
                    .when(active, |t| {
                        t.bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                            // C-2：激活方盒的 hover/按压走蓝色系反馈（非黑叠加）
                            .custom(
                                ButtonCustomVariant::new(cx)
                                    .hover(cx.theme().primary_hover)
                                    .active(cx.theme().primary_active),
                            )
                    })
                    .when(!active, |t| {
                        t.text_color(abc.idle_fg).custom(
                            ButtonCustomVariant::new(cx)
                                .hover(abc.hover_pill)
                                .active(abc.press_pill),
                        )
                    })
                    .on_click(on_click),
            )
    }

    /// 活动栏虚拟分组开关按钮（收藏 / 最近使用）
    ///
    /// 开关型：激活态读主页面 selected_group（而非 panel_kind）；点击把
    /// 分组下拉切到对应虚拟分组（同组再点回「全部」）；开着面板先关闭
    fn activity_quick_btn(
        &self,
        id: &'static str,
        icon: IconName,
        tooltip: &'static str,
        gid: &'static str,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let abc = tokens::activity_bar_colors(cx);
        let active = self.active_panel.is_none()
            && self
                .main_page
                .as_ref()
                .is_some_and(|mp| mp.read(cx).selected_group.as_deref() == Some(gid));
        h_flex()
            .flex_none()
            .relative()
            .size_8()
            .items_center()
            .justify_center()
            .child(
                Button::new(id)
                    .icon(icon)
                    .small()
                    // 方案 C：激活态实底方盒（R_ICON_BOX）+ 白图标
                    .rounded(px(tokens::R_ICON_BOX))
                    .tooltip(tooltip)
                    .when(active, |t| {
                        t.bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                            // C-2：激活方盒的 hover/按压走蓝色系反馈
                            .custom(
                                ButtonCustomVariant::new(cx)
                                    .hover(cx.theme().primary_hover)
                                    .active(cx.theme().primary_active),
                            )
                    })
                    .when(!active, |t| {
                        t.text_color(abc.idle_fg).custom(
                            ButtonCustomVariant::new(cx)
                                .hover(abc.hover_pill)
                                .active(abc.press_pill),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // 打开的面板先关闭，让分组切换结果可见
                        if this.active_panel.is_some() {
                            this.active_panel = None;
                            this.panel_title = None;
                            this.panel_kind = None;
                        }
                        if let Some(mp) = this.main_page.clone() {
                            mp.update(cx, |p, cx| p.toggle_virtual_group(gid, cx));
                        }
                        cx.notify();
                    })),
            )
    }

    /// 活动栏置顶按钮（开关型）：置顶中 primary 图标 + 激活胶囊 + 左侧竖条
    ///（复用激活视觉语言但语义是状态开关，与 panel_kind 面板高亮互不影响）
    fn activity_pin_btn(&self, cx: &Context<Self>) -> impl IntoElement {
        let abc = tokens::activity_bar_colors(cx);
        let pinned = self.pinned;
        h_flex()
            .flex_none()
            .relative()
            .size_8()
            .items_center()
            .justify_center()
            .child(
                Button::new("act-pin")
                    .icon(IconName::Pin)
                    .small()
                    // 方案 C：置顶中 = 实底方盒（R_ICON_BOX）+ 白图标
                    .rounded(px(tokens::R_ICON_BOX))
                    .tooltip(if pinned {
                        t("取消置顶")
                    } else {
                        t("窗口置顶")
                    })
                    .when(pinned, |t| {
                        t.bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                            // C-2：激活方盒的 hover/按压走蓝色系反馈
                            .custom(
                                ButtonCustomVariant::new(cx)
                                    .hover(cx.theme().primary_hover)
                                    .active(cx.theme().primary_active),
                            )
                    })
                    .when(!pinned, |t| {
                        t.text_color(abc.idle_fg).custom(
                            ButtonCustomVariant::new(cx)
                                .hover(abc.hover_pill)
                                .active(abc.press_pill),
                        )
                    })
                    .on_click(cx.listener(SapApp::toggle_pin)),
            )
    }

    /// 活动栏分区分隔线（24×1 水平线，深壳白 14% / 浅壳黑 14%）
    fn activity_divider(cx: &Context<Self>) -> impl IntoElement {
        div()
            .mx(px(6.))
            .my_1()
            .w_6()
            .h(px(1.))
            .rounded_full()
            .bg(tokens::activity_bar_colors(cx).divider)
    }
}

impl Focusable for SapApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SapApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 弹窗 / 通知 / tooltip / 触摸选择浮层自 gpui-kit v0.7.0 起由 component 侧的
        // WindowState 插件在 Root 内统一渲染（component::init 自动注册，gpui_kit::init
        // 已转发），不再需要应用层手动 render_dialog_layer / render_notification_layer

        // 根布局：h_flex 顶层（活动栏 48px + 右侧主内容区填满剩余）
        //
        // 修复历程（7 个 commit，前 6 个失败）：
        //   1. 早期 size_full() / flex_1() 各种尝试 → 失败
        //   2. 扁平化 5→3 层嵌套 → 失败
        //   3. SapApp 根 size_full → absolute().inset_0()（绕过 Entity 边界）→ 部分改善
        //      但 h_flex 默认 align-items 是 center（gpui-kit 故意这样设计，
        //      让一行图标+标签水平居中），所以右侧 v_flex 被垂直居中、上下都有空白
        //   4. 加 .items_stretch() → 让 column 子项拉满行高 ✓
        //
        // 最终方案：absolute().inset_0() + items_stretch()
        //   - absolute().inset_0() 解决 Entity 边界 size_full() 失效
        //   - items_stretch() 让右侧 v_flex 拉满 h_flex 高度
        // 参考 gpui-kit 文档："A row whose children are columns says items_stretch()"
        // Island 浮动布局（v1.5.7 方案 B，设置可切换，默认关）：开启时根底色换
        // canvas 层（比 background 深一档、带 alpha 透 Mica），活动栏与右列各成
        // 一块圆角浮岛；浮岛表面仍是 background（标题栏/MainPage 铺色不变、
        // 浮岛内零改动），间隙透出 canvas。锁定页走经典布局不受影响。
        let island = {
            let s = AppState::global(cx).store.lock();
            s.settings.island_layout
        };

        let root = h_flex()
            .id("sap-app")
            .absolute()
            .inset_0()
            .items_stretch()
            .track_focus(&self.focus_handle)
            // 自动锁定活动探针：未被子级消费的键鼠事件冒泡到根时重置闲置计时
            .on_key_down(|_, _, cx| crate::state::touch_activity(cx))
            // Esc 返回主界面（2026-09-29 标题条合一配套）：面板打开时 Esc 关面板。
            // 不冲突性：弹窗/弹层打开时 Esc 由 gpui-kit Dialog/Popover 宿主消费、
            // 不会冒泡到这里；命令面板与凭据表单自带 Esc 处理并 stop_propagation。
            // 面板态 Ctrl+F 转投命令面板（v1.5.3 设计，v1.5.7 搜索回退后恢复）：
            // 搜索框仅凭据页渲染（MainPage 未挂载时其 Ctrl+F 处理器不触发），
            // 全局搜索语义由 Ctrl+K 命令面板承担（全字段模糊 + Enter 直接登录）。
            // 主界面态打字即搜（type_to_search，仅凭据页生效）
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.active_panel.is_some() {
                    this.close_panel_impl(cx);
                } else if event.keystroke.modifiers.control
                    && event.keystroke.key == "f"
                    && this.active_panel.is_some()
                {
                    cx.stop_propagation();
                    cx.dispatch_action(&OpenCommandPalette);
                } else {
                    this.type_to_search(event, window, cx);
                }
            }))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                crate::state::touch_activity(cx)
            })
            .on_mouse_move(|_, _, cx| crate::state::touch_activity(cx))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::open_sap_import))
            .on_action(cx.listener(Self::open_export))
            .on_action(cx.listener(Self::open_group_manager))
            .on_action(cx.listener(Self::lock_app_action))
            .on_action(cx.listener(Self::show_about))
            .on_action(cx.listener(Self::open_command_palette))
            .on_action(cx.listener(Self::open_script_library))
            .bg(if island {
                tokens::island_canvas(cx)
            } else {
                cx.theme().background
            });
        // island 拓扑竖排（顶岛在上、rail+内容岛在下），classic 保持横排
        let root = if island { root.flex_col() } else { root };

        if let Some(page) = self.main_page.clone() {
            // 已解锁：左侧活动栏（48px）+ 右侧主内容区（占满剩余）
            // 右侧 = TitleBar（面板态含返回+标题）+ (面板 or MainPage)
            let panel = self.active_panel.clone();

            let right_content = if let Some(panel) = panel {
                // 面板模式：标题条合一后，返回按钮/标题已并入自绘标题栏
                //（render_title_bar 的 panel_title 分支），这里只剩内容容器
                //（relative：供面板内视图 absolute().inset_0() 锚定）
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(div().flex_1().min_h_0().min_w_0().relative().child(panel))
            } else {
                // 正常模式：MainPage
                // wrapper 加 relative：MainPage 根用 absolute().inset_0() 锚定到此容器
                //（Entity 边界处 flex_1/size_full 高度解析不可靠——同 SapApp 根的修法）
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(div().flex_1().min_h_0().min_w_0().relative().child(page))
            };

            if !island {
                // classic：rail + 右列（标题栏 + 内容）边到边，v1.5.x 原布局
                root.child(self.render_activity_bar(cx)).child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        // 自绘标题栏：拖拽区 + 面板态返回/标题 + 窗口三键
                        .child(self.render_title_bar(cx, false, self.panel_title.clone()))
                        .child(right_content),
                )
            } else {
                // island（v1.5.7 方案 Y 贴边 chrome + 浮动内容岛）：
                // rail 直角贴边通高（同 classic 形态）；顶岛取消——搜索/拖拽/
                // 窗控为内容区上方的一条透明细 chrome 行（窗控贴窗口右上角、
                // 拖拽区最大化，参考 Win11 原生与 JLInking 式「直角 chrome +
                // 浮起内容」语言）。「岛」只保留给内容区：canvas 之上浮动
                // 圆角内容岛，层级 = rail(贴边) < canvas < 内容岛(浮)。
                // 搜索框仅凭据页显示（v1.5.7 回退）；面板返回/标题仍在内容岛
                // header（render_panel_header）。内容岛自铺背景 + 圆角
                //（overflow_hidden 是矩形裁切，页面根经 tokens::page_bg 透明）
                let mut content_island = v_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .mt(px(tokens::ISLAND_GAP))
                    .ml(px(tokens::ISLAND_GAP))
                    .mr(px(tokens::ISLAND_GAP))
                    .mb(px(tokens::ISLAND_GAP))
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .overflow_hidden();
                if let Some(title) = self.panel_title.clone() {
                    content_island = content_island.child(self.render_panel_header(cx, title));
                }

                root.child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .items_stretch()
                        .child(self.render_activity_bar(cx))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                // 顶部细 chrome 行：无岛容器、透明底（canvas 透出），
                                // 右缘直达窗口边——窗控贴右上角（Win11 原生习惯）。
                                // 左缘补 ISLAND_GAP：行内容与内容岛左缘共线（spine 对齐）
                                .child(
                                    self.title_chrome_row(cx, false, false)
                                        .pl(px(tokens::ISLAND_GAP)),
                                )
                                .child(content_island.child(right_content)),
                        ),
                )
            }
            // 弹窗/通知层 v0.7.0 由 kit 插件统一渲染（见 render 顶部说明）。
            // 行为差异：弹窗遮罩由「仅活动栏右侧」变为全窗范围
            //（Dialog 居中坐标仍以窗口为准，弹窗本体的定位不受影响）
            .into_any_element()
        } else {
            // 锁定：精简标题栏（无应用名/锁定/置顶）+ 解锁页占满剩余空间
            //
            // 整体绘制在高优先级 deferred 层（300 > gpui-kit tooltip 浮层的 200）：
            // tooltip 浮层挂在 Root 顶层且无公开的清除 API，悬停元素触发 tooltip
            // 后若被自动锁定卸载（如空闲超时锁定时鼠标正停在按钮上），浮层会
            // 残留在锁定页上方持续显示（上游修复 PR #2659 已被放弃，暂无库层
            // 修复）。这里用更高绘制优先级的锁定层盖住残留浮层；解锁回主界面
            // 后悬停机制自动恢复，无需额外处理。
            // 背景垫同色不透明底：Mica 模式下页面背景带 alpha，盖不住不透明的
            // tooltip 胶囊本体，必须先垫一块去掉 alpha 的同色纯色底。
            let mut opaque_bg = cx.theme().background;
            opaque_bg.a = 1.0;
            let locked = div().absolute().inset_0().bg(opaque_bg).child(
                root.child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .child(self.render_title_bar(cx, true, None))
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .min_w_0()
                                .relative()
                                .child(self.unlock_page.clone()),
                        ),
                ),
            );
            // 通知层由 kit 插件统一渲染（v0.7.0）；锁定页整体绘制在 priority 300
            // 的 deferred 层（高于通知/提示浮层），锁定前发出的 toast（如「已发起
            // SAP 登录」）不会残留在锁定页——v1.4.0 的安全边界语义保持
            gpui::deferred(locked).with_priority(300).into_any_element()
        }
    }
}

/// 导入来源（导入面板三选一：SAP 配置 / JSON 备份 / BAT 脚本）
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportSource {
    /// SAP 配置导入（本机 Landscape 表格页）
    SapConfig,
    /// JSON 备份文件（明文 JSON，包含所有凭据密码，切勿外发）
    Backup,
    /// sapshcut BAT 脚本（批量录入）
    Bat,
}
/// 应用主题（供设置页调用）：写入存储并切换。
///
/// v5 Should #4：不再预改 guard（那样 `save_settings` 的 `mem::replace`
/// 捕获到的 old 已是新值，失败回滚等于没回滚）。改为只读快照 → 改副本 →
/// 保存命令 → 失败即通知用户并 return，绝不假装「已切换」。
pub fn apply_theme_and_save(theme: &str, window: &mut Window, cx: &mut App) {
    let store = AppState::global(cx).store.clone();
    let mut settings = {
        let s = store.lock();
        s.settings.clone()
    };
    settings.theme = theme.to_string();
    if let Err(e) = sap_backend::commands::settings::save_settings(&store, settings) {
        log::warn!("保存主题设置失败：{e}");
        window.push_notification(tf("保存主题设置失败：{e}", &[&e.to_string()]), cx);
        return;
    }
    crate::state::apply_theme(theme, Some(window), cx);
}

/// UI 冒烟测试（健康度整改 Step C）。
///
/// 故意不用 `#[gpui::test]` 宏：该宏展开在本工具链上会触发 rustc
/// STATUS_STACK_OVERFLOW（2026-11-03 已用最小用例隔离定位），改为手动
/// 构造 `TestAppContext::build(TestDispatcher::new(seed), None)`（两者均
/// 为 pub，已对照 gpui-pre 0.3.9 源码核实）。因此本模块不 glob 导入
/// gpui_kit（test-support 下其 `test` 宏会遮蔽内建 `#[test]`）。
#[cfg(test)]
mod smoke_tests {
    use super::SapApp;
    use crate::state::AppState;
    use gpui_kit::AppContext as _;

    /// 冒烟链路：init → AppState → 建窗 → 渲染一帧根视图。
    /// 首次启动（无 store.json）应走「设置主密码」引导页且不 panic。
    #[test]
    fn smoke_app_root_renders_setup_page() {
        let mut cx = gpui_kit::TestAppContext::build(gpui_kit::TestDispatcher::new(0), None);
        cx.update(|cx| {
            gpui_kit::init(cx);
            AppState::init(cx);
        });
        // 参照 kit 官方测试范式（base/src/button.rs）：add_window_view +
        // window.draw() 强制走一帧完整 layout/prepaint/paint。
        // 注意闭包签名与 main.rs 的 open_window 不同：这里收到 Context<V>
        // 且需直接返回 V（Root 本体），而非 Entity。
        let (_, cx) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| SapApp::new(window, cx));
            gpui_kit::component::Root::new(view, window, cx)
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }

    /// 测试用空视图：仅用于给 active_panel 造一个真实存在的面板。
    struct EmptyTestView;

    impl gpui_kit::Render for EmptyTestView {
        fn render(
            &mut self,
            _window: &mut gpui_kit::Window,
            _cx: &mut gpui_kit::Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            gpui_kit::div()
        }
    }

    /// 回归 v4 Must #4：锁定必须一次清空面板与全局引用，
    /// 否则主密码副本驻留整个锁定期，解锁后还会复现锁定前的旧面板。
    #[test]
    fn lock_clears_panel_state() {
        let mut cx = gpui_kit::TestAppContext::build(gpui_kit::TestDispatcher::new(0), None);
        cx.update(|cx| {
            gpui_kit::init(cx);
            AppState::init(cx);
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let app = cx.new(|cx| SapApp::new(window, cx));
            app.update(cx, |app, cx| {
                // v5 Should #12：旧版测试只预置并断言 4 个面板字段，删掉
                // `AppState::global_mut(cx).main_page = None` 整行测试仍全绿。
                // 现在把全局引用与主密码副本一并预置并断言。
                let page = cx.new(|cx| {
                    super::main_page::MainPage::new(
                        super::MasterSecret::new(String::new()),
                        window,
                        cx,
                    )
                });
                app.main_page = Some(page.clone());
                AppState::global_mut(cx).main_page = Some(page.downgrade());
                app.master_password = Some(secrecy::SecretBox::new("secret".into()));
                app.session_password = Some(secrecy::SecretBox::new("secret".into()));
                app.active_panel = Some(cx.new(|_| EmptyTestView).into());
                app.panel_title = Some("编辑凭据".to_string());
                app.panel_kind = Some("add");
                app.lock(window, cx);
                assert!(app.active_panel.is_none(), "锁定未清 active_panel");
                assert!(app.panel_title.is_none(), "锁定未清 panel_title");
                assert!(app.panel_kind.is_none(), "锁定未清 panel_kind");
                assert!(app.search_input.is_none(), "锁定未清 search_input");
                assert!(app.main_page.is_none(), "锁定未清 main_page 实体");
                assert!(app.master_password.is_none(), "锁定未清主密码副本");
                // 免密关闭时会话密码也必须一并清除
                assert!(app.session_password.is_none(), "锁定未清会话密码副本");
                assert!(
                    AppState::global(cx).main_page.is_none(),
                    "锁定未清 AppState.main_page 全局引用"
                );
            });
            gpui_kit::component::Root::new(app, window, cx)
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }
}
