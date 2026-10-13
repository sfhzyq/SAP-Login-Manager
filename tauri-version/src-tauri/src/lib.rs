// SAP Login Manager - Tauri 2 desktop application
mod commands;
mod models;
mod storage;
mod crypto;
mod dpapi;
mod sap_parser;
mod sap_automation;

use std::sync::Mutex;
use tauri::Manager;
use tauri_plugin_log::{Target, TargetKind, RotationStrategy};

/// 获取诊断日志路径（exe 同目录下的 data/diagnostic.log）
fn get_diagnostic_log_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
        .join("data")
        .join("diagnostic.log")
}

/// 写诊断日志（独立于 tauri-plugin-log，确保 WebView 加载失败前也能记录）
fn diag_log(msg: &str) {
    use std::io::Write;
    let path = get_diagnostic_log_path();
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let _ = writeln!(file, "[{}] {}", timestamp, msg);
    }
    // 同时输出到控制台
    println!("[diag] {}", msg);
}

/// 检测 WebView2 Runtime 是否已安装
/// 如果未安装，尝试用 Edge 浏览器作为后备渲染引擎
fn ensure_webview2() {
    diag_log("=== 启动诊断 ===");

    // 记录环境信息
    diag_log(&format!("exe 路径: {:?}", std::env::current_exe()));
    diag_log(&format!("架构: {}", std::env::consts::ARCH));
    diag_log(&format!("OS: {}", std::env::consts::OS));

    // 如果用户已手动设置，不覆盖
    if let Ok(path) = std::env::var("WEBVIEW2_BROWSER_EXECUTABLE_PATH") {
        diag_log(&format!("用户已设置 WEBVIEW2_BROWSER_EXECUTABLE_PATH={}", path));
        return;
    }

    // 检查 WebView2 Runtime 是否已安装（通过注册表）
    #[cfg(windows)]
    {
        use winreg::enums::*;
        use winreg::RegKey;

        // 微软官方推荐：检查以下 4 个注册表路径
        // 64 位 Windows: HKLM\WOW6432Node + HKCU\Software
        // 32 位 Windows: HKLM\Software + HKCU\WOW6432Node
        // 加上 KEY_WOW64_64KEY 标志避免 ARM64 进程的重定向
        let reg_paths = [
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}", "HKLM\\SOFTWARE (64位原生)"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}", "HKLM\\WOW6432Node (32位兼容)"),
            (HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}", "HKCU\\SOFTWARE (当前用户)"),
            (HKEY_CURRENT_USER, r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}", "HKCU\\WOW6432Node (32位兼容)"),
        ];

        let mut webview2_found = false;
        for (hive, path, label) in &reg_paths {
            let root = RegKey::predef(*hive);
            // 优先尝试 KEY_WOW64_64KEY 读取原生视图
            let result = root.open_subkey_with_flags(path, KEY_READ | KEY_WOW64_64KEY)
                .or_else(|_| root.open_subkey(path));

            match result {
                Ok(key) => {
                    let pv: String = key.get_value("pv").unwrap_or_default();
                    let name: String = key.get_value("name").unwrap_or_default();
                    diag_log(&format!("注册表命中: {} pv={} name={}", label, pv, name));
                    if !pv.is_empty() && pv != "0.0.0.0" {
                        webview2_found = true;
                    }
                }
                Err(e) => {
                    diag_log(&format!("注册表未命中: {} ({})", label, e));
                }
            }
        }

        // 检查文件系统（权威依据：msedgewebview2.exe 是否真实存在）
        // 64 位系统: C:\Program Files (x86)\Microsoft\EdgeWebView\Application\xxx\
        // ARM64 系统: C:\Program Files\Microsoft\EdgeWebView\Application\xxx\
        let fs_paths = [
            (r"C:\Program Files (x86)\Microsoft\EdgeWebView\Application", "x86 路径"),
            (r"C:\Program Files\Microsoft\EdgeWebView\Application", "ARM64 路径"),
        ];
        let mut webview2_exe_found: Vec<String> = Vec::new();
        for (path, label) in &fs_paths {
            match std::fs::read_dir(path) {
                Ok(entries) => {
                    let version_dirs: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .filter(|n| n != "SetupMetrics" && n.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
                        .collect();
                    for vdir in &version_dirs {
                        let exe_path = std::path::Path::new(path).join(vdir).join("msedgewebview2.exe");
                        if exe_path.exists() {
                            diag_log(&format!("找到 msedgewebview2.exe: {} ({})", exe_path.display(), label));
                            webview2_exe_found.push(exe_path.to_string_lossy().to_string());
                        }
                    }
                    if version_dirs.is_empty() {
                        diag_log(&format!("文件系统空目录: {} ({})", path, label));
                    } else {
                        diag_log(&format!("文件系统目录: {} -> {:?} ({})", path, version_dirs, label));
                    }
                }
                Err(e) => {
                    diag_log(&format!("文件系统未命中: {} ({}, {})", path, label, e));
                }
            }
        }

        // 架构判断：x86_64 vs ARM64
        let is_arm64 = std::env::consts::ARCH == "aarch64";

        if is_arm64 {
            // ARM64 处理：优先使用 ARM64 原生 WebView2，否则回退到 x86 WebView2 (通过 Prism 模拟)
            // 关键：用文件系统检测作为决定性依据 (因为注册表在 ARM64 上有重定向)
            let arm64_native = webview2_exe_found.iter()
                .find(|p| p.contains(r"C:\Program Files\Microsoft\") && !p.contains("(x86)"));
            let x86_fallback = webview2_exe_found.iter()
                .find(|p| p.contains("(x86)"));

            if let Some(path) = arm64_native {
                diag_log(&format!("✓ 找到 ARM64 原生 WebView2: {}", path));
                diag_log("使用 ARM64 原生 WebView2（无需环境变量）");
                return;
            }

            if let Some(path) = x86_fallback {
                // x86 WebView2 可在 ARM64 Windows 上通过 Prism 模拟层运行
                // 直接指向 msedgewebview2.exe 父目录中的 ebwebview2.exe
                let p = std::path::Path::new(path);
                diag_log(&format!("使用 x86 WebView2 (Prism 模拟): {}", path));
                std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_PATH", p);
            } else {
                diag_log("ERROR: 找不到任何 WebView2 Runtime");
            }

            // 设置 WebView2 用户数据目录到用户空间（避免权限问题）
            if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
                let user_data = std::path::PathBuf::from(local_app_data)
                    .join("sap-login-manager").join("webview2");
                let _ = std::fs::create_dir_all(&user_data);
                std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &user_data);
                diag_log(&format!("WebView2 用户数据目录: {:?}", user_data));
            }
        } else {
            // x86_64: 注册表命中即可
            if webview2_found {
                diag_log("WebView2 Runtime 已安装，无需后备");
                return;
            }

            // WebView2 未安装，尝试查找 Edge 浏览器
            diag_log("WebView2 Runtime 未找到，尝试使用 Edge 浏览器作为后备");
            let edge_paths = [
                r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
                r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
                r"C:\Program Files (WindowsApps)\Microsoft.MicrosoftEdge_8wekyb3d8bbwe\msedge.exe",
            ];

            for path in &edge_paths {
                if std::path::Path::new(path).exists() {
                    diag_log(&format!("找到 Edge 浏览器: {}，设为 WebView2 后备引擎", path));
                    std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_PATH", path);
                    return;
                } else {
                    diag_log(&format!("Edge 路径不存在: {}", path));
                }
            }

            diag_log("ERROR: WebView2 Runtime 和 Edge 浏览器均未找到");
        }

        // 弹出 Windows 消息框提示用户
        #[cfg(windows)]
        {
            show_message_box(
                "SAP Login Manager - WebView2 未找到",
                "未检测到 WebView2 Runtime 或 Edge 浏览器。\n\n请安装 WebView2 Runtime 后重试：\nhttps://developer.microsoft.com/microsoft-edge/webview2/\n\n诊断日志已写入 exe 同目录下的 data/diagnostic.log",
            );
        }
    }
}

