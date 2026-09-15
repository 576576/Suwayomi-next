# 发布流程与 CI 约定

面向维护者。CI（`.github/workflows/release.yml` + `build.yml`）只保留必要提示，决策与背景都在这里。

## CI 结构

| 文件 | 角色 |
|---|---|
| `build.yml` | **可复用构建工作流**（只由 `workflow_call` 触发）：算好参数后由它编译 + 打包全部 target，产物用 `upload-artifact` 上传。所有平台的构建逻辑只有这一份。 |
| `release.yml` | **唯一入口**：推送 main/dev → 自动 alpha；手动 dispatch → alpha/beta/release。负责算版本号、解析 WebUI 制品，然后 `uses: ./.github/workflows/build.yml` 构建，再用 `download-artifact` 收产物发布 Release。 |

- 产物约定分两套，由 `build.yml` 的 `pack_mode` 表达（调用方按触发方式传入）：
  `channel` = 手动发布（产物名带通道段，JRE 可选另出 `+jre`）；`alpha` = 自动构建（产物名固定 `+jre`，两平台都捆 JRE）。
- 因此 `build.yml` 里那些"看着多余"的分支（例如 `pack_mode` 的两条命名路径）**不要随手合并**——两条路径各自对应一个历史产物约定，产物名是用户可见的。
- 手动触发的 run 标题本应由 prep 里那段 `curl PATCH` 改成 `Release {VER}`，但该请求没有注入 `GITHUB_TOKEN`（恒 401 被 `|| true` 吞掉），实际一直是默认标题。合并 CI 时原样保留以求行为一致；要修就补 `env: GH_TOKEN: ${{ github.token }}`（会让 run 标题开始变化，属于行为变更）。
- 合并前是 `release.yml`（手动）+ `release-alpha.yml`（推送自动，workflow 名 `Auto build`）；合并后 Actions 侧边栏里两者的 workflow 名统一显示为 `Release`，推送触发的 run 标题仍是提交信息（默认行为，未变）。

## 通道与版本

| 通道 | 触发 | versionName | versionCode | tag | prerelease |
|---|---|---|---|---|---|
| alpha | 推送 main/dev（自动） | `r{code}` | 提交数+3000 | `r{code}-alpha.{run_id}` | true |
| alpha | 手动 release.yml | `r{code}` | 同上 | `r{code}-alpha.{run_id}` | true |
| beta | 手动 release.yml | `3.{n/100}.{n%100 补零两位}` | 同上 | `v3.y.z-beta.{run_id}` | false |
| release | 手动 release.yml | `3.y.z`（同上规则） | 同上 | `v3.y.z` | false |

- `versionCode = commit count + 3000`。
- 3.y.z 的末两位**必须补零**：tag 去非数字后要恰好等于 versionCode（`tag_to_num` 纯数字比大小），`3.2.5`→325 会小于 r3205 被判旧。
- `aboutServer.buildType` 走编译期 `SUWAYOMI_BUILD_TYPE`（build.rs 生成常量），运行时读不到 CI 变量，勿改成 env 读取。
- beta/release 共用 3.y.z 版本名，故产物文件名保留通道段：`Suwayomi-{VER}-{CH}-{TGT}`；alpha 产物无通道段。
- release 同名 tag 已存在时先 `gh release delete --cleanup-tag`；beta/alpha tag 天然唯一不删。

## 产物与捆绑

- 命名统一 `Suwayomi-` 前缀，两套约定见上表 `pack_mode`。手动通道（`channel`）：`Suwayomi-{VER}-{CH}-{TGT}` 是基础包，勾选 JRE 时 Windows 再额外出一个 `+jre` 包（独立文件，基础包不变）；自动构建（`alpha`）：`Suwayomi-{VER}-{TGT}+jre`，Windows/Linux 都自带 `jre/`。
- **不再捆绑 Electron**：WebUI 桌面窗口由托盘经系统 WebView 打开（Win WebView2 / Linux WebKitGTK），无 WebView 的环境托盘回退系统浏览器。
- 扩展沙盒（`bin/jvm-sandbox.jar`）：带桌面壳的产物都产（Windows 全支持；Linux 仅 x64，arm64 只发 server）。jar 是跨平台字节码，一次 gradle 构建共用。
- Linux 跑扩展需要 JRE：linux-x64 alpha 包自带 `jre/`；手动通道 Linux 基础包不捆 JRE（自行装系统 OpenJDK 或选 alpha 产物）。

## 捆绑 WebUI

- 产物 zip 内自带 `version.txt`（server 只读它上报版本），发布说明同时标注 `bundled WebUI: r{code}`。
- 分流：alpha/beta → 最新构建（r{code} 预发布）；release → 最新正式 release。
- **所有 target 共用一个 URL**：在 prep job 解析一次、job outputs 复用（两次解析间隙 WebUI 推新构建会造成各架构包不一致）。

## 桌面壳 / 扩展沙盒构建注意

- Linux runner 直接执行的脚本必须 git mode `100755`（Windows 提交默认 100644 且 `core.filemode=false`，需 `git update-index --chmod=+x`）——`./gradlew` 曾因此 Permission denied。
- 桌面壳二进制经 `bash suwayomi-tray/build-tray.sh` 构建；Windows 出 `suwayomi.exe`，Linux/macOS 无后缀。
