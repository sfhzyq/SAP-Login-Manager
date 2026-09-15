//! SAP Login Manager - GPUI 版 UI
//!
//! 根视图：解锁页 / 主界面路由 + 顶栏 + Dialog / Notification 层

pub mod command_palette;
pub mod credential_card;
pub mod credential_form;
pub mod group_manager;
pub mod i18n;
pub mod main_page;
pub mod sap_import;
pub mod settings_page;
pub mod tokens;
pub mod unlock_page;

use i18n::{t, tf};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Root, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::state::AppState;
use sap_backend::commands::{auth, transfer};
use sap_backend::models::ExportBundle;
use secrecy::{ExposeSecret as _, SecretBox, SecretString};

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
        OpenCommandPalette
    ]
);

/// 从任意子视图打开分组管理面板
///
/// 不用 dispatch_action：action 沿焦点链冒泡，窗口无焦点时会被静默丢弃
/// （表现为点「管理分组」没反应）。改为经 AppState 的根视图弱引用直接调用。
pub(crate) fn request_open_group_manager(window: &mut Window, cx: &mut App) {
    if let Some(app) = AppState::global(cx).weak_app.clone().and_then(|w| w.upgrade()) {
        app.update(cx, |app, cx| {
            app.open_group_manager(&OpenGroupManager, window, cx)
        });
    }
}

