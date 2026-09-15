//! 凭据卡片（降噪版）
//!
//! - 环境色头像（按环境固定映射：正式红/测试黄/开发绿/配置紫/未分组灰）
//! - 勾选框默认隐藏，悬停卡片或已有多选时显示（Ctrl 点选 / Shift 范围选择）
//! - 双击卡片登录，右键菜单操作
//! - 名称行：只留名称 + 热度火苗 / 缺失信息警告（客户端/SNC 下沉副标题）
//! - 副标题：纯文字 + 分隔点（用户名 · 客户端 · SSO · SNC · 消息服务器 · 语言[主色]）
//! - 右侧：复制成功 ✓（常显）+ 操作组（登录/置顶/收藏/更多，悬停浮现）

use super::i18n::{t, tf};
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use sap_backend::models::Credential;

use super::main_page::MainPage;

/// 环境色（与 Tauri 版 ENV_COLORS 一致）
pub fn env_color(env: &str) -> Hsla {
    match env {
        "production" => rgb(0xda1e28).into(),     // 红
        "test" => rgb(0xf1c21b).into(),            // 黄
        "development" => rgb(0x198038).into(),     // 绿
        "configuration" => rgb(0x8a3ffc).into(),    // 紫
        _ => rgb(0x6b7280).into(),                 // 灰（未分类）
    }
}

/// 环境浅色组（方案 B）：头像浅底 + 深字，对比度达 WCAG AA（黄底白字 1.6:1 的修复）
pub fn env_soft_colors(env: &str) -> (Hsla, Hsla) {
    match env {
        "production" => (rgb(0xfcebeb).into(), rgb(0x993c1d).into()), // 红
        "test" => (rgb(0xfaeeda).into(), rgb(0x633806).into()),       // 黄
        "development" => (rgb(0xeaf3de).into(), rgb(0x3b6d11).into()), // 绿
        "configuration" => (rgb(0xeeedfe).into(), rgb(0x534ab7).into()), // 紫
        _ => (rgb(0xf1efe8).into(), rgb(0x444441).into()),            // 灰（未分组）
    }
}

/// 环境标签文案（副标题 chip，颜色+文字双编码）；未分组不显示标签
pub fn env_label(env: &str) -> Option<&'static str> {
    match env {
        "production" => Some(t("正式")),
        "test" => Some(t("测试")),
        "development" => Some(t("开发")),
        "configuration" => Some(t("配置")),
        _ => None,
    }
}

/// 登录可用性：缺失信息提示（None = 可登录）
pub fn login_disabled_hint(cred: &Credential) -> Option<String> {
    let can_sso = cred.snc_enabled && cred.snc_sso;
    let mut missing: Vec<&str> = Vec::new();
    if cred.client.is_empty() {
        missing.push(t("客户端"));
    }
    if cred.language.is_empty() {
        missing.push(t("语言"));
    }
    if cred.username.is_empty() {
        missing.push(if cred.snc_enabled {
            t("SNC 账户")
        } else {
            t("用户名")
        });
    }
    let pw_missing = missing.is_empty()
        && !can_sso
        && !cred.username.is_empty()
        && cred.encrypted_password.is_empty();
    if !missing.is_empty() {
        return Some(tf("缺少关键信息：{}", &[&(missing.join(t("、"))).to_string()]));
    }
    if pw_missing {
        return Some(t("缺少密码").to_string());
    }
    None
}

