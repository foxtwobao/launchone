# LaunchOne lab 验收

日期：2026-11-07。只修改 LaunchOne 源码；lab 账号和邮箱设置操作按用户明确授权，通过正常 HTTPS API 完成。没有改动关联项目源码、数据库、服务构建或线上环境。

## 已完成

- IDONE 公开 Discovery 返回设备授权端点和 device_code grant。
- Reseller 的现有登录入口正确进入 lab IDONE。
- 临时关闭 `admin/tokenone` 注册 Email 验证，注册专用 LaunchOne lab 测试账号；在 finally 中恢复原 `Normal`，重新读取确认恢复成功。
- 使用测试账号完成真实 IDONE 授权码/PKCE 登录，Reseller 回调返回 303，`/api/distributor/v1/me` 返回 200。
- 测试账号保持普通 distributor 角色，没有给自己提权，没有伪标测试邮箱已验证，没有授予模型分组权益。
- 应用管理权限检查：`GET /api/admin/one-apps` 返回 403，没有通过其他入口绕过。
- 7 项界面测试、30 项 Rust 测试、13 项服务端测试通过；前端构建、Rust 桌面编译及链接通过，生产依赖 audit 未发现已知漏洞。

账号密码、本人 Cookie、注册响应和邮箱规则恢复记录位于本仓库忽略的 `private/`，文件权限为 600。不得复制进源码、日志或文档。

## 当前阻塞

现有可用的 Reseller 会话为普通用户，不能登记服务应用或生成应用凭证。IDONE 现有应用接入认证也不能读取管理签名证书；不尝试绕过权限或直接修改数据库。

需要现有 lab 管理员完成：

| 平台 | 最小登记项 |
| --- | --- |
| IDONE | 独立 LaunchOne public client；Device login、Device Code、Refresh Token；与 TokenOne 同一组织和 subject 语义，使用管理员确认的签名配置 |
| Reseller | `launchone`，仅 `keys:ensure`；选择可用且未归属其他应用的模型默认组；回调 `https://launchone.lab.home.arpa/auth/reseller/callback`；生成独立服务端凭证 |
| lab 入口 | 配置 LaunchOne 的 DNS、可信 HTTPS 和到 `127.0.0.1:55400` 的代理；候选片段在 `deploy/Caddyfile.lab` |

不需要把普通测试账号提升为管理员才能使用产品；也可以由现有管理员直接完成登记。应用凭证只放 `server/.env`，不放进桌面包。实际 CLI 默认模型需从所选分组确认。

## 待真实验收

- LaunchOne 设备登录及刷新，而不是只验证 Reseller 的已有授权码登录。
- 首次模型授权 → 回调兑换 → ensure 取 Key → 再次使用免授权。
- 实际 Key 的分组、额度、禁用、取消和过期处理。
- macOS / Windows 安装、配置恢复及 Codex/Claude Code 的流式、工具调用、多轮任务。

隔离 fixture 已覆盖软件流程，但不替代上述真实环境和双平台验收。
