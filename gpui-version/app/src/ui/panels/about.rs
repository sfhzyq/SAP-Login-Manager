//! 关于面板（替代弹窗）。
//!
//! 由 ui/mod.rs 拆分而来（v1.5.9）。

use super::super::i18n::{t, tf};
use super::super::tokens;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    scroll::ScrollableElement as _,
    v_flex, ActiveTheme as _, Icon, Sizable as _, WindowExt as _,
};
use gpui_kit::*;

/// 项目主页地址（关于面板展示/复制/打开）
pub const REPO_URL: &str = "https://github.com/sfhzyq/SAP-LoginManager";

/// 关于面板（替代弹窗）
pub struct AboutPanel {
    version: String,
    /// 整页滚动 handle（滚动条 overlay 读取位置用）
    scroll: ScrollHandle,
}

impl AboutPanel {
    pub fn new(version: String) -> Self {
        Self {
            version,
            scroll: ScrollHandle::new(),
        }
    }
}

impl Render for AboutPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let version = self.version.clone();
        // 滚动条姿势与设置页一致（v1.5.6 修复）：vertical_scrollbar 挂在
        // 「不滚动的锚容器」上，滚动的是内层子元素——挂滚动容器自身会跟着
        // 内容一起滚走（thumb 越滚越偏，见设置页 2026-10-05 注记）
        v_flex()
            .absolute()
            .inset_0()
            .vertical_scrollbar(&self.scroll)
            .child(
                v_flex()
                    .id("about-scroll")
                    .absolute()
                    .inset_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .px_6()
                    .py_5()
                    .items_center()
                    // 设计稿版式（v1.5.6 二轮重设计）：头部行 + 两列信息区 +
                    // 双列致谢列表，整体仍收进单一居中列防缩窗水平溢出
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(640.))
                            .gap(px(18.))
                            // ── 头部：Logo + 名称/版本/定位语，右侧项目主页按钮 ──
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .justify_between()
                                    .gap_3()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_3()
                                            // Logo：与任务栏图标同构（蓝色圆角方底 + 挂锁符号）
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_none()
                                                    .items_center()
                                                    .justify_center()
                                                    .size(px(44.))
                                                    .rounded(px(tokens::R_BADGE))
                                                    .bg(cx.theme().primary)
                                                    .child(
                                                        svg()
                                                            .path(IconName::Lock.path())
                                                            .size(px(22.))
                                                            .text_color(cx.theme().primary_foreground),
                                                    ),
                                            )
                                            .child(
                                                v_flex()
                                                    .min_w_0()
                                                    .gap(px(2.))
                                                    .child(
                                                        h_flex()
                                                            .items_center()
                                                            .gap_2()
                                                            .child(
                                                                div()
                                                                    .text_size(px(tokens::TS_LG))
                                                                    .font_weight(FontWeight::SEMIBOLD)
                                                                    .text_color(cx.theme().foreground)
                                                                    .child(t("SAP 登录管理器")),
                                                            )
                                                            .child(
                                                                div()
                                                                    .px_2()
                                                                    .py_0p5()
                                                                    .rounded_full()
                                                                    .bg({
                                                                        let mut c = cx.theme().primary;
                                                                        c.a = 0.10;
                                                                        c
                                                                    })
                                                                    .text_size(px(tokens::TS_XS))
                                                                    .font_weight(FontWeight::SEMIBOLD)
                                                                    .text_color(cx.theme().primary)
                                                                    .child(format!("v{version}")),
                                                            ),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(tokens::TS_SM))
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(t("凭据本地加密存储，无云端依赖")),
                                                    ),
                                            ),
                                    )
                                    // 项目主页（GitHub 仓库）——不放明文 URL（用户反馈）
                                    .child(
                                        Button::new("about-open-repo")
                                            .outline()
                                            .xsmall()
                                            .icon(IconName::ArrowUpRight)
                                            .label(t("项目主页"))
                                            .tooltip(t("在浏览器中打开项目主页"))
                                            .on_click(|_, _, cx| {
                                                // 不走 cmd /C start（会闪 cmd 黑窗），
                                                // 用 GPUI 内置 ShellExecuteW 打开，无窗口
                                                cx.open_url(REPO_URL);
                                            }),
                                    ),
                            )
                            // ── 中部两列：构建信息卡（左） + 安全须知（右） ──
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_stretch()
                                    .gap_4()
                                    .child(Self::build_info_card(cx))
                                    .child(Self::security_notes(cx)),
                            )
                            // ── 底部权责声明：左侧竖线引用样式（方案 C：比卡片
                            // 轻一档，法务脚注感，不与内容卡抢视觉） ──
                            .child({
                                let mut line_c = cx.theme().muted_foreground;
                                line_c.a *= 0.6;
                                h_flex()
                                    .w_full()
                                    .gap_2p5()
                                    .child(
                                        div()
                                            .w(px(2.))
                                            .flex_none()
                                            .self_stretch()
                                            .rounded_full()
                                            .bg(line_c),
                                    )
                                    .child(
                                        v_flex()
                                            .min_w_0()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_size(px(tokens::TS_XS))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(t(
                                                        "本软件按「现状」提供，作者不对因使用本软件导致的任何数据丢失、凭据泄露承担责任",
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(tokens::TS_XS))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(t(
                                                        "将公司凭据存入本软件前，请自行确认符合贵司信息安全与合规规定，相关责任由使用者承担",
                                                    )),
                                            ),
                                    )
                            })
                            // ── 开源项目致谢：完整 14 项双列展示，置于页面最末
                            // （用户要求：完整展示 + 放到最后）──
                            .child(Self::render_oss_section(cx)),
                    ),
            )
    }
}