/// 打开凭据表单右侧面板（新增/编辑通用）
///
/// 编辑也走面板而非弹窗：面板内容区自带滚动、底栏固定，
/// 小窗口下取消/保存按钮不会被挤出可视区。
pub fn open_credential_panel(
    cred_id: Option<String>,
    master_password: String,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(app) = AppState::global(cx).weak_app.clone().and_then(|w| w.upgrade()) else {
        return;
    };
    app.update(cx, |app, cx| {
        let is_edit = cred_id.is_some();
        let editing = cred_id.and_then(|id| {
            let s = AppState::global(cx).store.lock();
            s.credentials.iter().find(|c| c.id == id).cloned()
        });
        let Some(main_page) = app.main_page.clone() else {
            return;
        };
        let mp = master_password.clone();
        let form = cx.new(|cx| {
            credential_form::CredentialForm::new(
                editing,
                mp,
                main_page.downgrade(),
                window,
                cx,
            )
        });
        // 面板嵌入模式：根节点锚定 + 内容区内部滚动
        let title = if is_edit { t("编辑凭据") } else { t("新增凭据") };
        app.open_panel(title, "add", form.into(), window, cx);
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
    /// 当前激活的右侧面板（None = 显示 MainPage；Some = 显示面板）
    active_panel: Option<AnyView>,
    /// 面板标题（用于面板标题栏）
    panel_title: Option<String>,
    /// 面板种类（活动栏按钮激活高亮用："add"/"groups"/"import"/"export"/"settings"/"cmd"/"about"）
    panel_kind: Option<&'static str>,
    /// 窗口置顶状态（自绘标题栏置顶按钮用）
    pinned: bool,
}

impl SapApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = AppState::global(cx).store.clone();
        // 载入界面语言（zh/en）
        i18n::set_lang(&store.lock().settings.ui_language.clone());
        // 首次使用判定：存储中尚未设置主密码
        let is_setup = !auth::is_password_set(&store).unwrap_or(false);

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

        Self {
            focus_handle: cx.focus_handle(),
            master_password: None,
            session_password: None,
            unlock_page,
            main_page: None,
            active_panel: None,
            panel_title: None,
            panel_kind: None,
            pinned: false,
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

    /// 解锁成功：记录主密码并切换到主界面
    pub fn unlock(&mut self, password: String, window: &mut Window, cx: &mut Context<Self>) {
        let store = AppState::global(cx).store.clone();
        let password_free = store.lock().settings.password_free;
        if password_free {
            self.session_password = Some(SecretBox::new(password.clone().into_boxed_str()));
        }
        self.master_password = Some(SecretBox::new(password.clone().into_boxed_str()));
        let page = cx.new(|cx| main_page::MainPage::new(password, window, cx));
        AppState::global_mut(cx).main_page = Some(page.downgrade());
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
        let title = if is_edit { t("编辑凭据") } else { t("新增凭据") };
        self.open_panel(title, "add", form.into(), window, cx);
    }

    /// 锁定：清除主密码并回到解锁页；免密模式保留会话密码供快速解锁
    pub fn lock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // SecretString Drop 时自动把缓冲区覆写为 0（显式置 None 触发）
        self.master_password = None;
        self.main_page = None;

        let store = AppState::global(cx).store.clone();
        let password_free = store.lock().settings.password_free;
        if !password_free {
            self.session_password = None;
        }

        // 重建解锁页（清空上次输入）
        let weak_app = cx.entity().downgrade();
        let session_password = self
            .session_password
            .as_ref()
            .map(|s| s.expose_secret().to_string());
        self.unlock_page = cx.new(|cx| {
            unlock_page::UnlockPage::new(false, session_password, weak_app, window, cx)
        });
        cx.notify();
    }

    fn master_password(&self) -> String {
        self.master_password
            .as_ref()
            .map(|s| s.expose_secret().to_string())
            .unwrap_or_default()
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
    fn title_control_btn(
        &self,
        cx: &Context<Self>,
        id: &'static str,
        icon: IconName,
        area: gpui::WindowControlArea,
    ) -> AnyElement {
        h_flex()
            .id(id)
            .w(px(44.))
            .h_full()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(cx.theme().muted_foreground)
            .hover(|s| s.bg(cx.theme().secondary).text_color(cx.theme().foreground))
            .window_control_area(area)
            .child(Icon::new(icon).size_4())
            .into_any_element()
    }

    /// 自绘标题栏：左拖拽区（原生拖拽/双击最大化）+ 右侧 [锁定][置顶][最小化][最大化][关闭]
    ///
    /// 不用组件库 TitleBar：其窗口控制按钮渲染在最右且不可插入自定义按钮。
    /// 拖拽区与窗口控制按钮声明 window_control_area，交给系统按原生
    /// 标题栏处理（拖动、双击最大化、Snap、Alt+F4 等行为全部保留）。
    ///
    /// `locked`：锁定态精简模式——不显示应用名和锁定/置顶按钮（对已锁定窗口无意义），
    /// 只保留拖拽区 + 最小化/最大化/关闭。
    fn render_title_bar(&self, cx: &Context<Self>, locked: bool) -> AnyElement {
        let pinned = self.pinned;

        h_flex()
            .flex_none()
            .h(px(36.))
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .bg(cx.theme().title_bar)
            // 左侧：拖拽区（+ 应用名，锁定态隐藏）
            .child(
                div()
                    .id("titlebar-drag")
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .pl(px(12.))
                    .window_control_area(gpui::WindowControlArea::Drag)
                    .children((!locked).then(|| {
                        div()
                            .text_size(px(tokens::TS_SM))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().muted_foreground)
                            .child("SAP Login Manager")
                    })),
            )
            // 右侧：（锁定 | 置顶）| 最小化 | 最大化 | 关闭
            .child(
                h_flex()
                    .flex_none()
                    .h_full()
                    .items_center()
                    // 锁定态隐藏锁定/置顶按钮（对已锁定窗口无意义）；
                    // 两键拉开 6px 间距防误触（与最小化之间同样留空）
                    .when(!locked, |row| {
                        row.child(
                            h_flex()
                                .items_center()
                                .gap(px(6.))
                                .mr(px(4.))
                                // 锁定（功能按钮：ghost 小图标，带 tooltip）
                                .child(
                                    Button::new("tb-lock")
                                        .ghost()
                                        .small()
                                        .icon(IconName::Lock)
                                        .tooltip(t("锁定 (Ctrl+L)"))
                                        .on_click(cx.listener(|this, _, window, cx| this.lock(window, cx))),
                                )
                                // 置顶（激活时主色填充以示区分）
                                .child(
                                    Button::new("tb-pin")
                                        .small()
                                        .icon(IconName::Pin)
                                        .tooltip(if pinned { t("取消置顶") } else { t("窗口置顶") })
                                        .map(|b| {
                                            if pinned {
                                                b.primary()
                                            } else {
                                                b.ghost()
                                            }
                                        })
                                        .on_click(cx.listener(SapApp::toggle_pin)),
                                ),
                        )
                    })
                    // 窗口控制：交给系统按原生 caption 按钮处理
                    .child(self.title_control_btn(cx, "tb-min", IconName::Minus, gpui::WindowControlArea::Min))
                    .child(self.title_control_btn(cx, "tb-max", IconName::Square, gpui::WindowControlArea::Max))
                    .child(self.title_control_btn(cx, "tb-close", IconName::X, gpui::WindowControlArea::Close)),
            )
            .into_any_element()
    }

    /// 打开设置面板（替代弹窗）
    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        let mp = self.master_password();
        let page = cx.new(|cx| settings_page::SettingsPage::new(mp, window, cx));
        self.open_panel(t("设置"), "settings", page.into(), window, cx);
    }

    /// 打开 SAP 配置导入面板
    fn open_sap_import(&mut self, _: &ImportFromSap, window: &mut Window, cx: &mut Context<Self>) {
        let mp = self.master_password();
        let main_page = self.main_page.clone().unwrap().downgrade();
        let page = cx.new(|cx| sap_import::SapImportPage::new(mp, main_page, window, cx));
        self.open_panel(t("从 SAP 配置导入"), "import", page.into(), window, cx);
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
        let panel = cx.new(|cx| {
            ExportPanel::new(pw_state, pw_for_click, cred_count, weak_app, cx)
        });
        self.open_panel(t("导出凭据"), "export", panel.into(), window, cx);
    }

    /// 备份导入面板（从导出面板底部入口进入；活动栏高亮沿用「导出」）
    pub fn open_backup_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pw_state = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t("输入主密码以验证"))
        });
        let pw_for_click = pw_state.clone();
        let weak_app = cx.entity().downgrade();
        let panel = cx.new(|cx| BackupImportPanel::new(pw_state, pw_for_click, weak_app, cx));
        self.open_panel(t("从备份文件导入"), "export", panel.into(), window, cx);
    }

    /// 打开分组管理面板
    pub(crate) fn open_group_manager(
        &mut self,
        _: &OpenGroupManager,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let main_page = self.main_page.clone().unwrap().downgrade();
        let page = cx.new(|cx| group_manager::GroupManagerPage::new(main_page, window, cx));
        self.open_panel(t("分组管理"), "groups", page.into(), window, cx);
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
        let palette = cx.new(|cx| command_palette::CommandPalette::new(main_page.downgrade(), window, cx));
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
        self.active_panel = None;
        self.panel_title = None;
        self.panel_kind = None;
        cx.notify();
    }

    /// 渲染左侧活动栏（48px 宽，全高，垂直排列图标按钮）
    ///
    /// 布局（分隔线分三区）：S logo → ＋新增 · 📁分组管理 ─ ⬇导入 · ⬆导出
    ///       ─ ⚙设置 · ℹ关于 → (弹性)
    /// 打开的面板对应按钮高亮（主色底 + 左侧竖条）；主题切换只保留设置页入口
    fn render_activity_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_none()
            .w(px(48.))
            .h_full()
            .items_center()
            .gap(px(2.))
            .py(px(4.))
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().border)
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
                    .mb(px(4.))
                    .rounded(px(8.))
                    .bg(cx.theme().primary)
                    .text_size(px(18.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(cx.theme().primary_foreground)
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.85))
                    .on_click(cx.listener(Self::close_panel))
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
            // 弹性间距（把设置+关于推到底部；锁定已移至自绘标题栏）
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

    /// 活动栏图标按钮（质感层）：
    /// - 激活面板对应按钮：主色底 + 主色图标 + 左侧 2px 主色竖条（VS Code 风格）
    /// - 未激活：图标用次要色弱化，ghost 自带悬停反馈
    /// - 外层 32×32 容器放大点击热区（原 small 按钮偏小）
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
        let active_bg = {
            let mut c = cx.theme().primary;
            c.a = 0.14;
            c
        };
        h_flex()
            .flex_none()
            .relative()
            .size(px(32.))
            .items_center()
            .justify_center()
            // 激活指示：左侧 2px 主色竖条（贴活动栏左缘）
            .when(active, |t| {
                t.child(
                    div()
                        .absolute()
                        .left(px(-8.))
                        .top(px(6.))
                        .bottom(px(6.))
                        .w(px(2.))
                        .rounded_full()
                        .bg(cx.theme().primary),
                )
            })
            .child(
                Button::new(id)
                    .icon(icon)
                    .small()
                    .ghost()
                    .tooltip(tooltip)
                    .when(active, |t| t.bg(active_bg).text_color(cx.theme().primary))
                    .when(!active, |t| t.text_color(cx.theme().muted_foreground))
                    .on_click(on_click),
            )
    }

    /// 活动栏分区分隔线（24×1 水平线，图标功能分组的视觉边界）
    fn activity_divider(cx: &Context<Self>) -> impl IntoElement {
        div()
            .mx(px(6.))
            .my(px(4.))
            .w(px(24.))
            .h(px(1.))
            .rounded_full()
            .bg(cx.theme().border)
    }
}

