//! UI 设计 token：字号阶梯 + 跨主题辅助色
//!
//! 对齐 Tauri 版 `src/styles/global.css` 的排版体系（440px 紧凑窗口）：
//! 全 app 禁止直接写 `text_size(px(N))` 魔法数字与一次性硬编码色，统一引用此处。
//!
//! 字号阶梯（Tauri 版 --fs-*）：
//! - XS 10.5：chip / 热度 / SNC 徽标 / kbd 提示
//! - SM 11.5：副标题 / 表单标签 / 设置说明
//! - BASE 12.5：卡片名 / 正文 / 列表行 / 按钮默认
//! - MD 13.5：副标题（解锁页）/ 面板说明文字
//! - LG 15：面板标题
//! - XL 18 / XXL 22：关于页 / 解锁页大标题

use gpui_kit::{App, Hsla, rgb, rgba};

/// chip / 徽标 / kbd 最小可读字号
pub const TS_XS: f32 = 10.5;
/// 副标题 / 标签 / 说明文字
pub const TS_SM: f32 = 11.5;
/// 正文基准（卡片名、列表行、正文）
pub const TS_BASE: f32 = 12.5;
/// 强调正文 / 面板副标题
pub const TS_MD: f32 = 13.5;
/// 面板标题
pub const TS_LG: f32 = 15.0;

/// 次要文字色：介于 foreground 与 muted_foreground 之间的中间层级
///
/// gpui-kit 主题只有两级文本（foreground / muted_foreground），副标题、
/// 表单说明等"次要但需可读"的文字用 muted（#8b92a8）太浅。此辅助色
/// 对齐 Tauri 版 `--text-secondary`（light #545b73）。
pub fn text_secondary(cx: &App) -> Hsla {
    use gpui_kit::component::ActiveTheme as _;
    if cx.theme().is_dark() {
        rgb(0xa8b1c9).into() // 深色下的次要文字（浅灰蓝）
    } else {
        rgb(0x545b73).into() // Tauri 版 --text-secondary
    }
}

// ============ 活动栏（48px 左栏）配色 ============

/// 活动栏配色组：浅色主题浅壳 / 深色主题深壳（v0.3.0 VS Code 风，保持）
#[allow(missing_docs)]
pub struct ActivityBarColors {
    /// 栏底色
    pub bar_bg: Hsla,
    /// 右边线
    pub bar_border: Hsla,
    /// 未激活图标
    pub idle_fg: Hsla,
    /// 激活图标
    pub active_fg: Hsla,
    /// 悬停胶囊
    pub hover_pill: Hsla,
    /// 按压胶囊（比悬停重一档）
    pub press_pill: Hsla,
    /// 激活胶囊
    pub active_pill: Hsla,
    /// 激活指示竖条（左侧 2px）
    pub accent: Hsla,
    /// 分区分隔线
    pub divider: Hsla,
}

/// 按当前主题返回活动栏配色。
///
/// - 浅壳（浅色主题）：底 #DFE3EE 比内容区深半档保留层次；图标/竖条走主题色；
///   悬停/按压/激活叠加用黑色系（白色叠加在浅底上不可见，方向反转）。
/// - 深壳（深色主题）：#1E293B VS Code 风 + 白色系叠加（v0.3.0 原设计）。
pub fn activity_bar_colors(cx: &App) -> ActivityBarColors {
    use gpui_kit::component::ActiveTheme as _;
    let th = cx.theme();
    if th.is_dark() {
        ActivityBarColors {
            bar_bg: rgb(0x1E293B).into(),
            bar_border: rgb(0x16202E).into(),
            idle_fg: rgb(0x94A3B8).into(),
            active_fg: rgb(0xFFFFFF).into(),
            hover_pill: rgba(0xFFFFFF14).into(),  // 白 8%
            press_pill: rgba(0xFFFFFF33).into(),  // 白 20%
            active_pill: rgba(0xFFFFFF1F).into(), // 白 12%
            accent: rgb(0x60A5FA).into(),
            divider: rgba(0xFFFFFF24).into(), // 白 14%
        }
    } else {
        let mut active_pill = th.primary;
        active_pill.a = 0.12;
        ActivityBarColors {
            bar_bg: rgb(0xDFE3EE).into(),
            bar_border: rgb(0xC9CFDE).into(),
            idle_fg: th.muted_foreground,
            active_fg: th.primary,
            hover_pill: rgba(0x0000000F).into(),  // 黑 6%
            press_pill: rgba(0x0000001A).into(),  // 黑 10%
            active_pill,
            accent: th.primary,
            divider: rgba(0x00000024).into(), // 黑 14%
        }
    }
}
