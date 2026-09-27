# 发布流程与 CI 约定

面向维护者。CI（`.github/workflows/release.yml` + `build.yml`）只保留必要提示，决策与背景都在这里。

## CI 结构

| 文件 | 角色 |
|---|---|
| `build.yml` | **可复用构建工作流**（只由 `workflow_call` 触发）：算好参数后由它编译 + 打包全部 target，产物用 `upload-artifact` 上传。两个 job：`build`（桌面/服务端矩阵）与 `android`（APK）。所有平台的构建逻辑只有这一份。 |
| `release.yml` | **唯一入口**：推送 main → 自动 alpha；手动 dispatch → alpha/beta/release。负责算版本号、解析 WebUI 制品，然后 `uses: ./.github/workflows/build.yml` 构建，再用 `download-artifact` 收产物发布 Release。 |
| `clear.yml` | **预发布清理**（只手动 dispatch）：按「每 N 小时窗口内只留最新的 1 个预发布」删掉多余的 alpha Release，见「预发布清理」。 |

- 产物形态由五个正交开关表达（`pack_core` / `pack_jre` / `pack_msi` / `pack_exe` / `pack_oci`，见「产物形态」）。**推 main 的自动 alpha 与手动 dispatch 的默认值完全一致**（core 关、jre 开、msi 开、exe 关、oci 关），所以两条路径出的包一样，不必再按触发方式分叉。
- 产物名统一是 `Suwayomi-{VER}{通道段}-{TGT}[+jre]`（Windows 另出 `.msi` 与 `-setup.exe`），**通道段只有 beta 非空**（`-beta`）：beta 与 release 共用 3.y.z 版本名，不区分就会重名；alpha 的 `r{code}` 本身已表明通道。规则在 prep 里算一次（`channel_suffix`），`build.yml` 只负责拼 —— 别在 `build.yml` 里重新推导一遍。
- Android 的 ABI 矩阵同理由 prep 拼好（`android_targets`），`build.yml` 的 `android` job 直接吃 `include`。
- 手动触发的 run 标题本应由 prep 里那段 `curl PATCH` 改成 `Release {VER}`，但该请求没有注入 `GITHUB_TOKEN`（恒 401 被 `|| true` 吞掉），实际一直是默认标题。合并 CI 时原样保留以求行为一致；要修就补 `env: GH_TOKEN: <github.token>`（会让 run 标题开始变化，属于行为变更）。
- Actions 侧边栏里手动与推送两个触发源共用 `Release` 这个名字；推送触发的 run 标题仍是提交信息。
- **`on: push` 只跟 main**：本仓库只有 `main` 一条长期分支（原先的集成分支 `dev` 已删除，两者历史一致，不留分叉）。
- **纯文档改动不出包**：`push` 上配了 `paths-ignore: ['docs/**', '*.md', '**/*.md']`。一次桌面构建约 12 分钟、还会多出一个 alpha Release，而文档改动对产物没有影响。**只要改动里还有一个非文档文件就照跑**（paths-ignore 只在"全部改动都命中"时才跳过），所以"文档 + 代码"混在一起提交不会漏发布。手动 dispatch 不受影响。
  - 写成 `paths-ignore` 而**不是**顶层 `paths:` —— 后者是白名单语义，会把所有代码改动的 push 一起挡掉，而且完全静默。
  - `*.md` 与 `**/*.md` 两条都给：`**/` 能否匹配"零级目录"（即命中根目录的 `README.md`）在 glob 实现之间有歧义，两条并置后两种语义下都覆盖。
  - **前提**：`docs/` 下除 9 个 `.md` 外只有 `graphql/schema-baseline.graphql`（GraphQL 兼容对照物，没有 CI 步骤消费）；没有任何构建脚本或 Rust 源码把 `.md` 当输入读，打包步骤也不收 md。
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
- beta/release 共用 3.y.z 版本名，故产物名与镜像标签都保留通道段：`{VER}[-beta]`；alpha/release 不带。
- release 同名 tag 已存在时先 `gh release delete --cleanup-tag`；beta/alpha tag 天然唯一不删。

## 构建目标与 runner

手动 dispatch 的平台开关与对应的 runner（`release.yml` 里的 mapping）。**行序 = 触发页面上的输入框顺序**：先 Windows/Linux，再 Android，最后 macOS，形态开关垫底（顺序固定为 `pack_core` → `pack_jre` → `pack_msi` → `pack_exe` → `pack_oci`）；同族内一律 x64 在前、arm64 在后。

| 开关 | runner | rust target | 取的 JRE 资产 |
|---|---|---|---|
| `build_windows_x64` | `windows-latest` | `x86_64-pc-windows-msvc` | windows/x64 |
| `build_windows_arm64` | `windows-11-arm` | `aarch64-pc-windows-msvc` | windows/aarch64 |
| `build_linux_x64` | `ubuntu-latest` | `x86_64-unknown-linux-gnu` | linux/x64 |
| `build_linux_arm64` | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | linux/aarch64 |
| `build_android_x64` | `ubuntu-latest` | `x86_64-linux-android` | —（Android 不打包 JRE） |
| `build_android_arm64` | `ubuntu-latest` | `aarch64-linux-android` | —（同上） |
| `build_macos_x64` | `macos-15-intel` | `x86_64-apple-darwin` | mac/x64 |
| `build_macos_arm64` | `macos-15` | `aarch64-apple-darwin` | mac/aarch64 |
| `pack_msi` | 仅 Windows 的 `build` job | 同上 | 同 target（装的就是 `+jre` 那份） |
| `pack_exe` | 同上（与 `pack_msi` 共用一个步骤） | 同上 | 同 target（内容与 msi 完全一致） |
| `pack_oci` | 见下 | `linux/amd64` / `linux/arm64` | linux/x64 与 linux/aarch64（镜像里按 `uname -m` 各取对应那份） |