impl Focusable for SapApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for SapApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog_layer = Root::render_dialog_layer(window, cx);
        let notification_layer = Root::render_notification_layer(window, cx);

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
        let root = h_flex()
            .id("sap-app")
            .absolute()
            .inset_0()
            .items_stretch()
            .track_focus(&self.focus_handle)
            // 自动锁定活动探针：未被子级消费的键鼠事件冒泡到根时重置闲置计时
            .on_key_down(|_, _, cx| crate::state::touch_activity(cx))
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
            .bg(cx.theme().background);

        if let Some(page) = self.main_page.clone() {
            // 已解锁：左侧活动栏（48px）+ 右侧主内容区（占满剩余）
            // 右侧 = TitleBar + (面板 or MainPage)
            let panel = self.active_panel.clone();
            let panel_title = self.panel_title.clone();

            let right_content = if let Some(panel) = panel {
                // 面板模式：标题栏 + 返回按钮 + 面板内容
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    // 面板标题栏（与自绘标题栏同底色，视觉连续）：
                    // 返回按钮带浅色圆角底（明确的按钮暗示），标题绝对居中（页面感）
                    .child(
                        h_flex()
                            .relative()
                            .flex_none()
                            .items_center()
                            .h(px(40.))
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().title_bar)
                            // 居中标题（绝对定位覆盖层，无交互不挡返回按钮点击）
                            .child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .max_w(px(280.))
                                            .truncate()
                                            .text_size(px(tokens::TS_MD))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(cx.theme().foreground)
                                            .child(panel_title.unwrap_or_default()),
                                    ),
                            )
                            .child(
                                Button::new("panel-back")
                                    .secondary()
                                    .small()
                                    .icon(IconName::ArrowLeft)
                                    .label(t("返回"))
                                    .tooltip(t("返回主界面"))
                                    .on_click(cx.listener(Self::close_panel)),
                            ),
                    )
                    // 面板内容（relative：供面板内视图 absolute().inset_0() 锚定）
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .relative()
                            .child(panel),
                    )
            } else {
                // 正常模式：MainPage
                // wrapper 加 relative：MainPage 根用 absolute().inset_0() 锚定到此容器
                //（Entity 边界处 flex_1/size_full 高度解析不可靠——同 SapApp 根的修法）
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .relative()
                            .child(page),
                    )
            };

            root.child(self.render_activity_bar(cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        // 自绘标题栏：拖拽区 + 锁定/置顶/最小化/最大化/关闭
                        .child(self.render_title_bar(cx, false))
                        .child(right_content),
                )
                .children(dialog_layer)
                .children(notification_layer)
        } else {
            // 锁定：精简标题栏（无应用名/锁定/置顶）+ 解锁页占满剩余空间
            root.child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(self.render_title_bar(cx, true))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .relative()
                            .child(self.unlock_page.clone()),
                    ),
            )
            .children(dialog_layer)
            .children(notification_layer)
        }
    }
}

