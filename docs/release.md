# 发布流程与 CI 约定

面向维护者。CI（`.github/workflows/release.yml` + `build.yml`）只保留必要提示，决策与背景都在这里。

## CI 结构

| 文件 | 角色 |
|---|---|
| `build.yml` | **可复用构建工作流**（只由 `workflow_call` 触发）：算好参数后由它编译 + 打包全部 target，产物用 `upload-artifact` 上传。两个 job：`build`（桌面/服务端矩阵）与 `android`（APK）。所有平台的构建逻辑只有这一份。 |
| `release.yml` | **唯一入口**：推送 main → 自动 alpha；手动 dispatch → alpha/beta/release。负责算版本号、解析 WebUI 制品，然后 `uses: ./.github/workflows/build.yml` 构建，再用 `download-artifact` 收产物发布 Release。 |

- 产物约定分两套，由 `build.yml` 的 `pack_mode` 表达（调用方按触发方式传入）：
  `channel` = 手动发布（产物名带通道段，`pack_jre` 决定是否另出一份 `+jre`）；
  `alpha` = 自动构建（产物名固定 `+jre`，两平台都捆 JRE）。
- 因此 `build.yml` 里那些"看着多余"的分支（例如 `pack_mode` 的两条命名路径）**不要随手合并**——两条路径各自对应一个历史产物约定，产物名是用户可见的。
- 手动触发的 run 标题本应由 prep 里那段 `curl PATCH` 改成 `Release {VER}`，但该请求没有注入 `GITHUB_TOKEN`（恒 401 被 `|| true` 吞掉），实际一直是默认标题。合并 CI 时原样保留以求行为一致；要修就补 `env: GH_TOKEN: <github.token>`（会让 run 标题开始变化，属于行为变更）。
- 合并前是 `release.yml`（手动）+ `release-alpha.yml`（推送自动，workflow 名 `Auto build`）；合并后 Actions 侧边栏里两者的 workflow 名统一显示为 `Release`，推送触发的 run 标题仍是提交信息（默认行为，未变）。
- **`on: push` 只跟 main**：`dev` 分支已删除，原来那里只有 main/dev 两个分支在跑同一条自动构建。
- `run:` 里的 `${{ }}` 是**文本替换**，`#` 注释行一样会被求值 —— 注释里想提到表达式就写成普通文字（否则会被替换，还可能把 token 之类带进日志）。

## 通道与版本

| 通道 | 触发 | versionName | versionCode | tag | prerelease |
|---|---|---|---|---|---|
| alpha | 推送 main（自动） | `r{code}` | 提交数+3000 | `r{code}-alpha.{run_id}` | true |
| alpha | 手动 release.yml | `r{code}` | 同上 | `r{code}-alpha.{run_id}` | true |
| beta | 手动 release.yml | `3.{n/100}.{n%100 补零两位}` | 同上 | `v3.y.z-beta.{run_id}` | false |
| release | 手动 release.yml | `3.y.z`（同上规则） | 同上 | `v3.y.z` | false |

- `versionCode = commit count + 3000`。
- 3.y.z 的末两位**必须补零**：tag 去非数字后要恰好等于 versionCode（`tag_to_num` 纯数字比大小），`3.2.5`→325 会小于 r3205 被判旧。
- `aboutServer.buildType` 走编译期 `SUWAYOMI_BUILD_TYPE`（build.rs 生成常量），运行时读不到 CI 变量，勿改成 env 读取。
- beta/release 共用 3.y.z 版本名，故产物文件名保留通道段：`Suwayomi-{VER}-{CH}-{TGT}`；alpha 产物无通道段。
- release 同名 tag 已存在时先 `gh release delete --cleanup-tag`；beta/alpha tag 天然唯一不删。

## 构建目标与 runner

手动 dispatch 的平台开关与对应的 runner（`release.yml` 里的 mapping）：

| 开关 | runner | rust target | jlink 目标 | JDK 发行版 |
|---|---|---|---|---|
| `build_windows_x64` | `windows-latest` | `x86_64-pc-windows-msvc` | windows/x64 | temurin |
| `build_windows_arm64` | `windows-11-arm` | `aarch64-pc-windows-msvc` | windows/aarch64 | **zulu** |
| `build_linux_x64` | `ubuntu-latest` | `x86_64-unknown-linux-gnu` | linux/x64 | temurin |
| `build_linux_arm64` | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | linux/aarch64 | temurin |
| `build_macos_x64` | `macos-15-intel` | `x86_64-apple-darwin` | mac/x64 | temurin |
| `build_macos_arm64` | `macos-15` | `aarch64-apple-darwin` | mac/aarch64 | temurin |
| `build_android_arm64` | `ubuntu-latest` | `aarch64-linux-android` | —（Android 不打包 JRE） | temurin |

