# 更新与 Codex 会话

## 应用更新

- 桌面端启动时检查一次，进程运行期间每 24 小时检查一次，页脚可手动检查。
- 安装前由 Tauri updater 验证签名；缺少更新公钥或服务端清单时，客户端不安装更新。
- LaunchOne 服务公开读取 `LAUNCHONE_UPDATE_MANIFEST_FILE` 指向的 Tauri 更新清单。模型请求仍直接发送到 TokenOne，不经过 LaunchOne 服务。
- 构建时将更新公钥放入 `LAUNCHONE_UPDATE_PUBLIC_KEY`。发布构建另需通过 CI secret 提供 `TAURI_SIGNING_PRIVATE_KEY` 和对应密码，并发布带签名的 macOS、Windows 更新包及 `latest.json`。私钥不得进入代码库、普通构建日志或桌面安装包。
- `latest.json` 使用 `version`、可选的 `notes` 和 `platforms` 字段；平台键为 `darwin-aarch64`、`darwin-x86_64`、`windows-x86_64-nsis`，每项包含公开 HTTPS `url` 与构建产出的 `.sig` 签名文本。服务端按当前平台映射成 Tauri 动态更新响应。
- 服务端设置绝对路径 `LAUNCHONE_UPDATE_MANIFEST_FILE` 指向 `latest.json` 后启用更新检查；未设置时返回无更新。没有编译进客户端的公钥时，手动检查会显示“更新服务尚未配置”。
- 构建环境、平台打包和 lab 发布步骤见 [构建与更新](build-and-updates.md)。

## 会话切换

LaunchOne 只改 Codex `config.toml`，不修改 `sessions`、`state_5.sqlite`、官方 `auth.json`、项目列表或 SSH 项目。较新的 Codex 桌面版能显示多个 Provider 的历史；会话续接需要 `config.toml` 仍注册该会话创建时的 Provider。LaunchOne 保留 `openai` 与 `tokenone` 两个 Provider 标识：切换只改默认 Provider，官方模式仍保留 TokenOne 连接表，使已有 TokenOne 会话可以按会话记录里的来源路由。恢复原配置时，现有备份事务会移除 LaunchOne 管理的字段。

CC Switch 的“统一会话历史”是 Codex CLI 的增强：把官方会话映射到共享 `custom` 标识，并可选择迁移旧会话标签；迁移前备份 JSONL 与 SQLite 索引，关闭时可恢复。桌面端不需要这项列表迁移。统一标签只影响列表，不会转换后端加密的推理内容；把历史强行交给不同后端续接仍可能因 `encrypted_content` 解密失败。LaunchOne 面向桌面端，因此保留会话来源和 Provider 配置，不迁移历史数据库。若上游桌面版无法显示旧会话，应先更新 Codex；若续接报错，应检查对应 Provider 配置是否还在。
