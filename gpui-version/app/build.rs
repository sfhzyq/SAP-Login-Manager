//! 构建脚本：向 Windows 可执行文件嵌入应用图标资源。
//!
//! gpui 在 Windows 平台不设置窗口/类图标（源码中无 LoadIconW / WM_SETICON），
//! 任务栏与资源管理器会回退读取 exe 内嵌的图标资源——因此这里用 winresource
//! 把 icons/icon.ico 编译进 exe。
//!
//! rc.exe 发现策略：优先扫描 Windows SDK 标准安装目录（不依赖注册表查询，
//! 沙箱/精简环境下 reg.exe 可能被拦），找不到再交给 winresource 默认逻辑。
//! 嵌入失败仅告警，不阻断构建。

use std::path::PathBuf;

/// 扫描 Windows SDK 标准安装路径，返回含 rc.exe 的 bin 目录
fn find_sdk_bin_dir() -> Option<PathBuf> {
    const SDK_ROOTS: &[&str] = &[
        r"C:\Program Files (x86)\Windows Kits\10",
        r"C:\Program Files\Windows Kits\10",
    ];
    for root in SDK_ROOTS {
        let bin = PathBuf::from(root).join("bin");
        let mut versions: Vec<_> = std::fs::read_dir(&bin)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        // 版本号降序，优先用最新 SDK
        versions.sort_by_key(|a| std::cmp::Reverse(a.file_name()));
        for v in versions {
            let dir = v.path().join("x64");
            if dir.join("rc.exe").is_file() {
                return Some(dir);
            }
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=icons/icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        if let Some(dir) = find_sdk_bin_dir() {
            res.set_toolkit_path(dir.to_str().expect("sdk path is utf-8"));
        }
        if let Err(e) = res.compile() {
            println!("cargo:warning=嵌入应用图标失败（任务栏将显示默认图标）: {e}");
        }
    }
}