/// 导出凭据面板（替代弹窗）
pub struct ExportPanel {
    pw_state: Entity<InputState>,
    pw_for_click: Entity<InputState>,
    cred_count: usize,
    weak_app: WeakEntity<SapApp>,
    /// 用户通过系统对话框选择的导出路径（None = 默认 data/exports/ 路径）
    save_path: Option<std::path::PathBuf>,
}

impl ExportPanel {
    pub fn new(
        pw_state: Entity<InputState>,
        pw_for_click: Entity<InputState>,
        cred_count: usize,
        weak_app: WeakEntity<SapApp>,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self { pw_state, pw_for_click, cred_count, weak_app, save_path: None }
    }
}

impl Render for ExportPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pw_state = self.pw_state.clone();
        let pw_for_click = self.pw_for_click.clone();
        let cred_count = self.cred_count;
        let weak_app = self.weak_app.clone();
        let chosen = self.save_path.clone();
        let path_display = chosen
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| export_path().display().to_string());

        v_flex()
            .gap_3()
            .p_3()
            // 危险提示
            .child(
                h_flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .py(px(8.))
                    .rounded(cx.theme().radius)
                    .bg({
                        let mut c = cx.theme().danger;
                        c.a = 0.08;
                        c
                    })
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .size_4()
                            .text_color(cx.theme().danger),
                    )
                    .child(
                        div()
                            .text_size(px(tokens::TS_BASE))
                            .text_color(cx.theme().danger)
                            .child(t("导出文件包含明文密码，请妥善保管！")),
                    ),
            )
            // 预览数量
            .child(
                div()
                    .text_size(px(tokens::TS_BASE))
                    .text_color(cx.theme().muted_foreground)
                    .child(tf("将导出 {cred_count} 条凭据到 JSON 文件", &[&cred_count.to_string()])),
            )
            // 导出位置：默认 data/exports/，可点「浏览…」用系统对话框另选
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(6.))
                            .min_w_0()
                            .child(
                                Icon::new(IconName::FolderOpen)
                                    .size_4()
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(tokens::TS_SM))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(path_display),
                            ),
                    )
                    .child(
                        Button::new("export-browse")
                            .ghost()
                            .xsmall()
                            .icon(IconName::FolderOpen)
                            .label(t("浏览…"))
                            .tooltip(t("选择导出位置"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let default = export_path();
                                let mut dialog = rfd::FileDialog::new().add_filter("JSON", &["json"]);
                                if let Some(dir) = default.parent() {
                                    dialog = dialog.set_directory(dir);
                                }
                                if let Some(name) = default.file_name().and_then(|n| n.to_str()) {
                                    dialog = dialog.set_file_name(name);
                                }
                                if let Some(p) = dialog.save_file() {
                                    this.save_path = Some(p);
                                    cx.notify();
                                }
                            })),
                    ),
            )
            // 密码输入
            .child(Input::new(&pw_state))
            // 确认按钮
            .child(
                Button::new("export-confirm")
                    .primary()
                    .label(t("验证并导出"))
                    .w_full()
                    .on_click(move |_, window, cx| {
                        let pw = pw_for_click.read(cx).value().to_string();
                        if pw.is_empty() {
                            window.push_notification(t("请输入主密码"), cx);
                            return;
                        }
                        let store = AppState::global(cx).store.clone();
                        let save_path =
                            chosen.clone().unwrap_or_else(export_path);
                        match transfer::export_credentials(
                            &store,
                            pw,
                            save_path.to_string_lossy().into_owned(),
                        ) {
                            Ok(()) => {
                                // 关闭面板
                                if let Some(app) = weak_app.upgrade() {
                                    app.update(cx, |app, cx| {
                                        app.active_panel = None;
                                        app.panel_title = None;
                                        app.panel_kind = None;
                                        cx.notify();
                                    });
                                }
                                window.push_notification(
                                    tf("导出成功：{}", &[&save_path.display().to_string()]),
                                    cx,
                                );
                            }
                            Err(e) => {
                                window.push_notification(tf("导出失败：{e}", &[&e.to_string()]), cx);
                            }
                        }
                    }),
            )
            // 已有备份文件？切到备份导入（活动栏高亮沿用导出）
            .child(
                h_flex().justify_center().child(
                    Button::new("export-to-backup-import")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Import)
                        .label(t("从备份文件导入"))
                        .on_click({
                            let weak_app_link = self.weak_app.clone();
                            move |_, window, cx| {
                                if let Some(app) = weak_app_link.upgrade() {
                                    app.update(cx, |app, cx| app.open_backup_import(window, cx));
                                }
                            }
                        }),
                ),
            )
    }
}

