//! 凭证新增/编辑表单（右侧面板 + 三页签：连接 / 凭据 / SNC）
//!
//! 对应 Tauri 版 CredentialModal.tsx：
//! - 必填校验与页签跳转逻辑一致
//! - 编辑时密码留空表示不修改
//! - 分组选择仅列自定义分组（系统分组由环境自动归类）
//!
//! 布局：根节点 absolute().inset_0() 锚定面板容器（绕过 Entity 边界高度链断裂），
//! 页签/页脚固定、内容卡片内部滚动 → 小窗口下底栏按钮不会被挤出可视区。

use super::i18n::{t, tf};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _}, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use sap_backend::commands::credentials as cred_cmd;
use sap_backend::models::{Credential, Group};

use crate::state::AppState;

use super::main_page::MainPage;

/// 页签（id，显示名运行时经 i18n 翻译）
const TABS: [&str; 3] = ["conn", "cred", "snc"];

/// 语言选项（仅中英日）代码
const LANG_CODES: [&str; 3] = ["ZH", "EN", "JA"];

/// 环境选项代码
const ENV_CODES: [&str; 5] = ["production", "test", "development", "configuration", ""];

/// SNC QOP 选项值
const SNC_QOP_VALUES: [&str; 5] = ["1", "2", "3", "8", "9"];

fn tab_label(id: &str) -> &'static str {
    match id {
        "conn" => t("连接"),
        "cred" => t("凭据"),
        _ => "SNC",
    }
}

/// 语言显示名
fn language_label(code: &str) -> &'static str {
    match code {
        "ZH" => t("中文"),
        "EN" => t("英文"),
        _ => t("日文"),
    }
}

/// 环境显示名
fn environment_label(code: &str) -> &'static str {
    match code {
        "production" => t("生产环境"),
        "test" => t("测试环境"),
        "development" => t("开发环境"),
        "configuration" => t("配置环境"),
        _ => t("未分类"),
    }
}

/// SNC QOP 显示名
fn snc_qop_label(v: &str) -> &'static str {
    match v {
        "1" => t("认证"),
        "2" => t("完整性"),
        "3" => t("隐私"),
        "8" => t("最大"),
        _ => t("默认"),
    }
}

/// 创建输入框状态（带初始值与占位符）
fn new_input(window: &mut Window, cx: &mut App, v: &str, ph: &str) -> Entity<InputState> {
    let state = cx.new(|cx| InputState::new(window, cx).placeholder(ph));
    if !v.is_empty() {
        state.update(cx, |s, cx| s.set_value(v.to_string(), window, cx));
    }
    state
}

pub struct CredentialForm {
    editing_id: Option<String>,
    /// 主密码（面板销毁时内存清零）
    master_password: String,
    weak_main: WeakEntity<MainPage>,

    active_tab: &'static str,
    /// 字段级校验错误（2026-09-29 审计④：内联红字替代纯 toast），key = 字段标识
    field_errors: std::collections::HashMap<&'static str, String>,
    /// 脏标记（2026-09-29 审计 P0-2）：任何输入/选择改动置位，取消/Esc 时用于确认
    dirty: bool,
    /// 输入框 Change 订阅（首次 render 时挂载——new() 阶段字段尚未构造完成）
    subscriptions_done: bool,
    /// 焦点句柄（根容器 track_focus，供 Esc / Ctrl+Enter 键处理）
    focus_handle: gpui::FocusHandle,
    connection_type: String,
    environment: String,
    color_tag: String,
    group_id: Option<String>,
    snc_enabled: bool,
    snc_sso: bool,
    snc_qop: String,
    language: String,
    /// 编辑时保留收藏状态（后端 update 不回写该字段）
    is_favorite: bool,
    /// 自定义分组（footer 选择用）
    custom_groups: Vec<Group>,

    // 文本输入
    display_name: Entity<InputState>,
    system_id: Entity<InputState>,
    connection_id: Entity<InputState>,
    client: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    app_server: Entity<InputState>,
    system_number: Entity<InputState>,
    message_server: Entity<InputState>,
    logon_group: Entity<InputState>,
    saprouter: Entity<InputState>,
    description: Entity<InputState>,
    snc_name: Entity<InputState>,
    /// 登录后动作类型: "" = 无 | "transaction" | "report" | "command"
    post_login_action_type: String,
    post_login_action: Entity<InputState>,
    /// 内容滚动 handle（内容卡片滚动条 overlay 读取位置用）
    content_scroll: ScrollHandle,
}