- **每个 target 的 runner 都是原生同架构**：Rust 二进制在本平台原生编译（linux-arm64 用 arm64 runner，顺带不再需要交叉工具链；桌面壳在 Suwayomi-tray 那边同样由原生 runner 出）。以前这里还有第二条理由 —— `+jre` 的 jlink 不能跨平台生成运行时；JRE 搬到 Suwayomi-ext-runtime 后这条约束不再落在本仓库，但"原生编译"本身仍然值得保留。
- `macos-13` 已被 GitHub 下线，x64 macOS 现为 `macos-15-intel`；Windows arm64 用 `windows-11-arm`（公开预览，公共仓库免费不限量），该镜像自带 VS 2022 + Windows SDK 26100（`build.rs` 嵌图标要的 `rc.exe`）与 Git for Windows，`shell: bash` 可直接用。
- **判定平台一律用 `runner.os`，不要拿矩阵 runner 标签比字面量**：Windows 的标签不止 `windows-latest`（现在还有 `windows-11-arm`），`matrix.os == "windows-latest"` 这类写法在加第二个 Windows target 时必然踩空。
- **`+jre` 相关的三个坑随脚本搬走了**：`windows-11-arm` 上 `uname -m` 会撒谎（Git for Windows 是 x64 版）、`+jre` 的入口/出口两道平台闸、`JAVA_HOME` 为 Windows 形式时反斜杠在 bash glob 里当转义符。这些都只影响 `make-jre.sh`，现在记在 Suwayomi-ext-runtime 的 `docs/EXTRACTION_RECORD.md`。
- **矩阵里每个桌面 target 都带托盘壳与 `bin/ext-runtime.jar`**（Windows x64+arm64 / Linux x64+arm64 / macOS x64+arm64）。两者都不再在本仓库编译：托盘壳从 Suwayomi-tray 的 Release 下载对应 target 的二进制（见「桌面壳从哪来」），所以本仓库的 Linux runner 不再装 webkit2gtk/appindicator 那套系统依赖。Android 不在这个矩阵里。
- 平台开关默认只勾 Windows x64 + Linux x64，发布通道默认 `alpha`。

## 产物形态（`pack_core` / `pack_jre` / `pack_msi` / `pack_exe` / `pack_oci`）

五个形态开关都与平台开关正交，**默认值与自动 alpha 逐项一致**（下表就是 dispatch 页面上的默认勾选）：

| 开关 | 默认 | 含义 |
|---|---|---|
| `pack_core` | ⬜ | 核心包：server + 托盘壳 + 沙盒 jar + WebUI，**不含 JRE**。 |
| `pack_jre` | ☑ | `+jre` 包：在核心包内容之上追加对应架构的 JRE（从 Suwayomi-ext-runtime 下载的 jlink 裁剪产物）。 |
| `pack_msi` | ☑ | 仅 Windows：出 `.msi` 安装包，装的是 `+jre` 那份内容（见「Windows 安装包」）。 |
| `pack_exe` | ⬜ | 仅 Windows：出 `-setup.exe` 安装包（Burn bundle，把 msi 裹成单文件），内容与 msi 完全一致。 |
| `pack_oci` | ⬜ | OCI 镜像（见「OCI 镜像」）。推 GHCR，**不进 Release 附件**。 |

- **`pack_core` 与 `pack_jre` 是两个正交开关，谁都不隐含谁**：早先「出不出基线包」由 `pack_mode` 隐式决定，现在要显式给。**两者都关就没有可发布的包** —— `build.yml` 对每个 target 都会 `::error::`，所以 prep 里预先拦一道（选了桌面目标却两种包都关时直接失败，别让每个 target 白跑一遍编译）。
- **`pack_msi` 与 `pack_exe` 也互不隐含**：两者装的都是 `+jre` 那份内容，所以**任一开着就必须同时开 `pack_jre`**（prep 里拦）。默认只勾 msi —— setup.exe 是给「想双击一个文件装完」的用户，对发布者来说多一个 80 MB 附件。
- **`pack_exe` 单独勾选也合法**：`setup.exe` 是 msi 的 Burn 外壳，`build.yml` 里无论勾不勾 `pack_msi` 都会先把 msi 构建出来（ICE 也照跑），只是 msi 本身进不进 Release 附件只看 `pack_msi`。

命名规则（**形态一律不进文件名**，不带后缀的那份就是核心包）：

| 形态 | 产物名 |
|---|---|
| 核心包 | `Suwayomi-{VER}[-beta]-{TGT}.zip`（Windows）/ `.tar.gz`（其余） |
| `+jre` | 同左，stem 追加 `+jre`：`Suwayomi-{VER}[-beta]-{TGT}+jre.zip` |
| 安装包 | `Suwayomi-{VER}[-beta]-{TGT}.msi` / `Suwayomi-{VER}[-beta]-{TGT}-setup.exe` |
| Android | `Suwayomi-{VER}[-beta]-android-arm64.apk` / `-android-x64.apk`（跑系统 ART，不用 JRE） |
| OCI | 镜像 tag `{VER}[-beta]`，只推 GHCR、不进附件 |

