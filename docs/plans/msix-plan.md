# MSIX 方案（评估与计划）

> 本文是**调研结论 + 分阶段计划**，不含实现。所有外部约束标注了来源；第 7 节单独列出
> 「我没有实机验证过、动手前必须先验」的项。

## 0. 结论先行

MSIX 能出包、能自动更新、能上 Store，但它**在语义上替不掉现在的 msi 向导**：

- 安装位置**不可选**（固定落在系统 PackageVolume，默认 `C:\Program Files\WindowsApps\<包名>`）；
- 快捷方式**不可选**（由 manifest 声明，系统的开始菜单项用户改不了；桌面快捷方式 MSIX 原生就不支持）；
- 安装范围**不可选**（MSIX 是 per-user 部署）；msi 这边同样只有默认档，范围页也还没接上（2.1）。

前两条是当前 msi 向导存在的理由（见 `release.md` 的「Windows 安装包（`pack_msi` / `pack_exe`）」一节）。
所以 MSIX 的定位只能是**第三个渠道**——面向「不想做选择、只想点一下」的用户，以及将来的
Store / winget 上架；**不是 msi / setup.exe 的替代**。

真正的两个拦路虎，优先级从高到低：

1. **运行时往安装目录写数据**：MSIX 的安装目录只读且被锁死。原来的布局把
   `settings/` `db/` `cache/` `extensions/` `logs/` `data/` 全放在 exe 同级 → 直接挂。
   **server 侧已改造完**（四项收敛到 `appdata/` 一个可写根，见 2.1），剩下托盘决定这个根落在哪。
2. **签名**：MSIX 不签名就没法给普通用户装（`-AllowUnsigned` 只对开了开发者模式的机器有效，
   且带可执行内容的包还需要管理员）。**已定：走 SignPath Foundation 的免费 OSS 签名**
   （第 5 节）。选它的直接理由是许可证形态正好落在它的准入条件里 —— MPL-2.0 是 OSI 认可许可、
   全仓无商业双许可、产物里没有专有组件；代价是签名主体变成 `SignPath Foundation` 而不是本项目名。

签名路线定下来之后，挡在 MSIX 前面的就不再是「能不能签」，而是签名**自身带来的三条前置条件**：

- **PE 版本元数据**：SignPath 的 artifact configuration 强制要求被签的二进制带版本资源，且
  product name / product version 由构建统一注入。现状是托盘与 server 都停在 `0.1.0`（见 2.6）。
- **一份 "Code signing policy"**：条款要求项目主页与下载页上有这一节（见 5.4）。
- **人工审批 + 全程 MFA**：每次签名请求都要 Approver 在 SignPath 上点批准，发不出全自动流水线。

**路径**：第 1 项（运行时目录改造）与签名前置条件互不依赖，可以并行。签名本身先落在**已经发布过**
的 msi / setup.exe 上（它们本来就未签名，签了立刻消掉安装警告），再轮到 MSIX —— 因为准入条件里
有一条是「项目必须在**待签名的形态上**已经发布过」，MSIX 得先以未签名形态发一次。

---

## 1. MSIX 与 msi 的语义对照

