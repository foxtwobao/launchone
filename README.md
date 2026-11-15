# LaunchOne

TokenOne 专用桌面应用安装和配置助手，面向 macOS、Windows。0.2.4 首页以 ChatGPT / Codex 和 Claude 桌面端为主，已有 CLI 适配器保留为代码中的补充能力。

已安装应用直接“打开”，不要求登录 IDONE。首次登录后默认准备已安装且支持接入的应用的 TokenOne 配置；卡片可直接切换“TokenOne / 官方订阅”，主动选择会持久保存。“恢复原配置”放在卡片的“高级”菜单，不与订阅切换混用。切换后提示确认重新打开；选择稍后会保留提醒。正常退出失败时不强制结束任务。切换前备份接入字段，失败回滚，保留无关设置。

**当前为桌面内测实现，尚未完成 Mac / Windows 目标应用的真实界面验收。** lab 已验证设备登录、刷新、Key 获取及两个 CLI 的模型和工具调用，不能据此宣称桌面端全部功能已通过。详见 [测试说明](docs/mac-test.md) 与 [lab 验收](docs/lab-acceptance.md)。

## 运行与检查

需要 Node.js 22.12+（满足锁定依赖 engines）、Rust 和对应平台 Tauri 构建依赖。

```bash
npm ci
npm run desktop
```

`npm run dev` 为只读浏览器预览，不伪造桌面安装或认证结果。客户端只固定 `LAUNCHONE_SERVICE_URL` 和 `LAUNCHONE_UPDATE_PUBLIC_KEY`，以及应用身份和版本；开发读取 [config/lab.json](config/lab.json)，正式构建格式见 [.env.example](.env.example)。登录和模型设置由服务端环境变量提供，修改后更新服务即可，不用重新打包。TokenOne 只填一个模型服务地址，Codex／Claude 的接口路径由程序自动处理。具体填写位置和生效时间见[打包操作说明](docs/build-and-updates.md)；独立 Reseller 应用凭证只放服务端。

```bash
npm run build
npm test
npm run test:core
npm run test:service
npm run test:updates
cargo check --manifest-path src-tauri/Cargo.toml
```

配置测试必须使用 `Context::isolated` 和临时目录，不能访问宿主机 CLI 配置、`CODEX_HOME`、`CLAUDE_CONFIG_DIR` 或系统钥匙串。

打包时，在 GitHub **Actions → Desktop packages → Run workflow** 选择测试版或正式版，以及 macOS、Windows 或两端。macOS 同一个安装包支持 Intel 和 M 系列，Windows 目前为 x64。第一次需要填写的配置、每项从哪里拿、填到哪里，以及后续更新发布步骤，见 [打包操作说明](docs/build-and-updates.md)。苹果签名材料沿用 Botone；Windows 暂不做发布者签名，无需 Azure 配置，应用内更新签名仍保留。

服务端镜像在推送或合并到 `main` 后自动发布，也可通过 **Actions → Publish Docker image → Run workflow** 手动运行，镜像名为 `docker.io/<DOCKERHUB_USERNAME>/launchone`。直接使用仓库已有的 `DOCKERHUB_USERNAME` 和 `DOCKERHUB_TOKEN` Secrets，支持 Linux x64／ARM64。登录和模型等配置在运行容器时传入，发布镜像不会自动替换现有服务；分支标签规则和运行命令见 [Docker 镜像发布说明](docs/build-and-updates.md#发布服务端-docker-镜像)。

未安装应用通过匿名镜像清单自动选择系统和原生芯片，下载支持中断续传，校验完整性和官方签名后安装；镜像尚未部署或对应组合尚未验证时提示不可用。Windows 完整包仍需发布者和依赖验收，不能把下载或安装器启动当作安装成功。

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

官方客户端镜像方案见 [官方安装包自动镜像](docs/official-download-mirror-plan.md)，实际部署步骤和当前平台边界见 [镜像部署与运行](docs/installer-mirror-operations.md)。

连接失效时可从应用卡片的“高级”菜单重新连接 TokenOne；切换 IDONE 账号后会提示更新仍属于之前账号的连接。开户、权限及额度错误提供前往 TokenOne 和重试入口；账户页面由服务端可选环境变量 `LAUNCHONE_TOKENONE_PORTAL_URL` 指定。客户端启动获取最新有效配置，失败时回退缓存；默认模型的变更用于下次连接或重新连接，不强改已接入应用的配置。