- 自动 alpha（推 main）固定 `windows-x64 + linux-x64`，所以它出的就是这两份 `+jre` 包，外加 Windows 的 msi（`pack_exe` 默认关，所以不出 setup.exe）。
- 只勾 Android 时桌面矩阵为空数组、`build` job 直接跳过；**只勾 OCI 时两个矩阵都空**，Release 会没有任何附件 —— 这是允许的，`publish` 里的附件列表用数组拼（裸 `artifacts/*` 在空目录下不展开，会把那个字面量当文件名传给 `gh`）。
- 归档格式：Windows 出 `.zip`，其余出 `.tar.gz`。**两者的归档布局一致**，都带顶层目录名（`Suwayomi-…/bin/…`）—— 用真实产物核对过：Windows 是 `Compress-Archive -Path <目录>`（会把目录本身收进归档），Linux/macOS 是 `tar -C dist`。将来想统一时先核对真实产物，别信直觉（本地打桩曾按错误模型写过这条断言）。
- 附件列表**只收 `Suwayomi-*`**，不是收 `artifacts/` 下所有文件：`download-artifact` 不区分来源，CI 内部的 artifact 也会一并拉下来 —— 实测 `docker/build-push-action` 的 `cache-to: type=gha` 就以上传缓存的形式产出 `~<owner>~<repo>~<hash>.dockerbuild`，被下载后名字里的 `~` 变 `.`、且**不含 `-` 分隔符**（早先按"名字里有没有 `-`"过滤根本挡不住），r3226 的两个 `.dockerbuild` 就这么混进了 Release。加过滤时按**正向白名单**写，别按黑名单。这条与 `download-artifact` 的 `merge-multiple: true` 是**成对约束**：`merge-multiple` 去掉后每个 artifact 会进各自子目录，扁平循环里的 `[ -f "$f" ]` 会把产物全判成目录跳过 → Release 零附件（打桩 harness 直接造文件，测不到这条路，所以脚本里做了结构断言）。

## Windows 安装包（`pack_msi` / `pack_exe`）

用 **WiX Toolset v7** 打两种壳，在同一个 `build` 步骤里（`packaging/windows/`）：

| 文件 | 出什么 | 开关 | 说明 |
|---|---|---|---|
| `Suwayomi.wxs` | `<BASE>.msi` | `pack_msi`（默认 ☑） | payload 是 `dist/<BASE>+jre/` 整棵树（846 个文件 / 79 MB），外加开始菜单快捷方式。 |
| `Suwayomi.Bundle.wxs` | `<BASE>-setup.exe` | `pack_exe`（默认 ⬜） | Burn bundle，链里只有上面那个 msi，UI 用 WixStdBA 的 `hyperlinkLicense` 主题。 |

- **两个开关互不隐含，但 exe 的构建隐含 msi 的构建**：那个步骤的 `if` 是 `(pack_msi || pack_exe)`，进去先无条件打 msi + 跑 ICE，再按 `pack_exe` 决定要不要裹成 bundle；最后进附件的列表按开关拼（`pack_msi=false` 时 msi 只是构建出来给 bundle 用，不发）。

- **安装范围是「默认用户目录 + 向导可选」**：`Package/@Scope="perUserOrMachine"`（ALLUSERS=2 + MSIINSTALLPERUSER=1）。默认装 `%LOCALAPPDATA%\Programs\Suwayomi`，管理员在向导里能改选「所有用户」。配 `<SetDirectory Id="INSTALLFOLDER" Value="[PerUserProgramFilesFolder]Suwayomi" Condition="MSIINSTALLPERUSER" />` 让路径跟着范围走 —— 别写死 `ProgramFiles64Folder`，普通用户对它没有写权限。
- **msi 只装程序、不带数据目录**：托盘的数据目录解析是「设置里的 `data_dir` 优先，否则 `base_dir()/data`」。装进 `Program Files` 后普通用户对那里没有写权限，首次启动会失败；而把数据塞进用户目录又会和绿色版两份数据打架。所以安装包就是「换个地方解压 + 建快捷方式」，数据目录仍按用户原来的习惯走（首次启动时托盘自己按可写位置建）。
- **版本号必须是数字点分**：`major < 256`、`minor < 256`、`build < 65536`。alpha 的 `r{code}` 与 `versionCode` 都不合法（`ICE24`），所以跟 beta/release 同款取 `3.$((COUNT/100)).$((COUNT%100))`。
- **两条硬约束**（都踩过）：
  - 产物**不能叫 `setup.exe`**：WiX `WIX0388` —— Windows 会为这个名字加载兼容性 shim，可被 DLL 劫持。所以叫 `-setup.exe`。
  - `Files@Include` 里裸 `**` 是**相对 .wxs 所在目录**展开的，必须用命名 bindpath `!(bindpath.payload)\**`；而 `-b` 传相对路径同样以 .wxs 所在目录为基准 → 一律给 `pwd -W` 出来的绝对 Windows 路径。传错只会静默收进几个文件（当年 msi 只有 1.9 MB）。
- **扩展 id 与 NuGet 包名不一致**：包名是 `WixToolset.Bal.wixext`，包内 dll 与扩展 id 都是 `WixToolset.BootstrapperApplications.wixext`。用包名装 `extension list` 会显示 `(damaged)`。另外 `-g`/`--global` 的短名是 `-g`，没有 `-global`。
- **v7 强制 OSMF EULA**：所有 `wix` 子命令都要 `--acceptEula wix7`，漏了直接 `WIX7015` 失败。
- 静态校验跑在 CI 里：`wix msi validate`（ICE）。**ICE57 在这个场景是误报**（它没考虑 ALLUSERS），改用**广告快捷方式**（`Shortcut Advertise="yes"`，挂在托盘 exe 的 `File` 下）让 key path 落在 exe 上；快捷方式上再显式写 `Icon` 会触发 ICE50（扩展名要和 key file 一致），索性不写、由 Windows 从 exe 取图标。
- 本地产物核对手段：`wix burn extract` 看 bundle 里嵌了什么、`wix msi decompile` 数实际收集的 File、`wix msi validate` 跑 ICE。

## OCI 镜像（`pack_oci`）