| 维度 | 现在的 msi / setup.exe | MSIX |
|---|---|---|
| 安装位置 | 向导可选（默认 `%LOCALAPPDATA%\Programs\Suwayomi`） | **不可选**，落在 PackageVolume（默认 `C:\Program Files\WindowsApps\<包名>`）。管理员可以把卷建在别的盘，但那是机器级配置，不是安装时的选项 |
| 快捷方式 | 向导里一个总开关（桌面 + 开始菜单） | **不可选**。开始菜单项由 manifest 的 `<Application>` 声明、系统放置；桌面快捷方式**原生不支持**，只能靠 `desktop7:shortcut`（有已知坑，需要包内带 `.lnk` 且目标可解析）或 PSF 首次运行跑脚本创建 |
| 安装范围 | `perUserOrMachine`，但**向导里没有范围页**（2.1），实际只装得出 per-user | **per-user 部署**（`Add-AppxPackage`）。要机器级只能 `Add-AppxProvisionedPackage`，且只对之后新建的用户生效 |
| 安装目录可写 | 是（这就是数据放同级能跑的原因） | **否**，只读 + 被锁；改动包内文件会导致拒绝启动 |
| 卸载 | 控制面板 / 设置 → 应用 | 设置 → 应用（同机两个渠道 = 两个独立卸载项） |
| 更新 | 用户自己下新包 | 包 `App Installer`（`.appinstaller`）自动 + 差分更新；Store 渠道由 Store 管 |
| 修复 / 重置 | 维护模式里的「修复」 | 系统自带「重置」（不需要我们做界面） |
| 静默安装 | 需要 `/qn` | `Add-AppxPackage` 本身就是静默的 |
| 首次安装的管理员 | 向导提供升权入口 | 已签名的 per-user 安装**不需要**管理员 |
| 包体积 | 82.7 MB（msi）/ 83.1 MB（setup.exe） | 同量级；MSIX 还会做跨包硬链接去重 |

---

## 2. 硬约束：现状会撞到哪

### 2.1 安装目录只读 —— 服务端侧已解决，只剩「根落在哪」

MSIX 装完，包内文件只读且被 OS 锁死（防篡改）。所以可写的东西必须全部落在包外。
**这一步已经做完**：缓存 / 库 / 日志 / 设置 / 扩展五项收敛到**一个** `appdata/` 根之下，
目录级环境变量只剩 `SUWAYOMI_APPDATA_DIR` 一个。

```
<安装根>\  suwayomi.exe  bin\  jre\  webui\        ← 只读，留在包内
           appdata\                                 ← 唯一需要可写的根
             cache\  db\  logs\  settings\  extensions\{apk,bin}\
           data\  downloads\ autobackup\ local\      ← 用户数据，WebUI 里只改 `data\` 这个根
```

