//! 设置弹窗：外观 / 安全 / 登录 / 列表 / 窗口
//!
//! 对应 Tauri 版 SettingsModal.tsx：
//! - 修改即时保存（与 Tauri 版一致）
//! - 免密模式：开启需验证主密码，经 DPAPI 加密记忆；关闭即清除
//! - 修改主密码：旧密码验证 + 新密码确认（所有凭据重新加密）

use super::i18n::{t, tf};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use sap_backend::commands::{auth, settings as settings_cmd};
use sap_backend::models::AppSettings;

use crate::state::{self, AppState};

/// 分组标题（Tauri 风格：小图标 + 小标题）
fn group_title(
    icon: IconName,
    title: &str,
    cx: &Context<SettingsPage>,
) -> impl IntoElement + use<> {
    h_flex()
        .items_center()
        .gap(px(7.))
        .pt(px(6.))
        .text_size(px(super::tokens::TS_SM))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(icon).size_3())
        .child(title.to_string())
}

/// 设置卡片容器（Tauri set-card 风格）：次级背景 + 边框 + 圆角，行间自动分隔线
fn card(rows: Vec<AnyElement>, cx: &Context<SettingsPage>) -> impl IntoElement + use<> {
    v_flex()
        .bg(cx.theme().secondary)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .children(rows.into_iter().enumerate().map(|(ix, row)| {
            div()
                .when(ix > 0, |this| {
                    this.border_t_1().border_color(cx.theme().border)
                })
                .child(row)
        }))
}

/// 行左侧：标签 + 可选描述
fn row_label(
    label: &str,
    desc: Option<&str>,
    cx: &Context<SettingsPage>,
) -> impl IntoElement + use<> {
    v_flex()
        .min_w_0()
        .flex_1()
        .gap(px(2.))
        .child(
            div()
                .text_size(px(super::tokens::TS_BASE))
                .text_color(cx.theme().foreground)
                .child(label.to_string()),
        )
        .when_some(desc.map(|s| s.to_string()), |this, d| {
            this.child(
                div()
                    .text_size(px(super::tokens::TS_XS))
                    .text_color(super::tokens::text_secondary(cx))
                    .child(d),
            )
        })
}

/// 开关行：左标签 + 右 Switch
fn toggle_row(
    id: &'static str,
    label: &str,
    desc: Option<&str>,
    checked: bool,
    cx: &Context<SettingsPage>,
    on_toggle: impl Fn(&mut SettingsPage, &mut Window, &mut Context<SettingsPage>) + 'static,
) -> AnyElement {
    h_flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px(px(14.))
        .py(px(11.))
        .child(row_label(label, desc, cx))
        .child(
            Switch::new(id)
                .checked(checked)
                .on_click(cx.listener(move |this, _, window, cx| {
                    on_toggle(this, window, cx);
                })),
        )
        .into_any_element()
}

/// 单选下拉行：左标签 + 右下拉按钮（Tauri set-row 风格）
fn choice_row(
    id: &'static str,
    label: &str,
    desc: Option<&str>,
    current: String,
    options: Vec<(String, String)>,
    cx: &Context<SettingsPage>,
    on_pick: impl Fn(&mut SettingsPage, String, &mut Window, &mut Context<SettingsPage>) + Clone + 'static,
) -> AnyElement {
    let active_value = options
        .iter()
        .find(|(_, l)| *l == current)
        .map(|(v, _)| v.clone());
    let weak = cx.entity().downgrade();
    h_flex()
        .items_center()
        .justify_between()
        .gap_2()
        .px(px(14.))
        .py(px(11.))
        .child(row_label(label, desc, cx))
        .child(
            Button::new(id)
                .secondary()
                .small()
                .icon(IconName::ChevronDown)
                .label(current)
                .dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for (value, text) in &options {
                        let value = value.clone();
                        let text = text.clone();
                        let checked = Some(&value) == active_value.as_ref();
                        let weak = weak.clone();
                        let on_pick = on_pick.clone();
                        menu = menu.item(
                            PopupMenuItem::new(text)
                                .checked(checked)
                                .on_click(move |_, window, cx| {
                                    if let Some(page) = weak.upgrade() {
                                        let value = value.clone();
                                        let on_pick = on_pick.clone();
                                        page.update(cx, |this, cx| {
                                            on_pick(this, value, window, cx);
                                        });
                                    }
                                }),
                        );
                    }
                    menu
                })
                .anchor(Anchor::BottomRight),
        )
        .into_any_element()
}

