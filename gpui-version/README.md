# SAP Login Manager（GPUI 原生版）

SAP 登录凭据管理小工具的 **Rust 原生重写版**：以 [Zed](https://zed.dev) 的 GPU 加速 UI 框架 **GPUI** 替代 Tauri + WebView 技术栈，无浏览器内核、无云端依赖，单 exe 便携运行。

> 同仓库的 `src/` + `src-tauri/` 为 Tauri 旧版（参照物）；本目录 `backend/` + `app/` 为 GPUI 版，是当前活跃开发版本。

## 功能特性

- **凭据管理**：SAP 连接凭据的增删改查，支持直连 / 负载均衡两种连接类型、SNC/SSO、颜色标签、备注
- **分组体系**：系统环境分组（生产/测试/开发/配置）+ 自定义分组，可折叠展示、按分组筛选
- **快捷登录**：调用本机 SAP GUI（`sapshcut.exe` / `saplogon.exe`）自动登录，支持批量登录、登录后动作
- **快速检索**：Ctrl+K 命令面板，多字段子序列模糊匹配，Enter 直接登录；全局热键唤起
- **导入 / 导出**：
  - 从本机 SAP Logon 配置（Landscape XML / 注册表）一键导入，按 Workspace 自动分组
  - 凭据导出为 JSON 备份（含系统对话框选择保存位置），备份文件可再导入合并（去重）
- **安全**：Argon2id 主密码派生 + AES-256-GCM 加密存储；锁定页 / 自动锁定；剪贴板自动清空
- **系统集成**：系统托盘、窗口置顶、自绘标题栏、任务栏图标、中英双语界面、明暗主题
- **便携化**：所有数据存储在 exe 同级 `data/` 目录，拷贝即迁移

## 技术栈

| 层次 | 技术 |
|---|---|
| 语言 | Rust 2021 edition（workspace：`backend` + `app` 两个 crate） |
| UI 框架 | [GPUI](https://gpui.rs)（`gpui-pre` 0.3.x，GPU 加速即时模式） |
| 组件库 | gpui-kit 0.6.1（[gpui-component](https://github.com/longbridge/gpui-component) fork，checkout `84f57fd`） |
| 目标平台 | Windows 10/11（保留跨平台编译能力） |
| 构建 | Cargo；`winresource` 嵌入 exe 图标资源；release 默认 O2 + codegen-units=1（不用 fat LTO） |

## 开源组件致谢

### UI 框架与组件

| 组件 | 版本 | 链接 | 用途 |
|---|---|---|---|
| gpui（gpui-pre） | 0.3.x | https://gpui.rs | GPU 加速 UI 框架（Zed） |
| gpui-kit | 0.6.1 | https://github.com/longbridge/gpui-component | UI 组件库（按钮/输入框/表格/弹窗/主题） |

### 加密与安全

| 组件 | 版本 | 链接 | 用途 |
|---|---|---|---|
| argon2 | 0.5.3 | https://github.com/RustCrypto/password-hashes | 主密码 Argon2id 派生（m=19MiB, t=2, p=1） |
| pbkdf2 / sha2 / hmac | 0.12 / 0.10 / 0.12 | https://github.com/RustCrypto/hashes | 旧版主密码派生与校验（兼容迁移） |
| aes / aes-gcm / cbc | 0.8 / 0.10 / 0.1 | https://github.com/RustCrypto/block-ciphers | 凭据加密（当前 AES-256-GCM，旧数据 CBC 兼容解密） |
| subtle | 2 | https://github.com/RustCrypto/utils | 恒定时间比较（防时序攻击） |
| rand | 0.8 | https://github.com/rust-random/rand | salt / nonce / IV 生成 |
| zeroize | 1 | https://github.com/RustCrypto/utils | 锁定/销毁时清零主密码内存 |
| secrecy | 0.10.3 | https://github.com/iqlusioninc/crates | 运行时主密码类型化封装（Drop 清零 + Debug 打码） |

### 序列化与数据处理

| 组件 | 版本 | 链接 | 用途 |
|---|---|---|---|
| serde / serde_json | 1 | https://github.com/serde-rs/serde | 数据序列化（store.json / 备份包） |
| chrono | 0.4 | https://github.com/chronotope/chrono | 日期时间（登录记录/备份时间） |
| uuid | 1 | https://github.com/uuid-rs/uuid | 凭据 ID 生成 |
| base64 | 0.22 | https://github.com/marshallpierce/rust-base64 | 密文与 salt 编解码 |
| xmltree | 0.10.3 | https://github.com/PoiScript/xmltree | SAP Logon Landscape 配置解析 |
| thiserror | 2 | https://github.com/dtolnay/thiserror | 错误类型定义 |

### 系统集成

| 组件 | 版本 | 链接 | 用途 |
|---|---|---|---|
| arboard | 3.6.1 | https://github.com/1Password/arboard | 跨平台剪贴板（复制密码/链接，自动清空） |
| winreg | 0.55 | https://github.com/gentoo/winreg-rs | Windows 注册表读取（SAP Logon 配置） |
| windows-sys | 0.59 | https://github.com/microsoft/windows-rs | Win32：DPAPI 加解密、窗口控制、文件落盘 |
| tray-icon | 0.19.3 | https://github.com/tauri-apps/tray-icon | 系统托盘（Tauri 生态独立 crate） |
| global-hotkey | 0.6.4 | https://github.com/tauri-apps/global-hotkey | 全局热键（唤起命令面板） |
| rfd | 0.15.4 | https://github.com/PolyMeilex/rfd | 原生文件对话框（导出另存 / 备份选择） |
| log | 0.4 | https://github.com/rust-lang/log | 日志门面 |

### 构建依赖

| 组件 | 版本 | 链接 | 用途 |
|---|---|---|---|
| winresource | 0.1.31 | https://github.com/mxre/winres | build.rs 将 icon.ico 嵌入 exe 资源（任务栏/资源管理器图标） |

## 安全设计

```
主密码 ──Argon2id(salt)──> 256-bit 密钥 ──AES-256-GCM──> 凭据密码密文
                              │
                              └──hash──> master_password_hash（防篡改校验）
```

- **加密格式版本化**：`key_derivation_version`（0=legacy → 1=PBKDF2 → 2=Argon2id）+ `encryption_version`（0=CBC → 1=GCM），旧数据在解锁验证成功后**原子自动迁移**（先全量解密到临时缓冲区，成功后统一重加密落盘）
- **免密模式**：主密码经 Windows DPAPI 加密存储（`remembered_master`），密文仅当前 Windows 用户可解密，拷贝到其他机器/账户无法还原
- **内存安全**：运行时主密码用 `SecretString` 持有（Drop 清零 + Debug 打码），锁定时主动清零；表单副本销毁时 zeroize
- **存储原子性**：写入先落临时文件 + fsync，再 rename 覆盖（Windows 下带退避重试对抗杀软瞬时占用），并保留 `.bak` 备份

## 构建与运行

```powershell
# 完整构建（release）
cd gpui-version
cargo build --release -p sap-login-manager-gpui

# 运行（便携版：把 exe 放任意目录，数据自动生成在同级 data/）
target\release\sap-login-manager-gpui.exe
```

- 仓库内提供了辅助脚本：根目录 `build-gpui.ps1`（check / build / run，支持 debug/release 参数）
- 构建图标资源需要 Windows SDK 中的 `rc.exe`（build.rs 自动扫描 `Windows Kits\10\bin`）
- 测试：`cargo test -p sap-backend`（加密格式往返等单元测试）

## 目录结构

```
gpui-version/
├── Cargo.toml            # workspace 配置（版本 0.3.0）
├── backend/              # sap-backend：存储/加密/SAP 解析/自动化（纯逻辑，无 UI）
│   └── src/
│       ├── crypto.rs     # Argon2id/PBKDF2 派生 + AES-GCM/CBC 加解密（含单元测试）
│       ├── storage.rs    # store.json 原子读写、主密码管理、自动迁移
│       ├── dpapi.rs      # Windows DPAPI 封装（免密模式）
│       ├── models.rs     # 数据模型（Credential/Group/StoreData/ExportBundle）
│       ├── sap_parser.rs # SAP Logon 配置解析
│       └── sap_automation.rs / window_ctl.rs  # 登录自动化与窗口控制
└── app/                  # sap-login-manager-gpui：GPUI 界面
    └── src/
        ├── main.rs       # 入口、窗口常量
        ├── state.rs      # 全局状态、主题
        ├── native_integration.rs  # 托盘/全局热键事件泵
        └── ui/           # 主界面/凭据卡片/表单/分组管理/设置/解锁页/命令面板等
```

## 数据存储

| 路径 | 内容 |
|---|---|
| `data/store.json` | 凭据（密码为密文）、分组、设置（原子写 + `.bak` 备份） |
| `data/exports/` | 导出的 JSON 备份文件 |

**许可证**：本项目代码随仓库根目录许可证分发；上述第三方组件版权与许可证归各自作者所有（致谢列表见「关于」页面，支持一键复制项目链接）。
