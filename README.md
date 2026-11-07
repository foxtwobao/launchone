# LaunchOne

TokenOne 专用桌面应用安装和配置助手，面向 macOS、Windows。0.2.3 首页以 ChatGPT / Codex 和 Claude 桌面端为主，已有 CLI 适配器保留为代码中的补充能力。

已安装应用直接“打开”，不要求登录 IDONE。首次登录后默认准备已安装且支持接入的应用的 TokenOne 配置；卡片可直接切换“TokenOne / 官方订阅”，主动选择会持久保存。“恢复原配置”放在卡片的“高级”菜单，不与订阅切换混用。切换后提示确认重新打开；选择稍后会保留提醒。正常退出失败时不强制结束任务。切换前备份接入字段，失败回滚，保留无关设置。

**当前为桌面内测实现，尚未完成 Mac / Windows 目标应用的真实界面验收。** lab 已验证设备登录、刷新、Key 获取及两个 CLI 的模型和工具调用，不能据此宣称桌面端全部功能已通过。详见 [测试说明](docs/mac-test.md) 与 [lab 验收](docs/lab-acceptance.md)。

## 运行与检查

需要 Node.js 22.12+（满足锁定依赖 engines）、Rust 和对应平台 Tauri 构建依赖。

```bash
npm ci
npm run desktop
```

`npm run dev` 为只读浏览器预览，不伪造桌面安装或认证结果。开发参数见 [.env.example](.env.example)，需导出到运行环境。发布参数编译进 Rust 后端，不能由界面改为任意供应商；独立 Reseller 应用凭证只放服务端。

```bash
npm run build
npm test
npm run test:core
npm run test:service
cargo check --manifest-path src-tauri/Cargo.toml
```

配置测试必须使用 `Context::isolated` 和临时目录，不能访问宿主机 CLI 配置、`CODEX_HOME`、`CLAUDE_CONFIG_DIR` 或系统钥匙串。

Mac 在本机或私有 GitHub Actions 运行 `npm run package:mac`，读取忽略的 `.env.local` 或同名仓库 Variables；参数缺失直接拒绝打包。通用 DMG、应用 ZIP、校验值和说明在 `output/macos/`。Windows 使用官方安装入口，由系统完成安装授权，返回 LaunchOne 后重新检测；不能把下载页打开当作安装成功。

## 代码与扩展

| 位置 | 职责 |
| --- | --- |
| `src/components/AppCard.tsx` | 打开 / 安装、TokenOne / 官方订阅和恢复入口 |
| `src-tauri/src/desktop_app.rs` | 原生桌面识别、官方安装包与打开；不通过 CLI 猜测 |
| `src-tauri/src/adapters/` | 内置应用差异、桌面配置及接入能力 |
| `src-tauri/src/service.rs` | 默认准备、切换、验证与恢复编排 |
| `src-tauri/src/preferences.rs` | 用户明确选择；防止再次登录覆盖订阅或恢复决定 |
| `src-tauri/src/config.rs` | 保留格式的字段编辑、私有备份与多文件中断恢复 |
| `src-tauri/src/auth.rs`、`server/app.mjs` | IDONE 与本人 Key 衔接；Key 不进入网页或界面 IPC |

Codex 桌面端与 CLI 共用同一配置所有权和备份，避免互相覆盖。Claude Desktop 使用独立配置库，不把 Claude Code CLI 的 settings 当作桌面配置。新应用通过内置适配器和注册信息增加，不建设第三方插件或供应商管理。

沿用 CC Switch 技术栈及 JSONC 编辑器，来源与许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。方案见 [产品方案](docs/tokenone-desktop-plan.md)，服务端协议见 [接入说明](docs/integration.md)。正式分发前仍需 Apple 签名公证、Windows 文件权限及双平台真机验收。
