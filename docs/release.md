# 发布流程与 CI 约定

面向维护者。CI（`.github/workflows/release.yml` + `build.yml`）只保留必要提示，决策与背景都在这里。

## CI 结构

| 文件 | 角色 |
|---|---|
| `build.yml` | **可复用构建工作流**（只由 `workflow_call` 触发）：算好参数后由它编译 + 打包全部 target，产物用 `upload-artifact` 上传。两个 job：`build`（桌面/服务端矩阵）与 `android`（APK）。所有平台的构建逻辑只有这一份。 |
| `release.yml` | **唯一入口**：推送 main → 自动 alpha；手动 dispatch → alpha/beta/release。负责算版本号、解析 WebUI 制品，然后 `uses: ./.github/workflows/build.yml` 构建，再用 `download-artifact` 收产物发布 Release。 |

- 产物约定分两套，由 `build.yml` 的 `pack_mode` 表达（调用方按触发方式传入）：
  `channel` = 手动发布（产物名带通道段，形态由 `pack_core` / `pack_jre` 决定）；
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

| 开关 | runner | rust target | jlink 目标 |
|---|---|---|---|
| `build_windows_x64` | `windows-latest` | `x86_64-pc-windows-msvc` | windows/x64 |
| `build_linux_x64` | `ubuntu-latest` | `x86_64-unknown-linux-gnu` | linux/x64 |
| `build_linux_arm64` | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | linux/aarch64 |
| `build_macos_x64` | `macos-15-intel` | `x86_64-apple-darwin` | mac/x64 |
| `build_macos_arm64` | `macos-15` | `aarch64-apple-darwin` | mac/aarch64 |
| `build_android_arm64` | `ubuntu-latest` | `aarch64-linux-android` | —（Android 不打包 JRE） |

- **每个 target 的 runner 必须与目标同架构**：`+jre` 用 jlink 生成，而 **jlink 不能跨平台生成运行时**（实测：Windows 的 jlink + linux-aarch64 的 jmods，产出的 `bin/java` 是 PE 头加一堆 `.dll` —— launcher 与原生库取自宿主 JDK）。所以 linux-arm64 用 arm64 runner（原生编译，顺带不再需要交叉工具链），x64 的 macOS 用 `macos-15-intel`。
- `macos-13` 已被 GitHub 下线，x64 macOS 现为 `macos-15-intel`。
- 平台开关默认只勾 Windows x64 + Linux x64。

## 产物形态（`-core` / `+jre`）

手动 dispatch 还有两个**互相独立、可同时勾选**的形态开关（不是二选一）：

| 开关 | 默认 | 含义 |
|---|---|---|
| `pack_core` | ✅ 勾选 | `-core`：最小包，不打包 JRE。 |
| `pack_jre` | ⬜ | `+jre`：在 `-core` 的内容之上追加对应架构的 JRE（jlink 裁剪）。 |

命名：

| 模式 | `-core` | `+jre` |
|---|---|---|
| 手动（`channel`） | `Suwayomi-{VER}-{CH}-{TGT}-core.zip/.tar.gz` | `Suwayomi-{VER}-{CH}-{TGT}+jre.zip/.tar.gz` |
| 自动（`alpha`） | —（自动构建不出 core 包） | `Suwayomi-{VER}-{TGT}+jre` |
| Android | `Suwayomi-{VER}[-{CH}]-android-arm64.apk` | —（跑系统 ART，不用 JRE） |

- 只勾 `+jre` 也可以：`+jre` 包本身就是完整包（`-core` 的全部内容 + `jre/`）。
- 两个都不勾会被 prep 拦下并报错（桌面/服务端目标会没有任何产物）。
- 只勾 Android 时桌面矩阵为空数组，`build` job 直接跳过。
- 归档格式：Windows 出 `.zip`，其余出 `.tar.gz`。
- **归档布局的既有差异**：Linux/macOS 用 `tar -C dist`，包内有顶层目录名（`Suwayomi-…/bin/…`）；Windows 用 `Compress-Archive`，目录**内容**直接进 zip 根（`bin/…`）。合并 CI 之前就是这样，本轮没动；`.workbuddy/verify/ci_pack_check.py` 里对这个差异有显式断言，将来想统一时先看那条用例。

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
- 不打包 JRE 的场合：`-core`、Android。桌面端 Linux 基础包历来也不捆（可自行装系统 OpenJDK 或取 `+jre` 包）。

## 产物与捆绑

- **不再捆绑 Electron**：WebUI 桌面窗口由托盘经系统 WebView 打开（Win WebView2 / Linux WebKitGTK），无 WebView 的环境托盘回退系统浏览器。
- 扩展沙盒（`bin/jvm-sandbox.jar`）：带桌面壳的产物都产（Windows 全支持；Linux 仅 x64，arm64 只发 server）。jar 是跨平台字节码，各 target 各自 gradle 构建。
- Linux 跑扩展需要 JRE：勾 `+jre` 即自带 `jre/`。

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
- 桌面壳二进制经 `bash suwayomi-tray/build-tray.sh` 构建；Windows 出 `suwayomi.exe`，Linux/macOS 无后缀。

## CI 改动的本地验证

改 workflow 不要靠推上去试错（一轮矩阵十几分钟还污染 release 列表）。用：

```bash
python .workbuddy/verify/ci_pack_check.py    # 本轮 -core/+jre/Android 的验证（171 项）
python .workbuddy/verify/ci_equiv.py         # 上一轮"合并两个 workflow"的等价性对照
```

做法（详见 `gh-actions-verify` 技能）：把 `run:` 块抽出来、按场景替换 `${{ }}`、外部 CLI 打桩、在最小的假仓库骨架里真跑，断言 `$GITHUB_OUTPUT` / 产物名 / 归档内容 / gh 的 `--notes`。