#[cfg(windows)]
fn show_message_box(title: &str, body: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_OK, MB_ICONERROR, MB_SYSTEMMODAL,
    };
    use std::os::windows::ffi::OsStrExt;

    let title_wide: Vec<u16> = std::ffi::OsStr::new(title)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let body_wide: Vec<u16> = std::ffi::OsStr::new(body)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            body_wide.as_ptr(),
            title_wide.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SYSTEMMODAL,
        );
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 设置 panic hook，捕获所有 panic 信息
    std::panic::set_hook(Box::new(|panic_info| {
        let msg = format!("PANIC: {}", panic_info);
        diag_log(&msg);
        eprintln!("{}", msg);
    }));

    diag_log("=== run() 开始执行 ===");

    // 在 Tauri 初始化之前，确保 WebView2 可用
    ensure_webview2();

    diag_log("ensure_webview2() 完成");

    // 初始化存储数据（失败时使用空数据而非 panic，避免白屏）
    let store_data = storage::init_store().unwrap_or_else(|e| {
        diag_log(&format!("初始化存储失败: {}, 使用空数据启动", e));
        storage::create_empty_store()
    });

    diag_log("存储初始化完成");

    diag_log("开始构建 Tauri Builder");
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_log::Builder::new()
            .targets([
                Target::new(TargetKind::Stdout),
                Target::new(TargetKind::Folder {
                    path: std::env::current_exe()
                        .ok()
                        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
                        .join("data"),
                    file_name: Some("app.log".to_string()),
                }),
                Target::new(TargetKind::Webview),
            ])
            .rotation_strategy(RotationStrategy::KeepOne)
            .max_file_size(2_000_000)
            .build())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        // 手机尺寸窗口：仅恢复位置，尺寸始终使用 tauri.conf.json（440×956），
        // 避免插件恢复旧桌面尺寸覆盖手机布局
        .plugin(tauri_plugin_window_state::Builder::new()
            .with_state_flags(tauri_plugin_window_state::StateFlags::POSITION)
            .build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None::<Vec<&str>>,
        ))
        .manage(Mutex::new(store_data))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 检查 close_to_tray 设置
                let app = window.app_handle();
                if let Some(state) = app.try_state::<Mutex<models::StoreData>>() {
                    let data = state.lock().unwrap_or_else(|e| e.into_inner());
                    if data.settings.close_to_tray {
                        // 阻止关闭，改为隐藏到托盘
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
            // 最小化到托盘：窗口最小化时隐藏到托盘（由 minimize_to_tray 设置控制）
            if let tauri::WindowEvent::Resized(_) = event {
                let app = window.app_handle();
                if let Some(state) = app.try_state::<Mutex<models::StoreData>>() {
                    let minimize_to_tray = {
                        let data = state.lock().unwrap_or_else(|e| e.into_inner());
                        data.settings.minimize_to_tray
                    };
                    if minimize_to_tray && window.is_minimized().unwrap_or(false) {
                        let _ = window.unminimize();
                        let _ = window.hide();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // Auth commands
            commands::auth::set_master_password,
            commands::auth::verify_master_password,
            commands::auth::change_master_password,
            commands::auth::is_password_set,
            commands::auth::remember_master_password,
            commands::auth::get_remembered_password,
            commands::auth::clear_remembered_password,
            // Credential commands
            commands::credentials::get_credentials,
            commands::credentials::add_credential,
            commands::credentials::update_credential,
            commands::credentials::delete_credential,
            commands::credentials::restore_credentials,
            commands::credentials::get_decrypted_password,
            commands::credentials::get_credential,
            commands::credentials::toggle_favorite,
            commands::credentials::reorder_favorites,
            commands::credentials::share_credential,
            commands::credentials::batch_login,
            // Group commands
            commands::credentials::get_groups,
            commands::credentials::add_group,
            commands::credentials::update_group,
            commands::credentials::rename_group,
            commands::credentials::delete_group,
            commands::credentials::delete_group_with_options,
            commands::credentials::move_credential_to_group,
            commands::credentials::batch_move_to_group,
            commands::credentials::batch_favorite,
            // SAP commands
            commands::sap::open_sap_logon,
            commands::sap::login_to_sap,
            commands::sap::get_sap_connections,
            commands::sap::import_sap_connections,
            // Transfer commands
            commands::transfer::export_credentials,
            commands::transfer::import_credentials,
            // Settings commands
            commands::settings::get_settings,
            commands::settings::save_settings,
        ])
        .setup(|app| {
            diag_log("setup() 开始执行");

            // 强制移除窗口系统装饰（双重保障，防止 window-state 插件恢复装饰）
            if let Some(window) = app.get_webview_window("main") {
                diag_log(&format!("主窗口已创建: {:?}", window.title()));
                let _ = window.set_decorations(false);

                // 检查 WebView2 是否真正加载
                diag_log(&format!("窗口 URL: {:?}", window.url()));
                diag_log(&format!("窗口可见: {}", window.is_visible().unwrap_or(false)));

                // 正常情况下窗口已通过 tauri.conf.json 的 window.url 加载 index.html。
                // 若 URL 仍为 about:blank（异常回退），使用 Tauri 自定义协议的绝对地址导航，
                // 避免使用 "/index.html" 相对路径被 WebView 解析成 http://localhost 导致“拒绝连接”。
                let url = window.url();
                if let Ok(ref u) = url {
                    if u.scheme() == "about" {
                        diag_log("URL 是 about:blank，回退导航到 tauri://localhost/index.html");
                        // Windows 上 Tauri 2 自定义协议为 http://tauri.localhost
                        let js = "window.location.replace('http://tauri.localhost/index.html');";
                        match window.eval(js) {
                            Ok(_) => diag_log("回退导航 JS 执行成功"),
                            Err(e) => diag_log(&format!("回退导航 JS 执行失败: {:?}", e)),
                        }
                    } else {
                        diag_log(&format!("URL scheme 正常: {}", u.scheme()));
                    }
                }

                // 尝试执行 JS 检查 WebView2 是否真正工作
                match window.eval("console.log('WebView2 OK')") {
                    Ok(_) => diag_log("WebView2 JS 执行成功（WebView2 已加载）"),
                    Err(e) => diag_log(&format!("WebView2 JS 执行失败: {:?}", e)),
                }

                // 窗口初始 visible:false，待 window-state 插件恢复尺寸后再显示，
                // 避免「从宽到窄」的视觉跳变。
                let _ = window.show();
                let _ = window.set_focus();
            } else {
                diag_log("ERROR: 主窗口创建失败！");
            }
            setup_tray(app)?;
            // 根据配置同步开机自启状态（防止外部手动改动导致不一致）
            {
                use tauri_plugin_autostart::ManagerExt;
                let want = app
                    .try_state::<Mutex<models::StoreData>>()
                    .map(|s| s.lock().unwrap_or_else(|e| e.into_inner()).settings.auto_start)
                    .unwrap_or(false);
                let mgr = app.autolaunch();
                let enabled = mgr.is_enabled().unwrap_or(false);
                if want && !enabled {
                    let _ = mgr.enable();
                } else if !want && enabled {
                    let _ = mgr.disable();
                }
            }
            diag_log("setup() 完成");
            Ok(())
        })
        .build(tauri::generate_context!());

    match app {
        Ok(app) => {
            diag_log("app.build() 成功，进入 run_iteration 循环");
            app.run(|_app_handle, _event| {});
        }
        Err(e) => {
            diag_log(&format!("FATAL: Tauri build 失败: {:?}", e));
            show_message_box("启动失败", &format!("应用启动失败: {}", e));
        }
    }
}

use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState},
};

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItemBuilder::with_id("show", "显示主窗口").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&show)
        .separator()
        .item(&quit)
        .build()?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().ok_or("未找到默认窗口图标")?.clone())
        .menu(&menu)
        .tooltip("SAP Login Manager")
        .on_menu_event(|app, event| {
            match event.id.as_ref() {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            // 左键单击托盘图标时显示主窗口
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}