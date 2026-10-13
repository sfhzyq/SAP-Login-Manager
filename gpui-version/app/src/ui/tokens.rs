//! UI 设计 token：字号阶梯 + 跨主题辅助色
//!
//! 对齐 Tauri 版 `src/styles/global.css` 的排版体系（440px 紧凑窗口）：
//! 全 app 禁止直接写 `text_size(px(N))` 魔法数字与一次性硬编码色，统一引用此处。
//!
//! 字号阶梯（Tauri 版 --fs-*）：
//! - XS 11：chip / 热度 / SNC 徽标 / kbd 提示
//! - SM 11.5：副标题 / 表单标签 / 设置说明
//! - BASE 12.5：卡片名 / 正文 / 列表行 / 按钮默认
//! - MD 13.5：副标题（解锁页）/ 面板说明文字
//! - LG 15：面板标题
//! - XL 18 / XXL 22：关于页 / 解锁页大标题

use gpui_kit::{App, Hsla, rgb, rgba};

/// chip / 徽标 / kbd 最小可读字号
///
/// v1.4.2：10.5 → 11——中文 10.5px 过小，且叠加灰底对比度问题放大；
/// 11px 是中文非关键信息的可读下限
pub const TS_XS: f32 = 11.0;
/// 副标题 / 标签 / 说明文字
pub const TS_SM: f32 = 11.5;
/// 正文基准（卡片名、列表行、正文）
pub const TS_BASE: f32 = 12.5;
/// 强调正文 / 面板副标题
pub const TS_MD: f32 = 13.5;
/// 面板标题
pub const TS_LG: f32 = 15.0;
/// 关于页大标题 / logo 字形
pub const TS_XL: f32 = 18.0;
/// 解锁页大标题
pub const TS_XXL: f32 = 22.0;

// ============ 圆角阶梯（v1.2.0 全界面圆角规范）============
//
// 窗口/弹窗 14 = theme.radius_lg；按钮/输入框 10 = theme.radius（kit 内部消费，
// 不在此重复定义）；胶囊形一律 rounded_full（批量栏/搜索框）。
// 微元素 2/4px（指示点/微徽标）保留字面量，不值得进 token。

/// 卡片圆角（凭据卡、浮动条等自有浮起面）
pub const R_CARD: f32 = 12.0;
/// chip / 环境筛选胶囊圆角
pub const R_CHIP: f32 = 10.0;
/// 内嵌小元素（kbd、行悬停底、小标签底）
pub const R_INNER: f32 = 6.0;
/// 图标容器（头像底、S logo 方底以外的圆角方形容器）
pub const R_ICON_BOX: f32 = 8.0;
/// 主色方徽章（解锁页锁徽章 / 关于页 logo 徽章，均 56px 方底）
///
/// v1.4.2 统一：此前解锁页 rounded_2xl(16)、关于页 px(14) 两种圆角并存
pub const R_BADGE: f32 = 14.0;

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

/// 活动栏配色组：浅色主题白壳 / 深色主题深壳
#[allow(missing_docs)]
pub struct ActivityBarColors {
    /// 栏底色
    pub bar_bg: Hsla,
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
/// - 白壳（浅色主题，v1.2.0）：底 #FBFCFE 近白，与内容区 #EEF0F6 靠**明度**分层
///   （壳最亮 → 画布次之 → 白卡浮起），蓝只保留给激活态；右侧不再画分割线
///   （明度差即分界）。悬停/按压/激活叠加用黑色系（白色叠加在浅底上不可见）。
/// - 深壳（深色主题）：#1E293B VS Code 风 + 白色系叠加（v0.3.0 原设计），
///   同样不画右边线，与浅色镜像对称。
pub fn activity_bar_colors(cx: &App) -> ActivityBarColors {
    use gpui_kit::component::ActiveTheme as _;
    let th = cx.theme();
    if th.is_dark() {
        ActivityBarColors {
            bar_bg: rgb(0x1E293B).into(),
            idle_fg: rgb(0xA8B1C4).into(),
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
            // 灰阶梯（2026-09-30 定稿方案 B）：v1.4.0 标题栏统一画布后，白壳与
            // 白卡片同值争层；改 secondary #D8DDE8 形成 rail(深)→画布→白卡(亮)
            // 明度阶梯，中性灰不参与彩色竞争，白壳"退出颜色竞争"初衷保留
            bar_bg: rgb(0xD8DDE8).into(),
            // 同步加深一档：muted_foreground 对灰底约 3.5:1，#5A6280 回到 ~4:1
            idle_fg: rgb(0x5A6280).into(),
            active_fg: th.primary,
            hover_pill: rgba(0x0000000F).into(),  // 黑 6%
            press_pill: rgba(0x0000001A).into(),  // 黑 10%
            active_pill,
            accent: th.primary,
            divider: rgba(0x00000024).into(), // 黑 14%
        }
    }
}