`webui\` 是只读用途（server 只是静态托管），留包内没问题。

**剩下的只有一件事**：托盘决定这个根落在哪。默认是 exe 同级的 `appdata/`，而 per-machine
（msi「所有用户」）与 MSIX 下那里不可写。定两个信息来源，**预置优先、探测兜底**：

1. **安装包预置**（per-machine 的 msi / setup.exe）：随包落一份
   `<安装根>\appdata\settings\tray.json`，把两个根直接指到用户目录（见下方）。
2. **写探测**（绿色版 zip / MSIX / 预置缺失时）：探 exe 同级能不能写，不能写就落
   `%LOCALAPPDATA%\Suwayomi`。

两条都收敛到同一个可写根，`webui\` 仍取包内；根定下来之后由 `server_env()` 显式传给 server。
用**运行期预置 + 写探测**而不是编译期开关，一份托盘产物同时适配 msi / 绿色版 / msix 三个渠道。

`Suwayomi-tray/src/main.rs` 现在的形态：

- `base_dir()` —— exe 同级，`data_dir_of()` 的兜底基于它；
- `appdata_dir()` —— `SUWAYOMI_APPDATA_DIR` 优先，否则 `base_dir()/appdata`；
  `settings_path()`（`<appdata>/settings/tray.json`）与 `logs_dir()`
  （`<appdata>/logs`）都由它派生；
- `server_env()` —— 显式传 `SUWAYOMI_APPDATA_DIR` / `SUWAYOMI_DATA_DIR` /
  `SUWAYOMI_WEBUI_DIR`（根既然能变，就必须显式传；下载 / 本地图源 / 自动备份都在
  data 根之下，没有各自的环境变量）。

#### 预置托盘设置文件（per-machine 的 msi / setup.exe）

安装包随包落一份 `<安装根>\appdata\settings\tray.json`，内容即两个根：

```json
{
  "appdataDir": "%LOCALAPPDATA%\\Suwayomi",
  "dataDir": "%USERPROFILE%\\Pictures\\Suwayomi"
}
```

- 源文件放 `packaging/windows/tray.preset.json`，由 WiX 组件安装到
  `[INSTALLFOLDER]appdata\settings\tray.json`，**条件 `ALLUSERS = 1`** —— per-user 安装下
  `<安装根>` 本身就是 `%LOCALAPPDATA%\Programs\Suwayomi`、可写，预置反而多一层。
- 路径**写环境变量、不写死绝对路径**：per-machine 安装由管理员执行，写死就等于把数据落到
  管理员的用户目录。`%VAR%` 由托盘在运行期展开，每个用户拿到自己的那两个目录。
- `data` 落在 `…\Pictures\Suwayomi` 而不是 `Pictures` 根：`downloads\` / `local\` /
  `autobackup\` 三棵子树不该散进用户的图片目录。
- 预置文件只是**默认值**：设置仍写 `<解析出的 appdata 根>\settings\tray.json`，用户改过之后
  以那份为准。`SettingsPatch` 带 `deny_unknown_fields`，所以旧托盘读到带 `appdataDir` 的
  预置文件会整份解析失败、回落全默认 —— 预置文件只随同一份产物发布，不单独投放给旧版本。
- 托盘因此要多两处能力，都还没做：
  1. `appdata_dir()` 在 `SUWAYOMI_APPDATA_DIR` 之外还能认预置文件里的 `appdataDir`。
     预置文件本身就放在默认位置（`<exe 同级>\appdata\settings\`），不存在「要知道根才能读到根」的循环；
  2. `appdataDir` / `dataDir` 支持 `%VAR%` 展开，展开后仍含未定义变量的值按未设置处理
     （否则会真的建出一个叫 `%FOO%` 的目录）。

**前提：现在装不出 per-machine。** 实测 `Suwayomi-3.2.13-windows-x64.msi` 的 `Dialog` 表里没有
`InstallScopeDlg`（只有 `InstallDirDlg` / `ShortcutDlg`），属性是 `ALLUSERS=2` +
`MSIINSTALLPERUSER=1` —— 向导**恒装 per-user**，per-machine 只能靠
`msiexec /i … MSIINSTALLPERUSER=""` 命令行。要走「msi 里选全局」，得先把官方那页「安装范围」
接进 `Suwayomi.UI.wxs` 的序列（`WixUI_zh-CN.wxl` 里 `InstallScopeDlg*` 的中文文案已齐全）。

**独立价值**：这条修好之后，per-machine 的 msi（装进 `C:\Program Files`）也一并受益 ——
`release.md` 里那条「普通用户对 Program Files 没有写权限、首次启动会失败」的坑会一起消失。
**所以它值得先做，与 MSIX 是否上无关。**

### 2.2 包标识与完整性

包内文件被改动 → Windows 拒绝启动并触发修复。所以**不能在打包后自替换文件**
（好在托盘没有 updater 插件，`Cargo.toml` 里没有 `tauri-plugin-updater`，不存在自更新冲突）。

### 2.3 不能预置防火墙规则、不能写 HKLM

- 本地服务监听 `127.0.0.1` **不受影响**：full trust 的打包应用不在 AppContainer 里，loopback 限制不适用。
  （将来若有人提议改成 AppContainer / 部分信任，**loopback 会被封**，需要额外豁免 —— 这是个否决项。）
- 要让局域网访问时，Windows 防火墙弹窗仍需用户点「允许」；包内**无法预置规则**（MSIX 不能写 HKLM 防火墙策略）。
- 现在的快捷方式组件用 `HKMU` 注册表值当 key path —— MSIX 版里这套整个不需要。

### 2.4 依赖：WebView2

托盘是 Tauri（wry），需要 WebView2 Runtime。Win11 自带；Win10 大概率有，但不保证。

声明方式（官方文档给的）是外部依赖：

```xml
<win32dependencies:ExternalDependency Name="Microsoft.WebView2"
  Publisher="CN=Microsoft Windows, O=Microsoft Corporation, L=Redmond, S=Washington, C=US"
  MinVersion="…" Optional="false" />
