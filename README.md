# Suwayomi-next

简体中文 | [English](docs/en/README.md)

![128x128](./assets/images/icons/128x128.png)

Suwayomi-next是一个漫画阅读器项目，支持基于Tachiyomi拓展的插件系统，并提供现代化和跨平台的一体式桌面阅读/服务客户端分离阅读体验。本项目的实现有赖于对[Suwayomi-Server](https://github.com/Suwayomi/Suwayomi-Server)和[Mihon](https://github.com/mihonapp/mihon)的参考。

![page-setting](./assets/images/page-setting.png)

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

仓库组件结构如下：

![Suwayomi-next 组件结构](./assets/images/project-struct-simple.png)

产物的目录结构如下：

```
suwayomi             桌面托盘
bin/				 发行产物二进制
  ├─ suwayomi-server   服务器
  └─ ext-runtime.jar   扩展运行时
webui/               Suwayomi-WebUI 构建产物（随发布捆绑）
jre/                 裁剪后的 JRE（随发布捆绑，供 ext-runtime.jar 使用）

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

### 本地构建

#### Windows

> 支持架构：`x64` | `arm64`

环境依赖:

- **Rust**：stable 工具链，1.91+

  - 宿主目标 `x86_64-pc-windows-msvc` | `aarch64-pc-windows-msvc`

    Visual Studio 生成工具：提供 MSVC 链接器与 Windows SDK

  - 宿主目标 `x86_64-pc-windows-gnullvm` | `aarch64-pc-windows-gnullvm`

    LLVM/Clang工具链：提供llvm-mingw 或 MSYS2 CLANG64

- **Git**：克隆仓库及提供git bash

- **JDK 25+**：构建扩展运行时与裁剪 JRE

- **Node 24.20+、pnpm 11**：构建 WebUI

```bash
# 使用Git bash(或任何兼容bash)
git clone https://github.com/576576/Suwayomi-next         Suwayomi-next
git clone https://github.com/576576/Suwayomi-tray         tray
git clone https://github.com/576576/Suwayomi-ext-runtime  ext-runtime
git clone https://github.com/576576/Suwayomi-WebUI        WebUI

# 四个组件都是发布布局的固定组成部分，缺一件最后就归位不成
cd Suwayomi-next  && cargo build --release -p suwayomi-server  # → target/release/suwayomi-server.exe
cd ../tray        && cargo build --release                     # → target/release/suwayomi.exe
cd ../ext-runtime && ./gradlew jar                             # → build/libs/ext-runtime.jar
cd ../WebUI       && pnpm i && pnpm build                      # → build/

# 按发布布局归位，最终产物目录Suwayomi-build
cd ../
mkdir -p Suwayomi-build/bin
cp Suwayomi-next/target/release/suwayomi-server.exe Suwayomi-build/bin
cp tray/target/release/suwayomi.exe Suwayomi-build/
cp ext-runtime/build/libs/ext-runtime.jar Suwayomi-build/bin
cp -r WebUI/build Suwayomi-build/webui
cp -r Suwayomi-next/assets/templates/directory/. Suwayomi-build/ && find Suwayomi-build -name .gitkeep -delete

# 可选：打包裁剪jre：x64|aarch64
bash ext-runtime/scripts/make-jre.sh windows x64 Suwayomi-build/jre
ls Suwayomi-build/
```

仓库根的 **`build.bat`** 是上面这些的一键版：它**不**构建另外三仓，而是拉各自的 Release 资产来组装
（只需 cargo / git / curl / Python / PowerShell，不需要 JDK 与 Node），产物在 `target\artifacts\`。

#### Linux

> 支持架构：`x64` | `arm64`

环境依赖:

- **Rust**：stable 工具链，1.91+
  - 宿主目标：`x86_64-unknown-linux-gnu` | `aarch64-unknown-linux-gnu`
- **编译依赖**：`pkg-config`、`libssl-dev`、`perl`，其中 `perl` 供构建脚本使用
- **托盘依赖**：`libwebkit2gtk-4.1-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`、`libxdo-dev`
- **Git**：克隆仓库
- **JDK 25+**：构建扩展运行时与裁剪 JRE
- **Node 24.20+、pnpm 11**：构建 WebUI

```bash
# 系统依赖（下面给的是 Debian/Ubuntu 系的包名）
sudo apt-get install -y pkg-config libssl-dev perl \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev

git clone https://github.com/576576/Suwayomi-next         Suwayomi-next
git clone https://github.com/576576/Suwayomi-tray         tray
git clone https://github.com/576576/Suwayomi-ext-runtime  ext-runtime
git clone https://github.com/576576/Suwayomi-WebUI        WebUI

# 四个组件都是发布布局的固定组成部分，缺一件最后就归位不成
cd Suwayomi-next  && cargo build --release -p suwayomi-server  # → target/release/suwayomi-server
cd ../tray        && cargo build --release                     # → target/release/suwayomi
cd ../ext-runtime && ./gradlew jar                             # → build/libs/ext-runtime.jar
cd ../WebUI       && pnpm i && pnpm build                      # → build/

# 按发布布局归位，最终产物目录Suwayomi-build
cd ../
mkdir -p Suwayomi-build/bin
cp Suwayomi-next/target/release/suwayomi-server Suwayomi-build/bin
cp tray/target/release/suwayomi Suwayomi-build/
cp ext-runtime/build/libs/ext-runtime.jar Suwayomi-build/bin
cp -r WebUI/build Suwayomi-build/webui
cp -r Suwayomi-next/assets/templates/directory/. Suwayomi-build/ && find Suwayomi-build -name .gitkeep -delete

# 可选：打包裁剪jre：x64|aarch64
bash ext-runtime/scripts/make-jre.sh linux x64 Suwayomi-build/jre
ls Suwayomi-build/
```

#### macOS

> 支持架构：`arm64` | @Deprecated `x64`

环境依赖:

- **Rust**：stable 工具链，1.91+
  - 宿主目标 `aarch64-apple-darwin` | @Deprecated `x86_64-apple-darwin`
- **Xcode Command Line Tools**：提供 clang 与 macOS SDK（`xcode-select --install`）
- **Git**：克隆仓库
- **JDK 25+**：构建扩展运行时与裁剪 JRE
- **Node 24.20+、pnpm 11**：构建 WebUI

```bash
git clone https://github.com/576576/Suwayomi-next         Suwayomi-next
git clone https://github.com/576576/Suwayomi-tray         tray
git clone https://github.com/576576/Suwayomi-ext-runtime  ext-runtime
git clone https://github.com/576576/Suwayomi-WebUI        WebUI

# 四个组件都是发布布局的固定组成部分，缺一件最后就归位不成
cd Suwayomi-next  && cargo build --release -p suwayomi-server  # → target/release/suwayomi-server
cd ../tray        && cargo build --release                     # → target/release/suwayomi
cd ../ext-runtime && ./gradlew jar                             # → build/libs/ext-runtime.jar
cd ../WebUI       && pnpm i && pnpm build                      # → build/

# 按发布布局归位，最终产物目录Suwayomi-build
cd ../
mkdir -p Suwayomi-build/bin
cp Suwayomi-next/target/release/suwayomi-server Suwayomi-build/bin
cp tray/target/release/suwayomi Suwayomi-build/
cp ext-runtime/build/libs/ext-runtime.jar Suwayomi-build/bin
cp -r WebUI/build Suwayomi-build/webui
cp -r Suwayomi-next/assets/templates/directory/. Suwayomi-build/ && find Suwayomi-build -name .gitkeep -delete

# 可选：打包裁剪jre：x64|aarch64
bash ext-runtime/scripts/make-jre.sh mac aarch64 Suwayomi-build/jre
ls Suwayomi-build/
```

#### Android

> 支持架构：`x64` | `arm64`

环境依赖:

- **Rust**：stable 工具链，1.91+
  - Android 目标 (`rustup target add aarch64-linux-android x86_64-linux-android`)
- **Android SDK**：`platforms;android-37.0` 与 `ndk;28.2.13676358`
  - 配置环境变量 `ANDROID_HOME`、`ANDROID_NDK_HOME` 指到它们
- **JDK 25+**：Gradle 9 与 AGP 9.2.1
- **Node 24.20+、pnpm 11**：构建 WebUI
- **Git、python3、curl**：取扩展共享源码、打包 assets
- 交叉编译的两个坑与工具链版本表见 **`docs/agent/android.md`**

```bash
# 先准备一份 WebUI 构建产物，放到与 Suwayomi-next 同级的 WebUI/
# （package-webui.sh 也接受从 Release 下载的 webui zip）
git clone https://github.com/576576/Suwayomi-WebUI ../WebUI
(cd ../WebUI && pnpm i && pnpm build)

# ABI=arm64|x86_64|all
ABI=arm64 bash android/scripts/build-rust.sh      # server → cdylib → app/src/main/jniLibs/arm64-v8a/
bash android/scripts/fetch-ext-runtime-src.sh     # 下载 :extension-host 用的 ext-runtime 共享源码部分
bash android/scripts/package-webui.sh ../WebUI/build  # WebUI 产物 → app/src/main/assets/webui.zip
cd android && ./gradlew :app:assembleRelease --no-daemon
# → android/app/build/outputs/apk/release/app-release.apk
```

## 仓库结构

```
crates/              各 Rust crate（分层见上面的结构图）
android/             Android 宿主工程（独立 Gradle/AGP 构建，不并入主工程）
scripts/             CI/辅助脚本（resolve-webui.sh / unzip_any.py 等）
assets/              图片（images/：图标、截图、结构图）与目录树模板（templates/）
docs/                文档（zh/ 中文、en/ 英文、agent/ 给维护者与 AI）
```

## 可选环境变量

| 变量 | 默认 | 说明 |
| --- | --- | --- |
| `SUWAYOMI_APPDATA_DIR` | exe 上级 `appdata/` | 程序自身状态的**唯一可写根**：`cache/`、`db/`、`settings/`、`extensions/{apk,bin}` 都在它下面，各自没有目录变量 |
| `SUWAYOMI_DATA_DIR` | exe 上级 `data/` | 用户数据根：`downloads/`、`local/`、`autobackup/`；也可在 WebUI「数据与存储 → 存储位置」改 |
| `SUWAYOMI_PORT` | `4567` | HTTP 端口；被占用或落在 Windows 动态保留区时自动顺延 |
| `SUWAYOMI_IP` | `0.0.0.0` | 监听地址；改成 `127.0.0.1` 可只允许本机访问 |
| 其余变量 | — | 见 [`docs/zh/user-guide.md`](docs/zh/user-guide.md)：WebUI 目录、数据库后端、沙盒、认证、JWT 等 |

## 数据库后端

- **默认**：内建 `rheos-tokio-rusqlite` crate
  - `<appdata>/db/suwayomi.db`
- **备选**：外部 PostgreSQL 连接
  - `SUWAYOMI_DB_BACKEND=postgres`（**只有它决定后端**，别的取值一律落 SQLite）
  - `SUWAYOMI_DB_URL=postgres://user:pass@host:5432/db`（只说 PostgreSQL 在哪，设了它不会换来后端）

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

## 关键文档

- `docs/zh/user-guide.md` — 用户指南（配置/备份/OPDS/Docker）
- `docs/agent/release.md` — 发布流程与 CI 约定
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