/// 渲染单张凭据卡片
///
/// - `ix`：在当前过滤列表中的序号（Shift 范围选择基准）
/// - `any_selected`：列表中存在任意多选（勾选框常显）
/// - `is_hovered`：鼠标悬停在卡片上（控制置顶/收藏按钮显隐）
/// - `compact`：紧凑模式（卡片高度 48px）
/// - `just_copied`：刚复制密码成功（操作区显示绿色 ✓，1.5s 后由主页面清除）
pub fn render_card(
    cred: &Credential,
    ix: usize,
    is_selected: bool,
    is_pinned: bool,
    is_hovered: bool,
    any_selected: bool,
    compact: bool,
    just_copied: bool,
    weak: &WeakEntity<MainPage>,
    cx: &App,
) -> impl IntoElement + use<> {
    let id = cred.id.clone();
    let display_name = cred
        .display_name
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| cred.connection_id.clone());
    let hint = login_disabled_hint(cred);
    let login_disabled = hint.is_some();
    let is_fav = cred.is_favorite;
    let login_count = cred.login_count;
    let env_c = env_color(&cred.environment);
    let (avatar_bg, avatar_text_c) = env_soft_colors(&cred.environment);
    let env_label_text = env_label(&cred.environment);
    let avatar_text: String = (if cred.system_id.is_empty() {
        display_name.clone()
    } else {
        cred.system_id.clone()
    })
    .chars()
    .take(3)
    .collect::<String>()
    .to_uppercase();

    let card_h = if compact { px(48.) } else { px(56.) };
    let avatar_size = if compact { px(30.) } else { px(34.) };

    let sel_bg = {
        let mut c = cx.theme().primary;
        c.a = 0.06;
        c
    };
    let pin_bg = {
        let mut c = cx.theme().primary;
        c.a = 0.12;
        c
    };
    let fav_color = rgb(0xf1c21b);
    let fav_bg = {
        let mut c: Hsla = fav_color.into();
        c.a = 0.15;
        c
    };

    let group_name: SharedString = format!("cred-card-{ix}").into();
    let check_visible = is_selected || any_selected;

    // 副标题：环境标签 · 用户名 · 客户端 · SSO · 消息服务器 · 语言（主色突出）
    // 全部纯文字 + 分隔点；SNC 启用由 SSO 标识覆盖（SNC 是传输层细节，降噪去掉）
    let has_user = !cred.username.is_empty();
    let has_client = !cred.client.is_empty();
    let has_msg =
        !cred.message_server.is_empty() && cred.connection_type == "load_balancing";
    let has_lang = !cred.language.is_empty();
    let has_sso = cred.snc_enabled && cred.snc_sso;
    let has_label = env_label_text.is_some();
    let has_sub = has_label || has_user || has_client || has_msg || has_lang;

    div()
        .id(("cred-card", ix))
        .group(group_name.clone())
        .flex()
        .items_center()
        .relative()
        .overflow_hidden()
        .h(card_h)
        .px_2()
        .rounded_lg()
        .border_1()
        .border_color(if is_selected {
            cx.theme().primary
        } else {
            cx.theme().border
        })
        .when(is_selected, |this| this.bg(sel_bg))
        .when(!is_selected, |this| this.bg(cx.theme().popover))
        // 左缘环境色条（方案 B）：饱和色只做标识条，头像/标签用浅底深字
        .child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(3.))
                .bg(env_c),
        )
        .on_click({
            let weak = weak.clone();
            let cred_id = id.clone();
            move |e: &ClickEvent, window, cx| {
                let Some(page) = weak.upgrade() else {
                    return;
                };
                let m = e.modifiers();
                if e.click_count() >= 2 && !m.secondary() && !m.shift {
                    // 双击登录
                    page.update(cx, |p, cx| p.login(cred_id.clone(), window, cx));
                } else if m.secondary() {
                    // Ctrl 点选
                    page.update(cx, |p, cx| p.toggle_select(cred_id.clone(), ix, cx));
                } else if m.shift {
                    // Shift 范围选择
                    page.update(cx, |p, cx| p.select_range(ix, cx));
                }
            }
        })
        // 悬停状态：控制置顶/收藏按钮显隐（离开时清除，避免残留）
        // 注意必须在 .context_menu() 之前调用——context_menu 包装后的
        // ContextMenu<E> 不再暴露 on_hover
        .on_hover({
            let weak = weak.clone();
            move |hovered: &bool, _, cx| {
                if let Some(page) = weak.upgrade() {
                    page.update(cx, |p, cx| {
                        let new_state = if *hovered { Some(ix) } else { None };
                        if p.hovered_card != new_state {
                            p.hovered_card = new_state;
                            cx.notify();
                        }
                    });
                }
            }
        })
        .context_menu({
            let weak = weak.clone();
            let id = id.clone();
            let fav_label: &'static str = if is_fav { t("取消收藏") } else { t("收藏") };
            let pin_label: &'static str = if is_pinned { t("取消置顶") } else { t("置顶") };
            move |menu, _, _| {
                let (w1, c1) = (weak.clone(), id.clone());
                let (w2, c2) = (weak.clone(), id.clone());
                let (w3, c3) = (weak.clone(), id.clone());
                let (w4, c4) = (weak.clone(), id.clone());
                let (w5, c5) = (weak.clone(), id.clone());
                let (w6, c6) = (weak.clone(), id.clone());
                let (w7, c7) = (weak.clone(), id.clone());
                menu.item(
                    PopupMenuItem::new(t("登录"))
                        .disabled(login_disabled)
                        .on_click(move |_, window, cx| {
                            if let Some(page) = w1.upgrade() {
                                page.update(cx, |p, cx| p.login(c1.clone(), window, cx));
                            }
                        }),
                )
                .item(PopupMenuItem::new(pin_label).on_click(move |_, window, cx| {
                    if let Some(page) = w2.upgrade() {
                        page.update(cx, |p, cx| p.toggle_pin(c2.clone(), window, cx));
                    }
                }))
                .item(PopupMenuItem::new(fav_label).on_click(move |_, window, cx| {
                    if let Some(page) = w3.upgrade() {
                        page.update(cx, |p, cx| p.toggle_favorite(c3.clone(), window, cx));
                    }
                }))
                .item(
                    PopupMenuItem::new(t("编辑")).on_click(move |_, window, cx| {
                        if let Some(page) = w4.upgrade() {
                            page.update(cx, |p, cx| p.open_edit(c4.clone(), window, cx));
                        }
                    }),
                )
                .item(
                    PopupMenuItem::new(t("复制副本")).on_click(move |_, window, cx| {
                        if let Some(page) = w5.upgrade() {
                            page.update(cx, |p, cx| p.duplicate(c5.clone(), window, cx));
                        }
                    }),
                )
                .item(
                    PopupMenuItem::new(t("复制密码")).on_click(move |_, window, cx| {
                        if let Some(page) = w6.upgrade() {
                            page.update(cx, |p, cx| p.copy_password(c6.clone(), window, cx));
                        }
                    }),
                )
                .separator()
                .item(
                    PopupMenuItem::new(t("删除")).on_click(move |_, window, cx| {
                        if let Some(page) = w7.upgrade() {
                            page.update(cx, |p, cx| p.delete_credential(c7.clone(), window, cx));
                        }
                    }),
                )
            }
        })
        // 勾选框：低频操作默认隐藏（悬停卡片或已有多选/选中时显示），列表更干净
        .child(
            h_flex()
                .flex_none()
                .w(px(16.))
                .mr(px(8.))
                .items_center()
                .when(!check_visible, |t| t.opacity(0.0))
                .group_hover(group_name.clone(), |s| s.opacity(1.0))
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
                .child(
                    Checkbox::new(format!("card-check-{id}"))
                        .checked(is_selected)
                        .on_click({
                            let weak = weak.clone();
                            let id = id.clone();
                            move |checked, _, cx| {
                                if let Some(page) = weak.upgrade() {
                                    page.update(cx, |p, cx| {
                                        if *checked {
                                            p.selected.insert(id.clone());
                                            p.last_selected_ix = Some(ix);
                                        } else {
                                            p.selected.remove(&id);
                                        }
                                        cx.notify();
                                    });
                                }
                            }
                        }),
                ),
        )
        // 环境色头像
        .child(
            div()
                .flex_none()
                .mr(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .size(avatar_size)
                .rounded(px(8.))
                .bg(avatar_bg)
                .text_size(px(super::tokens::TS_XS))
                .font_weight(FontWeight::BOLD)
                .text_color(avatar_text_c)
                .child(avatar_text),
        )
        // 名称 + 副标题
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                // 名称行：只留名称 + 热度火苗 + 警告（客户端/SNC 下沉副标题，减少首行信息量）
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(4.))
                        .min_w_0()
                        .child(
                            div()
                                .min_w_0()
                                .text_size(px(super::tokens::TS_BASE))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .truncate()
                                .child(display_name),
                        )
                        .when(login_count >= 5, |this| {
                            this.child(
                                h_flex()
                                    .flex_none()
                                    .items_center()
                                    .gap(px(2.))
                                    .text_size(px(super::tokens::TS_XS))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        Icon::new(IconName::Flame)
                                            .size_3()
                                            .text_color(cx.theme().warning),
                                    )
                                    .child(if login_count >= 100 {
                                        // 不写字面量 "99+"：3 字节常量在链接期会被
                                        // 字符串合并优化吞掉（debug 在、release 丢）
                                        format!("{}+", 99)
                                    } else {
                                        login_count.to_string()
                                    }),
                            )
                        })
                        .when_some(hint.clone(), |this, h| {
                            this.child(
                                // 必填字段不全无法登录：悬停显示缺失项（Div 层无文本
                                // tooltip 扩展，用 ghost 图标按钮承载）
                                Button::new(format!("card-warn-{ix}"))
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::TriangleAlert)
                                    .tooltip(h)
                                    .flex_none(),
                            )
                        }),
                )
                // 副标题：环境标签 · 用户名 · 客户端 · SSO · 消息服务器 · 语言（纯文字，语言主色突出）
                .when(has_sub, |this| {
                    // 分隔点：仅在前面已有内容时显示（避免行首出现 "·"）
                    let dot_user = has_label;
                    let dot_client = has_label || has_user;
                    let dot_sso = has_label || has_user || has_client;
                    let dot_msg = has_label || has_user || has_client;
                    let dot_lang =
                        has_label || has_user || has_client || has_msg;
                    this.child(
                        h_flex()
                            .items_center()
                            .gap(px(4.))
                            .min_w_0()
                            .text_size(px(super::tokens::TS_SM))
                            .text_color(super::tokens::text_secondary(cx))
                            // 环境标签（颜色+文字双编码，色弱可辨）
                            .when_some(env_label_text, |t, label| {
                                t.child(
                                    div()
                                        .flex_none()
                                        .px(px(5.))
                                        .py(px(1.))
                                        .rounded(px(4.))
                                        .bg(avatar_bg)
                                        .text_size(px(super::tokens::TS_XS))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(avatar_text_c)
                                        .child(label),
                                )
                            })
                            .when(has_user, |t| {
                                t.child(
                                    h_flex()
                                        .min_w_0()
                                        .items_center()
                                        .gap(px(4.))
                                        .when(dot_user, |t| {
                                            t.child(div().flex_none().opacity(0.4).child("·"))
                                        })
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(cred.username.clone()),
                                        ),
                                )
                            })
                            .when(has_client, |t| {
                                t.child(
                                    h_flex()
                                        .flex_none()
                                        .items_center()
                                        .gap(px(4.))
                                        .when(dot_client, |t| {
                                            t.child(div().flex_none().opacity(0.4).child("·"))
                                        })
                                        .child(div().child(cred.client.clone())),
                                )
                            })
                            // SSO（主色，单点登录标识）
                            .when(has_sso, |t| {
                                t.child(
                                    h_flex()
                                        .flex_none()
                                        .items_center()
                                        .gap(px(4.))
                                        .when(dot_sso, |t| {
                                            t.child(div().flex_none().opacity(0.4).child("·"))
                                        })
                                        .child(
                                            div()
                                                .text_size(px(super::tokens::TS_XS))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(cx.theme().primary)
                                                .child("SSO"),
                                        ),
                                )
                            })
                            // 消息服务器（仅负载均衡）
                            .when(has_msg, |t| {
                                t.child(
                                    h_flex()
                                        .min_w_0()
                                        .items_center()
                                        .gap(px(4.))
                                        .when(dot_msg, |t| {
                                            t.child(div().flex_none().opacity(0.4).child("·"))
                                        })
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .child(cred.message_server.clone()),
                                        ),
                                )
                            })
                            // 语言（主色加粗，无底框）
                            .when(has_lang, |t| {
                                t.child(
                                    h_flex()
                                        .flex_none()
                                        .items_center()
                                        .gap(px(4.))
                                        .when(dot_lang, |t| {
                                            t.child(div().opacity(0.4).child("·"))
                                        })
                                        .child(
                                            div()
                                                .text_size(px(super::tokens::TS_XS))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(cx.theme().primary)
                                                .child(cred.language.clone()),
                                        ),
                                )
                            }),
                    )
                }),
        )
        // 右侧：复制成功 ✓（常显）+ 操作组（悬停浮现）
        .child(
            h_flex()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                })
                // 复制密码成功的原地反馈（toast 之外的克制提示）
                .when(just_copied, |this| {
                    this.child(
                        Icon::new(IconName::CircleCheck)
                            .size_4()
                            .flex_none()
                            .text_color(cx.theme().success),
                    )
                })
                // 操作组：默认隐藏，悬停卡片时浮现（透明度过渡，占位不变防布局跳动）
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(2.))
                        .when(!is_hovered, |t| t.opacity(0.0))
                        .group_hover(group_name.clone(), |s| s.opacity(1.0))
                        .child(
                            Button::new(format!("card-login-{id}"))
                                .outline()
                                .xsmall()
                                .label(t("登录"))
                                .disabled(login_disabled)
                                .when_some(
                                    if login_disabled { hint.clone() } else { None },
                                    |this, h| this.tooltip(h),
                                )
                                .on_click({
                                    let weak = weak.clone();
                                    let cred_id = id.clone();
                                    move |_, window, cx| {
                                        if let Some(page) = weak.upgrade() {
                                            page.update(cx, |p, cx| {
                                                p.login(cred_id.clone(), window, cx)
                                            });
                                        }
                                    }
                                }),
                        )
                        // 置顶：悬停或已置顶时显示；隐藏时用同尺寸占位，保持 ⋮ 按钮位置稳定
                        .child(if is_hovered || is_pinned {
                            Button::new(format!("card-pin-{id}"))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Pin)
                                .tooltip(if is_pinned { t("取消置顶") } else { t("置顶") })
                                .when(is_pinned, |this| {
                                    this.bg(pin_bg).text_color(cx.theme().primary)
                                })
                                .on_click({
                                    let weak = weak.clone();
                                    let cred_id = id.clone();
                                    move |_, window, cx| {
                                        if let Some(page) = weak.upgrade() {
                                            page.update(cx, |p, cx| {
                                                p.toggle_pin(cred_id.clone(), window, cx)
                                            });
                                        }
                                    }
                                })
                                .into_any_element()
                        } else {
                            div().size_5().into_any_element()
                        })
                        // 收藏：同置顶（悬停或已收藏时显示）
                        .child(if is_hovered || is_fav {
                            Button::new(format!("card-fav-{id}"))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Star)
                                .tooltip(if is_fav { t("取消收藏") } else { t("收藏") })
                                .when(is_fav, |this| this.bg(fav_bg).text_color(fav_color))
                                .on_click({
                                    let weak = weak.clone();
                                    let cred_id = id.clone();
                                    move |_, window, cx| {
                                        if let Some(page) = weak.upgrade() {
                                            page.update(cx, |p, cx| {
                                                p.toggle_favorite(cred_id.clone(), window, cx)
                                            });
                                        }
                                    }
                                })
                                .into_any_element()
                        } else {
                            div().size_5().into_any_element()
                        })
                        .child(
                            Button::new(format!("card-more-{id}"))
                                .ghost()
                                .xsmall()
                                .icon(IconName::EllipsisVertical)
                                .tooltip(t("更多操作"))
                                .dropdown_menu({
                                    let weak = weak.clone();
                                    let id = id.clone();
                                    move |menu, _, _| {
                                        let (w1, c1) = (weak.clone(), id.clone());
                                        let (w2, c2) = (weak.clone(), id.clone());
                                        let (w3, c3) = (weak.clone(), id.clone());
                                        menu.item(
                                            PopupMenuItem::new(t("编辑")).on_click(
                                                move |_, window, cx| {
                                                    if let Some(page) = w1.upgrade() {
                                                        page.update(cx, |p, cx| {
                                                            p.open_edit(c1.clone(), window, cx)
                                                        });
                                                    }
                                                },
                                            ),
                                        )
                                        .item(
                                            PopupMenuItem::new(t("复制副本")).on_click(
                                                move |_, window, cx| {
                                                    if let Some(page) = w2.upgrade() {
                                                        page.update(cx, |p, cx| {
                                                            p.duplicate(c2.clone(), window, cx)
                                                        });
                                                    }
                                                },
                                            ),
                                        )
                                        .item(
                                            PopupMenuItem::new(t("复制密码")).on_click(
                                                move |_, window, cx| {
                                                    if let Some(page) = w3.upgrade() {
                                                        page.update(cx, |p, cx| {
                                                            p.copy_password(
                                                                c3.clone(),
                                                                window,
                                                                cx,
                                                            )
                                                        });
                                                    }
                                                },
                                            ),
                                        )
                                    }
                                })
                                .anchor(Anchor::BottomRight),
                        ),
                ),
        )
}