/// 导出文件路径：exe 目录 data/exports/sap-credentials-{时间戳}.json
fn export_path() -> std::path::PathBuf {
    let dir = crate::data_dir().join("exports");
    let _ = std::fs::create_dir_all(&dir);
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    dir.join(format!("sap-credentials-{ts}.json"))
}

/// 备份导入面板：选择此前导出的 JSON 备份文件，验证主密码后合并导入
///
/// 备份中的连接按 connection_id 去重（已存在的跳过），
/// 明文密码会用当前主密码重新加密后入库。
pub struct BackupImportPanel {
    file: Option<std::path::PathBuf>,
    bundle: Option<ExportBundle>,
    pw_state: Entity<InputState>,
    pw_for_click: Entity<InputState>,
    weak_app: WeakEntity<SapApp>,
}

impl BackupImportPanel {
    pub fn new(
        pw_state: Entity<InputState>,
        pw_for_click: Entity<InputState>,
        weak_app: WeakEntity<SapApp>,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self { file: None, bundle: None, pw_state, pw_for_click, weak_app }
    }

    /// 用系统对话框选择备份文件并解析预览
    fn pick_file(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file() {
            match std::fs::read_to_string(&p)
                .map_err(|e| e.to_string())
                .and_then(|content| {
                    serde_json::from_str::<ExportBundle>(&content).map_err(|e| e.to_string())
                }) {
                Ok(bundle) => {
                    self.file = Some(p);
                    self.bundle = Some(bundle);
                }
                Err(_) => {
                    window.push_notification(t("备份文件无效或已损坏"), cx);
                }
            }
            cx.notify();
        }
    }
}

