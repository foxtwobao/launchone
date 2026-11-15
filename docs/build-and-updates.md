# LaunchOne 打包操作说明

给用户发正式安装包，按下面第 1～5 步配置一次，以后直接点按钮打包。只做内网测试，可以跳到[内部测试包](#内部测试包lab)。

配置分两个地方：**GitHub 保存打包和签名材料，服务端环境变量保存登录及模型设置**。以后换模型、换 TokenOne 地址，只改服务端环境变量并重新创建服务容器，不用重新打包。这些由维护者填写，使用安装包的用户不用配置。

## 1. 到 GitHub 建一个保存配置的地方

打开 **LaunchOne 仓库 → Settings → Environments → New environment**，名称填 **`desktop-release`**。已经有这个名称就直接打开。

打包配置填在这个页面：

| 页面上的位置 | 用来放什么 | 怎么填 |
| --- | --- | --- |
| **Environment secrets → Add secret** | 密钥、密码、签名材料 | Name 填下表中的配置名，Secret 填对应内容 |
| **Environment variables → Add variable** | LaunchOne 服务入口和更新公钥 | Name 填下表中的配置名，Value 填对应内容 |

下面写 **Secret** 就填第一处，写 **Variable** 就填第二处。配置名原样复制，大小写和下划线都要保留。[GitHub 官方操作说明](https://docs.github.com/en/actions/how-tos/write-workflows/choose-what-workflows-do/use-secrets#creating-secrets-for-an-environment)。

## 2. GitHub 填固定入口，服务器填登录和模型

### GitHub：只把这两项放进安装包

| 配置名 | 填什么 | 从哪里拿 |
| --- | --- | --- |
| `LAUNCHONE_SERVICE_URL` | LaunchOne 服务地址，例如 `https://launchone.example.com` | 你部署本仓库 `server/` 服务时使用的 HTTPS 域名，负责下发配置、授权取 Key、安装包清单和应用更新 |
| `LAUNCHONE_UPDATE_PUBLIC_KEY` | 验证 LaunchOne 更新的公钥 | 按第 3 步生成，从 `.pub` 文件复制完整内容 |

这两项都填为 **Variable**。表中 `example.com` 换成自己的正式域名。客户端还固定应用名称、身份和版本；下面的登录地址、客户端 ID、模型等由服务器提供。

### 服务器：登录和模型都用环境变量

用 Docker 部署时，把下表加入现有 LaunchOne 服务容器的环境变量，除标注“可选”的以外都要填。已有 Compose 配置就继续用它，不需要另外建配置服务或配置文件。

| 服务端环境变量名 | 填什么 | 从哪里拿 |
| --- | --- | --- |
| `LAUNCHONE_CLIENT_ID` | 给 LaunchOne 登记的客户端 ID | IDONE 后台 → 应用 → LaunchOne → OIDC/OAuth，复制 **Client ID** |
| `LAUNCHONE_TOKENONE_URL` | TokenOne 模型调用域名，例如 `https://tokenone.example.com` | 你的 TokenOne 服务域名。Codex 和 Claude 共用，只填 HTTPS 根地址，接口路径由程序自动补齐 |
| `LAUNCHONE_CODEX_MODEL` | Codex 默认模型 ID | TokenOne 后台模型配置中的**请求模型名称**，且 LaunchOne 对应分组有权限调用 |
| `LAUNCHONE_CLAUDE_MODEL` | Claude 默认模型 ID | 同样从 TokenOne 后台复制可用的请求模型名称 |
| `LAUNCHONE_TOKENONE_PORTAL_URL` | 可选：账号、额度和充值网页地址 | TokenOne 用户网站地址。留空就不显示“前往 TokenOne”按钮 |

IDONE 登录地址继续使用服务已有的 **`IDONE_ISSUER`**。它的值来自正式 IDONE 域名，与该站点 `/.well-known/openid-configuration` 返回的 `issuer` 完全一致，包括末尾有没有 `/`。服务端配置格式见 [`server/.env.example`](../server/.env.example)，Node／systemd 部署时直接填 `server/.env`，沿用[现有服务部署方式](integration.md#lab-准备和运行)。

用 Docker Compose 的，更新服务环境变量后运行：

```sh
docker compose up -d
```

这会按新配置重新创建需要更新的容器；只执行 `docker restart` 不会读入新的环境变量。直接用 Node／systemd 运行的，修改 `server/.env` 后重启 LaunchOne 服务。

在 IDONE 的 LaunchOne 应用中，登录方式启用 **Device login**，授权类型启用 **Device Code** 和 **Refresh Token**。使用不需要 Client Secret 的公开客户端。这里拿的是 LaunchOne 的 Client ID，不直接套用 Botone 的网页登录客户端。

服务通过公开的 `GET /v1/client-config` 提供登录地址、公开 Client ID、模型和账号网页这些普通设置。模型名填请求模型名称，不填界面昵称或渠道的上游模型名。签名私钥、密码和服务端的 `RESELLER_APP_CREDENTIAL` 都不会通过这个接口返回。

### 改完以后什么时候生效

| 改了什么 | 需要做什么 |
| --- | --- |
| 服务端的模型、TokenOne 地址或账号网页环境变量 | 更新容器环境并执行 `docker compose up -d`；Node／systemd 则重启服务。用户下次启动 LaunchOne 时取到新配置，不用重新打包 |
| 默认模型 | 用户重开 LaunchOne 取得新配置后，下次连接或“重新连接 TokenOne”使用新值；不会直接改掉已连接应用里的模型 |
| 服务端的 `LAUNCHONE_CLIENT_ID` 或 `IDONE_ISSUER` | 同上，更新服务后生效，不用重打包；用户取到新配置后需要重新登录 |
| GitHub 中的服务入口或更新公钥 | 它们在安装包里，需要重新打包 |

已打开的 LaunchOne 窗口不会即时收到配置推送。启动时取不到云端配置，会使用该服务和测试／正式环境各自最近一次成功的缓存；首次使用没有缓存时，仍可打开已安装应用、安装应用、使用官方模式及检查更新，登录和连接 TokenOne 暂不可用。

## 3. 生成一次 LaunchOne 的更新密钥

这对密钥用来让已安装的 LaunchOne 确认“新版本确实是我们发布的”。**macOS 和 Windows 共用一对，后续版本继续使用，不能每次打包重新生成。** 已有正式版密钥就沿用，跳过生成命令。

首次准备时，在本机安装 Node.js 24，然后在 LaunchOne 项目目录运行：

```sh
npm ci
npm run tauri -- signer generate -w ../launchone-production.key
```

按提示设置并保存密码。命令会在项目目录的上一层生成两个文件，分别打开后，把完整内容填到 GitHub：

| 配置名 | 填到哪里 | 内容从哪里来 |
| --- | --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | **Secret** | `launchone-production.key` 文件的完整内容 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | **Secret** | 刚才设置的密码；没有设置密码则不添加这一项 |
| `LAUNCHONE_UPDATE_PUBLIC_KEY` | **Variable** | `launchone-production.key.pub` 文件的完整内容 |

填文件里的文字，不填文件路径，也不用再编码。私钥和密码只在 GitHub 或发布机器上使用，不进入客户端；安装包只保存公钥。把私钥文件和密码保存到自己的密码库或安全备份中，以后给其他电脑配置打包也要用它。

`TAURI` 是打包工具沿用的配置名前缀，照表复制即可。这对更新密钥由 LaunchOne 单独生成，下面的苹果签名材料继续复用 Botone。

## 4. 填所需平台的签名配置

macOS 按 4A 配置苹果签名；Windows 目前不需要发布者签名材料，见 4B。苹果配置名沿用 Botone **当前启用**的 [macOS 发布流程](https://github.com/foxtwobao/botone/blob/main/.github/workflows/botone-release-macos.yml)，旧 `release-desktop.yml` 已停用。

**GitHub 不能显示已保存的 Secret 内容。** 复用时，要从你当初给 Botone 配置时保存的文件或密码库中取值，再填到 LaunchOne。如果已有组织级 Secret，也可以直接给 LaunchOne 仓库开放使用权限。

### 4A. macOS：沿用 Botone 的 5 项 Secret

这五项可以与 Botone 完全共用，无需为 LaunchOne 新建苹果开发者账号、签名证书或公证 API Key。当前通过安装包分发，也无需在 App Store Connect 新建一条 App 记录；LaunchOne 使用自己的应用标识，构建配置已处理。参见 [Apple 官网分发与公证说明](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow)。

下表全部填为 **Secret**：

| 配置名 | 填什么 | 从哪里拿 |
| --- | --- | --- |
| `MACOS_DEVELOPER_ID_P12_BASE64` | 苹果应用签名证书的文本形式 | Botone 同名值的原始备份；只有 `.p12` 文件时，按下文转成文本 |
| `MACOS_DEVELOPER_ID_P12_PASSWORD` | `.p12` 证书密码 | 当初导出证书时设置的密码，沿用 Botone 的值 |
| `APP_STORE_CONNECT_KEY_ID` | 苹果公证用的 Key ID | Botone 原有值，或 App Store Connect → Users and Access → Integrations → App Store Connect API 中对应密钥的 **Key ID** |
| `APP_STORE_CONNECT_ISSUER_ID` | 同一页面的 Issuer ID | Botone 原有值，或上述 Apple 页面里的 **Issuer ID** |
| `APP_STORE_CONNECT_API_KEY_BASE64` | 苹果 `.p8` 密钥文件的文本形式 | Botone 同名值的原始备份；只有 `.p8` 文件时，按下文转成文本 |

如果只保存了证书和密钥原文件：在仓库外找一个文件夹，放入 `.p12` 和 `.p8` 文件，分别命名为 `DeveloperID.p12` 和 `AuthKey.p8`。在那个文件夹打开终端，运行：

```sh
node -e "const fs = require('node:fs'); fs.writeFileSync('DeveloperID.p12.base64.txt', fs.readFileSync('DeveloperID.p12').toString('base64'), {mode: 0o600})"
node -e "const fs = require('node:fs'); fs.writeFileSync('AuthKey.p8.base64.txt', fs.readFileSync('AuthKey.p8').toString('base64'), {mode: 0o600})"
```

打开生成的两个 `.txt`，分别复制到上表对应的 `*_BASE64` Secret 中。这就是“Base64”：把文件变成能粘贴的文字，不是另一份证书或密码。

如果 `.p12` 备份找不到，但签名用的 Mac 还在，可在“钥匙串访问 → 我的证书”中导出 **Developer ID Application** 签名身份为 `.p12`，导出时设置密码。苹果 `.p8` 只能下载一次，原文件丢失需要在 Apple 后台新建 Key，并同时换掉对应的 Key ID。参见 [Apple 证书说明](https://developer.apple.com/help/account/certificates/create-developer-id-certificates/)和 [Apple API Key 说明](https://developer.apple.com/help/app-store-connect/get-started/app-store-connect-api)。

### 4B. Windows：暂不做发布者签名，无需 Azure 配置

Windows 测试包和正式包都可以直接构建，不需要 `DESKTOP_WIN_*` 配置、Azure 账号或签名工具。已有的这些配置不会再被构建使用，可以删除。Windows 安装时可能显示“未知发布者”或 SmartScreen 提示。

第 3 步的 **LaunchOne 更新密钥仍需保留**：它用来验证应用内更新，与 Windows 安装包的发布者签名是两回事。正式包继续使用 `desktop-release` 中的固定入口及更新密钥，测试包使用 `lab`。

## 5. 点按钮打包，下载结果

工作流文件 `.github/workflows/desktop-packages.yml` 进入 GitHub 默认分支后：

1. 打开 LaunchOne 仓库 → **Actions → Desktop packages → Run workflow**。
2. 选择要打包的代码分支。
3. `environment` 选 **`production`**。它会自动使用前面 `desktop-release` 中保存的配置。
4. `platform` 选 **`all`**（两端）、**`macos`** 或 **`windows`**，点击 **Run workflow**。
5. 等待成功，打开这次运行，在页面底部 **Artifacts** 下载 `LaunchOne-production-平台-运行序号`。

下载后先解压 GitHub 提供的外层 ZIP：

| 平台 | 给用户安装的文件 | 其他文件的用途 |
| --- | --- | --- |
| macOS | `.dmg`，同一个包支持 Intel 和 M 系列 Mac | 应用 ZIP 可备用；`.app.tar.gz` 和 `.sig` 留作应用内更新 |
| Windows | `.exe` 安装包，目前为 x64 | 对应 `.sig` 留作应用内更新 |
| 两端 | — | `SHA256SUMS.txt` 是下载完整性校验清单 |

打包结果在 Actions 中保留 **14 天**，需要长期保存就及时下载。这个按钮负责生成安装包；要让已安装用户看到新版本，还要完成下面的[发布应用内更新](#发布应用内更新目前为-lab)。

## 内部测试包（lab）

内部测试的服务入口和更新公钥已放在 `config/lab.json` 中，**不用重新填第 2 步的 GitHub Variables，也不用填苹果或 Windows 签名材料**。登录和模型配置由 Lab 服务的环境变量提供。

1. 打开 LaunchOne 仓库 → **Settings → Environments → lab**；没有就新建 `lab`。
2. 在 **Environment secrets** 添加 `TAURI_SIGNING_PRIVATE_KEY`。本维护机已有文件 `private/updates-lab/signing.key`，填它的完整内容。
3. 如果这把私钥设置过密码，再添加 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，填当时保存的密码。
4. 到 **Actions → Desktop packages → Run workflow**，`environment` 选 **`lab`**，`platform` 选所需平台。
5. 成功后，在运行页面的 **Artifacts** 下载 `LaunchOne-lab-平台-运行序号`。

继续用现有 Lab 密钥，它与 `config/lab.json` 中的公钥配套。不要拿正式版密钥替换它。维护 Lab 服务时，按第 2 步在服务端配置 Lab 对应的 Client ID、TokenOne 地址和默认模型；现有 Node／systemd 部署修改 `server/.env` 后重启服务即可。

测试包名为 **LaunchOne Lab**，测试电脑需要能访问 lab 内网，并信任 lab 根证书。它没有正式包的苹果公证或 Windows 发布者签名。测试版与正式版可以同时安装，登录单独保存；但它们操作的是电脑上同一套 Codex／Claude，接入配置和恢复备份会共用。旧 0.2.3 用户需要手动安装新 Lab 包一次，之后才有应用内更新入口。

## 本机开发或打包时再看

用 GitHub 打包可跳过本节。Mac 安装包在 Mac 上构建，Windows 安装包在 Windows 上构建，平台开发工具需要先装好。

| 想做什么 | 命令 | 固定服务入口和更新公钥从哪里读 |
| --- | --- | --- |
| 日常开发 | `npm run desktop` | `config/lab.json` |
| Mac 内部测试包 | `npm run package:mac -- --environment lab` | `config/lab.json` |
| Windows 内部测试包 | `npm run package:windows -- --environment lab` | `config/lab.json` |
| 正式包 | 将上面打包命令的 `lab` 换成 `production` | 根目录 `.env.production`，或终端环境变量 |

本机正式配置可以复制 `.env.example` 为 `.env.production`，只填第 2 步的服务入口和第 3 步的更新公钥。登录和模型仍由云端提供。更新私钥、密码和苹果签名配置通过同名环境变量提供，不写进该文件或仓库；Windows 无需额外的签名配置。

内部测试的固定入口读取 `config/lab.json`。正式包缺服务入口或更新公钥、使用 `localhost` 或 `*.home.arpa` 服务地址时会停止打包。模型和登录设置不再作为打包参数。

GitHub 工作流兼容 Botone 以前保存的 macOS 配置名：证书和密码也可叫 `DESKTOP_MAC_CSC_LINK`、`DESKTOP_MAC_CSC_KEY_PASSWORD`；Apple Key ID 和 Issuer 也可叫 `APPLE_API_KEY_ID`、`APPLE_API_ISSUER`。新配置和本机打包按第 4A 步的名称填写即可，不需要额外配置 `APPLE_CERTIFICATE` 或 `APPLE_API_KEY_PATH`。

## 发布服务端 Docker 镜像

这个工作流发布的是 LaunchOne 的 `server/` 服务，桌面安装包仍使用 **Desktop packages**。

GitHub **Settings → Secrets and variables → Actions → Secrets** 已有的两个仓库 Secret 会直接使用，不用再添加变量或 Environment：

| 已有 Secret | 用途 |
| --- | --- |
| `DOCKERHUB_USERNAME` | Docker Hub 用户名，同时决定镜像放在哪个账号下 |
| `DOCKERHUB_TOKEN` | 向该账号推送镜像的访问令牌 |

镜像名为 **`docker.io/<DOCKERHUB_USERNAME>/launchone`**，同时提供 `linux/amd64` 和 `linux/arm64`，Docker 拉取时自动选择当前服务器的架构。

发布工作流文件为 `.github/workflows/publish-dockerhub.yml`，在 Actions 中叫 **Publish Docker image**。每次推送或合并到 `main` 都会自动测试、构建并发布镜像；其他分支和版本标签不会自动发布。桌面安装包仍然手动打包。

需要重新发布或测试其他分支时，也可打开 **Actions → Publish Docker image → Run workflow**，选择代码分支后点击运行，无需额外输入：

| 选择的分支 | 生成什么标签 |
| --- | --- |
| `main` | `latest`，以及 `sha-完整40位commit` |
| 其他分支 | 只发布 `sha-完整40位commit`，不覆盖 `latest` |

如果构建期间 `main` 又有了新提交，本次只保留提交号标签，不覆盖 `latest`；最新提交会由后续自动构建发布。

先将工作流代码提交到 GitHub，等待工作流运行成功，镜像才会出现在 Docker Hub。**发布镜像不会自动重启或替换 Lab／生产环境中的容器。**

### 运行镜像

登录、模型和服务凭证仍通过**容器运行时的环境变量**提供，按第 2 步准备，完整格式见 [`server/.env.example`](../server/.env.example)。它们不是镜像构建参数，也不打进镜像。

已有可直接使用的 [`deploy/compose.yml`](../deploy/compose.yml) 和 [`deploy/.env.example`](../deploy/.env.example)。把两个文件放在服务器的同一个目录，将 `.env.example` 复制为 `.env`，按注释填写 Docker Hub 用户名、正式域名和应用配置，然后在该目录运行：

```sh
chmod 600 .env
mkdir -p data/updates data/installers
docker compose config --quiet
docker compose pull
docker compose up -d
curl -fsS http://127.0.0.1:55400/health
```

`LAUNCHONE_PUBLIC_ORIGIN` 必须与 GitHub `desktop-release` 中的 `LAUNCHONE_SERVICE_URL` 一致。配置该域名的 DNS 和 HTTPS 反向代理：宿主机上的反代指向 `127.0.0.1:55400`；反代也运行在 Docker 中时，将两个服务接入同一个 Docker 网络，指向 `launchone:55400`。

在 Reseller 后台给 LaunchOne 创建独立服务应用，取得 `RESELLER_APP_CREDENTIAL`，权限填 `keys:ensure`，允许的 issuer 与 `IDONE_ISSUER` 一致，精确回调填 `https://你的LaunchOne域名/auth/reseller/callback`。为应用设置可用的默认分组，并确认两个默认模型在该分组中可用。容器不需要苹果证书或 LaunchOne 更新签名私钥。

应用内更新和官方安装包镜像的环境变量先留空；准备好文件后，再按 `.env.example` 中的路径启用。`data/` 只放可分发的安装包和清单，容器以只读方式挂载。更新镜像时运行 `docker compose pull && docker compose up -d`；只修改环境变量时运行 `docker compose up -d`。

直接用 `docker run` 部署时，把环境变量保存在服务器的 `/opt/launchone/server.env`，首次运行可用：

```sh
docker run -d --env-file /opt/launchone/server.env -e HOST=0.0.0.0 -e PORT=55400 -p 127.0.0.1:55400:55400 --name launchone --restart unless-stopped docker.io/your-dockerhub-user/launchone:latest
```

把 `your-dockerhub-user` 换成你的 Docker Hub 用户名，文件路径换成实际保存位置。示例中的两个 `-e` 将容器监听地址固定为 **`0.0.0.0:55400`**，覆盖环境文件中原有的 `HOST` 和 `PORT`，与端口映射保持一致。服务器对外 HTTPS 入口继续由现有反向代理提供。

如果以后把 Lab 服务迁入 Docker，还需将 Lab 根证书只读挂载进容器，并用 `NODE_EXTRA_CA_CERTS` 指向该证书文件，让容器能信任 Lab 的 HTTPS。当前 Lab 仍按现有 Node／systemd 方式运行。

如果启用安装包镜像或应用更新下载，用 volume 挂载对应的数据目录，并让环境变量中的目录路径指向容器内的挂载位置。更新镜像或修改环境变量后，需自行重新创建服务容器；已有 Compose 部署继续使用 `docker compose pull` 和 `docker compose up -d`。

## 发布应用内更新（目前为 Lab）

本节是把测试包发布到现有 Lab 更新服务。**当前仓库只提供 Lab 的发布命令，正式版更新服务还需单独部署。** 正式安装包可以按第 5 步生成和分发；GitHub 打包成功不代表正式版已自动上线更新。

### 服务机只配置一次

更新下载与登录接入共用 `launchone-lab.service`。在它使用的服务端配置中填入：

```dotenv
LAUNCHONE_UPDATE_MANIFEST_FILE=/home/hulei/projects/launchone/output/updates/lab/latest.json
LAUNCHONE_UPDATE_ARTIFACT_DIRECTORY=/home/hulei/projects/launchone/output/updates/lab/files
```

这两项是**服务机上的文件路径**，不填到 GitHub。换一台部署机器时，改成那台机器的实际路径。首次修改服务配置后重启服务；以后正常发布新版本不用重启。

### 每次发布新版本

先增加应用版本号，再打包。下载并解压 Actions 产物，将文件放到维护机项目中的 `output/lab/macos/` 和 `output/lab/windows/`，然后在项目根目录运行：

```sh
npm run updates:publish:lab
```

脚本检查安装包签名、版本号和文件校验值，通过后才更新下载文件和 `latest.json`。可以只发布一个平台。已发布的同名文件不能换成不同内容，要修复就增加版本号重新打包。

客户端启动时检查更新，之后每 24 小时检查一次，也可以点击应用底部手动检查。发现更新先提示，用户点击后才安装。回退服务器上的更新清单不会让用户电脑自动降级。

### 发布后检查

在 Lab 主机的项目目录运行：

```sh
cargo run --manifest-path src-tauri/Cargo.toml --features updater-smoke --example probe_updates -- config/lab.json output/updates/lab/probe
```

只检查 Mac，在末尾加 `macos`；只检查 Windows，加 `windows`。它会实际下载更新包并检查签名、版本和篡改拒绝，不安装软件。安装替换和重启仍需在 Mac／Windows 真机检查。

服务端接口供排查时使用：`/v1/updates/<target>/<arch>/<currentVersion>` 返回更新清单，`/updates/files/<version>/<filename>` 提供下载；没有对应平台的更新时返回 204。下载只公开专门的更新目录。
