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

use gpui_kit::{App, Hsla, rgb};

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
