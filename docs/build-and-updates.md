# 构建环境与 lab 更新

## 环境选择

只设 `lab` 和 `production`，构建时确定，应用内不增加切换入口。

| 用法 | 命令 | 配置来源 |
| --- | --- | --- |
| 日常开发 | `npm run desktop` | `config/lab.json` |
| macOS 测试包 | `npm run package:mac -- --environment lab` | `config/lab.json` |
| Windows 测试包 | `npm run package:windows -- --environment lab` | `config/lab.json` |
| 正式包 | 上述打包命令的 `lab` 改为 `production` | `.env.production` 或 CI 环境变量 |

打包必须明确环境、提供对应的 `TAURI_SIGNING_PRIVATE_KEY`；密码用 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。私钥只保存在受保护的本地 private 目录及 GitHub Environment secret，不进入安装包或仓库。`config/lab.json` 只有可公开的连接地址、客户端 ID、默认模型和更新公钥。Lab 不读取外部正式地址覆盖；正式包缺配置或地址是 `*.home.arpa` 时停止构建。Apple 正式签名、公证需另配置正式发布凭据。

Lab 的应用名称为 `LaunchOne Lab`，左上角显示版本与 `Lab`，应用身份、IDONE 钥匙串及更新签名和正式版分开。它们配置的目标 Codex/Claude 仍是用户同一套应用，因此接入备份、切换偏好和配置锁共用原目录，延续 0.2.3 的原始备份，避免重复备份把已修改配置当成原配置。旧版首次安装 Lab 包后需要重新登录。

GitHub Actions 的 **Desktop packages** 选择 `lab` / `production`，同时构建 macOS 通用包和 Windows x64。更新签名 secret 按同名 GitHub Environment 保存。产物在 `output/<环境>/<macos 或 windows>/`，也可从对应 Actions 运行下载。正式配置尚未提供，因此目前可实际发布的是 lab。

## Lab 部署和发布

更新和鉴权共用 `launchone-lab.service`，模型请求仍直连 TokenOne。服务配置：

```dotenv
LAUNCHONE_UPDATE_MANIFEST_FILE=/home/hulei/projects/launchone/output/updates/lab/latest.json
LAUNCHONE_UPDATE_ARTIFACT_DIRECTORY=/home/hulei/projects/launchone/output/updates/lab/files
```

`/v1/updates/<target>/<arch>/<currentVersion>` 无需登录，按平台返回更新清单；`/updates/files/<version>/<filename>` 提供公开下载及断点续传。只公开专门的产物目录。缺少清单或平台时返回 204；已安装版本是否更新由 Tauri 比较，禁止自动降级。

下载 Actions 产物到 `output/lab/macos/` 和 `output/lab/windows/` 后：

```sh
npm run updates:publish:lab
```

发布脚本验证安装包的 Minisign 签名、签名中的版本号、篡改包拒绝及 SHA256，再复制到带版本号的目录，最后原子替换 `latest.json`。已发布的同名文件不可替换为不同内容；修复需增加版本号。支持先发布单个平台，另一平台未发布时不宣称有可用更新。普通版本发布无需重启服务。回退更新清单不会自动给客户端降级。

客户端启动检查一次，随后每 24 小时检查；也可点击底部手动检查。检查只提示，用户点击更新才安装，安装前校验签名及签名版本。Lab 电脑需要解析内网域名并在系统中信任 lab 根证书。旧的 0.2.3 不含更新入口，需要手动安装新 Lab 包一次。Linux 上可以验证 HTTPS 下载和真实签名；macOS/Windows 的安装替换及重启需在对应系统验收。

## 实际 updater 验证

发布后可在 lab 主机运行以下命令；它使用应用同版本的 Tauri updater，验证 HTTPS、三个平台的下载和签名、签名版本一致性、篡改拒绝，以及同版本和较新版本不会降级；不会安装软件或操作用户配置：

```sh
cargo run --manifest-path src-tauri/Cargo.toml --features updater-smoke --example probe_updates -- config/lab.json output/updates/lab/probe
```