pub struct SettingsPage {
    master_password: String,
    /// SAP Logon 路径输入
    sap_logon_path: Entity<InputState>,
    /// 路径输入变化订阅
    _subscription: Subscription,
    /// 整页滚动 handle（滚动条 overlay 读取位置用）
    scroll: ScrollHandle,
}

impl SettingsPage {
    pub fn new(
        master_password: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = AppState::global(cx).store.clone();
        let path = {
            let s = store.lock();
            s.settings.sap_logon_path.clone().unwrap_or_default()
        };

        let sap_logon_path = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("留空自动检测 SAP Logon"))
        });
        if !path.is_empty() {
            sap_logon_path.update(cx, |s, cx| s.set_value(path, window, cx));
        }

        // 路径输入变化即时保存
        let subscription = cx.subscribe_in(
            &sap_logon_path,
            window,
            |this, _, _: &InputEvent, _, cx| {
                let path = this.sap_logon_path.read(cx).value().to_string();
                this.save_setting(cx, |s| {
                    s.sap_logon_path = if path.is_empty() { None } else { Some(path) }
                });
            },
        );

        Self {
            master_password,
            sap_logon_path,
            _subscription: subscription,
            scroll: ScrollHandle::new(),
        }
    }

    /// 保存单个设置项（读改写整份设置）
    fn save_setting(&mut self, cx: &mut Context<Self>, mutate: impl FnOnce(&mut AppSettings)) {
        let store = AppState::global(cx).store.clone();
        // 必须先释放 store 锁再调用 save_settings（其内部会再次 lock，同线程重入会死锁）
        let settings = {
            let mut s = store.lock();
            mutate(&mut s.settings);
            s.settings.clone()
        };
        if let Err(e) = settings_cmd::save_settings(&store, settings) {
            log::warn!("保存设置失败：{e}");
        }
    }

    /// 读取当前设置
    fn settings(&self, cx: &App) -> AppSettings {
        AppState::global(cx).store.lock().settings.clone()
    }

    /// 免密模式切换
    fn toggle_password_free(
        &mut self,
        enable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = AppState::global(cx).store.clone();
        if enable {
            // 开启：弹窗输入主密码验证后记忆（DPAPI）
            let pw_state = cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(t("输入主密码以验证"))
            });
            let pw_for_click = pw_state.clone();

            window.open_dialog(cx, move |dialog, _, _| {
                // content / on_click 均为 Fn 闭包，创建前先 clone 捕获变量
                let pw_state = pw_state.clone();
                let pw_for_click = pw_for_click.clone();
                let store = store.clone();
                dialog
                    .title(t("开启免密模式"))
                    .min_w(px(340.))
                    .content(move |content, _, _| {
                        let pw_for_click = pw_for_click.clone();
                        let store = store.clone();
                        content.child(
                            v_flex()
                                .gap_3()
                                .pb_2()
                                .child(
                                    div()
                                        .text_size(px(super::tokens::TS_BASE))
                                        .child(t("验证主密码后将用 Windows DPAPI 加密记忆，仅当前 Windows 用户可解密。")),
                                )
                                .child(Input::new(&pw_state))
                                .child(
                                    Button::new("enable-pw-free-confirm")
                                        .primary()
                                        .label(t("验证并开启"))
                                        .w_full()
                                        .on_click(move |_, window, cx| {
                                            let pw =
                                                pw_for_click.read(cx).value().to_string();
                                            if pw.is_empty() {
                                                window.push_notification(t("请输入主密码"), cx);
                                                return;
                                            }
                                            match auth::remember_master_password(
                                                &store, pw,
                                            ) {
                                                Ok(()) => {
                                                    let settings = {
                                                        let mut s = store.lock();
                                                        s.settings.password_free = true;
                                                        s.settings.clone()
                                                    };
                                                    let _ = settings_cmd::save_settings(
                                                        &store, settings,
                                                    );
                                                    window.push_notification(
                                                        t("已开启免密模式"),
                                                        cx,
                                                    );
                                                }
                                                Err(e) => {
                                                    window.push_notification(
                                                        tf("开启失败：{e}", &[&e.to_string()]),
                                                        cx,
                                                    );
                                                }
                                            }
                                        }),
                                ),
                        )
                    })
            });
        } else {
            // 关闭：清除记忆 + 保存设置
            let _ = auth::clear_remembered_password(&store);
            self.save_setting(cx, |s| s.password_free = false);
            window.push_notification(t("已关闭免密模式"), cx);
        }
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.settings(cx);

        // 主题
        let theme_current = match settings.theme.as_str() {
            "dark" => t("深色"),
            "light" => t("浅色"),
            _ => t("跟随系统"),
        }
        .to_string();
        let theme_options = vec![
            ("light".to_string(), t("浅色").to_string()),
            ("dark".to_string(), t("深色").to_string()),
            ("system".to_string(), t("跟随系统").to_string()),
        ];

        // 自动锁定 / 剪贴板 / 批量间隔（数值选项）
        let lock_secs: [(u32, &str); 6] = [
            (0, t("不锁定")),
            (1, t("1 分钟")),
            (3, t("3 分钟")),
            (5, t("5 分钟")),
            (10, t("10 分钟")),
            (30, t("30 分钟")),
        ];
        let lock_current = lock_secs
            .iter()
            .find(|(v, _)| *v == settings.auto_lock_minutes)
            .map(|(_, l)| l.to_string())
            .unwrap_or_else(|| t("3 分钟").to_string());
        let lock_options: Vec<(String, String)> = lock_secs
            .iter()
            .map(|(v, l)| (v.to_string(), l.to_string()))
            .collect();

        let clip_secs: [(u32, &str); 5] = [
            (0, t("不清空")),
            (10, t("10 秒")),
            (20, t("20 秒")),
            (30, t("30 秒")),
            (60, t("60 秒")),
        ];
        let clip_current = clip_secs
            .iter()
            .find(|(v, _)| *v == settings.clipboard_clear_seconds)
            .map(|(_, l)| l.to_string())
            .unwrap_or_else(|| t("20 秒").to_string());
        let clip_options: Vec<(String, String)> = clip_secs
            .iter()
            .map(|(v, l)| (v.to_string(), l.to_string()))
            .collect();

        let batch_secs: [(u32, &str); 5] = [
            (0, t("无间隔")),
            (1, t("1 秒")),
            (2, t("2 秒")),
            (3, t("3 秒")),
            (5, t("5 秒")),
        ];
        let batch_current = batch_secs
            .iter()
            .find(|(v, _)| *v == settings.batch_login_interval)
            .map(|(_, l)| l.to_string())
            .unwrap_or_else(|| t("2 秒").to_string());
        let batch_options: Vec<(String, String)> = batch_secs
            .iter()
            .map(|(v, l)| (v.to_string(), l.to_string()))
            .collect();

        // 界面语言（zh/en，切换后全窗口立即生效）
        let ui_lang_choices: Vec<(&str, &str)> = vec![("zh", t("中文")), ("en", "English")];
        let ui_lang_options: Vec<(String, String)> = ui_lang_choices
            .iter()
            .map(|(v, l)| (v.to_string(), l.to_string()))
            .collect();
        let ui_lang_current = ui_lang_choices
            .iter()
            .find(|(v, _)| *v == settings.ui_language)
            .map(|(_, l)| l.to_string())
            .unwrap_or_else(|| t("中文").to_string());

        // 默认登录语言（新增凭据预填；后端 Settings.default_language）
        let lang_choices: Vec<(&str, &str)> =
            vec![("ZH", t("中文")), ("EN", t("英文")), ("JA", t("日文"))];
        let lang_options: Vec<(String, String)> = lang_choices
            .iter()
            .map(|(v, l)| (v.to_string(), l.to_string()))
            .collect();
        let lang_current = lang_choices
            .iter()
            .find(|(v, _)| *v == settings.default_language)
            .map(|(_, l)| l.to_string())
            .unwrap_or_else(|| t("中文").to_string());

        // 高度跟随主界面（面板内容区）：absolute 锚定撑满可用区域，窗口越大可见内容越多。
        // 早期弹窗模式曾固定 620 上限；改面板后随窗口伸缩并启用原生滚动 + 滚动条指示。
        // （根已 absolute 定位，自带 containing block，无需再 relative）
        v_flex()
            .id("settings-scroll")
            .absolute()
            .inset_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .vertical_scrollbar(&self.scroll)
            // 面板内四周留白：卡片不贴左右上边缘
            .px_3()
            .pt_3()
            .gap(px(10.))
            .pb_3()
            // === 外观 ===
            .child(group_title(IconName::Palette, t("外观"), cx))
            .child(
                card(
                    vec![
                        choice_row(
                            "set-theme",
                            t("主题"),
                            None,
                            theme_current,
                            theme_options,
                            cx,
                            |_, v, window, cx| {
                                crate::ui::apply_theme_and_save(&v, window, cx);
                            },
                        ),
                        choice_row(
                            "set-ui-language",
                            t("界面语言"),
                            Some(t("切换后立即生效")),
                            ui_lang_current,
                            ui_lang_options,
                            cx,
                            |_, v, window, cx| {
                                // 保存设置 + 更新运行时语言 + 全窗口重绘
                                crate::ui::i18n::set_lang(&v);
                                let store = AppState::global(cx).store.clone();
                                let settings = {
                                    let mut s = store.lock();
                                    s.settings.ui_language = v.clone();
                                    s.settings.clone()
                                };
                                if let Err(e) =
                                    settings_cmd::save_settings(&store, settings)
                                {
                                    log::warn!("保存设置失败：{e}");
                                }
                                window.refresh();
                            },
                        ),
                        choice_row(
                            "set-default-language",
                            t("默认登录语言"),
                            Some(t("新增凭据时预填的 SAP 登录语言")),
                            lang_current,
                            lang_options,
                            cx,
                            |this, v, _, cx| {
                                this.save_setting(cx, |s| s.default_language = v);
                            },
                        ),
                    ],
                    cx,
                ),
            )
            // === 安全 ===
            .child(group_title(IconName::ShieldCheck, t("安全"), cx))
            .child(
                card(
                    vec![
                        toggle_row(
                            "set-password-free",
                            t("免密模式"),
                            Some(t("验证后用 Windows DPAPI 记忆主密码，下次启动免输入")),
                            settings.password_free,
                            cx,
                            |this, window, cx| {
                                let enable = !this.settings(cx).password_free;
                                this.toggle_password_free(enable, window, cx);
                                cx.notify();
                            },
                        ),
                        choice_row(
                            "set-lock",
                            t("自动锁定"),
                            Some(t("无操作指定时间后自动锁定应用")),
                            lock_current,
                            lock_options,
                            cx,
                            |this, v, _, cx| {
                                let v = v.parse().unwrap_or(3);
                                this.save_setting(cx, |s| s.auto_lock_minutes = v);
                            },
                        ),
                        choice_row(
                            "set-clip",
                            t("剪贴板自动清空"),
                            Some(t("复制密码后到时自动清空剪贴板")),
                            clip_current,
                            clip_options,
                            cx,
                            |this, v, _, cx| {
                                let v = v.parse().unwrap_or(20);
                                this.save_setting(cx, |s| s.clipboard_clear_seconds = v);
                            },
                        ),
                        // 修改主密码
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .px(px(14.))
                            .py(px(11.))
                            .child(row_label(
                                t("修改主密码"),
                                Some(t("所有凭据将用新密码重新加密")),
                                cx,
                            ))
                            .child(
                                Button::new("set-change-password")
                                    .small()
                                    .outline()
                                    .label(t("修改…"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let mp = this.master_password.clone();
                                        open_change_password_dialog(mp, window, cx);
                                    })),
                            )
                            .into_any_element(),
                    ],
                    cx,
                ),
            )
            // === 登录 ===
            .child(group_title(IconName::LogIn, t("登录"), cx))
            .child(
                card(
                    vec![
                        choice_row(
                            "set-batch",
                            t("批量登录间隔"),
                            Some(t("批量登录多个凭据时的间隔时间")),
                            batch_current,
                            batch_options,
                            cx,
                            |this, v, _, cx| {
                                let v = v.parse().unwrap_or(2);
                                this.save_setting(cx, |s| s.batch_login_interval = v);
                            },
                        ),
                        // SAP Logon 路径（堆叠行）
                        v_flex()
                            .gap(px(6.))
                            .px(px(14.))
                            .py(px(11.))
                            .child(
                                div()
                                    .text_size(px(super::tokens::TS_BASE))
                                    .text_color(cx.theme().foreground)
                                    .child(t("SAP Logon 路径")),
                            )
                            .child(Input::new(&self.sap_logon_path))
                            .child(
                                div()
                                    .text_size(px(super::tokens::TS_XS))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t("留空则自动检测 saplogon.exe / sapshcut.exe")),
                            )
                            .into_any_element(),
                    ],
                    cx,
                ),
            )
            // === 列表 ===
            .child(group_title(IconName::List, t("列表"), cx))
            .child(
                card(
                    vec![
                        toggle_row(
                            "set-compact-mode",
                            t("紧凑模式"),
                            Some(t("更小的卡片高度，列表显示更多凭据")),
                            settings.compact_mode,
                            cx,
                            |this, _, cx| {
                                let enable = !this.settings(cx).compact_mode;
                                this.save_setting(cx, |s| s.compact_mode = enable);
                                // 通知主界面立即应用新卡片高度
                                if let Some(page) = AppState::global(cx)
                                    .main_page
                                    .clone()
                                    .and_then(|w| w.upgrade())
                                {
                                    page.update(cx, |_, cx| cx.notify());
                                }
                            },
                        ),
                    ],
                    cx,
                ),
            )
            // === 窗口 ===
            .child(group_title(IconName::Monitor, t("窗口"), cx))
            .child(
                card(
                    vec![toggle_row(
                        "set-auto-start",
                        t("开机自启"),
                        Some(t("写入 Windows 注册表（当前用户）")),
                        settings.auto_start,
                        cx,
                        |this, _, cx| {
                            let enable = !this.settings(cx).auto_start;
                            state::set_auto_start(enable);
                            this.save_setting(cx, |s| s.auto_start = enable);
                            cx.notify();
                        },
                    ),
                    toggle_row(
                        "set-minimize-to-tray",
                        t("最小化到托盘"),
                        Some(t("最小化时隐藏窗口，从托盘左键唤回")),
                        settings.minimize_to_tray,
                        cx,
                        |this, _, cx| {
                            let enable = !this.settings(cx).minimize_to_tray;
                            this.save_setting(cx, |s| s.minimize_to_tray = enable);
                            cx.notify();
                        },
                    ),
                    toggle_row(
                        "set-close-to-tray",
                        t("关闭窗口时最小化到托盘"),
                        Some(t("点关闭按钮不退出，从托盘菜单退出可彻底关闭")),
                        settings.close_to_tray,
                        cx,
                        |this, _, cx| {
                            let enable = !this.settings(cx).close_to_tray;
                            this.save_setting(cx, |s| s.close_to_tray = enable);
                            cx.notify();
                        },
                    )],
                    cx,
                ),
            )
    }
}

