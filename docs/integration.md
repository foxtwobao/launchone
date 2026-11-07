# IDONE / TokenOne 接入说明

已只读核对 IDONE、CreativeOne 与 Reseller 源码，并查询 lab 的公开接口。LaunchOne 已实现设备登录、最小可信服务端和桌面取 Key；隔离测试已通过，已建立真实 lab 测试账号并完成 Reseller SSO 登录；独立客户端、应用登记及真实取 Key 尚待完成。

## 用户流程

1. 点击「登录 IDONE」：应用打开浏览器设备确认页，显示确认码，后台按 IDONE 指定间隔查询结果。
2. 点击工具「启用 TokenOne」：已有模型授权则直接取本人 Key；缺少授权时打开浏览器确认，返回应用自动继续。
3. 桌面 Rust 后端写入工具配置，模型请求由 CLI 直连 TokenOne。Key 不进入网页、URL 或界面 IPC。

首页仍只有账号和应用卡片。新增工具只需内置适配器，不改变登录或取 Key 接口。

## IDONE 设备登录

lab `https://idone.lab.home.arpa/.well-known/openid-configuration` 已声明：

- `device_authorization_endpoint`：`/api/device-auth`
- `token_endpoint`：`/api/login/oauth/access_token`
- Grant：`urn:ietf:params:oauth:grant-type:device_code`

LaunchOne 从固定 issuer 的 Discovery 读取端点，核对 issuer 和同源地址；只发送 public `client_id`，不内置 client secret，不监听本地回调端口。登录请求 scope 为 `openid profile email offline_access`。处理等待、减速、拒绝、过期和取消，Token 存入系统凭据存储，通过 UserInfo 校验 `iss/sub`。HTTP 使用系统可信根证书，不跳过 TLS 校验。

IDONE 的 `verification_uri` 已包含用户码；直接打开该地址。当前返回对象没有 `verification_uri_complete`，不能假定另有标准手工输码页面。

**平台支持不代表具体客户端已启用**。独立 LaunchOne 客户端必须启用 `Device login` 登录方式及 Device Code、Refresh Token 授权类型，并与 TokenOne 使用相同的 subject 语义。当前 Reseller 使用的 `tokenone` 客户端公开配置只有 Password 登录方式，不能直接复用为 LaunchOne 设备客户端。

## TokenOne 现有协议

CreativeOne 服务端已使用以下 Reseller 接口：

```http
POST /api/apps/keys/ensure
Authorization: Bearer <服务端应用接入凭证>
Content-Type: application/json

{"identity":{"issuer":"<已验证 issuer>","subject":"<已验证 sub>"}}
```

成功响应为 `status=ready`，个人模型 Key 在 `api_key.key`，ID 均保留字符串类型。ensure 检查已有授权和 Key，不负责首次创建。

只有 `403 APP_USER_AUTHORIZATION_REQUIRED` 才启动授权：

1. `POST /api/apps/authorizations`：提交已验证 identity、精确 HTTPS 回调、随机 state 和 S256 code_challenge。
2. 浏览器进入返回的网关地址，完成原生 IDONE SSO/必要交互。
3. 回调先验证并一次性认领 state，再用原应用凭证调用 `POST /api/apps/authorizations/complete`，提交 code、code_verifier。
4. complete 返回 authorized；再调用 ensure 取得 Key。兑换结果未知时先 ensure 检查，不重放一次性 code。

应用凭证允许服务端提交 identity，**不能放进桌面安装包**。用户的 IDONE access token 不能直接作为此接口的应用 Bearer。LaunchOne 使用独立 `launchone` 服务应用、`keys:ensure` 最小权限和自己的默认分组，不复用 CreativeOne 的凭证或分组归属。

## LaunchOne 已实现的衔接

`server/app.mjs` 是一个 Node.js/Express 进程，无用户数据库、管理后台或模型代理。通过固定 issuer 的 Discovery 和 UserInfo 验证用户原 access token，由响应确定身份，不接受调用方指定 issuer/subject。邮箱注册策略由 IDONE 负责，LaunchOne 不重复增加邮箱验证规则；模型权限仍由 Reseller/TokenOne 的实际授权决定。

桌面只向这个服务发送本人 IDONE Bearer：

| 接口 | 请求与结果 |
| --- | --- |
| `POST /v1/credentials` | 空 JSON `{}`。已有授权返回 `{"status":"ready","key":"..."}`；否则返回 `status=authorization_required`、事务 id、浏览器 authorization_url、expires_in |
| `GET /authorize/:id` | 浏览器进入入口，跳转固定 Reseller 网关；不交付 Key |
| `GET /auth/reseller/callback` | Reseller 精确 HTTPS 回调，认领 state、校验原登录仍有效并一次性兑换 |
| `GET /v1/authorizations/:id` | 同一桌面登录 Bearer 查询；返回 pending 或本人 Key；其他登录会话不可读取 |
| `GET /health` | 仅检查进程存活，不表示上游接入已通过 |

