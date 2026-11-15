# 官方安装包镜像部署与运行

镜像代码已接入服务端、同步任务和客户端。部署域名、存储目录及 GitHub 校验凭据后才会产生可下载的清单；仓库中的配置模板不表示服务已上线。macOS / Windows 的原生校验和安装仍需在对应系统验收。

## 当前范围

- macOS：Codex / ChatGPT 与 Claude 的官方完整 DMG，按 Intel / Apple Silicon 请求清单；Universal 包需包含请求的架构。
- Claude Windows：已提供官方 MSIX 下载适配器、原生校验及安装路径。发布者精确固定及完整安装依赖尚未验收，适配器暂不发布此组合。
- Codex Windows：没有经过确认的完整离线安装渠道，返回不支持。
- LaunchOne 首期发行目标仍为 macOS Universal 和 Windows x64；协议中的 Windows ARM64 能力不代表已经发行或验收 ARM64 客户端。

镜像校验来源、官方签名、应用身份、包内版本、架构、系统要求和完整性；不维护每个上游版本的 TokenOne 兼容白名单，不逐版人工放行。TokenOne 配置时复用连接验证，用户遇到连接问题可重新连接。

## 一次性部署

1. 将代码和 Node 依赖安装到服务机器；同步任务要求 Node.js 22.12+、`flock` 和 `unzip`。提供足够的磁盘空间保存各架构当前版、上一版、7 天保留期旧包和候选包。
2. 建立安装包根目录，例如 `/var/lib/launchone/installers`。同步任务用户可写，HTTP 服务用户只读。使用同一 Unix 用户/组或配置读取权限；现有 HTTP 服务的只读文件系统保护可以保留。
3. 在 HTTP 服务环境中填写以下两项；两者必须同时设置，或同时留空禁用镜像：

   ```dotenv
   LAUNCHONE_DOWNLOAD_BASE_URL=https://downloads.example.com
   LAUNCHONE_INSTALLER_DIRECTORY=/var/lib/launchone/installers
   ```

4. 参照 `deploy/Caddyfile.downloads.example`，分别配置服务域名和下载域名。下载入口只代理 `/downloads/files/*`、`/updates/files/*`，保留原路径；服务入口提供 API。替换域名、上游监听地址，签发 HTTPS 证书，验证大文件及 Range 请求。下载 Caddy 可以单独部署，但必须能访问同一 HTTP 服务。
5. 将 `.github/workflows/verify-installers.yml` 和校验脚本提交到目标 GitHub 仓库的受保护分支。同步任务仅接受该分支的预期提交、workflow 和具体运行记录；分支未保护会拒绝验证。macOS / Windows runner 只检查候选包，不运行其中的应用或安装器，不使用 LaunchOne 发布签名密钥。
6. 将 `deploy/installers.env.example` 复制到 `/etc/launchone/installers.env`，限制为运维用户读取。填写实际仓库、受保护分支和该仓库专用 GitHub 凭据。凭据需要 Actions 读写（触发、查询运行及读取报告）及 Contents 读取（查询分支）；只放同步任务环境中。Lab / production 分别使用自己的目录、环境文件及域名。
7. 按实际部署路径和运行用户调整 `deploy/launchone-installers.service`，再安装 service 和 timer。模板默认 `/opt/launchone`、`launchone` 用户及 `/usr/bin/node`，不是当前开发目录。安装后执行：

   ```bash
   sudo systemctl daemon-reload
   sudo systemctl start launchone-installers.service
   sudo systemctl enable --now launchone-installers.timer
   ```

首次同步立即运行，之后每 6 小时执行；关机错过的计划在开机后补跑。手动运行可以使用 `node --env-file=/etc/launchone/installers.env scripts/sync-installers.mjs`，或在已加载环境变量的 shell 执行 `npm run installers:sync`。手动运行与定时任务使用同一文件锁。

客户端只编译服务域名，无需编译下载域名。下载域名迁移时修改服务端配置；未完成任务会在旧地址失效后重新获取清单，只有同一内容哈希才继续已有部分文件。

## 发布与故障处理

服务从 `manifests/<app>/<platform>/<arch>.json` 读取当前清单，从 `files/` 提供不可变安装包。`jobs/` 和 `pending/` 保存可恢复的校验任务，文件旁的验证报告可用于审计；这些目录不作为文件下载入口暴露。

通过 `journalctl -u launchone-installers.service` 查看每个应用、平台和架构的同步结果。下载失败、原生验证失败、GitHub 排队超时或身份不匹配时继续提供已有版本；首次尚未成功则返回 `503 INSTALLER_UNAVAILABLE`。未知组合及 Codex Windows 返回 `404 INSTALLER_UNSUPPORTED`，客户端不跳转海外下载站。

原始包先完整落盘并校验，再切换清单。当前版和上一版保留，退出清单的其他版本至少保留 7 天；清理与同步共用锁。下载文件使用 SHA256 强 ETag，支持 `HEAD`、`Range` 和 `If-Range`。清单缓存 60 秒，版本文件长缓存。

官方入口路径变化时更新内置适配器，或通过同步任务环境中的受限来源 URL 配置更新路径；允许的主机仍由代码中的适配器约束。官方发布者身份变化需更新固定身份并重新验证。用户无需手动下载、上传或切版本。

## 最小上线验收

- 未登录 IDONE 的客户端可获取清单、下载安装包；错误架构或过低系统版本在下载前被拒绝。
- 从下载域名下载、暂停和续传；中断重启、发布新版和返回 `200/206/416/404` 时不混合内容；篡改包在安装前被拒绝。
- GitHub 校验报告绑定同一候选 SHA256 与任务；错误 run、ref、提交、attempt、环境或过期报告都不发布。
- 在不能访问官方下载域名的目标系统上完成安装、重新检测、打开；记录必要的系统授权和依赖。Windows 完整安装条件验证通过之前继续保持该组合不可用。
