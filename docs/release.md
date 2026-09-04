# 发布流程与 CI 约定

面向维护者。CI（`.github/workflows/release*.yml`）只保留必要提示，决策与背景都在这里。

## 通道与版本

| 通道 | 触发 | versionName | versionCode | tag | prerelease |
|---|---|---|---|---|---|
| alpha | 推送 main/dev（release-alpha.yml） | `r{code}` | 提交数+3000 | `r{code}-alpha.{run_id}` | true |
| beta | 手动 release.yml | `3.{n/100}.{n%100 补零两位}` | 同上 | `v3.y.z-beta.{run_id}` | false |
| release | 手动 release.yml | `3.y.z`（同上规则） | 同上 | `v3.y.z` | false |

- `versionCode = commit count + 3000`。
- 3.y.z 的末两位**必须补零**：tag 去非数字后要恰好等于 versionCode（`tag_to_num` 纯数字比大小），`3.2.5`→325 会小于 r3205 被判旧。
- `aboutServer.buildType` 走编译期 `SUWAYOMI_BUILD_TYPE`（build.rs 生成常量），运行时读不到 CI 变量，勿改成 env 读取。
- beta/release 共用 3.y.z 版本名，故产物文件名保留通道段：`Suwayomi-{VER}-{CH}-{TGT}`；alpha 产物无通道段。
- release 同名 tag 已存在时先 `gh release delete --cleanup-tag`；beta/alpha tag 天然唯一不删。

## 产物与捆绑

- 命名统一 `Suwayomi-` 前缀。无后缀=基础包；`+jre` = 捆绑 Temurin JRE 25（`jre/`，仅 Windows 手动通道可选）。
- **不再捆绑 Electron**：WebUI 桌面窗口由托盘经系统 WebView 打开（Win WebView2 / Linux WebKitGTK），无 WebView 的环境托盘回退系统浏览器。
- 扩展沙盒（`bin/jvm-sandbox.jar`）：带桌面壳的产物都产（Windows 全支持；Linux 仅 x64，arm64 只发 server）。jar 是跨平台字节码，一次 gradle 构建共用。
- `oliphaunt-runtime/resources`：全平台捆绑（编译期 OUT_DIR 在 runner 上，运行时回退 exe 上级 bundled 目录，见 `suwayomi-core/src/db/manager.rs`），缺目录 server 秒退。
- Linux 跑扩展需要 JRE：linux-x64 alpha 包自带 `jre/`；手动通道 Linux 基础包不捆 JRE（自行装系统 OpenJDK 或选 alpha 产物）。

## 捆绑 WebUI

- 产物 zip 内自带 `version.txt`（server 只读它上报版本），发布说明同时标注 `bundled WebUI: r{code}`。
- 分流：alpha/beta → 最新构建（r{code} 预发布）；release → 最新正式 release。
- **所有 target 共用一个 URL**：在 prep job 解析一次、job outputs 复用（两次解析间隙 WebUI 推新构建会造成各架构包不一致）。

## 桌面壳 / 扩展沙盒构建注意

- Linux runner 直接执行的脚本必须 git mode `100755`（Windows 提交默认 100644 且 `core.filemode=false`，需 `git update-index --chmod=+x`）——`./gradlew` 曾因此 Permission denied。
- 桌面壳二进制经 `bash suwayomi-tray/build-tray.sh` 构建；Windows 出 `suwayomi.exe`，Linux/macOS 无后缀。