impl AboutPanel {
    /// 构建信息卡：运行时 / 口令哈希 / 数据加密（v1.5.6 设计稿版式）
    fn build_info_card(cx: &Context<Self>) -> AnyElement {
        let rows = [
            ("运行时", "Rust · GPUI 原生"),
            ("口令哈希", "Argon2id"),
            ("数据加密", "AES-GCM"),
        ];
        v_flex()
            .flex_1()
            .min_w_0()
            .gap_1p5()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(
                div()
                    .text_size(px(tokens::TS_BASE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().foreground)
                    .child(t("构建信息")),
            )
            .children(rows.iter().map(|(label, value)| {
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(tokens::TS_SM))
                            .text_color(cx.theme().muted_foreground)
                            .child(t(label)),
                    )
                    .child(
                        div()
                            .text_size(px(tokens::TS_SM))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().foreground)
                            .child(t(value)),
                    )
            }))
            .into_any_element()
    }

    /// 安全须知：主密码重要性与不可恢复性（v1.5.6 方案 C：danger 淡红卡，
    /// 与左侧构建信息卡对齐，警示语义 > 内容卡）
    fn security_notes(cx: &Context<Self>) -> AnyElement {
        let danger = cx.theme().danger;
        let mut bg = danger;
        bg.a = 0.08;
        v_flex()
            .flex_1()
            .min_w_0()
            .gap_1p5()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(danger)
            .bg(bg)
            .child(
                h_flex()
                    .items_center()
                    .gap_1p5()
                    .child(Icon::new(IconName::TriangleAlert).size_4().text_color(danger))
                    .child(
                        div()
                            .text_size(px(tokens::TS_BASE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child(t("安全须知")),
                    ),
            )
            .child(
                div()
                    .text_size(px(tokens::TS_SM))
                    .text_color(tokens::text_secondary(cx))
                    .child(t(
                        "主密码是解锁本地数据的唯一凭证，丢失后无法找回。请务必妥善保管主密码，并定期备份数据目录。",
                    )),
            )
            .into_any_element()
    }

    /// 开源项目致谢（v1.5.6 设计稿版式，v1.5.7 调整）：双列链接 + 「复制全部链接」按钮
    ///
    /// 名称左键在浏览器打开主页，右键复制单个链接。全部直接依赖完整
    /// 双列展示（不折叠），块置于关于页最末（用户要求）。
    fn render_oss_section(cx: &Context<Self>) -> AnyElement {
        // (名称, 链接, 用途)——运行时构建以便 i18n 翻译用途描述
        let oss: Vec<(&str, &str, &str)> = vec![
            ("gpui", "https://gpui.rs", t("GPU 加速 UI 框架（Zed）")),
            (
                "gpui-kit",
                "https://github.com/longbridge/gpui-component",
                t("UI 组件库（按钮/输入/菜单/主题）"),
            ),
            (
                "serde",
                "https://github.com/serde-rs/serde",
                t("数据序列化"),
            ),
            (
                "chrono",
                "https://github.com/chronotope/chrono",
                t("日期时间"),
            ),
            ("uuid", "https://github.com/uuid-rs/uuid", t("凭据 ID 生成")),
            (
                "aes / aes-gcm",
                "https://github.com/RustCrypto/block-ciphers",
                t("凭据加密"),
            ),
            (
                "sha2 / hmac / pbkdf2",
                "https://github.com/RustCrypto/hashes",
                t("主密码派生与校验"),
            ),
            (
                "arboard",
                "https://github.com/1Password/arboard",
                t("跨平台剪贴板"),
            ),
            (
                "xmltree",
                "https://github.com/PoiScript/xmltree",
                t("SAP Logon 配置解析"),
            ),
            (
                "winreg",
                "https://github.com/gentoo/winreg-rs",
                t("Windows 注册表读取"),
            ),
            (
                "windows-sys",
                "https://github.com/microsoft/windows-rs",
                t("Win32 窗口控制/剪贴板"),
            ),
            (
                "base64",
                "https://github.com/marshallpierce/rust-base64",
                t("Base64 编解码"),
            ),
            (
                "thiserror",
                "https://github.com/dtolnay/thiserror",
                t("错误定义"),
            ),
            ("log", "https://github.com/rust-lang/log", t("日志门面")),
        ];
        let all_urls: String = oss
            .iter()
            .map(|(_, url, _)| *url)
            .collect::<Vec<_>>()
            .join("\n");
        // 全部展示（用户要求），对半分两列；7 个时左列多 1 个
        let mid = oss.len().div_ceil(2);

        // 单个致谢条目：下划线链接名（左键打开 / 右键复制）+ 「· 用途」
        fn oss_link(
            idx: usize,
            name: &'static str,
            url: &'static str,
            desc: &'static str,
            cx: &Context<AboutPanel>,
        ) -> AnyElement {
            let primary = cx.theme().primary;
            let muted = cx.theme().muted_foreground;
            h_flex()
                .min_w_0()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .id(format!("about-oss-link-{}", idx))
                        .flex_none()
                        .cursor_pointer()
                        .text_size(px(tokens::TS_SM))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(primary)
                        .underline()
                        .hover(|s| s.opacity(0.75))
                        .on_click(move |_, _, cx| {
                            // 同上：cx.open_url 走 ShellExecuteW，不弹 cmd 窗
                            cx.open_url(url);
                        })
                        .on_mouse_down(gpui::MouseButton::Right, move |_, window, cx| {
                            // 写入失败如实报告，不再假成功
                            match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(url)) {
                                Ok(()) => window.push_notification(t("链接已复制到剪贴板"), cx),
                                Err(e) => window
                                    .push_notification(tf("复制失败：{e}", &[&e.to_string()]), cx),
                            }
                        })
                        .child(name),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(tokens::TS_XS))
                        .text_color(muted)
                        .child(format!("· {}", desc)),
                )
                .into_any_element()
        }

        let left_col = v_flex().flex_1().min_w_0().gap_1p5().children(
            oss.iter()
                .take(mid)
                .enumerate()
                .map(|(i, e)| oss_link(i, e.0, e.1, e.2, cx)),
        );
        let right_col = v_flex().flex_1().min_w_0().gap_1p5().children(
            oss.iter()
                .skip(mid)
                .enumerate()
                .map(|(i, e)| oss_link(i + mid, e.0, e.1, e.2, cx)),
        );

        // 方案 C：致谢整体装卡（background 底，比构建卡的 secondary 浅一档，
        // 层级递减：danger 红卡 > secondary 信息卡 > background 内容卡）
        v_flex()
            .w_full()
            .gap_2()
            .p_3()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(tokens::TS_BASE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child(t("开源项目致谢")),
                    )
                    .child(
                        Button::new("about-copy-all-oss")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Copy)
                            .label(t("复制全部链接"))
                            .tooltip(t("点击复制全部项目链接"))
                            .on_click(move |_, window, cx| {
                                // 写入失败如实报告，不再假成功
                                match arboard::Clipboard::new()
                                    .and_then(|mut cb| cb.set_text(all_urls.clone()))
                                {
                                    Ok(()) => window.push_notification(t("链接已复制到剪贴板"), cx),
                                    Err(e) => window.push_notification(
                                        tf("复制失败：{e}", &[&e.to_string()]),
                                        cx,
                                    ),
                                }
                            }),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .items_start()
                    .gap_6()
                    .child(left_col)
                    .child(right_col),
            )
            .child(
                div()
                    .text_size(px(tokens::TS_XS))
                    .text_color(cx.theme().muted_foreground)
                    .child(tf(
                        "共 {n} 个开源项目 · 左键打开主页，右键复制链接",
                        &[oss.len().to_string().as_str()],
                    )),
            )
            .into_any_element()
    }
}
