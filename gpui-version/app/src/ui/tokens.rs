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

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{rgb, rgba, App, Hsla};

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
    /// 悬停叠加（idle 反馈）
    pub hover_pill: Hsla,
    /// 按压叠加（比悬停重一档）
    pub press_pill: Hsla,
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
    let th = cx.theme();
    if th.is_dark() {
        ActivityBarColors {
            bar_bg: rgb(0x1E293B).into(),
            idle_fg: rgb(0xA8B1C4).into(),
            hover_pill: rgba(0xFFFFFF14).into(), // 白 8%
            press_pill: rgba(0xFFFFFF33).into(), // 白 20%
            divider: rgba(0xFFFFFF24).into(),    // 白 14%
        }
    } else {
        ActivityBarColors {
            // 灰阶梯（2026-09-30 定稿方案 B）：v1.4.0 标题栏统一画布后，白壳与
            // 白卡片同值争层；改 secondary #D8DDE8 形成 rail(深)→画布→白卡(亮)
            // 明度阶梯，中性灰不参与彩色竞争，白壳"退出颜色竞争"初衷保留
            bar_bg: rgb(0xD8DDE8).into(),
            // 同步加深一档：muted_foreground 对灰底约 3.5:1，#5A6280 回到 ~4:1
            idle_fg: rgb(0x5A6280).into(),
            hover_pill: rgba(0x0000000F).into(), // 黑 6%
            press_pill: rgba(0x0000001A).into(), // 黑 10%
            divider: rgba(0x00000024).into(),    // 黑 14%
        }
    }
}

// ============ Island 浮动布局（v1.5.7 方案 B，可切换主题）============
//
// VS Code island 风（v1.5.7 方案 1 拓扑）：窗口 canvas 底色上，顶部窗口 chrome、
// 活动栏与主内容区作为三块圆角浮岛（radius_lg + border + 间距）呈现。内容岛
// 表面 = theme().background（标题栏/MainPage 本就铺 background，岛内零改动）；
// 活动栏保留 bar_bg tint 身份。canvas 比全部分层深一档，形成四级明度阶梯
// （2026-10-23 真机反馈：原 canvas 与岛面只差一档读不出浮起，遂加深 +
// rail 恢复 tint + 间隙 8→6）：
// - 浅色：#C9CFDD —— canvas < rail #D8DDE8 < 岛面 #E2E6EF < 卡片 #FFF
// - 深色：#03040A —— canvas < 岛面 #0A0B14 < rail #1E293B（VS Code 风）
// 均带 alpha 透 Mica（间隙透出壁纸，与「列表空隙透 Mica」哲学一致）。

/// 浮岛与窗口边缘 / 浮岛之间的间隙
pub const ISLAND_GAP: f32 = 6.0;

/// Island 模式 canvas 底色（窗口背景层，间隙处可见）
pub fn island_canvas(cx: &App) -> Hsla {
    if cx.theme().is_dark() {
        rgba(0x03040AB3).into()
    } else {
        rgba(0xC9CFDDE6).into()
    }
}

/// 「页面根 / 全出血条」底色（v1.5.7 岛缘瑕疵修复）
///
/// island 态返回全透明：岛容器自铺背景 + 圆角边框后，页面根、页脚条等
/// 全出血子元素若再铺不透明底，会以方形盖住岛的圆角与边框内半——
/// gpui-pre 0.3.9 的 ContentMask 只有 bounds 无 corner_radii，
/// `overflow_hidden` 是矩形裁切，圆角必须靠"子元素不画、岛容器画"保证。
/// classic 态维持原 background，观感不变。
pub fn page_bg(cx: &App) -> Hsla {
    let island = {
        let s = crate::state::AppState::global(cx).store.lock();
        s.settings.island_layout
    };
    if island {
        rgba(0x00000000).into()
    } else {
        cx.theme().background
    }
}

// ============ 环境分类色（数据色，按环境固定映射）============
//
// 与 Tauri 版 ENV_COLORS 一致；颜色本身承载环境语义（生产红 / 测试黄 / 开发绿 /
// 配置紫 / 未分类灰），属指南允许的"颜色即数据"审计例外，集中在本 token 层而非组件内。
// 浅色用原色；深色改用同色相深底浅字（主题感知，见各函数注释）。

/// 环境主色（头像 / 标记基准色）
pub fn env_color(env: &str) -> Hsla {
    match env {
        "production" => rgb(0xda1e28).into(),    // 红
        "test" => rgb(0xf1c21b).into(),          // 黄
        "development" => rgb(0x198038).into(),   // 绿
        "configuration" => rgb(0x8a3ffc).into(), // 紫
        _ => rgb(0x6b7280).into(),               // 灰（未分类）
    }
}

/// 环境色点色：test 黄对白底仅 1.68:1（8px 圆点不可见），单独加深到 #9A7B00
/// （对白 4.03 / 对画布 3.23，过图形 3:1 线）；其余沿用 env_color
pub fn env_dot_color(env: &str) -> Hsla {
    match env {
        "test" => rgb(0x9a7b00).into(), // 深黄（色点可见性修正）
        other => env_color(other),
    }
}

/// 环境浅 / 柔色组（头像浅底 + 深字）；主题感知：深色改深底浅字（色相同源、
/// 明度反转），浅字对深底过 AA
pub fn env_soft_colors(env: &str, cx: &App) -> (Hsla, Hsla) {
    if cx.theme().is_dark() {
        match env {
            "production" => (rgb(0x3a2027).into(), rgb(0xf5a8ad).into()), // 红
            "test" => (rgb(0x3a3115).into(), rgb(0xe8c96a).into()),       // 黄
            "development" => (rgb(0x1e3322).into(), rgb(0x9fd8a8).into()), // 绿
            "configuration" => (rgb(0x2c2749).into(), rgb(0xc4b8f8).into()), // 紫
            _ => (rgb(0x272730).into(), rgb(0xb8bcc4).into()),            // 灰
        }
    } else {
        match env {
            "production" => (rgb(0xfcebeb).into(), rgb(0x993c1d).into()), // 红
            "test" => (rgb(0xfaeeda).into(), rgb(0x633806).into()),       // 黄
            "development" => (rgb(0xeaf3de).into(), rgb(0x3b6d11).into()), // 绿
            "configuration" => (rgb(0xeeedfe).into(), rgb(0x534ab7).into()), // 紫
            _ => (rgb(0xf1efe8).into(), rgb(0x444441).into()),            // 灰（未分组）
        }
    }
}