```

**注意**：这个外部依赖只有走 **App Installer** 安装时才解析（App Installer 去 winget 源拉），
而 App Installer 又要求包已签名。签名路线已定（第 5 节），所以这条可以用上；未签名的本地验证包里
加它不会生效。

### 2.5 身份与版本

- `<Identity Name>` 只能 `[A-Za-z0-9.\-]`，需要一个**包身份名**（与 `Package/@Id` 的 `Suwayomi.Next` 无关）；
- `Version` 必须是四段 `Major.Minor.Build.Revision`，每段 `0..65535`，且**新版本必须严格递增**。
  现有 `3.2.13` → `3.2.13.0`。注意 MSIX 的段上限是 65535，而现在 msi 用的是 `3.$((COUNT/100)).$((COUNT%100))`，
  这个映射仍然是安全的（COUNT 到 6.5M 才溢出 Build）。
- `Publisher` 必须与签名证书的 subject **完全一致**，否则拒绝安装。证书是 SignPath Foundation 的，
  所以这里写的是它的证书主体（不是本项目名，也不能照抄别的项目写 `CN=SignPath Foundation` ——
  完整 DN 要从实际签名请求回传的证书上取，见 7.6）。**包身份因此绑在签名渠道上**：将来若换成自家
  证书（Azure Trusted Signing / OV），`Publisher` 变了就是另一个应用身份，不能覆盖升级。

### 2.6 被签 PE 的版本元数据

SignPath 的 artifact configuration 会强制校验被签文件的元数据：每个二进制都要有版本资源，
`product name` 指向项目名、`product version` 在每次构建里是同一个值。**这两条现在都不满足**
（实测 2026-09-30 的 `Suwayomi-latest` 实例）：

| 文件 | ProductName | ProductVersion | 问题 |
|---|---|---|---|
| `suwayomi.exe` | `Suwayomi Tray` | `0.1.0` | 版本与发布版本无关；name 是 shell 名而非项目名 |
| `bin/suwayomi-server.exe` | `suwayomi-server` | `0.1.0` | 同上 |

两边的来源不同：托盘写在 `Cargo.toml` / `tauri.conf.json`（都是 `0.1.0`），server 没有注入任何
Windows 资源、拿的是 Cargo 包版本。要做的是**在构建期注入**（托盘走 `tauri-build` 的
`WindowsAttributes`，server 走 `build.rs` 的 Windows 资源），值取发布版本（`version.txt` 那条链，
见 `release.md`），而不是把它们改成手写常量。

---

## 3. 需要新增的文件

打包**不需要** MSIX Packaging Tool（那是给「已有安装程序、要重新捕获」的场景用的 Store 应用）。
我们是源码构建，需要的只是「一个目录 + 一份 manifest」，`makeappx pack` 直接吃：

| 文件 | 作用 |
|---|---|
| `packaging/windows/AppxManifest.xml` | manifest 模板，构建时替换 `Version` / `Publisher` / `DisplayName` |
| `packaging/windows/msix/Assets/*.png` | `Square44x44Logo` / `Square150x150Logo` / `StoreLogo`，均为透明底 PNG |
| `.workbuddy/verify/make_msix_assets.py` | 从 `assets/images/icon.png` 生成上面几张（沿用 `make_installer_bitmaps.py` 的写法） |
| `packaging/windows/Suwayomi.appinstaller` | 可选，自动更新用；只有签名之后才有意义 |

AppxManifest 的骨架（full trust 桌面应用）：

```xml
<Applications>
  <Application Id="Tray" Executable="suwayomi.exe" EntryPoint="Windows.FullTrustApplication">
    <uap:VisualElements DisplayName="Suwayomi" … Square150x150Logo="Assets\…" Square44x44Logo="Assets\…">
      <uap:DefaultTile/>
    </uap:VisualElements>
  </Application>
</Applications>
<Capabilities>
  <rescap:Capability Name="runFullTrust"/>
</Capabilities>
```

`bin/suwayomi-server.exe`、`jre/`、`webui/` 都只是包内文件，**不需要**各自声明成 Application
（它们由托盘进程拉起）。

---

## 4. 打包流程（CI）

现有 staging 目录 `dist/<BASE>+jre` 已经就是 MSIX 需要的那棵「一个目录一棵树」，所以步骤很短：

1. 把 `AppxManifest.xml`（替换过 `Version`）与 `Assets/` 拷进 staging 根；
2. `makeappx pack /d <staging> /p <BASE>.msix`（x64；arm64 另出一次）；
3. 两个架构都出的话：`makeappx bundle /d <目录> /p <BASE>.msixbundle`；
4. `makeappx validate /p <包>`；
5. 签名（见第 5 节）；
6. 上传。

**工具路径的坑**：`makeappx.exe` 在 `C:\Program Files (x86)\Windows Kits\10\bin\<SDK版本>\x64\`，
版本号随 runner 镜像变。工作流里要么按目录搜最高版本，要么在步骤里装 SDK；不要写死路径
（写死会在某次镜像更新后以「打包失败」的形式炸掉，看起来不像路径问题）。
`signtool.exe` 同源，且官方故障排除文档明说它**不在**标准 CI 镜像里保证存在。

---

## 5. 签名：SignPath Foundation（已定）

**已定（2026-09-30）**：Windows 侧走 **SignPath Foundation**，即 SignPath.io 的免费 OSS 签名，
不再考虑买证书或换渠道。未签名的 MSIX 对普通用户等于不可用 —— `Add-AppxPackage -AllowUnsigned`
需要 Windows 11 + 开发者模式（带可执行内容的包还要管理员），只能当开发验证手段。

选它的理由不是「免费」，而是本项目的形态正好落在它的准入条件里（5.2 逐条对照）。代价只有一条：
**证书签发对象是 SignPath Foundation 而不是本项目** —— UAC / SmartScreen 里显示的是
SignPath Foundation，`AppxManifest` 的 `Publisher` 也必须写成它的证书主体（见 2.5）。

### 5.1 支持的文件格式与产物边界

`docs.signpath.io/artifact-configuration/reference` 的格式表里，`.msix` / `.msixbundle` /
`.msi` / `.msp` / PE（`.exe` `.dll`）都在支持范围内，且 MSI 与 MSIX 都是 **composite** ——
可以嵌套签名（先签包内的 PE、再签外层包）。由此得到两条：

- **msi / setup.exe 可以顺带一起签。** 它们现在完全未签名，签了立刻消掉安装时的 SmartScreen 警告，
  比 MSIX 更早见效，也是把 CI 接线跑通的最简形态。
- **要签哪些 PE 必须显式列出，不能通配。** `jre/**` 下的 `java.exe` 是 Temurin 的二进制，
  条款禁止用本项目的订阅去签上游 OSS 的二进制（允许的只是「把它们放进签名包」，例如 MSI 安装器）。
  本项目要签的只有 `suwayomi.exe` 与 `bin/suwayomi-server.exe`。

另有一条 MSIX 专属约束：`hash-algorithm` 不支持 `sha1`，且必须与 `AppxBlockMap.xml` 里的一致
（默认即 sha256，保持默认即可）。

### 5.2 准入条件（逐条对照）

来源 `signpath.org/terms`（2026-09-30 读取）。

| 条件 | 本项目 |
|---|---|
| OSI 认可许可、全组件无商业双许可 | ✅ MPL-2.0，单一许可 |
| 无专有 / 非开源组件 | ✅ 产物由本仓源码构建（`jre/` 属 System Libraries，见 5.1 的边界） |
| 无恶意代码 | ✅ |
| 活跃维护 | ✅ |
| **已在待签名的形态上发布过** | ⚠️ msi / setup.exe 已发布；**MSIX 尚未发布过，需先发一次未签名版** |
| 功能有文档（下载页 / 商店条目） | ⚠️ 仓库 `homepage` 为空；README 有「Release 包结构与用法」一节，但没有 Code signing policy / 隐私声明（5.4） |
| 全体成员双因素认证 | ⚠️ 需在 GitHub 与 SignPath 两侧都开 MFA |
| 签名团队 = 开发维护团队、拥有仓库 | ✅ |
| 只签自己构建的二进制、构建可验证 | ✅ GitHub Actions（SignPath 的 trusted build system） |

**已定（2026-09-30）**：准入条件按上表逐条消掉，并接受「签名先落在 msi / setup.exe 上」的顺序 ——
两者已在**待签名的形态上**发布过，签完立刻消掉安装警告，也是把 CI 接线跑通的最短路径；
MSIX 排其后，先在 P1 以未签名形态发一次（见 5.1、第 6 节）。

### 5.3 fork 条款：以「与 Kotlin 版兼容 + 保留许可与署名」立论

条款对「签一份上游软件的修改版」另有条件，字面上需**全部**满足：

> - the upstream project publishes signed builds
> - your project visibly uses a fork of the upstream project, e.g. using GitHub's fork feature
> - the release branches you use for signing are based on upstream branches that are usually signed

两条按字面对不上：上游 `Suwayomi/Suwayomi-Server`（同为 MPL-2.0）**不发布签名构建** ——
它的 workflow 里没有任何 signtool / SignPath 引用，发布资产（含 `windows-x64.msi`）全部未签名；
本仓在 GitHub 上也不是 fork（`fork: false`、无 parent）。

**申请材料按「与 Kotlin 版兼容 + 保留许可与署名」陈述（已定 2026-09-30）**：

- 本项目与 Kotlin 版（上游 `Suwayomi/Suwayomi-Server`）**兼容**：同一套数据模型、GraphQL / REST /
  OPDS 接口与 Mihon 扩展体系，客户端与既有工具链不必区分两者；
- 采用 Suwayomi Project 相关代码的部分，**原样保留 MPL-2.0 的许可与署名** —— 版权与许可通知
  （MPL 的 Exhibit A 形态）在 `README.md` 的「许可证」一节与 `docs/en/README.md` 的「License」
  一节，署名仍是 `Copyright (C) Contributors to the Suwayomi project`，未改写、未删除。
  MPL-2.0 只要求保留许可与署名、不要求 fork 关系，这一层是成立且可核查的。

**不再论证「独立重写」** —— 那是判断而非事实，材料里越少越好。这一条能否通过仍由 SignPath 判定，
是方案里唯一不能自行关闭的不确定项。

### 5.4 硬性要求：三条会变成实际工作量的

- **Code signing policy 必须出现在项目主页与下载 / release 页**，并用 "Code signing policy"
  作为小节标题或链接文字。内容至少要包含：
  - "Free code signing provided by SignPath.io, certificate by SignPath Foundation" 这一句；
  - 团队角色与成员（Authors / Reviewers / Approvers）；
  - 隐私声明 —— 链接隐私政策，或写 "This program will not transfer any information to other
    networked systems unless specifically requested by the user or the person installing or operating it"。
- **每次签名请求都要人工审批**（Approver 在 SignPath 控制台上批准）。自动构建可以照常出未签名产物，
  但签名那一步必须有人点 —— 不存在全自动的签名流水线。
- **MFA**：GitHub 与 SignPath 两侧都要开。

条款里还有一条「不得绕过 SignPath 的技术约束」，其中明确列出：产物必须以可验证的方式从源码构建。

### 5.5 CI 接线

用 `SignPath/github-action-submit-signing-request`（v3，node24）。输入：

| 输入 | 值 |
|---|---|
| `connector-url` | 默认 `https://pipelineconnector.connectors.signpath.io/GitHubActions/GitHubCom` |
| `api-token` | SignPath 上建的 API token（Submitter 角色），放 repo secrets |
| `organization-id` / `project-slug` / `signing-policy-slug` | 审批通过后从 SignPath 控制台取 |
| `artifact-configuration-slug` | 同上；建议把它连同产物结构一起签入仓库，产物结构变了才不会静默失配 |
| `github-artifact-id` | 上一步 `actions/upload-artifact` 的 `outputs.artifact-id` —— 待签产物必须先作为 workflow artifact 上传 |
| `output-artifact-directory` | 落地签名产物；不填则只签名、不下载 |

该 action **同步等待**签名完成（`wait-for-completion` 默认 true，超时 600s），输出
`signing-request-id` / `signed-artifact-download-url`。因为是人工审批，这一步会把 job 停住等人批准 ——
超时值要按「人可能不在电脑前」来设。

### 5.6 其余路线（不采用，留档备查）

| 路线 | 成本 | 证书主体 | 不采用的原因 |
|---|---|---|---|
| **Microsoft Store** | 开发者账号一次性 $19 | 微软代签 | 顺带白送自动更新 + 完全绕开 SmartScreen，但要过 Store 政策审查；这是漫画聚合 + 扩展沙盒应用，能否过审未知。**注意 Store 只负责 MSIX 的签名，无法签我们自己的 msi / setup.exe** |
| **Azure Trusted Signing**（原名 Azure Code Signing / Artifact Signing） | ≈ $9.99/月（Basic） | 自己 | 签发 ~3 天有效期的短证书 + 时间戳，CI 集成好。**身份验证按 MS Learn 口径：个人仅美 / 加；组织已扩到美 / 加 / 欧盟 / 英国** —— 仍需当地主体，弃 |
| 自购 OV 证书 | ≈ $100–300/年 | 自己 | 2023 年起私钥必须放硬件令牌 / 云 HSM，无人值守 CI 签名很麻烦 |
| 自签证书 | 免费 | 自己（不受信） | **只能本机开发测试**：每台用户机都要先手动信任证书，等于把风险转给用户 |

---

## 6. 分阶段计划

**P0 —— 不依赖 SignPath 审批，可以现在做**

- 托盘运行时目录改造（2.1 的剩余部分）：认预置文件里的 `appdataDir` + `%VAR%` 展开、
  写探测兜底落 `%LOCALAPPDATA%\Suwayomi`，并把 `SUWAYOMI_APPDATA_DIR` 显式传给 server。
- msi 的 per-machine 档（2.1）：给向导接上安装范围页，并随包落 `tray.preset.json`
  （组件条件 `ALLUSERS = 1`）。不做这一步，「msi 里选全局」根本没有入口。
- 版本元数据注入（2.6）：`suwayomi.exe` 与 `bin/suwayomi-server.exe` 在构建期写入项目名与发布版本。
  它与 MSIX 无关，是签名本身的前置条件。
- 手工出一份未签名 msix，本机用开发者模式 + `Add-AppxPackage -AllowUnsigned` 装一次，
  验证 2.1 的改造真的够、且 ext-runtime 在包标识下能起来。
- 验收：与 msi 版功能等价（扩展能加载、下载能落盘、数据目录可写、托盘能起 server）。

**P1 —— CI 出包，并先以未签名形态发布**

- `AppxManifest.xml` + `Assets/` + `make_msix_assets.py`；
- `build.yml` 加 `pack_msix` 开关，出 `.msix`（x64 / arm64 → `.msixbundle`）；
- 产物标注「未签名，仅开发者可用」，与 msi / setup.exe 并列上传 —— 这一步同时满足准入条件里的
  「已在待签名的形态上发布过」。

**P2 —— 申请 SignPath 并接进 CI**

- 申请前补齐 5.4 的三条：项目主页 / 下载页上的 "Code signing policy"、两侧 MFA、Approver 角色。
- 提申请（仓库 URL + 下载页 URL + 描述 + 许可证）：fork 条款按 5.3 的口径陈述 —— 与 Kotlin 版
  兼容、并原样保留采用 Suwayomi Project 代码部分的 MPL-2.0 许可与署名。
- 通过后建 project / artifact configuration / signing policy / trusted build system，
  取 4 个 slug 与 organization id。
- **先签 msi / setup.exe**（已发布、格式最简单），接线跑通后再签 MSIX。
- workflow 形态：产物 `upload-artifact` → SignPath action → 下载签名产物 → 替换待发布的资产。
  签名步骤会被人工审批挡住，超时要按这个来设（5.5）。

**P3 —— 签名落地之后**

- `.appinstaller` 自动更新 + 差分更新（未签名时它没有意义，见第 3 节文件表）；
- 提交 winget（`microsoft/winget-pkgs`）；
- 同机 msi / msix 并存的检测与提示（两套独立卸载项、两份用户数据，需要引导）。

---

## 7. 待实机验证清单

以下都**没有实机验证过**，是动手前必须先跑的：

1. **full-trust 打包应用的 `%LOCALAPPDATA%` 写入是否被重定向**。文档两处口径不一致
   （容器化那篇说 full trust 直通；desktop bridge 那篇说 AppData 写入被重定向到
   `%LOCALAPPDATA%\Packages\<PFN>\LocalCache\…`）。这个结论直接决定数据目录选哪个路径，
   也决定它会不会和 msi / 绿色版打架。
2. **ext-runtime 在包标识下能否正常起 JVM**：`-javaagent` 挂载、子进程继承包标识后的文件
   访问、`java.io.tmpdir`。
3. **WebView2 的用户数据目录**是否可写（Tauri 默认走 `app_data_dir`，但要看真实落点）。
4. **MSIX 版监听端口后，局域网访问的防火墙弹窗**长什么样、能否预置。
5. 扩展沙盒、`data/` 迁移脚本在同机有 msi 版数据时的行为。
6. **SignPath Foundation 证书的完整 subject DN**。`AppxManifest` 的 `Publisher` 必须与它逐字一致，
   但公开资料只到 `CN=SignPath Foundation` 这一层，完整 DN 要从实际签名请求回传的证书上取。
7. **签名后的产物在干净机器上能否装上**：MSIX 的包外签名、MSI 的 Authenticode、以及包内
   `suwayomi-server.exe` 一起走 deep signing 的情形。
8. **PE 版本资源注入的实际取值**：`(Get-Item .\suwayomi.exe).VersionInfo` 是否与发布版本一致，
   以及 SignPath 的 file metadata restriction 是否接受这套值。
9. **SmartScreen 的实际表现**：证书主体是 SignPath Foundation 的共享证书，首次下载是否仍有警告
   要实测 —— OV 证书的声誉是逐步建立的，不能假设"签了就一定没有警告"。
10. **per-machine 的 msi 真装一次**：范围页选「所有用户」后，`tray.preset.json` 是否真落到
   `<安装根>\appdata\settings\tray.json`（条件 `ALLUSERS = 1`），托盘是否按它换根，以及
   `%VAR%` 展开出的是**当前用户**的 `%LOCALAPPDATA%` / `%USERPROFILE%` 而不是安装管理员的。
   条件类问题 ICE 一律查不出（同 `release.md` 里 `SetDirectory` 那条），只能真装。
11. **范围页接进序列后 `ALLUSERS` 的实际取值**：`ALLUSERS=2` + `MSIINSTALLPERUSER=1` 是 WiX 给
   `Scope="perUserOrMachine"` 的默认组合，加页之后要核对两种选择各自落到的属性组合对不对。

---

## 8. 与现有渠道的关系

- **msi / setup.exe 保留**：向导里的安装位置、快捷方式开关、安装范围是明确需求，MSIX 给不了。
  per-machine 档还要随包落预置托盘设置（2.1），把 appdata / data 两个根指到用户目录 ——
  否则装进 `C:\Program Files` 后普通用户写不了，首次启动就会失败。
  它们也会一起上签名（5.1）—— 那是 SignPath 落地的第一步，不依赖 MSIX 的任何进展。
- **绿色版 zip 不变**（zip 不参与 Authenticode 签名）。
- **MSIX 只作为附加渠道**：签名与出包都落地之前不进 Release 的公开下载页；P1 阶段先以未签名形态
  与其它产物并列，标注「未签名，仅开发者可用」。