/// 修改主密码表单（作为弹窗内容的独立视图）：
/// 校验错误显示在对应输入框下方的红字，而不是顶部飘通知
struct ChangePasswordForm {
    old: Entity<InputState>,
    new: Entity<InputState>,
    confirm: Entity<InputState>,
    error_old: Option<String>,
    error_new: Option<String>,
    error_confirm: Option<String>,
}

/// 单个密码输入行：输入框 + 可选红字错误（错误时输入框下方，无错误时占位为空）
fn pw_field(
    label: &str,
    state: &Entity<InputState>,
    error: &Option<String>,
    cx: &App,
) -> impl IntoElement {
    let mut col = v_flex()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(super::tokens::TS_SM))
                .text_color(super::tokens::text_secondary(cx))
                .child(label.to_string()),
        )
        .child(Input::new(state).mask_toggle());
    if let Some(msg) = error {
        col = col.child(
            div()
                .text_size(px(super::tokens::TS_XS))
                .text_color(cx.theme().danger)
                .child(msg.clone()),
        );
    }
    col
}

impl ChangePasswordForm {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let old = self.old.read(cx).value().to_string();
        let new = self.new.read(cx).value().to_string();
        let confirm = self.confirm.read(cx).value().to_string();

        self.error_old = None;
        self.error_new = None;
        self.error_confirm = None;

