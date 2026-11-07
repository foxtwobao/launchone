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

0.1.1 隔离自动化覆盖 7 项界面、30 项 Rust、13 项服务测试，前端构建与 Linux 桌面编译链接通过；配置写入仅在临时目录，不访问真实用户配置或系统钥匙串。0.2.0 的新增验证见下文。

尚待 macOS / Windows 真机验收：系统钥匙串、官方安装与登录、配置恢复、流式输出和多轮任务；此前真实 CLI 文本及工具调用已验证，不能替代目标平台桌面验收。真实 HTTP 成功不等于这些项目已通过。取消、过期、禁用和额度耗尽在隔离测试中覆盖，尚未对所有真实 lab 状态逐项执行。

## 0.2.0 桌面适配的验证边界

新增桌面应用识别、安装 / 打开、卡片模式切换和持久选择。配置测试包含 Claude 多文件预检、失败回滚、中断恢复、配置库保留与订阅 / 原始恢复的区别；使用隔离目录，不访问真实钥匙串。当前本地通过 13 项界面、37 项 Rust、13 项服务测试及桌面编译检查。Playwright 使用明确的 UI fixture 检查 980×740、680×600 的布局及切换；此 fixture 不作为真实安装、登录或桌面模型调用证据。

Windows、macOS 的实际目标应用界面、模型请求及重启生效仍待真机验收。原有 lab CLI 调用结果不能替代此项。

## 0.2.1 确认重开与高级菜单

切换或恢复后提示确认重新打开；稍后选择和待重开状态会保留，失败不强制结束目标进程。“高级”入口直接显示恢复原配置及无备份说明。本地 18 项界面、37 项 Rust 测试和桌面编译检查通过；新增 Mac AppleScript 编译及 Windows 正常退出隔离测试由目标平台构建机执行，不操作真实目标应用。

## 0.2.2 运行检测与账号状态保留

点击打开时实时检查目标应用是否运行，确认后才正常退出并重开；Mac 测试机使用临时 AppKit 应用验证，Windows 使用隔离 PowerShell fixture 验证精确目标匹配。

此前 Codex provider 固定 `requires_openai_auth=false`，会将桌面端报告的账号置空。现在有原账号时保留身份，同时明确设置 TokenOne 的 `experimental_bearer_token`；新用户保持免官方登录。file 存储只读 `auth.json`，keyring/auto 通过官方应用附带引擎的只读 `login status` 查询，由官方处理原生凭据格式；拒绝访问、超时或无法识别时停止，不覆盖配置。未改写账号、项目数据库、SSH 配置或问卷状态。

检查了官方 macOS 26.1103.41825 安装包中的部分状态管理代码，发现按账号隔离的 renderer state；这是问卷和项目列表问题的机制线索，尚非实际桌面问卷根因的完整验证。对应 Codex rust-v0.167.0 源码中 `model-provider/src/provider.rs::account_state`、`model-provider/src/auth.rs::resolve_provider_auth` 分别确认账号报告和明确 provider token 的优先级。研究仅静态读取，未执行下载的官方应用。

本地 20 项界面、42 项 Rust 测试与桌面编译检查通过。配置往返测试验证本地与 SSH 项目配置、auth、全局桌面状态及数据库 fixture 字节未变；fixture 仅证明不改文件，不代表真实问卷界面已验收。另用隔离 Codex 0.167.0 对本地 HTTP SSE fixture 发出真实请求并完成回复，确认 Authorization 使用 provider fixture Key，原官方 fixture Key 文件未改；无真实用户凭据或外部模型请求。

仍需用户 Mac 验收：安装新包后重新选择 TokenOne、确认重开，检查原项目（含 SSH）、已完成问卷及模型调用。此前旧版产生的另一身份下新增项目不做自动数据库合并。

## 0.2.3 源码修改（暂未构建安装包）

按用户要求，“高级 → 恢复原配置”直接用首次备份覆盖 LaunchOne 管理的接入字段，不再因这些字段后来被客户端或用户修改而拒绝恢复。无关项目、SSH 和其他设置继续保留；多文件恢复仍使用已有事务处理，支持失败回滚和中断恢复。常规模式切换仍保留原有冲突处理。

恢复改动通过隔离 Rust 测试，包括修改过接入字段后直接恢复、保留无关设置、多文件恢复中断后重试。窗口底部新增 `LaunchOne v0.2.3` 显示，版本来自 package.json，与打包版本一致，无需单独配置。此次 Mac 测试包构建状态及校验结果另行记录。