/// 销毁时把主密码内存清零
impl Drop for CredentialForm {
    fn drop(&mut self) {
        use zeroize::Zeroize as _;
        self.master_password.zeroize();
    }
}

impl CredentialForm {
    pub fn new(
        editing: Option<Credential>,
        master_password: String,
        weak_main: WeakEntity<MainPage>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = AppState::global(cx).store.clone();
        let custom_groups = {
            let s = store.lock();
            s.groups
                .iter()
                .filter(|g| !g.is_system && !g.is_default)
                .cloned()
                .collect::<Vec<_>>()
        };

        let e = editing.unwrap_or_default();
        let is_edit = !e.id.is_empty();
        let editing_id = if is_edit { Some(e.id.clone()) } else { None };

        Self {
            editing_id,
            master_password,
            weak_main,
            active_tab: "conn",
            field_errors: std::collections::HashMap::new(),
            dirty: false,
            subscriptions_done: false,
            focus_handle: cx.focus_handle(),
            connection_type: if e.connection_type.is_empty() {
                "direct".to_string()
            } else {
                e.connection_type.clone()
            },
            environment: e.environment.clone(),
            color_tag: e.color_tag.clone(),
            group_id: e.group_id.clone(),
            snc_enabled: e.snc_enabled,
            snc_sso: e.snc_sso,
            snc_qop: if e.snc_qop.is_empty() { "9".to_string() } else { e.snc_qop.clone() },
            language: if e.language.is_empty() { "ZH".to_string() } else { e.language.clone() },
            is_favorite: e.is_favorite,
            custom_groups,
            display_name: new_input(
                window,
                cx,
                &e.display_name.clone().unwrap_or_default(),
                t("自定义别名（可选）"),
            ),
            system_id: new_input(window, cx, &e.system_id, t("如 PRD")),
            connection_id: new_input(window, cx, &e.connection_id, t("如 SAP-PRD-100")),
            client: new_input(window, cx, &e.client, t("如 100")),
            username: new_input(window, cx, &e.username, t("SAP 用户名")),
            // 编辑时密码留空 = 不修改（masked 为 builder 方法，需在创建时设置；
            // 眼睛开关 mask_toggle 挂在渲染的 Input 上）
            password: {
                let ph = if is_edit { t("留空保持不变") } else { t("密码") };
                cx.new(|cx| InputState::new(window, cx).masked(true).placeholder(ph))
            },
            app_server: new_input(window, cx, &e.app_server, t("应用服务器地址")),
            system_number: new_input(window, cx, &e.system_number, t("如 00")),
            message_server: new_input(window, cx, &e.message_server, t("消息服务器地址")),
            logon_group: new_input(window, cx, &e.logon_group, t("登录组")),
            saprouter: new_input(window, cx, &e.saprouter, t("如 /H/1.2.3.4/H/")),
            description: new_input(window, cx, &e.description, t("备注描述")),
            snc_name: new_input(window, cx, &e.snc_name, t("如 p:CN=ERP, O=Company, C=DE")),
            post_login_action_type: e.post_login_action_type.clone().unwrap_or_default(),
            post_login_action: new_input(
                window,
                cx,
                &e.post_login_action.clone().unwrap_or_default(),
                t("如 SE38"),
            ),
            content_scroll: ScrollHandle::new(),
        }
    }

    fn val(&self, s: &Entity<InputState>, cx: &App) -> String {
        s.read(cx).value().trim().to_string()
    }