        if old.is_empty() {
            self.error_old = Some(t("请输入当前主密码").into());
            cx.notify();
            return;
        }
        if new.is_empty() {
            self.error_new = Some(t("请输入新主密码").into());
            cx.notify();
            return;
        }
        if new.chars().count() < 6 {
            self.error_new = Some(t("新主密码至少 6 位").into());
            cx.notify();
            return;
        }
        if new != confirm {
            self.error_confirm = Some(t("两次输入的新密码不一致").into());
            cx.notify();
            return;
        }

        let store = AppState::global(cx).store.clone();
        match auth::change_master_password(&store, old, new) {
            Ok(()) => {
                window.close_dialog(cx);
                window.push_notification(t("主密码已修改，所有凭据已重新加密"), cx);
            }
            Err(e) => {
                // 旧密码错误等后端校验失败 → 标到旧密码框下方
                self.error_old = Some(e.to_string());
                cx.notify();
            }
        }
    }
}

impl Render for ChangePasswordForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let old = self.old.clone();
        let new = self.new.clone();
        let confirm = self.confirm.clone();
        let error_old = self.error_old.clone();
        let error_new = self.error_new.clone();
        let error_confirm = self.error_confirm.clone();

        v_flex()
            .gap_3()
            .pb_2()
            .w_full()
            .child(pw_field(t("当前主密码"), &old, &error_old, cx))
            .child(pw_field(t("新主密码（至少 6 位）"), &new, &error_new, cx))
            .child(pw_field(t("再次输入新主密码"), &confirm, &error_confirm, cx))
            .child(
                Button::new("change-pw-confirm")
                    .primary()
                    .label(t("确认修改"))
                    .w_full()
                    .on_click(move |_, window, cx| {
                        if let Some(form) = weak.upgrade() {
                            form.update(cx, |f, cx| f.submit(window, cx));
                        }
                    }),
            )
    }
}

/// 修改主密码弹窗：旧密码 + 新密码 + 确认新密码
pub fn open_change_password_dialog(_current_mp: String, window: &mut Window, cx: &mut App) {
    let old_state = cx.new(|cx| {
        InputState::new(window, cx).masked(true).placeholder(t("当前主密码"))
    });
    let new_state = cx.new(|cx| {
        InputState::new(window, cx)
            .masked(true)
            .placeholder(t("新主密码（至少 6 位）"))
    });
    let confirm_state = cx.new(|cx| {
        InputState::new(window, cx).masked(true).placeholder(t("再次输入新主密码"))
    });

    let form = cx.new(|_cx| ChangePasswordForm {
        old: old_state,
        new: new_state,
        confirm: confirm_state,
        error_old: None,
        error_new: None,
        error_confirm: None,
    });

    window.open_dialog(cx, move |dialog, _, _| {
        let form = form.clone();
        dialog
            .title(t("修改主密码"))
            .min_w(px(340.))
            .content(move |content, _, _| {
                content.child(form.clone())
            })
    });
}