事务最多五分钟，只在内存中保存；服务重启后由用户重新发起。最多 256 个待处理事务，定时清理过期项，不保存已交付 Key。重复开始合并；回调只能消费一次；兑换响应丢失后走 ensure 恢复。浏览器 API 请求被拒绝，不开启 CORS。错误响应只返回允许的业务码，不透传上游正文和凭据。

权限、停用、额度、网络错误不自动转成授权。取消等待不等于撤销用户已经在平台完成的授权。退出 IDONE 只清除本机会话，不删除工具配置或上游 Key。

## lab 准备和运行

已确认三个已有 origin：IDONE `https://idone.lab.home.arpa`、Reseller `https://tokenonereseller.lab.home.arpa`、模型 `https://tokenone.lab.home.arpa`。LaunchOne 的拟用服务 origin 为 `https://launchone.lab.home.arpa`，不是已部署地址。

需登记：

- IDONE：独立 LaunchOne public client，启用上述设备登录和刷新，核验 subject 语义。
- Reseller：`launchone` 服务应用、`keys:ensure`、可用且未归属其他应用的默认组、精确回调 `https://launchone.lab.home.arpa/auth/reseller/callback`；生成专用应用凭证。
- LaunchOne：配置 DNS/HTTPS 入口；本仓库提供 `deploy/Caddyfile.lab` 片段，不自动改写其他仓库的代理配置。
- 测试账号：已沿 lab 原生注册/登录链路建立并成功登录 Reseller。按用户明确授权，注册期间临时将 `admin/tokenone` 的 Email 规则改为 `No verification`，在 finally 中恢复原 `Normal` 并重新查询确认；没有把测试邮箱伪标为已验证。注册本身不授予管理员权限，也不自动提供模型分组权益。

服务端准备（`.env` 只放服务机，权限设为 600）：

```bash
cp server/.env.example server/.env
chmod 600 server/.env
# 填入独立应用凭证；lab 证书须在系统可信根中。
NODE_EXTRA_CA_CERTS=/etc/ssl/certs/ca-certificates.crt npm run service
```

服务默认只监听 `127.0.0.1:55400`，通过 HTTPS 反向代理访问。代理、APM 不采集认证路径的 query/body/header；不启用认证 URL 的访问日志。服务重启会使进行中的授权失效，不影响 Reseller 已保存的授权。

桌面发布方配置见根目录 `.env.example`，只包含 issuer、public client id、接入服务地址、工具 Base URL 和模型，不包含服务端凭证。Codex Base URL 含 `/v1`，Claude Code 不含 `/v1`，避免重复拼接。

默认模型需按 LaunchOne 的实际分组确定，不能从 CreativeOne 的创作模型配置推断；需验证 Codex Responses 与 Claude Messages 的流式响应、工具调用及多轮任务。

## 验证状态

2026-11-07：已通过 13 项服务端测试、30 项 Rust 测试及前端构建/界面测试。身份和模型服务均使用隔离 fixture；不访问系统凭据存储，配置写入只使用临时目录。通过 fixture 不代表真实 lab 或 macOS/Windows 真机验收完成。

本次 lab 已确认公开 Discovery、SSO 入口、客户端登录方式，完成真实测试账号注册及 Reseller 登录，`/api/distributor/v1/me` 返回 200。测试账号角色为 distributor，`GET /api/admin/one-apps` 返回 403；当前正常会话无权登记应用。尚未登记 `launchone` 应用、生成应用凭证、部署入口或调用真实模型。继续真实取 Key 验收需要现有 lab 管理员完成应用登记与客户端配置；不通过直接改库伪造身份或提权。账号、Cookie 和恢复记录仅保存在本仓库忽略的 `private/`，权限为当前用户可读写，内容不进入文档、日志和提交。

完整 lab 验收记录见 [lab-acceptance.md](lab-acceptance.md)。

## 源码依据

- IDONE：`controllers/auth.go`、`controllers/token.go`、`object/token_oauth.go`、`object/wellknown_oidc_discovery.go`。
- CreativeOne：`server/src/tokenone.ts`、`server/src/reseller-auth.ts`、`server/src/creative-user-auth.ts`。
- Reseller：`internal/server/applications.go`、`internal/server/application_authorization.go`、`docs/APPLICATIONS.md`。