    /// 请求关闭表单：有未保存修改时弹确认，无修改直接关闭（审计 P0-2）
    fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dirty {
            window.open_dialog(cx, |dialog, _, _| {
                dialog
                    .title(t("放弃修改？"))
                    .min_w(px(340.))
                    .content(|content, _, _cx| {
                        content.child(
                            v_flex()
                                .gap_3()
                                .pb_2()
                                .child(
                                    div()
                                        .text_size(px(super::tokens::TS_BASE))
                                        .child(t("有未保存的修改，确定放弃并返回主界面吗？")),
                                )
                                .child(
                                    h_flex()
                                        .justify_end()
                                        .gap_2()
                                        .child(
                                            Button::new("discard-keep")
                                                .label(t("继续编辑"))
                                                .on_click(|_, window, cx| {
                                                    window.close_dialog(cx);
                                                }),
                                        )
                                        .child(
                                            Button::new("discard-confirm")
                                                .primary()
                                                .label(t("放弃修改"))
                                                .on_click(|_, window, cx| {
                                                    window.close_dialog(cx);
                                                    if let Some(app) = AppState::global(cx)
                                                        .weak_app
                                                        .clone()
                                                        .and_then(|w| w.upgrade())
                                                    {
                                                        app.update(cx, |app, cx| {
                                                            app.close_panel_impl(cx);
                                                        });
                                                    }
                                                }),
                                        ),
                                ),
                        )
                    })
            });
        } else {
            self.close_panel_now(cx);
        }
    }

    /// 关闭面板回主界面（放弃修改确认 / 无修改时的直接关闭）
    fn close_panel_now(&mut self, cx: &mut Context<Self>) {
        if let Some(app) =
            AppState::global(cx).weak_app.clone().and_then(|w| w.upgrade())
        {
            app.update(cx, |app, cx| app.close_panel_impl(cx));
        }
    }

    /// 收集表单为 Credential
    fn collect(&self, cx: &App) -> Credential {
        Credential {
            id: self.editing_id.clone().unwrap_or_default(),
            connection_id: self.val(&self.connection_id, cx),
            client: self.val(&self.client, cx),
            username: self.val(&self.username, cx),
            password: self.val(&self.password, cx),
            encrypted_password: String::new(),
            language: self.language.clone(),
            connection_type: self.connection_type.clone(),
            system_id: self.val(&self.system_id, cx),
            app_server: self.val(&self.app_server, cx),
            system_number: self.val(&self.system_number, cx),
            message_server: self.val(&self.message_server, cx),
            message_server_port: String::new(),
            logon_group: self.val(&self.logon_group, cx),
            saprouter: self.val(&self.saprouter, cx),
            description: self.val(&self.description, cx),
            post_login_action: if self.post_login_action_type.is_empty() {
                None
            } else {
                let v = self.val(&self.post_login_action, cx);
                (!v.is_empty()).then_some(v)
            },
            post_login_action_type: if self.post_login_action_type.is_empty() {
                None
            } else {
                Some(self.post_login_action_type.clone())
            },
            group_id: self.group_id.clone(),
            environment: self.environment.clone(),
            login_count: 0,
            last_login_at: None,
            color_tag: self.color_tag.clone(),
            is_favorite: self.is_favorite,
            favorite_order: None,
            // 空串按未填写处理，避免存出 Some("") 干扰显示逻辑
            display_name: {
                let v = self.val(&self.display_name, cx);
                Some(v).filter(|v| !v.is_empty())
            },
            uuid: String::new(),
            snc_enabled: self.snc_enabled,
            snc_name: self.val(&self.snc_name, cx),
            snc_qop: self.snc_qop.clone(),
            snc_sso: self.snc_sso,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    /// 保存（校验 → add/update → 关闭弹窗 → 刷新列表）
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = self.collect(cx);

        // 必填校验（与 Tauri 版一致）；错误落到字段级内联红字（审计④），
        // 不再整包塞进 toast——错误就在出错字段下方看得见
        let mut field_errors: Vec<(&'static str, String)> = Vec::new();
        let mut jump_tab: Option<&'static str> = None;
        if form.system_id.is_empty() {
            field_errors.push(("system_id", t("系统标识必填").into()));
            jump_tab = Some("conn");
        }
        if form.client.is_empty() {
            field_errors.push(("client", t("客户端必填").into()));
            jump_tab = Some("cred");
        }
        if form.language.is_empty() {
            field_errors.push(("language", t("语言必填").into()));
            jump_tab = Some("cred");
        }
        if form.connection_type == "direct" {
            if form.app_server.is_empty() {
                field_errors.push(("app_server", t("应用服务器必填").into()));
                jump_tab = Some("conn");
            }
            if form.system_number.is_empty() {
                field_errors.push(("system_number", t("实例编号必填").into()));
                jump_tab = Some("conn");
            }
        } else {
            if form.message_server.is_empty() {
                field_errors.push(("message_server", t("消息服务器必填").into()));
                jump_tab = Some("conn");
            }
            if form.logon_group.is_empty() {
                field_errors.push(("logon_group", t("登录组必填").into()));
                jump_tab = Some("conn");
            }
        }
        if form.snc_enabled && form.snc_name.is_empty() {
            field_errors.push(("snc_name", t("SNC 名称必填").into()));
            jump_tab = Some("snc");
        }
        if form.snc_enabled && form.username.is_empty() {
            field_errors.push(("username", t("SNC 账户必填").into()));
            jump_tab = Some("snc");
        }

        self.field_errors = field_errors.into_iter().collect();
        if !self.field_errors.is_empty() {
            if let Some(tab) = jump_tab {
                self.active_tab = tab;
            }
            cx.notify();
            return;
        }

        let store = AppState::global(cx).store.clone();
        let mp = self.master_password.clone();

        let result = if self.editing_id.is_some() {
            cred_cmd::update_credential(&store, mp, form).map(|_| ())
        } else {
            cred_cmd::add_credential(&store, mp, form).map(|_| ())
        };

        match result {
            Ok(()) => {
                // 保存成功：关闭面板回主界面
                if let Some(app) =
                    AppState::global(cx).weak_app.clone().and_then(|w| w.upgrade())
                {
                    app.update(cx, |app, cx| {
                        app.active_panel = None;
                        app.panel_title = None;
                        app.panel_kind = None;
                        cx.notify();
                    });
                }
                window.push_notification(t("凭据已保存"), cx);
                if let Some(page) = self.weak_main.upgrade() {
                    page.update(cx, |p, cx| p.refresh(cx));
                }
            }
            Err(e) => {
                window.push_notification(tf("保存失败：{e}", &[&e.to_string()]), cx);
            }
        }
    }

    // === 渲染辅助 ===

    /// 字段行：label + 必填星号 + 输入框（+ 字段级内联错误红字，紧贴出错字段）
    fn render_field(
        label: &str,
        required: bool,
        input: impl IntoElement,
        error: Option<&str>,
        cx: &App,
    ) -> AnyElement {
        v_flex()
            .gap(px(4.))
            .child(
                h_flex()
                    .gap(px(2.))
                    .text_size(px(super::tokens::TS_SM))
                    .text_color(super::tokens::text_secondary(cx))
                    .child(label.to_string())
                    .when(required, |this| {
                        this.child(
                            div()
                                .text_color(cx.theme().danger)
                                .child("*"),
                        )
                    }),
            )
            .child(input)
            // 内联错误（2026-09-29 审计④）：错误就在出错字段下方，不用去 toast 里找
            .when_some(error, |this, e| {
                this.child(
                    div()
                        .text_size(px(super::tokens::TS_XS))
                        .text_color(cx.theme().danger)
                        .child(e.to_string()),
                )
            })
            .into_any_element()
    }

    /// 页签条（分段控件样式：灰底轨道 + 白色活动段，全宽三等分，点击区域大）
    fn render_tabs(weak: &WeakEntity<Self>, active_tab: &str, _cx: &Context<Self>) -> AnyElement {
        h_flex()
            .flex_none()
            .w_full()
            .gap(px(3.))
            .p(px(3.))
            .bg(_cx.theme().secondary)
            .rounded(_cx.theme().radius)
            .children(TABS.iter().map(|id| {
                let active = active_tab == *id;
                let id = *id;
                let label = tab_label(id);
                let weak = weak.clone();
                div()
                    .id(format!("form-tab-{id}"))
                    .flex_1()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(super::tokens::R_INNER))
                    .cursor_pointer()
                    .text_size(px(super::tokens::TS_BASE))
                    .map(|this| {
                        if active {
                            this.bg(_cx.theme().popover)
                                .border_1()
                                .border_color(_cx.theme().border)
                                .text_color(_cx.theme().foreground)
                        } else {
                            this.text_color(_cx.theme().muted_foreground)
                        }
                    })
                    .child(label)
                    .on_click(move |_, _, cx| {
                        if let Some(form) = weak.upgrade() {
                            form.update(cx, |form, cx| {
                                form.active_tab = id;
                                cx.notify();
                            });
                        }
                    })
            }))
            .into_any_element()
    }

    /// 单选 chips 行（label + 值列表）
    fn render_choice_row(
        weak: &WeakEntity<Self>,
        label: &str,
        options: Vec<(String, String, bool)>,
        cx: &Context<Self>,
        on_pick: impl Fn(&mut Self, String) + Clone + 'static,
    ) -> AnyElement {
        v_flex()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(super::tokens::TS_SM))
                    .text_color(super::tokens::text_secondary(cx))
                    .child(label.to_string()),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .children(options.into_iter().map(move |(value, text, active)| {
                        let on_pick = on_pick.clone();
                        let weak = weak.clone();
                        div()
                            .id(format!("choice-{value}"))
                            .flex_none()
                            .px(px(10.))
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .rounded_full()
                            .cursor_pointer()
                            .text_size(px(super::tokens::TS_SM))
                            .map(|this| {
                                if active {
                                    this.bg(cx.theme().primary).text_color(cx.theme().primary_foreground)
                                } else {
                                    this.bg(cx.theme().secondary)
                                        .text_color(cx.theme().foreground)
                                }
                            })
                            .child(text)
                            .on_click(move |_, _, cx| {
                                if let Some(form) = weak.upgrade() {
                                    form.update(cx, |form, cx| {
                                        on_pick(form, value.clone());
                                        cx.notify();
                                    });
                                }
                            })
                    })),
            )
            .into_any_element()
    }

    /// 登录语言下拉行（仅中英日）
    fn render_lang_row(&self, weak: WeakEntity<Self>, cx: &Context<Self>) -> AnyElement {
        let current_label = LANG_CODES
            .iter()
            .find(|v| **v == self.language)
            .map(|v| format!("{} ({v})", language_label(v)))
            .unwrap_or_else(|| format!("{} (ZH)", t("中文")));
        let language = self.language.clone();

        v_flex()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(super::tokens::TS_SM))
                    .text_color(super::tokens::text_secondary(cx))
                    .child(t("登录语言")),
            )
            .child(
                Button::new("lang-select")
                    .secondary()
                    .small()
                    .w_full()
                    .icon(IconName::ChevronDown)
                    .label(current_label)
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu;
                        for v in LANG_CODES {
                            let checked = language == v;
                            let weak_item = weak.clone();
                            menu = menu.item(
                                PopupMenuItem::new(format!("{} ({v})", language_label(v)))
                                    .checked(checked)
                                    .on_click(move |_, _, cx| {
                                        if let Some(form) = weak_item.upgrade() {
                                            form.update(cx, |form, cx| {
                                                form.language = v.to_string();
                                                form.field_errors.remove("language");
                                                form.dirty = true;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            );
                        }
                        menu
                    })
                    .anchor(Anchor::TopLeft),
            )
            // 语言为下拉选择（无 Input Change 事件），错误在选中语言时清除
            .when_some(self.field_errors.get("language").map(|e| e.as_str()), |this, e| {
                this.child(
                    div()
                        .text_size(px(super::tokens::TS_XS))
                        .text_color(cx.theme().danger)
                        .child(e.to_string()),
                )
            })
            .into_any_element()
    }

    /// 连接类型分段按钮
    fn render_conn_type(
        weak: &WeakEntity<Self>,
        conn_type: &str,
        cx: &Context<Self>,
    ) -> AnyElement {
        v_flex()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(super::tokens::TS_SM))
                    .text_color(super::tokens::text_secondary(cx))
                    .child(t("连接类型")),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("conn-type-direct")
                            .px(px(12.))
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .rounded_md()
                            .cursor_pointer()
                            .text_size(px(super::tokens::TS_BASE))
                            .map(|this| {
                                if conn_type == "direct" {
                                    this.bg(cx.theme().primary).text_color(cx.theme().primary_foreground)
                                } else {
                                    this.bg(cx.theme().secondary)
                                        .text_color(cx.theme().foreground)
                                }
                            })
                            .child(t("直连"))
                            .on_click({
                                let weak = weak.clone();
                                move |_, _, cx| {
                                    if let Some(form) = weak.upgrade() {
                                        form.update(cx, |form, cx| {
                                            form.connection_type = "direct".into();
                                            form.dirty = true;
                                            cx.notify();
                                        });
                                    }
                                }
                            }),
                    )
                    .child(
                        div()
                            .id("conn-type-lb")
                            .px(px(12.))
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .rounded_md()
                            .cursor_pointer()
                            .text_size(px(super::tokens::TS_BASE))
                            .map(|this| {
                                if conn_type == "load_balancing" {
                                    this.bg(cx.theme().primary).text_color(cx.theme().primary_foreground)
                                } else {
                                    this.bg(cx.theme().secondary)
                                        .text_color(cx.theme().foreground)
                                }
                            })
                            .child(t("负载均衡"))
                            .on_click({
                                let weak = weak.clone();
                                move |_, _, cx| {
                                    if let Some(form) = weak.upgrade() {
                                        form.update(cx, |form, cx| {
                                            form.connection_type = "load_balancing".into();
                                            form.dirty = true;
                                            cx.notify();
                                        });
                                    }
                                }
                            }),
                    ),
            )
            .into_any_element()
    }

    /// 连接页内容
    fn render_conn_tab(&self, weak: &WeakEntity<Self>, cx: &Context<Self>) -> AnyElement {
        v_flex()
            .gap_2()
            .child(Self::render_field(
                t("显示名称"),
                false,
                Input::new(&self.display_name),
                None,
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        // 系统标识 : 连接名 = 3 : 7（与 Tauri 版一致）
                        div().flex_grow(3.).child(Self::render_field(
                            t("系统标识"),
                            true,
                            Input::new(&self.system_id),
                            self.field_errors.get("system_id").map(|e| e.as_str()),
                            cx,
                        )),
                    )
                    .child(
                        div()
                            .flex_grow(7.)
                            .min_w_0()
                            .child(Self::render_field(
                                t("连接名"),
                                false,
                                Input::new(&self.connection_id),
                                None,
                                cx,
                            )),
                    ),
            )
            .child(Self::render_choice_row(
                weak,
                t("环境"),
                ENV_CODES
                    .iter()
                    .map(|v| (v.to_string(), environment_label(v).to_string(), self.environment == *v))
                    .collect(),
                cx,
                |form, v| {
                    form.environment = v;
                    form.dirty = true;
                },
            ))
            .child(Self::render_conn_type(weak, &self.connection_type, cx))
            // 按连接类型显示对应字段
            .when(self.connection_type == "direct", |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_grow(7.)
                                .min_w_0()
                                .child(Self::render_field(
                                    t("应用服务器"),
                                    true,
                                    Input::new(&self.app_server),
                                    self.field_errors.get("app_server").map(|e| e.as_str()),
                                    cx,
                                )),
                        )
                        .child(
                            div().flex_grow(3.).child(Self::render_field(
                                t("实例编号"),
                                true,
                                Input::new(&self.system_number),
                                self.field_errors.get("system_number").map(|e| e.as_str()),
                                cx,
                            )),
                        ),
                )
            })
            .when(self.connection_type == "load_balancing", |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_grow(7.)
                                .min_w_0()
                                .child(Self::render_field(
                                    t("消息服务器"),
                                    true,
                                    Input::new(&self.message_server),
                                    self.field_errors.get("message_server").map(|e| e.as_str()),
                                    cx,
                                )),
                        )
                        .child(
                            div().flex_grow(3.).child(Self::render_field(
                                t("登录组"),
                                true,
                                Input::new(&self.logon_group),
                                self.field_errors.get("logon_group").map(|e| e.as_str()),
                                cx,
                            )),
                        ),
                )
            })
            .child(Self::render_field(
                t("路由字符串"),
                false,
                Input::new(&self.saprouter),
                None,
                cx,
            ))
            .child(Self::render_field(
                t("描述"),
                false,
                Input::new(&self.description),
                None,
                cx,
            ))
            .into_any_element()
    }

    /// 凭据页内容
    fn render_cred_tab(&self, weak: &WeakEntity<Self>, cx: &Context<Self>) -> AnyElement {
        v_flex()
            .gap_2()
            .child(Self::render_field(
                t("客户端"),
                true,
                Input::new(&self.client),
                self.field_errors.get("client").map(|e| e.as_str()),
                cx,
            ))
            .child(Self::render_field(
                if self.snc_enabled { t("SNC 账户") } else { t("用户名") },
                false,
                Input::new(&self.username),
                self.field_errors.get("username").map(|e| e.as_str()),
                cx,
            ))
            .child(Self::render_field(
                t("密码"),
                false,
                Input::new(&self.password).mask_toggle(),
                None,
                cx,
            ))
            .child(self.render_lang_row(weak.clone(), cx))
            .child(self.render_post_login_row(weak.clone(), cx))
            .into_any_element()
    }

    /// 登录后动作：类型分段选择（无/事务码/报表/命令）+ 选中时的内容输入框。
    /// 登录成功后以 sapshcut -type/-command 直达对应功能。
    fn render_post_login_row(&self, weak: WeakEntity<Self>, cx: &Context<Self>) -> AnyElement {
        let tpe = self.post_login_action_type.as_str();
        let options = vec![
            (
                String::new(),
                t("无").to_string(),
                tpe.is_empty(),
            ),
            (
                "transaction".to_string(),
                t("事务码").to_string(),
                tpe == "transaction",
            ),
            (
                "report".to_string(),
                t("报表").to_string(),
                tpe == "report",
            ),
            (
                "command".to_string(),
                t("命令").to_string(),
                tpe == "command",
            ),
        ];
        // InputState 的 placeholder 为创建期固定值（如 SE38），类型说明用 hint 小字动态展示

        v_flex()
            .gap(px(4.))
            .child(Self::render_choice_row(
                &weak,
                t("登录后动作"),
                options,
                cx,
                |form, value| {
                    form.post_login_action_type = value;
                    form.dirty = true;
                },
            ))
            .when(!tpe.is_empty(), |this| {
                // 类型说明小字（InputState placeholder 为创建期固定值，无法动态切换）
                let hint = match tpe {
                    "report" => t("填写报表程序名"),
                    "command" => t("填写 ok-code（可含空格）"),
                    _ => t("登录后直达该事务码"),
                };
                this.child(
                    div()
                        .text_size(px(super::tokens::TS_XS))
                        .text_color(super::tokens::text_secondary(cx))
                        .child(hint.to_string()),
                )
                .child(Input::new(&self.post_login_action))
            })
            .into_any_element()
    }

    /// 分组下拉（弹窗底部左下角）
    fn render_group_select(&self, weak: WeakEntity<Self>) -> impl IntoElement + use<> {
        let default_label = t("默认（按环境归类）");
        let current_label = match self.group_id.as_deref() {
            None | Some("") => default_label.to_string(),
            Some(gid) => self
                .custom_groups
                .iter()
                .find(|g| g.id == gid)
                .map(|g| g.group_name.clone())
                .unwrap_or_else(|| default_label.to_string()),
        };
        let group_id = self.group_id.clone();
        let custom_groups = self.custom_groups.clone();

        Button::new("form-group-select")
            .secondary()
            .small()
            .icon(IconName::FolderInput)
            .label(current_label)
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.label(t("分组"));
                // 默认分组
                let checked = group_id.is_none() || group_id.as_deref() == Some("");
                let weak_default = weak.clone();
                menu = menu.item(
                    PopupMenuItem::new(default_label)
                        .checked(checked)
                        .on_click(move |_, _, cx| {
                            if let Some(form) = weak_default.upgrade() {
                                form.update(cx, |form, cx| {
                                    form.group_id = None;
                                    form.dirty = true;
                                    cx.notify();
                                });
                            }
                        }),
                );
                // 自定义分组
                for g in custom_groups.iter() {
                    let checked = group_id.as_deref() == Some(g.id.as_str());
                    let gid = g.id.clone();
                    let weak_item = weak.clone();
                    menu = menu.item(
                        PopupMenuItem::new(g.group_name.clone())
                            .checked(checked)
                            .on_click(move |_, _, cx| {
                                if let Some(form) = weak_item.upgrade() {
                                    form.update(cx, |form, cx| {
                                        form.group_id = Some(gid.clone());
                                        form.dirty = true;
                                        cx.notify();
                                    });
                                }
                            }),
                    );
                }
                menu
            })
            .anchor(Anchor::TopLeft)
    }

    /// SNC 页内容
    fn render_snc_tab(&self, weak: &WeakEntity<Self>, cx: &Context<Self>) -> AnyElement {
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .py(px(2.))
                    .child(
                        v_flex().gap(px(2.)).child(
                            div()
                                .text_size(px(super::tokens::TS_BASE))
                                .text_color(cx.theme().foreground)
                                .child(t("启用 SNC")),
                        ),
                    )
                    .child(
                        Switch::new("snc-enabled")
                            .checked(self.snc_enabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.snc_enabled = !this.snc_enabled;
                                this.dirty = true;
                                cx.notify();
                            })),
                    ),
            )
            .when(self.snc_enabled, |this| {
                this.child(Self::render_field(
                    t("SNC 名称"),
                    true,
                    Input::new(&self.snc_name),
                    self.field_errors.get("snc_name").map(|e| e.as_str()),
                    cx,
                ))
                .child(Self::render_choice_row(
                    weak,
                    t("保护质量（QOP）"),
                    SNC_QOP_VALUES
                        .iter()
                        .map(|v| (v.to_string(), format!("{} ({v})", snc_qop_label(v)), self.snc_qop == *v))
                        .collect(),
                    cx,
                    |form, v| {
                        form.snc_qop = v;
                        form.dirty = true;
                    },
                ))
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .py(px(2.))
                        .child(
                            v_flex()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(super::tokens::TS_BASE))
                                        .text_color(cx.theme().foreground)
                                        .child(t("SSO 免密登录")),
                                )
                                .child(
                                    div()
                                        .text_size(px(super::tokens::TS_XS))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(t("启用后登录无需密码（使用 SNC 证书）")),
                                ),
                        )
                        .child(
                            Switch::new("snc-sso")
                                .checked(self.snc_sso)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.snc_sso = !this.snc_sso;
                                    this.dirty = true;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when(!self.snc_enabled, |this| {
                this.child(
                    v_flex()
                        .items_center()
                        .gap_2()
                        .py_6()
                        .child(
                            Icon::new(IconName::ShieldOff)
                                .size_6()
                                .text_color(cx.theme().muted_foreground),
                        )
                        .child(
                            div()
                                .text_size(px(super::tokens::TS_SM))
                                .text_color(cx.theme().muted_foreground)
                                .child(t("未启用 SNC，使用密码登录")),
                        ),
                )
            })
            .into_any_element()
    }
}