- 与桌面矩阵**完全独立**的一份构建：`oci` job 自己从源码编 server、自己 gradle 打沙盒 jar、自己 jlink 一个 JRE，内容与桌面包基本一致，但**不含托盘壳**（容器里没有 GUI，也没有 webkit2gtk/appindicator，装进去只是个跑不起来的死文件）。
- 镜像名 `ghcr.io/<owner>/<repo>`，标签 = 产物名那套规则（`{VER}[-beta]`），另推 `-amd64` / `-arm64` 两个单架构标签；`oci_manifest` job 再用 `docker buildx imagetools create` 合成多架构 manifest 覆盖主标签。**两个架构各跑在同架构 runner 上**（`ubuntu-latest` / `ubuntu-24.04-arm`）：jlink 不能跨平台，用 QEMU 模拟只是把同一件错事做得更慢。
- **镜像不进 Release 附件**，所以发布说明里没有它的文字行 —— 它是下载架构表 **Linux 格末尾那枚 `OCI` 徽章**（`pack_oci` 勾上才出现），点进去是包页面。这是找到镜像地址的唯一入口。
- **权限链**：被调工作流的权限不能超过调用方 —— `release.yml` 的 `build` job 必须显式给 `packages: write`，`build.yml` 的 `oci` / `oci_manifest` 两个 job 也给同一组。漏了的表现是「build 时 push 403」，而且**只在勾了 OCI 的那次才暴露**。
- **推之前先冒烟**：`docker/build-push-action` 只 `load: true`，冒烟通过才 `docker push`。冒烟两段：① `--version` + `jre/bin/java -version` + `ldd` 查缺库 + 三件套（webui / 沙盒 jar / jre）在位；② 真起容器等 HTTP 有响应。第一段能抓到「缺 `libssl3t64`」这类问题 —— Linux 的 server 动态链接 `libssl.so.3`（`default-tls` 只对 android 换成 rustls），缺它连 `--version` 都起不来。
- `provenance: false`：多架构 manifest 由 imagetools 合成，混进 attestation 会让 index 里多出平台未知的条目。
- Dockerfile 的目录布局必须与 server 的路径解析约定一致（`bin/` 下时 `jre/` 在上一级）：`/opt/suwayomi/{bin/suwayomi-server, bin/ext-runtime.jar, jre, webui}`，数据在 `/data`。改动时以 `Dockerfile` 与本节的路径为准，本地用 `docker run` 真起一次确认三件套都在（OCI 冒烟的第一段就在做这件事）。

## 发布说明

**段落顺序固定**：`## 标题` → 手填说明 → 捆绑组件表 → 下载架构表；`--generate-notes` 的 changelog 由 GitHub 追加在这之后。

| 段 | 内容 |
|---|---|
| 标题 | `## Suwayomi {VER} · {通道}`。 |
| 手填说明 | 只有手动 dispatch 有（`release_notes` 输入）；自动 alpha 这一段恒为空。 |
| 捆绑组件表 | webui / ext-runtime / tray 三个仓库各自集成的版本，**版本号是指向该仓库 release 页面的链接**。 |
| 下载架构表 | 「Download based on your OS」，一行一个 OS，格内 shields.io 徽章直达附件。 |

