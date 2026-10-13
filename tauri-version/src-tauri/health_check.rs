// 独立健康检查程序：测试 WebView2 检测逻辑
// 编译：rustc --edition 2021 -L dependency target\release\deps health_check.rs
// 或者用 cargo test 形式

use std::process::Command;

fn main() {
    println!("=== WebView2 健康检查 ===\n");

    // 1. 系统信息
    println!("架构: {}", std::env::consts::ARCH);
    println!("OS: {}", std::env::consts::OS);
    println!();

    // 2. 注册表检测（用 reg query 命令，独立于 winreg 库）
    println!("=== 注册表检测 (reg query) ===");
    let reg_paths = [
        r"HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        r"HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        r"HKCU\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        r"HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}\pv",
    ];

    for path in &reg_paths {
        let output = Command::new("reg")
            .args(&["query", path, "/v", "pv"])
            .output();
        match output {
            Ok(o) => {
                if o.status.success() {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    println!("✓ 命中: {}", path);
                    for line in stdout.lines() {
                        if line.contains("pv") || line.contains("REG_SZ") {
                            println!("  {}", line.trim());
                        }
                    }
                } else {
                    println!("✗ 未命中: {}", path);
                }
            }
            Err(e) => println!("✗ 错误: {} ({})", path, e),
        }
    }

    println!();

    // 3. 文件系统检测
    println!("=== 文件系统检测 ===");
    let fs_paths = [
        r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application",
        r"C:\Program Files\Microsoft\EdgeWebView\Application",
    ];

    for path in &fs_paths {
        println!("\n检查: {}", path);
        match std::fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    if is_dir && name != "SetupMetrics" {
                        let exe_path = std::path::Path::new(path).join(&name).join("msedgewebview2.exe");
                        let exe_exists = exe_path.exists();
                        println!("  版本目录: {} -> msedgewebview2.exe 存在: {}", name, exe_exists);
                        if exe_exists {
                            // 检查文件大小
                            if let Ok(meta) = std::fs::metadata(&exe_path) {
                                println!("    文件大小: {} bytes", meta.len());
                            }
                        }
                    }
                }
            }
            Err(e) => println!("  目录不存在: {}", e),
        }
    }

    println!();

    // 4. Edge 浏览器检测
    println!("=== Edge 浏览器检测 ===");
    let edge_paths = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ];
    for path in &edge_paths {
        println!("{}: 存在={}", path, std::path::Path::new(path).exists());
    }

    println!();

    // 5. 环境变量
    println!("=== 环境变量 ===");
    match std::env::var("WEBVIEW2_BROWSER_EXECUTABLE_PATH") {
        Ok(v) => println!("WEBVIEW2_BROWSER_EXECUTABLE_PATH = {}", v),
        Err(_) => println!("WEBVIEW2_BROWSER_EXECUTABLE_PATH = (未设置)"),
    }
    match std::env::var("WEBVIEW2_USER_DATA_FOLDER") {
        Ok(v) => println!("WEBVIEW2_USER_DATA_FOLDER = {}", v),
        Err(_) => println!("WEBVIEW2_USER_DATA_FOLDER = (未设置)"),
    }

    println!("\n=== 健康检查完成 ===");
}