impl Render for CredentialForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_edit = self.editing_id.is_some();
        let weak = cx.entity().downgrade();

        // 首次 render 挂输入框 Change 订阅（new() 阶段字段尚未构造完成）：
        // 任何输入改动置脏标记 + 清除该字段校验错误
        if !self.subscriptions_done {
            self.subscriptions_done = true;
            let inputs: Vec<(Entity<InputState>, Option<&'static str>)> = vec![
                (self.display_name.clone(), None),
                (self.system_id.clone(), Some("system_id")),
                (self.connection_id.clone(), None),
                (self.client.clone(), Some("client")),
                (self.username.clone(), Some("username")),
                (self.password.clone(), None),
                (self.app_server.clone(), Some("app_server")),
                (self.system_number.clone(), Some("system_number")),
                (self.message_server.clone(), Some("message_server")),
                (self.logon_group.clone(), Some("logon_group")),
                (self.saprouter.clone(), None),
                (self.description.clone(), None),
                (self.snc_name.clone(), Some("snc_name")),
                (self.post_login_action.clone(), None),
            ];
            for (entity, key) in inputs {
                cx.subscribe_in(
                    &entity,
                    window,
                    move |this, _, event: &InputEvent, _, _| {
                        if let InputEvent::Change = event {
                            this.dirty = true;
                            if let Some(k) = key {
                                this.field_errors.remove(k);
                            }
                        }
                    },
                )
                .detach();
            }

            // 客户端限 3 位数字（v1.4.0）：SAP client 为 3 位数字（000-999），
            // 输入即过滤非数字字符并截断到 3 位；set_value 不触发 Change
            //（无回写循环），用户键入产生的 Change 已先经上方订阅置脏
            let client = self.client.clone();
            cx.subscribe_in(&client, window, |this, _, event, window, cx| {
                if let InputEvent::Change = event {
                    let raw = this.client.read(cx).value().to_string();
                    let sanitized: String =
                        raw.chars().filter(|c| c.is_ascii_digit()).take(3).collect();
                    if sanitized != raw {
                        this.client
                            .update(cx, |s, cx| s.set_value(sanitized.as_str(), window, cx));
                    }
                }
            })
            .detach();
        }

        // 页签条（固定不滚动）
        let tabs = Self::render_tabs(&weak, self.active_tab, cx);
        // 内容卡片：白底 + 边框 + 内边距，让表单区域和面板背景区分开
        let content = v_flex()
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .p(px(12.))
            .child(match self.active_tab {
                "conn" => self.render_conn_tab(&weak, cx),
                "cred" => self.render_cred_tab(&weak, cx),
                _ => self.render_snc_tab(&weak, cx),
            });
        // 页脚：左分组下拉 + 右操作按钮（flex_none 不随内容移动）
        let footer = h_flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap_2()
            .pt_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .min_w_0()
                    .child(self.render_group_select(weak.clone())),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .flex_none()
                    .child(
                        Button::new("form-cancel")
                            .label(t("取消"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                // 有未保存修改先确认（审计 P0-2），无修改直接关闭
                                this.request_close(window, cx);
                            })),
                    )
                    .child(
                        Button::new("form-save")
                            .primary()
                            .label(if is_edit { t("保存修改") } else { t("创建凭据") })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save(window, cx);
                            })),
                    ),
            );

        // 根节点 absolute().inset_0() 锚定面板容器（绕过 Entity 边界高度链断裂），
        // 页签/页脚固定，内容卡片内部滚动 → 页脚不随页签切换跳动、不贴面板边缘；
        // 键盘：Ctrl+Enter 保存 / Esc 取消（脏检查），stop_propagation 防止
        // SapApp 根级 Esc 处理直接关面板（审计 P0-2）
        v_flex()
            .absolute()
            .inset_0()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(
                |this, event: &gpui::KeyDownEvent, window, cx| {
                    let key = event.keystroke.key.as_str();
                    if event.keystroke.modifiers.control && key == "enter" {
                        cx.stop_propagation();
                        this.save(window, cx);
                    } else if key == "escape" {
                        cx.stop_propagation();
                        this.request_close(window, cx);
                    }
                },
            ))
            // 面板四周留白：页签不贴顶、页脚不贴左右下边缘
            .px(px(12.))
            .pt(px(10.))
            .pb(px(12.))
            .gap_2()
            .child(tabs)
            .child(
                div()
                    .id("form-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.content_scroll)
                    .relative()
                    .vertical_scrollbar(&self.content_scroll)
                    .child(content),
            )
            .child(footer)
    }
}