impl Render for BackupImportPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pw_state = self.pw_state.clone();
        let pw_for_click = self.pw_for_click.clone();
        let weak_app = self.weak_app.clone();
        let bundle = self.bundle.clone();
        let file_info: Option<AnyElement> = self.bundle.as_ref().map(|bundle| {
            let name = self
                .file
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let time = bundle.exported_at.with_timezone(&chrono::Local);
            let time_str = time.format("%Y-%m-%d %H:%M").to_string();
            v_flex()
                .gap_1()
                .p_2()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            h_flex()
                                .items_center()
                                .gap(px(6.))
                                .min_w_0()
                                .child(
                                    Icon::new(IconName::File)
                                        .size_4()
                                        .text_color(cx.theme().muted_foreground),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(tokens::TS_SM))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(cx.theme().foreground)
                                        .child(name),
                                ),
                        )
                        .child(
                            Button::new("backup-pick-again")
                                .ghost()
                                .xsmall()
                                .label(t("重新选择"))
                                .on_click(cx.listener(Self::pick_file)),
                        ),
                )
                .child(
                    div()
                        .text_size(px(tokens::TS_XS))
                        .text_color(cx.theme().muted_foreground)
                        .child(tf(
                            "包含 {cred_count} 条连接、{group_count} 个分组 · 备份时间：{time}",
                            &[
                                &bundle.connection_count.to_string(),
                                &bundle.group_count.to_string(),
                                &time_str,
                            ],
                        )),
                )
                .into_any_element()
        });

        // 未选文件时的选择入口
        let pick_entry = if self.bundle.is_none() {
            Some(
                Button::new("backup-pick")
                    .ghost()
                    .icon(IconName::FolderOpen)
                    .label(t("选择 JSON 备份文件"))
                    .w_full()
                    .on_click(cx.listener(Self::pick_file)),
            )
        } else {
            None
        };

        v_flex()
            .gap_3()
            .p_3()
            // 提示
            .child(
                div()
                    .text_size(px(tokens::TS_BASE))
                    .text_color(cx.theme().muted_foreground)
                    .child(t("选择此前导出的 JSON 备份，连接将合并导入（同名跳过），密码用当前主密码重新加密。")),
            )
            .children(pick_entry)
            .children(file_info)
            // 主密码输入（用于验证 + 重新加密）
            .child(Input::new(&pw_state))
            // 确认按钮
            .child(
                Button::new("backup-import-confirm")
                    .primary()
                    .label(t("验证并导入"))
                    .w_full()
                    .on_click(move |_, window, cx| {
                        let Some(bundle) = bundle.clone() else {
                            window.push_notification(t("未选择备份文件"), cx);
                            return;
                        };
                        let pw = pw_for_click.read(cx).value().to_string();
                        if pw.is_empty() {
                            window.push_notification(t("请输入主密码"), cx);
                            return;
                        }
                        let store = AppState::global(cx).store.clone();
                        // 先验证主密码：导入会把明文密码用该密码重新加密，
                        // 密码错误会导致新导入的凭据永远无法解密
                        match auth::verify_master_password(&store, pw.clone()) {
                            Ok(true) => {}
                            Ok(false) => {
                                window.push_notification(t("主密码错误"), cx);
                                return;
                            }
                            Err(e) => {
                                window.push_notification(tf("导入失败：{e}", &[&e.to_string()]), cx);
                                return;
                            }
                        }
                        match transfer::import_credentials(&store, pw, bundle) {
                            Ok(0) => {
                                window.push_notification(t("备份中的连接均已存在，未新增"), cx);
                            }
                            Ok(n) => {
                                if let Some(app) = weak_app.upgrade() {
                                    app.update(cx, |app, cx| {
                                        if let Some(page) = app.main_page.clone() {
                                            page.update(cx, |p, cx| p.refresh(cx));
                                        }
                                        app.active_panel = None;
                                        app.panel_title = None;
                                        app.panel_kind = None;
                                        cx.notify();
                                    });
                                }
                                window.push_notification(
                                    tf("已导入 {n} 条连接", &[&n.to_string()]),
                                    cx,
                                );
                            }
                            Err(e) => {
                                window.push_notification(tf("导入失败：{e}", &[&e.to_string()]), cx);
                            }
                        }
                    }),
            )
    }
}