- **每个 target 的 runner 必须与目标同架构**：`+jre` 用 jlink 生成，而 **jlink 不能跨平台生成运行时**（实测：Windows 的 jlink + linux-aarch64 的 jmods，产出的 `bin/java` 是 PE 头加一堆 `.dll` —— launcher 与原生库取自宿主 JDK）。所以 linux-arm64 用 arm64 runner（原生编译，顺带不再需要交叉工具链），x64 的 macOS 用 `macos-15-intel`。
- `macos-13` 已被 GitHub 下线，x64 macOS 现为 `macos-15-intel`；Windows arm64 用 `windows-11-arm`（公开预览，公共仓库免费不限量），该镜像自带 VS 2022 + Windows SDK 26100（`build.rs` 嵌图标要的 `rc.exe`）与 Git for Windows，`shell: bash` 可直接用。
- **判定平台一律用 `runner.os`，不要拿矩阵 runner 标签比字面量**：Windows 的标签不止 `windows-latest`（现在还有 `windows-11-arm`），`matrix.os == "windows-latest"` 这类写法在加第二个 Windows target 时必然踩空。
- **`uname -m` 在 `windows-11-arm` 上会撒谎**：镜像确实是原生 arm64、装的也是货真价实的 `win_aarch64` JDK，但 runner 上的 **Git for Windows 是 x64 版**，MSYS 的 `uname -m` 因此报 `x86_64`。`make-jre.sh` 的「宿主架构必须等于目标架构」这道闸因此误判过一次（run 35074440661，jlink 都没来得及启动）。现在宿主架构**优先读 `$JAVA_HOME/bin/java` 的可执行文件头**（与产物自检同一套偏移表），再退到 `release` 的 `OS_ARCH`，最后才是 `uname -m`。
- **`+jre` 有两道闸**：入口比「宿主平台 vs 目标平台」，末尾核「产物 magic **+ 架构**」。只判 magic 拦不住同格式但错架构的产物（x64 的 jlink + aarch64 的 jmods 就会产出那种），装上就是 `UnsatisfiedLinkError`。自检代码在 `make-jre.sh` 的 `binary-probe` 标记块里，被 `.workbuddy/verify/check_jre_arch.sh` 整块抽出来单测（合成夹具 + CI 真产物夹具，共 35 项）。
- **`jdk` 那一列是为什么**：Adoptium（Temurin）对 `windows/aarch64` **没有发布 JDK 25 的任何制品**（`jdk`/`jre`/`jmods` 三端点全 404；该平台在 Adoptium 上最高只到 JDK 21），而 `+jre` 必须有 jmods。**只有这一个 target 例外用 Azul Zulu**（其 `win_aarch64` 归档自带 `jmods/`），其余平台一律 Temurin —— 这是明确取舍，不是临时权宜；`ci_pack_check.py` 里有断言守着（「只有 windows-arm64 是 zulu」）。
- **`JAVA_HOME` 是 Windows 形式时不能直接做路径名展开**：CI 的 `setup-java` 注入的是 `C:\hostedtoolcache\…`，反斜杠在 bash 的 glob 里是转义符 —— `[[ -d "$JAVA_HOME/jmods" ]]` 认得，`compgen -G "$JAVA_HOME/jmods/*.jmod"` 却永远匹配不到。`make-jre.sh` 的 `jmods_dir()` 因此先把 `JAVA_HOME` 过 `cygpath -u` 归一化、并用 `find` 代替 glob。这个坑**本地复现不了**（本机 Temurin 25 按 JEP 493 不带 jmods，两条分支都走下载），只在 windows-arm64 上炸（run 35078586922）。
- **矩阵里每个桌面 target 都出托盘壳与 `bin/jvm-sandbox.jar`**（Windows x64+arm64 / Linux x64+arm64 / macOS x64+arm64）。Linux 侧因此无论架构都装同一套 webkit2gtk/appindicator 依赖；tray 有自己的 workspace 与 `target/`，各 runner 原生编译。Android 不在这个矩阵里。
- 平台开关默认只勾 Windows x64 + Linux x64，发布通道默认 `alpha`。

