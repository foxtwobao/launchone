# LaunchOne lab 验收

日期：2026-11-07。只修改 LaunchOne 源码；以下配置操作仅作用于 lab，依据用户授权完成。未改动关联项目源码或生产环境。

## 已完成

- 独立 IDONE `admin/launchone` 客户端已登记，启用 Device login、Device Code 和 Refresh Token，与 TokenOne 使用相同组织和 subject 语义。桌面仅使用 public client id。
- 专用普通测试账号已通过真实设备登录、UserInfo 校验和不带 client secret 的刷新；也通过正常 IDONE SSO 开通了自己的原生 TokenOne 账号。
- 首次 `credentials` 返回待授权；Playwright 浏览器完成网关 IDONE 登录并自动返回 LaunchOne 完成页；本人会话查询成功得到 Key。再次 `credentials` 直接返回同一 Key。
- 独立 Reseller `launchone` 服务应用，仅 `keys:ensure`；精确回调 `https://launchone.lab.home.arpa/auth/reseller/callback`；独立凭证仅在服务端。
- 通过原生管理 API 建立 `launchone` 组合组，成员为 `GPT-企业` 与 `Claude满血`，禁用跨组重试；仅将新组加入可选组，保留原有组配置。
- 默认模型 `gpt-5.6-sol` 的 Responses 与 `claude-sonnet-4-6` 的 Messages 均通过本人 Key 返回 HTTP 200。再次按两个适配器完全相同的 Bearer、请求体及输出上限验证，也通过成功响应检查。
- 使用隔离配置运行本机 Codex CLI 0.167.0 和 Claude Code 2.1.286：两者均完成真实文本请求；Codex 命令工具与 Claude Read 工具分别读取测试文件并返回预期内容，工具结果提交后的模型回复成功。未写入宿主用户 CLI 配置。
- 接入服务通过 systemd 运行于 `127.0.0.1:55410`，Caddy 提供 `https://launchone.lab.home.arpa`，可信 HTTPS 健康检查通过。入口配置见 `deploy/`。
- lab 测试用户 335 初始额度为 0；通过原生 `POST /api/user/manage` 的 `add_quota` 增加 500000 配额，仅用于本次专用账号验收。没有调整全局计价或其他用户余额。

## 配置方式与留档

Reseller 普通测试会话的应用管理接口返回 403，未提升账号角色或伪造管理员会话。利用用户授权的 lab 主机维护权限，仅在 Reseller 数据库事务中创建应用、独立凭证摘要和新组保留关系，写入 `lab.launchone.provision` 审计；未修改用户身份、角色、财务数据或表结构。原生 TokenOne 配置和测试额度均通过受支持的管理 API 修改，没有直接写原生数据库。

注册期间按用户此前授权临时关闭的 IDONE Email 验证已恢复为原 `Normal`，本次接入没有再次关闭。没有将测试邮箱伪标为已验证。

账号、密码、Key、Cookie、配置前快照、回调响应和维护记录保存在忽略的 `private/`，目录 700、文件 600。服务环境文件同为 600。不得复制进源码、提交或公开日志。

## 验证边界

隔离自动化覆盖 7 项界面、30 项 Rust、13 项服务测试，前端构建与 Linux 桌面编译链接通过；配置写入仅在临时目录，不访问真实用户配置或系统钥匙串。

尚待 macOS / Windows 真机验收：系统钥匙串、官方安装与登录、配置恢复、真实 CLI 的流式输出、工具调用和多轮任务。真实 HTTP 成功不等于这些项目已通过。取消、过期、禁用和额度耗尽在隔离测试中覆盖，尚未对所有真实 lab 状态逐项执行。
