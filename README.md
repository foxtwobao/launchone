# LaunchOne

TokenOne 专用的桌面安装配置助手。首版支持 Codex CLI、Claude Code，用户界面只有登录和应用卡片。

当前已实现单页界面、Tauri 命令、内置应用注册表、双工具配置适配器、官方渠道安装入口、字段备份恢复、IDONE 设备登录和最小服务端取 Key/授权衔接。**lab 已通过真实设备登录、刷新、首次浏览器授权、个人 Key 获取及默认模型 HTTP 调用；当前供内测，仍需 macOS / Windows 真机 CLI 验收。**

## 本地运行

需要 Node.js 22.12+、Rust，以及当前系统的 Tauri 2 构建依赖。Node.js 版本还需满足锁定依赖的 engines 要求。

```bash
npm ci
npm run dev
```

浏览器打开 `http://127.0.0.1:1420`。浏览器模式只读预览，不模拟安装、配置或登录成功。

在 macOS 或 Windows x64 上运行桌面应用：

```bash
npm run desktop
```

未配置服务地址时，登录和 TokenOne 启用入口会保持不可用。开发服务参数参考 [.env.example](.env.example)，需导出到运行环境；文件本身不会自动加载。生产构建将参数编译进 Rust 后端，前端无法修改为任意供应商。

```bash
npm run desktop:build
```

打包默认生成 macOS DMG 或 Windows NSIS 安装包，平台分别构建。应用包标识暂定为 `com.tokenone.launchone`，正式发布前确认品牌、签名和服务配置。CI 只构建、上传工作流制品，不创建 release、不部署服务。

macOS 通用测试包：在 Mac 上运行 `npm ci`、`npm run package:mac`，或手动运行 GitHub Actions 的 `macOS test build` 工作流。脚本生成支持 Apple Silicon / Intel 的 DMG、应用 ZIP、校验值和测试说明，位于 `output/macos/`。本地可用忽略的 `.env.local` 注入发布参数；CI 使用同名仓库 Variables。内测采用本地签名，未公证，安装和测试范围见 [macOS 测试说明](docs/mac-test.md)。

## 检查

```bash
npm run build
npm test
npm run test:core
npm run test:service
cargo check --manifest-path src-tauri/Cargo.toml
```

核心 Rust 测试不依赖桌面图形界面。配置集成测试必须使用 `Context::isolated` 和临时目录，忽略宿主机的 `CODEX_HOME`、`CLAUDE_CONFIG_DIR`。不得在真实用户目录中运行配置写入测试。

2026-11-07 在 Linux 开发环境已验证前端生产构建、7 项界面测试、30 项 Rust 测试、13 项服务端测试，以及 Tauri 桌面程序的编译和链接。Playwright 已检查首页、菜单与最小窗口布局；这些结果不代表 macOS / Windows 真机验收完成。

## 代码入口

| 位置 | 职责 |
| --- | --- |
| `src/App.tsx`、`src/components/AppCard.tsx` | 单页与通用应用卡片 |
| `src/lib/bridge.ts` | 桌面 IPC 与只读浏览器预览 |
| `src-tauri/src/adapters/` | 应用注册表，Codex 与 Claude Code 差异 |
| `src-tauri/src/service.rs` | 检测、安装、配置、验证的共用流程 |
| `src-tauri/src/config.rs` | 定点修改、私有备份、原子写入和冲突恢复 |
| `src-tauri/src/auth.rs` | 设备授权、系统凭据存储、个人 Key 获取 |
| `server/app.mjs` | 身份校验、Reseller 授权与取 Key 的最小服务端 |
| `src-tauri/src/settings.rs` | 发布方服务配置，限制可用目标 |

新增应用：实现 `AppAdapter`，在 `Registry` 注册，补充发布方端点与验证测试。首页和登录流程不需要知道应用 ID。适配器为内置代码，不支持用户导入脚本或第三方供应商。

## 源码基线与待完成项

采用 CC Switch 的 Tauri 技术栈，并直接复用其 JSONC 保留格式编辑器及原测试。为了减少无关功能，没有继承上游全量 UI、路由、供应商数据库和 IPC 命令。固定来源与许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

后续必须完成：

- macOS、Windows 真机验证安装、原生登录、实际 CLI 工具调用和已有配置兼容性。
- 完成 Windows 配置及备份文件的当前用户访问权限验证。
- 正式分发前配置 Apple Developer 签名与公证；内测可使用本地签名包。

已按授权配置独立 lab 应用、模型组和 HTTPS 接入服务，专用普通账号已通过真实授权取 Key 及模型请求，邮箱验证已恢复原设置。新版 Mac 包内置 lab 发布参数，缺少参数会在打包时失败。见 [lab 验收记录](docs/lab-acceptance.md)。

HTTP 网关测试成功只能说明基础接入可用，不能替代真实 CLI 的工具调用、多轮会话验收。

设计范围见 [方案](docs/tokenone-desktop-plan.md)，服务端衔接见 [接入说明](docs/integration.md)。