- **两张表整体垫在最末**：三个捆绑版本号在产物 zip 里都看不到，是发布说明独有的信息；放末尾既不挡手填说明、又紧挨 changelog。
- UI 上是**单行**输入框，要分段就写字面量 `\n`，`publish` 里用 `printf '%b'` 还原（`\r` 直接去掉）。手填说明与两张表之间会补空行 —— 表格前必须空行，否则 markdown 表被当成上一段的延续行。

  | 捆绑组件 | 集成的版本 |
  |---|---|
  | Suwayomi-WebUI | [`r{code}`](https://github.com/{owner}/Suwayomi-WebUI/releases/tag/r{code}) |
  | Suwayomi-ext-runtime | [`{V}`](https://github.com/{owner}/Suwayomi-ext-runtime/releases/tag/v{V}) |
  | Suwayomi-tray | [`{V}`](https://github.com/{owner}/Suwayomi-tray/releases/tag/v{V})，解析不到时写「（本次未捆绑）」（不带链接） |

  三者取制品的通道口径一致（release 取最新正式、alpha/beta 取最新构建），但这只在**挑制品**时起作用 —— 表里只写解析到的 tag，不带「最新构建 / 最新正式」之类的通道描述。推送触发的自动 alpha 用同一套渲染。
- **链接地址不是手抄的**：三个解析步骤各自从资产 URL 反推（`https://github.com/{owner}/{repo}` + `/releases/tag/{tag}`），tag 直接取 URL 里那一段而不是重拼 `v{V}`（资产名里的版本号与 tag 不保证同形）。所以 fork 之后改了 `scripts/resolve-*.sh` 里的仓库 slug，链接会跟着走，不必再改 workflow。
- 下载架构表的行序固定 **Windows → Linux → Android → macOS**（同族内先 x64 再 arm64、每个架构内先安装包再核心包再 `+jre`）：

  | OS | 格内徽章（形态 + 架构） |
  |---|---|
  | Windows | `MSI-x64`、`Installer EXE-x64`、`ZIP-x64`、`ZIP-x64 +JRE`（勾了 arm64 就再来一组；`MSI` 要 `pack_msi`、`Installer EXE` 要 `pack_exe`） |
  | Linux | `tar.gz-x64`、`tar.gz-x64 +JRE`…，**勾了 `pack_oci` 时格末尾多一枚 `OCI` 徽章** |
  | Android | `APK-x64` / `APK-arm64` |
  | macOS | 同 Linux，图标换成 apple |

  - **行只按实际收到的附件渲染**（命名规则见 `build.yml`），所以表里不会出现下不下来的链接。**只勾 OCI 时没有任何附件，表里就只剩 Linux 一行**（那枚 OCI 徽章）—— 镜像不进附件，这枚徽章是找到它的唯一入口。
  - 格内顺序是**显式固定**的：附件的字典序恰好把 `+jre` 排在核心包前、`arm64` 排在 `x64` 前，照遍历顺序渲染格子会乱。安装包（`.msi` / `-setup.exe`）进表的时机也在这里 —— 它们不被当成「另一种后缀的便携包」，而是按 `_MSI` / `_EXE` 单独占键、插在同架构核心包之前。
  - `MSI` / `Installer EXE` 两枚统一用**靛蓝**（`4a4e8f`）与青蓝的 `ZIP` 区分，一眼能看出这两枚是「装上去的」而不是解压即用。`EXE` 单看太含糊（zip 里也有 exe），所以徽章上写全 `Installer EXE`（shields 的下划线渲染成空格）；它指的是 `-setup.exe`（Burn bundle）而不是裸 exe。
  - **Windows 徽章的图标是内嵌的**：simple-icons 因商标下架了 `windows`（`logo=windows` 静默失效，徽章只是少个图标，不报错），所以 `WIN_LOGO` 里塞了一份 base64 的自绘图标 —— **Win11 形状的等宽四格**（轴对齐正方形，无透视；早先那版是带透视的倾斜四格）。同样的原因，别把 Windows 徽章改回 `logo=windows`；Linux / macOS / Android / Docker 的 `logo=linux|apple|android|docker` 都还在。
  - **图标的 viewBox 留了 4 单位内边距**（`-4 -4 32 32`，字形占 75%）：shields.io 给所有 logo 的图标位恒为 14px，所以字形多大**只由它自己占 viewBox 的比例决定**。四格是实心块，满幅（`0 0 24 24`，100%）在 14px 里比线描图标显重，收一圈才与 `linux` / `apple` 那几枚齐平。改 `WIN_LOGO` 时别把 viewBox 改回满幅，`release_inputs_check.py` 有占比断言拦着。
  - `+JRE` 里的加号在 shields.io 的 URL 里要写 `%2B`（`_` 渲染成空格，所以徽章文字是 `x64 +JRE`）。徽章的 `alt` 是文件名 / 镜像地址，图挂了也能看出该下哪个。
- **没有「版本计数」输入框**：版本号一律由 `git rev-list --count HEAD` 推导（`versionCode = 计数 + 3000`）。早先那个可以手填覆盖计数的框已移除，避免产物名与真实提交数脱钩。

## 预发布清理（`clear.yml`）

推 main 的自动 alpha 每次提交都会多一个 Release，列表很快被 `r{code}-alpha.{run_id}` 淹没。`clear.yml`（workflow 名 `Clear Release`）负责回收，**只手动触发**，三个输入：

| 输入 | 默认 | 含义 |
|---|---|---|
| `window_hours` | `24` | 窗口长度（小时）：**每个窗口内只留最新的 1 个预发布**，其余删掉。 |
| `dry_run` | ☑ | 只打印待删列表、不真删。默认开着 —— 删除不可撤销（Release 连资产一起没），先预览一遍。 |
| `cleanup_tag` | ☑ | 删 Release 时一并删关联 tag（`gh release delete --cleanup-tag`）。alpha 的 tag 带 run_id、天然唯一，留着只是让 `git fetch --tags` 越来越慢。 |

- **只动 `prerelease=true` 的** —— 实际就是 alpha。正式版 `v3.y.z` 不在范围内；**beta 是 `prerelease=false`**（见「通道与版本」），所以它也不会被清理。
- **窗口从最新那条往回推，不是按自然日 / 整点切分**：保留最新的一条当锚点，往回凡是距锚点不足 `window_hours` 的都删，遇到早于锚点整整一个窗口的那条就保留并成为新锚点，如此往旧推进。按绝对时刻切会出「同一天推 5 次 → 0 点前后各留一条」的结果，这里不会。
- **删除顺序从最旧的往回删**：中途失败（tag 受保护等）时留下的是最新的那批，而不是把最近一次构建先删掉。单个失败只打 `::warning::`，不中断整轮。
- 最新的一条永远是锚点、不会被删 —— 即便仓库里全是预发布、GitHub 把 Latest 标在它身上也安全。
- ⚠️ **删除步必须能跑 git**：`gh release delete --cleanup-tag` 内部会用 git 删本地 tag，job 里没有 `actions/checkout` 时**每条都失败**（`failed to run git: fatal: not a git repository`，一个都删不掉）。所以第一个 step 是 `actions/checkout@v7` 且 `fetch-depth: 0`（浅克隆没有本地 tag，`git tag -d` 照样失败），删除步开头另有一道 `git rev-parse` 自查，缺仓库时直接红在那一步。
- 结果写进 job summary（保留 / 删除逐条列表 + 成功失败计数）。

本地验证（不打线上 Release 的主意）：

```bash
python .workbuddy/verify/workflows_check.py   # 四个 workflow：YAML 可解析 + 每个 run 块过 bash -n
python .workbuddy/verify/clear_dryrun.py      # gh 打桩 + 假 Release 列表，真跑两个 run 块（9 个场景）
python .workbuddy/verify/release_inputs_check.py  # 输入顺序 + 发行说明渲染（同样用 gh 打桩真跑）
```

三个脚本都要 `pyyaml`（没装会直接 `ImportError`）。`release_inputs_check.py` 比对的是
渲染后的整段说明，所以动发行说明排版时它是唯一能提前发现「表被 markdown 当成延续行」
这类问题的地方。

## JRE 裁剪（`+jre` 用）

**裁剪已搬到 Suwayomi-ext-runtime 执行**（`scripts/make-jre.sh` 随沙盒一起搬走了）。本仓库只按 `<V>-<os>-<arch>` 下载 `ext-runtime-jre-<V>-<os>-<arch>.tar.gz` 资产、解开即用；`build.yml` 里给 jlink 用的「安装 JDK」步骤与矩阵的 `jdk` 列都已删除。

为什么搬过去：这份 JRE 存在的唯一目的是跑 `ext-runtime.jar`，模块白名单完全由沙盒需求决定（`jdk.httpserver` 是沙盒自己的 HTTP 宿主、`java.prefs` 是共享源码里 `PersistentCookieStore` 用的）。白名单与沙盒代码必须同仓演进 —— 否则改沙盒时加了个模块，运行时会静默缺模块，且只在 `+jre` 包上表现为 `NoClassDefFoundError`。另外 jlink **不能跨平台编译**，那边用原生 runner 出六份资产正合适（本仓库的桌面矩阵与它解耦，不再需要"runner 必须与目标同架构"这条约束）。

| 形态 | 解压后 | 压缩后 |
|---|---|---|
| Adoptium 完整 JRE 25（windows-x64） | 180 MB | 58 MB |
| jlink 裁剪产出 | 39 MB | 25 MB |

- 六份资产 = 6 个 `(os, arch)`：`windows|linux|mac` × `x64|aarch64`。手动发版出齐；推 main 的自动 alpha 只出本仓库 `+jre` 包在用的两份（`windows-x64` / `linux-x64`）。代价是 `windows/aarch64` 那一格要单独处理（Adoptium 对该平台没发 JDK 25 的任何制品，jdk/jre/jmods 三端点全 404），换 Azul Zulu（其 `win_aarch64` 归档自带 `jmods/`）。换来的好处是消费侧不必判断"这个版本到底有没有 JRE 资产"。
- 模块白名单是**实测**出来的（14 个模块，含 `jdk.httpserver` —— 桌面沙盒自己的 HTTP 宿主，和 `jdk.crypto.ec` —— TLS 必需）。验证方式：用产出的运行时真跑 `ext-runtime.jar` 并加载真实扩展。刻意排除 `java.desktop`（AWT/ImageIO，约 11 MB 压缩后）：扩展跑的是 Android API。
- `--include-locales=en,ja,zh` + `jdk.localedata`：只留这三种语言的 locale 数据。
- **jmods 要单独下载**：Temurin JDK 24 起启用 JEP 493，JDK 归档里不再带 `jmods/`，jlink 的 `--module-path` 没有现成来源。Adoptium 为每个平台单独发 jmods 包（约 85 MB）。脚本会先探 `$JAVA_HOME/jmods/`，有就直接用、不再下载。
- 脚本**强制校验宿主平台 = 目标平台**，不一致直接报错退出（宁可让 CI 明确失败，也不产出一个"装上去就 `UnsatisfiedLinkError`"的运行时）。
- 不打包 JRE 的场合：核心包（`pack_core`）、Android。桌面端 Linux 核心包历来也不捆（可自行装系统 OpenJDK 或取 `+jre` 包）。
- 本地要复现裁剪：在 Suwayomi-ext-runtime 里 `bash scripts/make-jre.sh <windows|linux|mac> <x64|aarch64> <输出目录>`。

## 产物与捆绑

- **不再捆绑 Electron**：WebUI 桌面窗口由托盘经系统 WebView 打开（Win WebView2 / Linux WebKitGTK / macOS WKWebView），无 WebView 的环境托盘回退系统浏览器。
- 桌面壳（Tauri 托盘）：**所有桌面 target 都带**（Windows / Linux x64+arm64 / macOS x64+arm64），Android 由独立的 `android` job 出 APK、不带桌面壳。二进制由独立仓库 Suwayomi-tray 发布，本仓库按 target 下载（见「桌面壳从哪来」）。
- 扩展沙盒（`bin/ext-runtime.jar`）：**所有桌面 target 都带**，server 跑扩展靠它，任何 target 都不能少。它**不再由本仓库构建** —— 见下面「ext-runtime 从哪来」。
- 不打包 JRE 的场合：核心包（`pack_core`）、Android。桌面端 Linux 核心包历来也不捆（可自行装系统 OpenJDK 或取 `+jre` 包），勾 `pack_jre` 即自带 `jre/`；Windows 的 msi / setup.exe 装的就是 `+jre` 那份。

## ext-runtime 从哪来

桌面沙盒与 Android 扩展宿主共用的那份代码已经剥离到独立仓库 **`576576/Suwayomi-ext-runtime`**（`jvm-sandbox` 改名为 `ext-runtime`，原来的 `extension-runtime` 共享源码树并入其中）。本仓库**不再**持有任何一份源码，两条消费链都从它发布的 Release 资产取：

| 消费方 | 取的资产 | 落到哪 |
| --- | --- | --- |
| 桌面 / 服务端 / Docker | `ext-runtime-<V>.jar` | 各 target 产物的 `bin/ext-runtime.jar` |
| 桌面 `+jre` / Docker 运行镜像 | `ext-runtime-jre-<V>-<os>-<arch>.tar.gz` | 解开后就是包里的 `jre/` |
| Android `:extension-host` | `ext-runtime-<V>-shared-sources.jar` | 展开到 `android/build/ext-runtime-src`，作为源目录参与编译 |

三条都走 **Release 资产而不是 GitHub Packages**：两者发布的 jar 是同一份，但 Packages 即使对公开包也要求 token（跨仓库取还要单独配 PAT），Release 资产免鉴权 —— 反正都要先下载再展开（Gradle 没法把一个依赖直接当源目录），没必要为一个 secret 付出 PAT 过期导致 401 的风险。

- 解析脚本：`scripts/resolve-ext-runtime.sh`（默认取桌面 jar，加 `--sources` 取共享源码包），三级探测同 `resolve-webui.sh`。它同时吐一个 `base=`（该 release 的资产下载前缀）—— 同一版本下其余资产按 `<base>/<资产名>` 拼即可，不必为每种 `(os, arch)` 再探测一遍。Android 侧再包一层 `android/scripts/fetch-ext-runtime-src.sh`，下载 + 展开 + 校验三个包根齐全。
- Android **只能吃源码**：`:extension-host` 由 AGP 9 内置的 Kotlin **2.3.20** 编译，而 ext-runtime 用 Kotlin **2.4.0**，元数据版本不兼容，2.3 读不了 2.4 编出来的 class。
- 版本由 `release.yml` 的 prep 解析一次、经 `build.yml` 的 `ext_runtime_version`（+ `ext_runtime_jre_base`）传给所有 target，**同一批产物用的是同一个 ext-runtime 版本**。
- 版本号是 `<AOSP API level>.{提交数/100}.{提交数%100}`（如 `30.0.47`）：大版本跟着沙盒 pin 的 Android API 基线走，后两位是那个仓库自己的提交数（`versionCode = 提交数 + 1000`，规则同本仓库，只是基线不同）。
- 改沙盒的流程：在 Suwayomi-ext-runtime 改 → 推 main（自动出 alpha，只有 jar 与两份 JRE）或手动 dispatch release 通道（出齐全六份 JRE 并发 Packages）→ 回这边跑一次发布即生效（无需改本仓库代码）。
- **通道到这里是分岔的**：那边推 main 会自动出 alpha 预发布，所以本仓库正式发布只认非预发布版本（`resolve-ext-runtime.sh --stable`），alpha/beta 才跟最新构建（`--build`）。要这边的 release 包吃到新沙盒，那边得 dispatch 一次 release/beta 通道。

## Android 产物

- `build.yml` 的 `android` job 是**矩阵**：`build_android_arm64` / `build_android_x64` 各是一个 job，`ABI` 由矩阵给出（`arm64` → `arm64-v8a`，`x86_64` → `x86_64`，映射在 `android/scripts/build-rust.sh` 里）。步骤：装满足 `compileSdk 37` 的 platform 与钉死版本的 NDK → 交叉编译出 `libsuwayomi_android.so` → 把 WebUI zip 放进 assets → 取 ext-runtime 共享源码（见上节）→ `./gradlew :app:assembleRelease` → 改名成上面的 APK 命名。
- **x64 APK 只对模拟器有意义**（真机基本是 arm64）；两个都勾就是两份独立构建，互不影响。
- **签名**：配了 `ANDROID_KEYSTORE_BASE64`（+ `_PASSWORD` / `_ALIAS` / `_KEY_PASSWORD`）就用它签；**没配则回退 AGP 的 debug key**，此时每次 CI 的 key 都不同，跨次覆盖安装前要先卸载（workflow 会打 `::warning::` 提示）。自用分发里"能装上"优先于"签名好看"。
- Android 侧的设计与阶段见 `docs/migration/ANDROID_IMPL.md`。

## 捆绑 WebUI

- 产物 zip 内自带 `version.txt`（server 只读它上报版本），发布说明的「捆绑组件」表里列出具名版本 `r{code}`（同表还有 ext-runtime 与 tray，见「发布说明」）。
- 分流：alpha/beta → 最新构建（r{code} 预发布）；release → 最新正式 release。
- **所有 target 共用一个 URL**：在 prep job 解析一次、job outputs 复用（两次解析间隙 WebUI 推新构建会造成各架构包不一致）。

## 桌面壳从哪来

桌面壳（Tauri 托盘）已剥离到独立仓库 **`576576/Suwayomi-tray`**（原 `suwayomi-tray/` 子目录）。本仓库不再持有它的源码、也不再编译它 —— 打包时按 target 从它的 Release 资产取：

| 消费方 | 取的资产 | 落到哪 |
| --- | --- | --- |
| 六个桌面 target | `suwayomi-tray-<V>-<target>[.exe]` | 各 target 产物根目录的 `suwayomi` / `suwayomi.exe` |

- 解析脚本：`scripts/resolve-tray.sh`，三级探测同 `resolve-webui.sh`，同样吐 `base=`（该 release 的资产下载前缀）—— 六个 target 的资产都在同一个 release 里，prep 解析一次，各 target 按 `<base>/suwayomi-tray-<V>-<target>[.exe]` 取，不必逐个探测。
- **解析不到或下载失败只打 `::warning::`，不让发布失败**：没有托盘壳时 server 本身照样可用。这是刻意选的（托盘仓库 CI 挂掉不该阻塞 server 发布），代价是可能静默出一个不含桌面壳的包 —— 看构建日志里的 warning。
- 版本号由托盘仓库自己管（算法与本仓库同款：`versionCode = 提交数 + 1000`，版本名 `1.{提交数/100}.{提交数%100}`；三通道共用同一个版本名，差异在 tag）。**不与本仓库的 `r{code}` / `3.y.z` 对齐**：exe 的 PE 版本资源显示的是托盘自己的版本。
- 改托盘的流程：在 Suwayomi-tray 改 → 推 main（自动出 alpha，只有 windows-x64 + linux-x64 两份）或手动 dispatch release 通道（六份出齐）→ 回这边跑一次发布即生效（无需改本仓库代码）。
- **通道到这里是分岔的**：那边推 main 会自动出 alpha 预发布，所以本仓库正式发布只认非预发布版本（`resolve-tray.sh --stable`），alpha/beta 才跟最新构建（`--build`）。
- 为什么拆：托盘是独立 workspace + 494 个 crate 的 Tauri 依赖树，原先在每个 desktop target 的 job 里**串行**编译一次，托盘代码没变也照编。拆走后本仓库每次构建只下载几 MB，顺带省掉 Linux 那套 webkit2gtk/appindicator 系统依赖。

## 桌面壳的形态与行为

- 桌面壳在 Suwayomi-tray 用普通 `cargo build --release` 构建（不走 `tauri build`）；Windows 出 `suwayomi.exe`，Linux/macOS 无后缀。因此产物里是**裸可执行文件**，没有 macOS `.app` bundle / `.dmg`、也没有 Linux AppImage —— 需要这些得改成 `tauri build` 并补打包依赖。
- Linux 上跑桌面壳要先装 webkit2gtk/appindicator 等系统依赖（装在 Suwayomi-tray 的 CI 里）；无图形会话时托盘自动降级为前台 server。
- macOS 上托盘进程设为 `ActivationPolicy::Accessory`（不占 Dock、不进 Cmd-Tab），与 Windows 托盘行为对齐。
- Linux runner 上**直接执行**的脚本必须 git mode `100755`（Windows 提交默认 100644 且 `core.filemode=false`，需 `git update-index --chmod=+x`）——`./gradlew` 曾因此 Permission denied。用 `bash <script>` 调用的（如 `build-tray.sh`）不受此限。

## CI 改动的本地验证

改 workflow 不要靠推上去试错（一轮矩阵十几分钟还污染 release 列表）。用（都要 `pyyaml`，跑托管 venv 里的解释器）：

```bash
python .workbuddy/verify/workflows_check.py         # 四个 workflow：YAML 可解析 + 每个 run 块过 bash -n + 注释块 ≤ 1 行
python .workbuddy/verify/release_inputs_check.py    # 输入顺序 + prep 形态开关 + 发行说明渲染（真跑 prep 的四段 run 脚本，105 项）
python .workbuddy/verify/clear_dryrun.py            # gh 打桩 + 假 Release 列表，真跑 clear.yml 两个 run 块（9 个场景）
python .workbuddy/verify/notes_preview.py           # 拿真渲染结果生成 GitHub 风格的 HTML 预览（改排版时肉眼核对，含离线图标对照）
```

- `release_inputs_check.py` 不只看渲染：它把 prep 的 `out` 步骤与**三个解析步骤**（`bash scripts/resolve-*.sh` 打成同名桩脚本交给真 bash 跑）都真跑一遍，断言 **auto 的五个形态开关与 dispatch 默认值逐项一致**、两条路径的矩阵一致、三个开关的独立性（只勾 `pack_exe` 放行）、两道守卫（`pack_msi`/`pack_exe` 缺 `pack_jre`、两种包都关）真的会红、以及捆绑组件表那三个 release 链接地址确实是从资产 URL 反推的。
- `notes_preview.py` 是给人看的（不做断言）：把真渲染结果转成 HTML 摆在 `notes_preview.html` 里，含「全形态 + 手填说明」「自动 alpha」「只勾 OCI」三个场景 —— 改排版时比读 `--notes` 的字面量快得多。它开头还会把 `WIN_LOGO` 解出来内联：**14px 真实尺寸三格（新 / 新 / 满幅旧版当尺子）+ 56px 放大两格**，这样机器没网时也能看图标形状与大小（徽章本身是 shields.io 的远程图片）。
- 它比对的是**渲染后的整段说明**，所以动排版时它是唯一能提前发现「表被 markdown 当成延续行」这类问题的地方。
- 打桩验不到 WiX 那一步（`pack_msi` / `pack_exe`）：`.wxs` 的验证是把真 WiX 装在本机、拿真 payload 跑 `wix build` + `wix msi validate` + `wix burn extract`，见「Windows 安装包」。
- 平台专属代码路径（Windows 的 PE 分支、macOS 的 Mach-O 分支、Android 的 SDK 安装）在本地根本不会被执行 → 这类问题只能真跑 CI，或本地人为复现条件。

JRE 裁剪那两个脚本（`check_jre_arch.sh` 的宿主探测 + 产物自检、`e2e_host_jmods.sh` 的「宿主自带 jmods 就跳过下载」端到端）**已随 `make-jre.sh` 搬到 Suwayomi-ext-runtime**，在那边 `.workbuddy-ai/verify/` 下跑。

做法（详见 `gh-actions-verify` 技能）：把 `run:` 块抽出来、按场景替换 `${{ }}`、外部 CLI 打桩、在最小的假仓库骨架里真跑，断言 `$GITHUB_OUTPUT` / 产物名 / 归档内容 / gh 的 `--notes`。

**但它验不到"脚本在真平台上会不会炸"**：打桩会把真脚本之类跳过去，platform 专属代码路径（Windows 的 PE 分支、macOS 的 Mach-O 分支、Android 的 SDK 安装）在本地根本不会被执行。这类问题只能真跑 CI，或本地人为复现条件。**后者有两种做法**：

1. 复现**环境条件** —— 例如 `PYTHONIOENCODING=cp1252` 复现 Windows 的 Python 编码；`JAVA_HOME='C:\…'`（反斜杠形式）复现 CI 注入的路径形态。
2. 复现**分支条件** —— 有些分支在本地是死代码（`e2e_host_jmods.sh` 针对的就是它：本机 Temurin 25 按 JEP 493 不带 jmods，那条"宿主自带"分支永远走不到）。做法是造夹具：假 `JAVA_HOME` 用目录联接指向真 JDK、塞进真 jmods，再用 CI 那种变量形态调真脚本。

打桩的行为也要跟真实工具对齐 —— zip 布局那条断言就曾按错误模型写（`Compress-Archive -Path <dir>` 其实把目录本身收进归档），核对真实产物才发现。