## 产物形态（默认包 / `+jre`）

手动 dispatch 只有一个形态开关：

| 开关 | 默认 | 含义 |
|---|---|---|
| `pack_jre` | ⬜ | 额外的 `+jre` 包：在默认包内容之上追加对应架构的 JRE（jlink 裁剪）。 |

命名（**默认包不带形态后缀** —— 不打包 JRE 的那份就是基线产物）：

| 模式 | 默认包 | `+jre` |
|---|---|---|
| 手动（`channel`） | `Suwayomi-{VER}-{CH}-{TGT}.zip/.tar.gz` | `Suwayomi-{VER}-{CH}-{TGT}+jre.zip/.tar.gz` |
| 自动（`alpha`） | —（自动构建只出 `+jre`） | `Suwayomi-{VER}-{TGT}+jre` |
| Android | `Suwayomi-{VER}[-{CH}]-android-arm64.apk` | —（跑系统 ART，不用 JRE） |

- 默认包**必然产出**、没有开关。早先那个 `-core` 后缀已废弃：它本来就只是"不带 JRE 的基线包"的代号，而基线包永远存在，给必然发生的事加后缀没有信息量。
- 只勾 Android 时桌面矩阵为空数组，`build` job 直接跳过。
- 归档格式：Windows 出 `.zip`，其余出 `.tar.gz`。**两者的归档布局一致**，都带顶层目录名（`Suwayomi-…/bin/…`）—— 用真实产物核对过：Windows 是 `Compress-Archive -Path <目录>`（会把目录本身收进归档），Linux/macOS 是 `tar -C dist`。`.workbuddy/verify/ci_pack_check.py` 里有对应断言，将来想统一时先看那条用例。

## JRE 裁剪（`+jre` 用）

由 `scripts/make-jre.sh` 生成（`scripts/` 下，被 `build.yml` 的打包步骤调用）：

| 形态 | 解压后 | 压缩后 |
|---|---|---|
| Adoptium 完整 JRE 25（windows-x64） | 180 MB | 58 MB |
| 本脚本 jlink 产出 | 39 MB | 25 MB |

- **jmods 要单独下载**：Temurin JDK 24 起启用 JEP 493，JDK 归档里不再带 `jmods/`，jlink 的 `--module-path` 没有现成来源。Adoptium 为每个平台单独发 jmods 包（约 85 MB），脚本从 `api.adoptium.net/v3/binary/latest/25/ga/{os}/{arch}/jmods/...` 取。所以 `+jre` 只在勾选时才付出这笔下载。
- 模块白名单是**实测**出来的（14 个模块，含 `jdk.httpserver` —— 桌面沙盒自己的 HTTP 宿主，和 `jdk.crypto.ec` —— TLS 必需）。验证方式：用产出的运行时真跑 `jvm-sandbox.jar` 并加载真实扩展。刻意排除 `java.desktop`（AWT/ImageIO，约 11 MB 压缩后）：扩展跑的是 Android API。白名单与理由都写在脚本注释里。
- `--include-locales=en,ja,zh` + `jdk.localedata`：只留这三种语言的 locale 数据。
- 脚本会**强制校验宿主平台 = 目标平台**，不一致直接报错退出（宁可让 CI 明确失败，也不产出一个"装上去就 UnsatisfiedLinkError"的运行时）。
- 不打包 JRE 的场合：默认包、Android。桌面端 Linux 基础包历来也不捆（可自行装系统 OpenJDK 或取 `+jre` 包）。

## 产物与捆绑

- **不再捆绑 Electron**：WebUI 桌面窗口由托盘经系统 WebView 打开（Win WebView2 / Linux WebKitGTK / macOS WKWebView），无 WebView 的环境托盘回退系统浏览器。
- 桌面壳（Tauri 托盘）：**所有桌面 target 都出**（Windows / Linux x64+arm64 / macOS x64+arm64），Android 由独立的 `android` job 出 APK、不带桌面壳。各平台都是原生 runner 编译（tray 有自己的 workspace 与 `target/`）。
- 扩展沙盒（`bin/jvm-sandbox.jar`）：**所有桌面 target 都带**（jar 是跨平台字节码，各 target 各自 gradle 构建）。server 跑扩展靠它，任何 target 都不能少。
- 不打包 JRE 的场合：默认包、Android。桌面端 Linux 基础包历来也不捆（可自行装系统 OpenJDK 或取 `+jre` 包），勾 `+jre` 即自带 `jre/`。

## Android 产物