/// 项目主页地址（关于面板展示/复制/打开）
pub const REPO_URL: &str = "https://github.com/sfhzyq/SAP-LoginManager";

/// 关于面板（替代弹窗）
pub struct AboutPanel {
    version: String,
}

impl AboutPanel {
    pub fn new(version: String) -> Self {
        Self { version }
    }
}

impl Render for AboutPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let version = self.version.clone();
        // 锚定面板容器 + 整页滚动（致谢列表较长，小窗口下需要滚动）
        // 四周留白：横向 padding 防止窄窗口下内容贴边
        v_flex()
            .absolute()
            .inset_0()
            .id("about-scroll")
            .overflow_y_scroll()
            .gap(px(16.))
            .px(px(24.))
            .py(px(24.))
            .items_center()
            // S logo
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(56.))
                    .rounded(px(14.))
                    .bg(cx.theme().primary)
                    .text_size(px(28.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(cx.theme().primary_foreground)
                    .child("S"),
            )
            // 名称 + 版本
            .child(
                v_flex()
                    .gap(px(4.))
                    .items_center()
                    .child(
                        div()
                            .text_size(px(tokens::TS_LG))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child(t("SAP 登录管理器")),
                    )
                    .child(
                        div()
                            .text_size(px(tokens::TS_BASE))
                            .text_color(cx.theme().muted_foreground)
                            .child(tf("版本 {version}", &[&version.to_string()])),
                    ),
            )
            // 描述
            .child(
                v_flex()
                    .gap(px(2.))
                    .items_center()
                    .child(
                        div()
                            .text_size(px(tokens::TS_SM))
                            .text_color(cx.theme().muted_foreground)
                            .child(t("Rust + GPUI 原生")),
                    )
                    .child(
                        div()
                            .text_size(px(tokens::TS_SM))
                            .text_color(cx.theme().muted_foreground)
                            .child(t("凭据本地加密存储，无云端依赖")),
                    ),
            )
            // 项目主页（GitHub 仓库）
            .child(
                h_flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        Icon::new(IconName::Github)
                            .size_4()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_size(px(tokens::TS_SM))
                            .text_color(cx.theme().primary)
                            .child("github.com/sfhzyq/SAP-LoginManager"),
                    )
                    .child(
                        Button::new("about-open-repo")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ExternalLink)
                            .label(t("打开"))
                            .tooltip(t("在浏览器中打开项目主页"))
                            .on_click(|_, _, _| {
                                let _ = std::process::Command::new("cmd")
                                    .args(["/C", "start", "", REPO_URL])
                                    .spawn();
                            }),
                    )
                    .child(
                        Button::new("about-copy-repo")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Copy)
                            .label(t("复制链接"))
                            .tooltip(t("点击复制项目链接"))
                            .on_click(|_, window, cx| {
                                if let Ok(mut cb) = arboard::Clipboard::new() {
                                    let _ = cb.set_text(REPO_URL);
                                }
                                window.push_notification(t("链接已复制到剪贴板"), cx);
                            }),
                    ),
            )
            // 开源致谢：主要依赖及链接
            .child(Self::render_oss_list(cx))
    }
}

