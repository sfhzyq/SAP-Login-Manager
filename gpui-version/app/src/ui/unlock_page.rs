//! 解锁页 / 首次设置主密码页
//!
//! 对应 Tauri 版 `SetupPage` + `UnlockPage`（src/components/common.tsx）
//! - 首次使用：主密码 + 确认密码 + 创建按钮
//! - 已有主密码：主密码 + 解锁按钮
//! - 免密模式：本次运行内锁定后可一键快速解锁（会话密码仅存内存）

use super::i18n::t;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::state::AppState;
use sap_backend::commands::auth;

/// 解锁页（含首次设置主密码模式）
pub struct UnlockPage {
    focus_handle: FocusHandle,
    /// 尚未设置主密码（首次使用）
    is_setup: bool,
    /// 主密码输入
    password: Entity<InputState>,
    /// 确认密码输入（仅首次设置显示）
    confirm: Entity<InputState>,
    /// 错误提示
    error: Option<String>,
    /// 免密模式会话密码（本次运行内锁定后可快速解锁）
    session_password: Option<String>,
    /// 根视图弱引用（解锁成功后切换路由）
    weak_app: WeakEntity<crate::ui::SapApp>,
}

impl UnlockPage {
    pub fn new(
        is_setup: bool,
        session_password: Option<String>,
        weak_app: WeakEntity<crate::ui::SapApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t("主密码"))
        });
        // 默认聚焦密码框
        password.update(cx, |state, cx| state.focus(window, cx));

        let confirm = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(t("确认密码"))
        });

        // Enter 提交；输入变化时清除错误提示
        cx.subscribe_in(&password, window, |this, _, event, window, cx| match event {
            InputEvent::PressEnter { .. } => this.submit(window, cx),
            InputEvent::Change => this.clear_error(cx),
            _ => {}
        })
        .detach();
        cx.subscribe_in(&confirm, window, |this, _, event, window, cx| match event {
            InputEvent::PressEnter { .. } => this.submit(window, cx),
            InputEvent::Change => this.clear_error(cx),
            _ => {}
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            is_setup,
            password,
            confirm,
            error: None,
            session_password,
            weak_app,
        }
    }

    /// 提交：首次设置 → 创建主密码；否则 → 校验解锁
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = AppState::global(cx).store.clone();
        let pw = self.password.read(cx).value().to_string();

        if self.is_setup {
            if pw.is_empty() {
                return self.fail(t("请输入主密码"), cx);
            }
            if pw.chars().count() < 4 {
                return self.fail(t("密码至少 4 个字符"), cx);
            }
            let cf = self.confirm.read(cx).value().to_string();
            if pw != cf {
                return self.fail(t("两次输入的密码不一致"), cx);
            }
            match auth::set_master_password(&store, pw.clone()) {
                Ok(()) => self.on_unlocked(pw, window, cx),
                Err(e) => self.fail(&e, cx),
            }
        } else {
            if pw.is_empty() {
                return self.fail(t("请输入主密码"), cx);
            }
            match auth::verify_master_password(&store, pw.clone()) {
                Ok(true) => self.on_unlocked(pw, window, cx),
                Ok(false) => self.fail(t("主密码错误"), cx),
                Err(e) => self.fail(&e, cx),
            }
        }
    }

    /// 免密模式快速解锁（使用会话中保存的主密码）
    fn quick_unlock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pw) = self.session_password.clone() else {
            return;
        };
        let store = AppState::global(cx).store.clone();
        match auth::verify_master_password(&store, pw.clone()) {
            Ok(true) => self.on_unlocked(pw, window, cx),
            Ok(false) => self.fail(t("主密码错误"), cx),
            Err(e) => self.fail(&e, cx),
        }
    }

    fn fail(&mut self, msg: &str, cx: &mut Context<Self>) {
        self.error = Some(msg.to_string());
        cx.notify();
    }

    fn clear_error(&mut self, cx: &mut Context<Self>) {
        if self.error.is_some() {
            self.error = None;
            cx.notify();
        }
    }

    /// 解锁/创建成功：通知根视图切换到主界面
    fn on_unlocked(&mut self, password: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app) = self.weak_app.upgrade() {
            app.update(cx, |app, cx| app.unlock(password, window, cx));
        }
    }

    /// 密码输入框：浅色模式下 kit 默认底色 = theme.background（input.rs
    /// input_background() 浅色分支返回 background），与解锁页背景同源、
    /// 边界几乎不可辨（用户反馈"不够明显"）→ 白底 + 边框提升对比；
    /// 深色模式 input_background 已有层次，保持 kit 默认。
    fn password_input(state: &Entity<InputState>, cx: &Context<Self>) -> Input {
        let input = Input::new(state).mask_toggle();
        if cx.theme().is_dark() {
            input
        } else {
            input.bg(gpui::white()).border_color(cx.theme().border)
        }
    }

    /// 蓝底白锁图标（对应 Tauri 版 56×56、圆角 16px）
    fn lock_badge(cx: &Context<Self>) -> impl IntoElement {
        div()
            .size(px(56.))
            .rounded_2xl()
            .bg(cx.theme().primary)
            .flex()
            .items_center()
            .justify_center()
            .child(
                Icon::new(IconName::Lock)
                    .size_7()
                    .text_color(cx.theme().primary_foreground),
            )
    }
}

