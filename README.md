# Suwayomi-next

简体中文 | [English](docs/en/README.md)

![128x128](./assets/images/128x128.png)

Suwayomi-next是一个漫画阅读器项目，支持基于Tachiyomi拓展的插件系统，并提供现代化和跨平台的一体式桌面阅读/服务客户端分离阅读体验。本项目的实现有赖于对[Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server)和[Mihon](https://github.com/mihonapp/mihon)的参考。

![page-setting](./assets/screenshots/page-setting.png)

## 使用方法

1. 运行suwayomi(.exe)
2. 系统WebView窗口将自动打开，并支持使用以下接口：

- WebUI：`http://localhost:4567`（端口不满足时将自动顺延）
- GraphQL：`/api/graphql`
- REST：`/api/v1`
- OPDS：`/api/opds/v1.2`（In-progress: `/api/opds/v2`）

> 桌面WebUI 窗口由**系统WebView** 打开（Tauri 2），不支持WebView 时回退浏览器
>
> 完整配置说明见 **`docs/zh/user-guide.md`**。

## 项目特色

| 特性                | 描述                                                         |
| ------------------- | ------------------------------------------------------------ |
| 兼容Tachiyomi/Mihon | 集成完全相同的存储/.tachibk备份结构，支持Tachiyomi/Mihon互操作 |
| 兼容Suwayomi        | 支持Suwayomi Project下的其他服务端/客户端，兼容互操作        |
| 更优的存储占用      | 进行了深度分析裁剪，比Suwayomi-Server小40%+，内存占用更小    |
| 内存安全            | 充分利用Rust/Kotlin的内存安全特性，以最佳实践编写            |
| 现代化的操作特性    | 引入桌面托盘、操作手势等特性，用户体验更好                   |

## 项目结构

本项目已拆分为3个仓库，以增强可审计性并降低ci耗时。并捆绑1个仓库产物作为默认前端：

| 组件       | 仓库                                                         |
| ---------- | ------------------------------------------------------------ |
| 服务器     | [Suwayomi-next](#Suwayomi-next)                              |
| 托盘       | [Suwayomi-tray](https://github.com/576576/Suwayomi-tray)     |
| 扩展运行时 | [Suwayomi-ext-runtime](https://github.com/576576/Suwayomi-ext-runtime) |
| 默认前端   | [576576/Suwayomi-WebUI](https://github.com/576576/Suwayomi-WebUI) |



```mermaid
graph TD
    subgraph repo ["Suwayomi-next（本仓 Rust workspace）"]
        dbm["suwayomi-db-macros<br>proc-macro"]
        db["suwayomi-db<br>双后端数据库层"]
        core["suwayomi-core<br>领域模型 / 配置 / 认证"]
        domain["suwayomi-domain<br>业务逻辑（唯一对外发 HTTP 的一层）"]
        api["suwayomi-api<br>AppState + 全站认证中间件"]
        rest["suwayomi-rest<br>REST API v1"]
        gql["suwayomi-graphql<br>GraphQL API"]
        opds["suwayomi-opds<br>OPDS"]
        server["suwayomi-server<br>入口：装配 + 静态托管 + 起沙盒"]
        android["suwayomi-android<br>JNI cdylib（随 APK 分发）"]
        db --> dbm
        core --> db
        domain --> core & db
        api --> core & domain
        rest --> api & core & domain & db
        gql --> api & core & domain & db
        opds --> api & core & domain & db
        server --> rest & gql & opds & api & domain & core & db
        android --> server & core & db
    end
    tray["Suwayomi-tray<br>桌面托盘"]
    ext["ext-runtime.jar<br>JVM 扩展沙盒"]
    webui["Suwayomi-WebUI<br>默认前端"]
    tray -->|"启动 server 进程、注入两个根"| server
    server -->|"java -jar（子进程）"| ext
    ext -.->|"扩展源回环 HTTP"| domain
    webui -->|"HTTP：REST / GraphQL / OPDS"| server
```



产物的目录结构如下：

```
suwayomi             桌面托盘
bin/				 发行产物二进制
  ├─ suwayomi-server   服务器
  └─ ext-runtime.jar   扩展运行时
webui/               Suwayomi-WebUI 构建产物（随发布捆绑，可选）
jre/                 JRE 运行时依赖（可选）

<data>/              数据目录（Tachiyomi 兼容）
  local/			 本地图源
  downloads/		 图源插件的下载内容
  autobackup/		 自动备份目录

<appdata>/			 应用数据目录
  extensions/{apk,bin}  扩展 APK 与 dex2jar 产物应用数据目录
  settings/  		 追踪器凭据与图源偏好
  cache/     		 漫画图片/缩略图/仓库索引/封面等缓存
  logs/      		 server/tray/sandbox日志文件
  db/        		 suwayomi.db 与 session.key
```

## 从源码构建

### Github Actions

GitHub Release 提供解压/安装即用的平台包。
构建不同组合（平台/架构/额外选项）的构建时：

1. Fork 本仓库

2. 在 Actions 页手动运行 `Release` workflow

3. 勾选目标平台、通道、配置选项

4. 运行完成后从 Release 页下载对应产物

   > 注意：自行构建的产物中Setup exe/msi(x)不含仓库特定的签名，可能需要自行配置

### Windows

> 支持架构：`x64` | `arm64`

需要 **Rust stable**（MSVC 工具链，`x86_64-pc-windows-msvc`）、**Git** 与 **Visual Studio 生成工具**（C++ 桌面开发）。只跑服务端有这些就够：

```bash
git clone https://github.com/576576/Suwayomi-next && cd Suwayomi-next
cargo run --release -p suwayomi-server
```

要完整产物（托盘 + 扩展运行时 + WebUI），把另外三个仓库克隆到同级再各自构建：

```bash
git clone https://github.com/576576/Suwayomi-tray        ../Suwayomi-tray
git clone https://github.com/576576/Suwayomi-ext-runtime ../Suwayomi-ext-runtime
git clone https://github.com/576576/Suwayomi-WebUI       ../Suwayomi-WebUI

cargo build --release -p suwayomi-server                # → target/release/suwayomi-server.exe
bash ../Suwayomi-tray/build-tray.sh                     # → ../Suwayomi-tray/target/release/suwayomi.exe
(cd ../Suwayomi-ext-runtime && ./gradlew jar)           # → build/libs/ext-runtime.jar（需 JDK 25）
(cd ../Suwayomi-WebUI && pnpm i && pnpm build)          # → build/（需 Node ≥ 24.20、pnpm 11）
```

再按发布布局归位（`bin/` 放服务端与扩展运行时，托盘与 `webui/` 在根）：

```bash
mkdir -p dist/bin
cp target/release/suwayomi-server.exe dist/bin/
cp ../Suwayomi-tray/target/release/suwayomi.exe dist/
cp ../Suwayomi-ext-runtime/build/libs/ext-runtime.jar dist/bin/
cp -r ../Suwayomi-WebUI/build dist/webui
cp -r assets/templates/directory/. dist/ && find dist -name .gitkeep -delete
```

`jre/` 是可选的：打了它沙盒就不再依赖系统 JDK —— 用
`bash ../Suwayomi-ext-runtime/scripts/make-jre.sh windows x64 <目录>` 生成 JRE，放到 `dist/jre/`。
不打时沙盒按 `JAVA_HOME` → `PATH` 上的 `java` 找（也可以用 `SUWAYOMI_JAVA` 指定）。

仓库根的 **`build.bat`** 是上面这些的一键版：它**不**构建另外三仓，而是拉各自的 Release 资产来组装
（只需 cargo / git / curl / Python / PowerShell，不需要 JDK 与 Node），产物在 `target\artifacts\`。

### Linux

> 支持架构：`x64` | `arm64`

```bash
sudo apt-get install -y build-essential pkg-config libssl-dev perl   # Debian / Ubuntu
git clone https://github.com/576576/Suwayomi-next && cd Suwayomi-next
cargo build --release -p suwayomi-server
```

`pkg-config` + `libssl-dev` 是 `reqwest` 用系统 OpenSSL 的编译期依赖，`perl` 供 `openssl-sys` 的构建脚本使用。`x64` 与 `arm64` 各自在对应架构的机器上原生构建（CI 的 arm64 用的是 `ubuntu-24.04-arm` runner）。

服务端自带 Web 界面，浏览器访问 `:4567` 即可；托盘是可选件 —— 它是 Tauri 2 应用，另需 Tauri 在 Linux 上的系统依赖（WebKitGTK、libappindicator 等），见 Tauri 官方前置说明。

### macOS

> 支持架构：`x64`@Deprecated | `arm64`

```bash
xcode-select --install      # 命令行工具：clang 与系统库
git clone https://github.com/576576/Suwayomi-next && cd Suwayomi-next
cargo build --release -p suwayomi-server
```

托盘（Tauri 2）在 macOS 上用系统 WebView（WKWebView），不需要额外的 WebView 依赖。`x64` 已弃用，新构建请用 `arm64`。

### Android

> 支持架构：`x64` | `arm64`

宿主工程在 `android/`（独立 Gradle/AGP 工程，不并入 Cargo workspace）。需要 **Rust**（带 Android target）、**JDK 25**，以及 Android SDK 的 `platforms;android-37.0` 与 `ndk;28.2.13676358`：

```bash
ABI=arm64 bash android/scripts/build-rust.sh      # server → cdylib → app/src/main/jniLibs/arm64-v8a/
bash android/scripts/fetch-ext-runtime-src.sh     # :extension-host 用的 ext-runtime 共享源码
bash android/scripts/package-webui.sh             # WebUI 产物 → app/src/main/assets/webui.zip
cd android && ./gradlew :app:assembleRelease --no-daemon
# → android/app/build/outputs/apk/release/app-release.apk
```

`ABI` 取 `arm64`（默认，发布用）、`x86_64`（模拟器）或 `all`；`package-webui.sh` 不带参数时自动找
`../Suwayomi-WebUI/build`。没配 release keystore 时回退 AGP 的 debug key，跨次覆盖安装前需先卸载。
形态差异、交叉编译的两个坑与工具链版本表见 **`docs/agent/android.md`**。

## 仓库结构

```
crates/              各 Rust crate（依赖关系见上面的结构图）
android/             Android 宿主工程（独立 Gradle/AGP 构建，不并入主工程）
scripts/             CI/辅助脚本（resolve-webui.sh / unzip_any.py 等）
assets/              图标与截图（images/、screenshots/）
docs/                文档（zh/ 中文、en/ 英文、agent/ 给维护者与 AI）
```

## 可选环境变量

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `SUWAYOMI_APPDATA_DIR` | exe 上级 `appdata/` | 程序自身状态的**唯一可写根**：`cache/`、`db/`、`settings/`、`extensions/{apk,bin}` 都在它下面，各自没有目录变量 |
| `SUWAYOMI_DATA_DIR` | exe 上级 `data/` | 用户数据根：`downloads/`、`local/`、`autobackup/`；也可在 WebUI「数据与存储 → 存储位置」改 |
| `SUWAYOMI_PORT` | `4567` | HTTP 端口；被占用或落在 Windows 动态保留区时自动顺延 |
| `SUWAYOMI_IP` | `0.0.0.0` | 监听地址；改成 `127.0.0.1` 可只允许本机访问 |
| `SUWAYOMI_WEBUI_DIR` | exe 同级的 `webui/` | 捆绑 WebUI 目录（不含 `index.html` 时忽略该项） |
| `SUWAYOMI_DB_BACKEND` | `sqlite` | 数据库后端：`sqlite` / `postgres` |
| `SUWAYOMI_DB_URL` | （空） | PostgreSQL 连接串；设置后自动改用外部 PostgreSQL |
| `SUWAYOMI_TRACKERS_CONFIG` | `<appdata>/settings/trackers.json` | 追踪器 OAuth 应用凭据文件 |
| `SUWAYOMI_SANDBOX_JAR` | exe 同级（或 `bin/`）的 `ext-runtime.jar` | JVM 扩展沙盒 jar；未找到时扩展不可用 |
| `SUWAYOMI_SANDBOX_PORT` | `4568` | 沙盒 HTTP 端口；托盘启动时会挑一个可用的传进来 |
| `SUWAYOMI_SANDBOX_PROXY` | （空） | 沙盒与扩展下载走的 HTTP 代理 |
| `SUWAYOMI_SANDBOX_URL` | （空） | 已在运行的扩展宿主地址（如 Android 宿主 App）；设了就不再自己拉沙盒 |
| `SUWAYOMI_JAVA` | 打包的 `jre/` → `JAVA_HOME` → `PATH` | 沙盒使用的 `java` 可执行文件 |
| `SUWAYOMI_AUTH_MODE` | `DISABLED` | 认证模式：`DISABLED` / `BASIC_AUTH` / `SIMPLE_LOGIN` / `UI_LOGIN` |
| `SUWAYOMI_AUTH_USERNAME` / `SUWAYOMI_AUTH_PASSWORD` | （空） | 认证凭据；启用认证时两者都不能为空，否则拒绝启动 |
| `SUWAYOMI_AUTH_COOKIE_SECURE` | （空） | 设 `1` 给会话 cookie 加 `Secure`（仅 HTTPS 反代之后开启） |
| `SUWAYOMI_SESSION_SECRET` | `<appdata>/db/session.key` | 会话与 JWT 的签名密钥；未设置时首次启动生成 |
| `SUWAYOMI_JWT_AUDIENCE` | `suwayomi-server-api` | JWT 的 `aud` 声明 |
| `SUWAYOMI_JWT_TOKEN_EXPIRY` | `5m` | 访问令牌有效期（也接受 `PT5M` 这类 ISO-8601 写法） |
| `SUWAYOMI_JWT_REFRESH_EXPIRY` | `60d` | 刷新令牌有效期 |

## 数据库后端

- **默认**：内建 `rheos-tokio-rusqlite` crate
  - `<appdata>/db/suwayomi.db`
- **备选**：外部 PostgreSQL 连接
  - `SUWAYOMI_DB_BACKEND=postgres`
  - `SUWAYOMI_DB_URL=postgres://user:pass@host:5432/db`

程序自身产生的东西（缓存、库、设置、扩展 APK 与转换产物）都在**一个可写根**
`SUWAYOMI_APPDATA_DIR`（以下简称 `<appdata>`）下，这四项没有各自的目录变量：

```
<appdata>/
  cache/     漫画图片/缩略图/仓库索引/封面等缓存
  logs/      server/tray/sandbox日志文件
  db/        suwayomi.db 与 session.key
  settings/  追踪器凭据与图源偏好
  extensions/{apk,bin}   扩展 APK 与 dex2jar 产物
```

数据库文件**刻意不放在数据目录里**：数据目录（`SUWAYOMI_DATA_DIR`，也可在 WebUI 的
「设置 → 数据与存储 → 存储位置」里改）是用户随时可以换的一项，而设置本身就存在这个库
里 —— 库跟着数据目录走的话，一改目录就把设置弄丢了。

环境变量：`SUWAYOMI_SANDBOX_JAR`（启用沙盒）、`SUWAYOMI_SANDBOX_PORT`（默认 4568）、`SUWAYOMI_APPDATA_DIR`（唯一的可写根，沙盒子进程继承它并自行派生扩展目录 `extensions/apk`、转换 jar 目录 `extensions/bin` 与 `settings/`）、`SUWAYOMI_SANDBOX_PROXY`（可选 HTTP 代理）。未配置时回退内置 `StubFetcher`。

## 关键文档

- `docs/zh/user-guide.md` — 用户指南（配置/备份/OPDS/Docker）
- `docs/agent/release.md` — 发布流程与 CI 约定
- `docs/zh/migrate-from-kotlin.md` — 从 Kotlin 版迁移操作指南（Mihon 备份导入）
- `docs/agent/rest-api.md` — REST v1 端点兼容基线
- `docs/agent/graphql.md` — GraphQL schema 基线说明
- `docs/agent/android.md` — Android 宿主工程（构建与验证）

## Docker

发布流程会把镜像推到 GHCR，标签与产物名同一套规则：alpha 为 `r<提交数>`，release/beta 为 `3.y.z`
（beta 另带 `-beta`）。**release / beta 通道同时打 `latest` 标签**，alpha 不打 —— 它是每次 push
都出的预发布，不该占着 `latest`。

```bash
docker run -p 4567:4567 -v suwayomi-data:/data ghcr.io/576576/suwayomi-next:latest
```

## Code signing policy

Free code signing provided by [SignPath.io](https://signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

本项目的 Windows 安装包产物（`msi`/`setup.exe`/后续 `msix`）将由 [SignPath.io](https://signpath.io/) 提供免费签名，由 [SignPath Foundation](https://signpath.org/) 签发证书。

| 角色 | 成员 |
|---|---|
| Authors | [@576576](https://github.com/576576) |
| Reviewers | [@576576](https://github.com/576576) |
| Approvers | [@576576](https://github.com/576576) |

每一次签名请求都由 Approver 在 SignPath 控制台上人工批准，没有无人值守的签名流水线。被签的文件只有
本项目自源码构建的产物（`suwayomi.exe` 与 `bin/suwayomi-server.exe`）；随包分发的第三方二进制
（`jre/` 下的 Temurin OpenJDK）不在签名范围内。

隐私见 [PRIVACY.md](PRIVACY.md)：本程序不会向你或任何第三方传输信息，除非你（安装或使用它的人）明确要求 —— 所有对外请求都由你的操作触发。

签名接入之前发布的产物是未签名的（`msi` / `setup.exe` 目前如此），产物在[Releases](https://github.com/576576/Suwayomi-next/releases)。

完整政策（角色、被签范围、审批流程）见 [CODE_SIGNING_POLICY.md](CODE_SIGNING_POLICY.md)。

## 许可证

Mozilla Public License, v.2.0

    Copyright (C) Contributors to the Suwayomi project
    
    This Source Code Form is subject to the terms of the Mozilla Public
    License, v. 2.0. If a copy of the MPL was not distributed with this
    file, You can obtain one at http://mozilla.org/MPL/2.0/.

## 免责声明

本应用的开发者与扩展仓库所提供的内容源 / 内容提供方没有任何关联。
