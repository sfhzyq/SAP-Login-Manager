//! 窗口状态记忆：保存/恢复主窗口位置与尺寸
//!
//! 数据保存在 exe 同目录 data/window-state.json；
//! 宽高为可选字段，兼容旧版只存位置的文件。
//!
//! 恢复位置时按当前虚拟屏幕（所有显示器的并集）钳制：
//! 上次会话的外接显示器已断开时，窗口不再恢复到不可见区域。

use std::path::PathBuf;

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct WindowStateData {
    x: f64,
    y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    w: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    h: Option<f64>,
}

fn state_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("data")
        .join("window-state.json")
}

#[cfg(windows)]
/// 把 (x, y, w, h) 钳制进虚拟屏幕范围，保证窗口至少有局部可见。
/// 虚拟屏幕 = 所有显示器的并集（GetSystemMetrics SM_*VIRTUALSCREEN）。
fn clamp_to_virtual_screen(x: f64, y: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    // SAFETY: GetSystemMetrics 是无副作用的系统指标查询
    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN) as f64,
            GetSystemMetrics(SM_YVIRTUALSCREEN) as f64,
            GetSystemMetrics(SM_CXVIRTUALSCREEN) as f64,
            GetSystemMetrics(SM_CYVIRTUALSCREEN) as f64,
        )
    };
    if vw <= 0.0 || vh <= 0.0 {
        return (x, y, w, h); // 指标异常时不动
    }
    // 至少保证 100×60 可见（多屏拼接极窄时退化）
    let vis_w = 100.0_f64.min(vw);
    let vis_h = 60.0_f64.min(vh);
    let w = w.min(vw);
    let h = h.min(vh);
    // 用 max/min 链而非 clamp：后者在 min>max（理论退化场景）时会 panic
    let x = x.max(vx + vis_w - w).min(vx + vw - vis_w);
    let y = y.max(vy).min(vy + vh - vis_h);
    (x, y, w, h)
}

/// 窗口坐标/尺寸对：(x, y) 或 (w, h)
pub type PointPair = (f64, f64);

/// 读取上次保存的窗口位置与尺寸（已按当前显示器布局钳制）
pub fn load_window_bounds() -> (Option<PointPair>, Option<PointPair>) {
    let path = state_path();
    let Ok(content) = std::fs::read_to_string(path) else {
        return (None, None);
    };
    let Ok(data) = serde_json::from_str::<WindowStateData>(&content) else {
        return (None, None);
    };
    let mut pos = Some((data.x, data.y));
    let mut size = data.w.zip(data.h);
    #[cfg(windows)]
    {
        let (x, y) = pos.unwrap_or((0.0, 0.0));
        let (w, h) = size.unwrap_or((0.0, 0.0));
        let (x, y, w, h) = clamp_to_virtual_screen(x, y, w, h);
        pos = Some((x, y));
        size = if data.w.is_some() && data.h.is_some() {
            Some((w, h))
        } else {
            None
        };
    }
    (pos, size)
}

/// 保存窗口位置与尺寸
pub fn save_window_bounds(x: f64, y: f64, w: f64, h: f64) {
    let path = state_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let data = WindowStateData {
        x,
        y,
        w: Some(w),
        h: Some(h),
    };
    if let Ok(json) = serde_json::to_string(&data) {
        // 临时文件 + rename 原子替换：写一半被杀进程/断电不会留下损坏 JSON
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}