impl AboutPanel {
    /// 开源项目致谢列表（名称 + 链接 + 用途）
    ///
    /// 完整依赖清单见 gpui-version/Cargo.toml 与 Cargo.lock；
    /// 这里列出直接依赖中有代表性的项目。
    fn render_oss_list(cx: &Context<Self>) -> AnyElement {
        // (名称, 链接, 用途)——运行时构建以便 i18n 翻译用途描述
        let oss: Vec<(&str, &str, &str)> = vec![
            ("gpui", "https://gpui.rs", t("GPU 加速 UI 框架（Zed）")),
            ("gpui-kit", "https://github.com/longbridge/gpui-component", t("UI 组件库（按钮/输入/菜单/主题）")),
            ("serde", "https://github.com/serde-rs/serde", t("数据序列化")),
            ("chrono", "https://github.com/chronotope/chrono", t("日期时间")),
            ("uuid", "https://github.com/uuid-rs/uuid", t("凭据 ID 生成")),
            ("aes / aes-gcm", "https://github.com/RustCrypto/block-ciphers", t("凭据加密")),
            ("sha2 / hmac / pbkdf2", "https://github.com/RustCrypto/hashes", t("主密码派生与校验")),
            ("arboard", "https://github.com/1Password/arboard", t("跨平台剪贴板")),
            ("xmltree", "https://github.com/PoiScript/xmltree", t("SAP Logon 配置解析")),
            ("winreg", "https://github.com/gentoo/winreg-rs", t("Windows 注册表读取")),
            ("windows-sys", "https://github.com/microsoft/windows-rs", t("Win32 窗口控制/剪贴板")),
            ("base64", "https://github.com/marshallpierce/rust-base64", t("Base64 编解码")),
            ("thiserror", "https://github.com/dtolnay/thiserror", t("错误定义")),
            ("log", "https://github.com/rust-lang/log", t("日志门面")),
        ];

        // 轻量卡片容器：名称/用途一行，右侧「复制链接」按钮（点击复制完整 URL）
        let rows = oss.iter().map(|(name, url, desc)| {
            let url_static: &'static str = url;
            h_flex()
                .id(format!("oss-{}", name.replace([' ', '/'], "-")))
                .items_center()
                .justify_between()
                .gap_2()
                .px_2()
                .py(px(5.))
                .rounded(px(6.))
                .hover(|s| s.bg(cx.theme().secondary))
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(8.))
                        .min_w_0()
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(tokens::TS_SM))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().foreground)
                                .child(*name),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(tokens::TS_XS))
                                .text_color(cx.theme().muted_foreground)
                                .child(*desc),
                        ),
                )
                .child(
                    Button::new(format!("oss-copy-{}", name.replace([' ', '/'], "-")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Copy)
                        .label(t("复制链接"))
                        .tooltip(t("点击复制项目链接"))
                        .on_click(move |_, window, cx| {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                let _ = cb.set_text(url_static);
                            }
                            window.push_notification(t("链接已复制到剪贴板"), cx);
                        }),
                )
        });

        v_flex()
            .mt(px(8.))
            .w(px(460.))
            .max_w_full()
            .child(
                div()
                    .text_size(px(tokens::TS_SM))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens::text_secondary(cx))
                    .child(t("开源项目致谢")),
            )
            .child(
                div()
                    .mt_1()
                    .p_1()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .child(v_flex().children(rows)),
            )
            .into_any_element()
    }
}

/// 应用主题（供设置页调用）：写入存储并切换
pub fn apply_theme_and_save(theme: &str, window: &mut Window, cx: &mut App) {
    let store = AppState::global(cx).store.clone();
    {
        let mut s = store.lock();
        s.settings.theme = theme.to_string();
        let _ = sap_backend::storage::save_store(&s);
    }
    crate::state::apply_theme(theme, Some(window), cx);
}
