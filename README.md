# SAP Login Manager

<p align="center">
  一款为 SAP 顾问与运维人员打造的<strong>本地化、加密存储、一键登录</strong>桌面凭据管理工具。
</p>

<p align="center">
  <img alt="version" src="https://img.shields.io/badge/version-1.3.0-blue.svg" />
  <img alt="platform" src="https://img.shields.io/badge/platform-Windows-0078D6.svg?logo=windows" />
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2.x-24C8DB.svg?logo=tauri" />
  <img alt="React" src="https://img.shields.io/badge/React-18-61DAFB.svg?logo=react" />
  <img alt="TypeScript" src="https://img.shields.io/badge/TypeScript-5.6-3178C6.svg?logo=typescript" />
  <img alt="Rust" src="https://img.shields.io/badge/Rust-stable-000000.svg?logo=rust" />
  <img alt="license" src="https://img.shields.io/badge/license-MIT-green.svg" />
</p>

---

## 📖 简介

**SAP Login Manager** 是一个基于 [Tauri 2](https://tauri.app/) 构建的轻量级 Windows 桌面应用，用于集中管理 SAP 系统的登录凭据。所有数据均使用 **AES-256-GCM** 加密后**保存在本地**（无任何云端上传），并支持从 `SAPUILandscape.xml` 导入连接、一键/批量启动 SAP Logon 登录，帮助你告别重复手动输入账号密码。

> 🔒 **隐私优先**：凭据仅存储在你自己的电脑上，密钥由主密码派生，明文密码永不落盘。

---

## 📦 目录结构

本仓库包含三个实现版本（共享同一份数据格式）：

| 目录 | 版本 | 状态 |
|---|---|---|
| `gpui-version/` | **GPUI 原生版**（Rust + gpui-kit） | ✅ 当前主力开发版本 |
| `tauri-version/` | Tauri 2 + React + TypeScript 版 | 🗄 参照实现，可独立构建（CI 使用） |
| `slint-app/` | Slint 版 | 🧪 技术探索 |
| `archive/` | 历史归档（设计稿预览页、旧截图、特性规格） | 📦 仅存档 |

- GPUI 版构建：仓库根执行 `.\build-gpui.ps1 [check|build|run] [debug|release]`
- Tauri 版构建：`tauri-version/` 内 `npm install && npm run tauri build`
- 版本详细说明见 `gpui-version/README.md`

---

## ✨ 核心功能

### 凭据管理
- 增删改查 SAP 连接凭据，支持显示名称、System ID、环境标签、备注
- 收藏置顶、拖拽排序、颜色标记
- 删除支持撤销（Undo）
- 全局搜索并高亮命中关键词

### 分组与视图
- 按环境（生产 / 测试 / 开发 / 配置）系统分组折叠展示
- 支持自定义分组，凭据可在分组间移动
- **默认打开的分组**：可在设置中指定启动后默认展示的分组（全部 / 收藏 / 系统分组 / 自定义分组）
- 紧凑密度模式，一屏显示更多凭据

### 登录与自动化
- 一键启动 SAP Logon 并自动填充登录
- **批量登录**：多选凭据后依次登录，并汇总成功/失败明细
- 从 `SAPUILandscape.xml` 导入 SAP 连接
- 支持自定义 SAP Logon 可执行文件路径

### 安全
- 主密码保护，PBKDF2 密钥派生（**600,000 轮**，OWASP 2023 推荐）
- 数据 **AES-256-GCM** 加密（带完整性认证），并自动从旧版格式安全迁移
- **免密模式（Windows DPAPI）**：基于当前 Windows 账户绑定加密主密码，实现下次启动自动解锁；密文离开本机/本账户无法解密
- 剪贴板自动清空（复制密码后定时清除）
- 空闲自动锁定

### 体验
- 中文 / English / 日本語 三语界面
- 亮色 / 暗色主题
- 系统托盘、开机自启、窗口置顶
- 加密导出 / 导入凭据包，便于迁移与备份

---

## 🖼️ 界面预览

> 运行后可自行截图替换此处。

```
┌──────────────────────────────┐
│  SAP Login Manager           │
│  🔍 搜索凭据...               │
│  ▸ 生产环境 (3)               │
│  ▸ 测试环境 (5)               │
│  ▸ 自定义分组 (2)             │
│  [ 一键登录 ] [ 批量登录 ]    │
└──────────────────────────────┘
```

---

## 🧱 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面框架 | Tauri 2.x（Rust + WebView2） |
| 前端 | React 18 + TypeScript 5.6 + Vite 6 |
| 后端 | Rust（stable） |
| 加密 | AES-256-GCM、PBKDF2-HMAC-SHA256、Windows DPAPI |
| 存储 | 本地 JSON（exe 同目录 `data/store.json`，便携化） |
| 打包 | NSIS / MSI 安装包 |

---

## 🚀 快速开始

### 环境要求
- [Node.js](https://nodejs.org/) 18+
- [Rust](https://www.rust-lang.org/tools/install)（stable 工具链）
- Windows 10/11 + [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)（Win11 已内置）

### 安装依赖
```bash
npm install
```

### 开发调试
```bash
npm run dev          # 仅前端（Vite）
npx tauri dev        # 完整桌面应用（前端 + Rust 后端热重载）
```

### 生产构建
```bash
npx tauri build
```
> 也可手动分步构建：
> ```bash
> npm run build                                           # 构建前端到 dist/
> cargo build --release --features custom-protocol \      # 构建后端并嵌入前端资源
>   --manifest-path src-tauri/Cargo.toml
> ```
> ⚠️ 生产构建必须启用 `custom-protocol` 特性，否则运行时会报 `asset not found: index.html`。

构建产物：
- 可执行文件：`src-tauri/target/release/sap-login-manager.exe`
- 安装包：`src-tauri/target/release/bundle/`（NSIS `.exe` 与 MSI `.msi`）

---

## 📂 项目结构

```
sap-login-manager/
├── src/                      # 前端（React + TypeScript）
│   ├── components/           # 组件（凭据/设置/导入等弹窗）
│   ├── i18n/                 # 多语言文案（zh / en / ja）
│   ├── styles/               # 全局样式
│   ├── App.tsx               # 主界面逻辑
│   └── types.ts              # 类型定义
├── src-tauri/                # 后端（Rust + Tauri）
│   ├── src/
│   │   ├── commands/         # Tauri 命令（auth/credentials/sap/settings/transfer）
│   │   ├── crypto.rs         # 加密与密钥派生
│   │   ├── dpapi.rs          # Windows DPAPI 封装（免密模式）
│   │   ├── storage.rs        # 本地存储读写
│   │   ├── models.rs         # 数据模型
│   │   ├── sap_parser.rs     # SAPUILandscape.xml 解析
│   │   └── sap_automation.rs # SAP Logon 登录自动化
│   ├── Cargo.toml
│   └── tauri.conf.json
└── package.json
```

---

## 🔐 安全说明

- 主密码**不会**以任何形式明文存储，仅保存其哈希用于校验；加密密钥由主密码 + 随机盐经 PBKDF2（600,000 轮）派生。
- 所有凭据密码使用 AES-256-GCM 加密，具备防篡改的完整性认证。
- **免密模式**通过 Windows DPAPI（`CryptProtectData` / `CryptUnprotectData`）加密主密码，密文与当前 Windows 用户账户绑定：即使复制 `store.json` 到其他电脑或账户，也无法解密还原。
- 数据文件位于 exe 同目录 `data/store.json`（便携模式）。**请注意**：执行 `cargo clean` 会连同 `target` 目录一起删除该数据，正式使用时建议将应用安装/复制到独立目录并定期备份。

---

## 🗺️ 版本亮点（v1.3.0）

- ✅ 新增 **免密模式（DPAPI 自动解锁）**
- ✅ 新增 **默认打开的分组** 设置（支持系统分组与自定义分组）
- ✅ 修复解锁后主界面偶发崩溃问题
- ✅ 大量列表 / 分组 / 键盘导航 / 批量登录体验优化

---

## 🤝 贡献

欢迎提交 Issue 与 Pull Request。建议在开发前先运行 `npx tauri dev` 确认环境正常。

## 📮 反馈

如有问题或建议，欢迎通过邮箱联系：`sfhzyq@qq.com`

## 📄 许可证

本项目采用 [MIT](./LICENSE) 许可证。