impl Focusable for UnlockPage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for UnlockPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_setup = self.is_setup;
        let has_quick_unlock = !is_setup && self.session_password.is_some();

        div()
            .id("unlock-page")
            .track_focus(&self.focus_handle)
            // Entity 边界高度链：用 absolute 锚定父容器（见 MEMORY.md 布局坑）
            .absolute()
            .inset_0()
            .bg(cx.theme().background)
            .child(
                v_flex()
                    .size_full()
                    // 标题栏由 SapApp 根视图统一渲染（自绘：锁定/置顶/窗口控制）
                    .child(
                        // 居中卡片
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                v_flex()
                                    .w(px(380.))
                                    .items_center()
                                    .gap_2()
                                    .child(Self::lock_badge(cx))
                                    // 标题
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(cx.theme().foreground)
                                            .child("SAP Login Manager"),
                                    )
                                    // 副标题
                                    .child(
                                        div()
                                            .text_size(px(super::tokens::TS_MD))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(if is_setup {
                                                t("设置主密码以保护您的凭据")
                                            } else {
                                                t("请输入主密码以解锁凭据")
                                            }),
                                    )
                                    // 输入框
                                    .child(
                                        v_flex()
                                            .gap_2()
                                            .w_full()
                                            .pt_5()
                                            .child(Self::password_input(&self.password, cx))
                                            .when(is_setup, |this| {
                                                this.child(Self::password_input(&self.confirm, cx))
                                            }),
                                    )
                                    // 错误提示
                                    .when_some(self.error.clone(), |this, err| {
                                        this.child(
                                            div()
                                                .text_size(px(super::tokens::TS_BASE))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(cx.theme().danger)
                                                .child(err),
                                        )
                                    })
                                    // 主按钮
                                    .child(
                                        Button::new("submit")
                                            .primary()
                                            .label(if is_setup { t("创建") } else { t("解锁") })
                                            .w_full()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.submit(window, cx);
                                            })),
                                    )
                                    // 免密快速解锁：secondary 实底 + 闪电图标——
                                    // 原 outline 描边在浅色解锁页上边框过淡几乎不可辨（用户反馈）
                                    .when(has_quick_unlock, |this| {
                                        this.child(
                                            Button::new("quick-unlock")
                                                .secondary()
                                                .icon(IconName::Zap)
                                                .label(t("快速解锁（免密）"))
                                                .w_full()
                                                .mt_2()
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.quick_unlock(window, cx);
                                                })),
                                        )
                                    })
                                    // 首次使用提示
                                    .when(is_setup, |this| {
                                        this.child(
                                            div()
                                                .text_size(px(super::tokens::TS_BASE))
                                                .text_color(cx.theme().muted_foreground)
                                                .line_height(relative(1.5))
                                                .text_center()
                                                .pt_3()
                                                .child(
                                                    t("主密码用于加密所有凭据，请妥善保管。忘记主密码将无法恢复数据。"),
                                                ),
                                        )
                                    }),
                            ),
                    ),
            )
    }
}