- `build.yml` 的 `android` job：装满足 `compileSdk 37` 的 platform 与钉死版本的 NDK → `android/scripts/build-rust.sh` 交叉编译出 `libsuwayomi_android.so` → 把 WebUI zip 放进 assets → `./gradlew :app:assembleRelease` → 改名成上面的 APK 命名。
- **签名**：配了 `ANDROID_KEYSTORE_BASE64`（+ `_PASSWORD` / `_ALIAS` / `_KEY_PASSWORD`）就用它签；**没配则回退 AGP 的 debug key**，此时每次 CI 的 key 都不同，跨次覆盖安装前要先卸载（workflow 会打 `::warning::` 提示）。自用分发里"能装上"优先于"签名好看"。
- Android 侧的设计与阶段见 `docs/migration/ANDROID_IMPL.md`。

## 捆绑 WebUI

- 产物 zip 内自带 `version.txt`（server 只读它上报版本），发布说明同时标注 `bundled WebUI: r{code}`。
- 分流：alpha/beta → 最新构建（r{code} 预发布）；release → 最新正式 release。
- **所有 target 共用一个 URL**：在 prep job 解析一次、job outputs 复用（两次解析间隙 WebUI 推新构建会造成各架构包不一致）。

## 桌面壳 / 扩展沙盒构建注意

- Linux runner 直接执行的脚本必须 git mode `100755`（Windows 提交默认 100644 且 `core.filemode=false`，需 `git update-index --chmod=+x`）——`./gradlew` 曾因此 Permission denied。
- 桌面壳二进制经 `bash suwayomi-tray/build-tray.sh` 构建（普通 `cargo build --release`，不走 `tauri build`）；Windows 出 `suwayomi.exe`，Linux/macOS 无后缀。因此产物里是**裸可执行文件**，没有 macOS `.app` bundle / `.dmg`、也没有 Linux AppImage —— 需要这些得改成 `tauri build` 并补 iOS/打包依赖。
- Linux 上跑桌面壳要先装 webkit2gtk/appindicator 等系统依赖（CI 里那条 `apt-get install` 就是）；无图形会话时托盘自动降级为前台 server。
- macOS 上托盘进程设为 `ActivationPolicy::Accessory`（不占 Dock、不进 Cmd-Tab），与 Windows 托盘行为对齐。

## CI 改动的本地验证

改 workflow 不要靠推上去试错（一轮矩阵十几分钟还污染 release 列表）。用：

```bash
python .workbuddy/verify/ci_pack_check.py    # 基线包/+jre/Android/托盘与沙盒的验证（210 项）
python .workbuddy/verify/ci_equiv.py         # 上一轮"合并两个 workflow"的等价性对照
bash   .workbuddy/verify/check_jre_arch.sh   # make-jre.sh 的宿主探测 + 产物自检（35 项，按标记抽真代码）
bash   .workbuddy/verify/e2e_host_jmods.sh   # 「宿主自带 jmods 就跳过下载」分支的端到端（本地默认走不到）
```

做法（详见 `gh-actions-verify` 技能）：把 `run:` 块抽出来、按场景替换 `${{ }}`、外部 CLI 打桩、在最小的假仓库骨架里真跑，断言 `$GITHUB_OUTPUT` / 产物名 / 归档内容 / gh 的 `--notes`。

**但它验不到"脚本在真平台上会不会炸"**：打桩会把 `make-jre.sh` 之类跳过去，platform 专属代码路径（Windows 的 PE 分支、macOS 的 Mach-O 分支、Android 的 SDK 安装）在本地根本不会被执行。这类问题只能真跑 CI，或本地人为复现条件。**后者有两种做法**：

1. 复现**环境条件** —— 例如 `PYTHONIOENCODING=cp1252` 复现 Windows 的 Python 编码；`JAVA_HOME='C:\…'`（反斜杠形式）复现 CI 注入的路径形态。
2. 复现**分支条件** —— 有些分支在本地是死代码（`e2e_host_jmods.sh` 针对的就是它：本机 Temurin 25 按 JEP 493 不带 jmods，那条"宿主自带"分支永远走不到）。做法是造夹具：假 `JAVA_HOME` 用目录联接指向真 JDK、塞进真 jmods，再用 CI 那种变量形态调真脚本。

打桩的行为也要跟真实工具对齐 —— zip 布局那条断言就曾按错误模型写（`Compress-Archive -Path <dir>` 其实把目录本身收进归档），核对真实产物才发现。
