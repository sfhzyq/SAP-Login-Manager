//! 窗口状态记忆：保存/恢复主窗口位置与尺寸
//!
//! 数据保存在 exe 同目录 data/window-state.json；
//! 宽高为可选字段，兼容旧版只存位置的文件。

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

/// 窗口坐标/尺寸对：(x, y) 或 (w, h)
pub type PointPair = (f64, f64);

/// 读取上次保存的窗口位置与尺寸
pub fn load_window_bounds() -> (Option<PointPair>, Option<PointPair>) {
    let path = state_path();
    let Ok(content) = std::fs::read_to_string(path) else {
        return (None, None);
    };
    let Ok(data) = serde_json::from_str::<WindowStateData>(&content) else {
        return (None, None);
    };
    let pos = Some((data.x, data.y));
    let size = data.w.zip(data.h);
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
        let _ = std::fs::write(path, json);
    }
}
