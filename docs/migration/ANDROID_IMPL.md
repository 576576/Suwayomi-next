# Android arm64 实现（android_impl 分支）

> 状态：进行中。本文是该分支的决策与施工记录，随实现推进更新。
> 前置：数据库双后端（`84b8f3a`）已完成并合并进 `main`，本分支从 `main` 起。

## 1. 目标

给一套能在 Android（arm64）上跑的构建：**扩展不再经 jvm-sandbox**，而是直接使用
**系统上已安装的 Mihon/Tachiyomi 扩展**（与 Mihon 自身加载扩展的方式一致）。

与桌面端的根本差别：

| | 桌面（现状） | Android（本分支） |
|---|---|---|
| server 进程 | 独立可执行文件 | 与宿主 App **同进程**（Rust 以 cdylib + JNI 嵌入） |
| 扩展来源 | 服务器下载 APK 落盘 `extensions/` | **系统已安装的扩展应用**（`PackageManager` 发现） |
| 扩展执行 | 子进程 JVM 沙盒：APK→dex2jar→ASM 修字节码→`URLClassLoader.defineClass` | 宿主 App 进程的 **ART** 内直接加载扩展 APK 的 dex |
| Android 框架 | `AndroidCompat-1.0.jar` stub 模拟 | 真实 Android 框架（系统提供） |
| 安装/卸载 | 服务器写/删 APK 文件 | 交给系统（PackageInstaller / 用户在扩展管理 App 内操作） |

## 2. 约束与决策

### C1 Android 10+ 禁止从 App 私有目录 exec 可执行文件

`targetSdk ≥ 29` 时 SELinux 拒绝对 app data 目录下文件 `exec`，所以「把
suwayomi-server 当子进程 spawn」这条桌面路径在 Android 上**不可用**。

**决策 D1**：Rust server 编译为 `cdylib`，随 APK 的 `jniLibs/arm64-v8a/` 分发，
由宿主 App `System.loadLibrary` 后经 JNI 启动 —— 只有 `dlopen`，不需要 `exec`。

### C2 Android 无桌面 JVM，扩展只能在 ART 内执行

**决策 D2**：宿主 App 加载扩展走 **Mihon 的路径**：`PackageManager` 发现已安装的
`eu.kanade.tachiyomi.extension.*` 包 → 取其 APK 路径 → `PathClassLoader`/`createPackageContext`
把扩展的 dex 加载进**宿主进程** → 实例化扩展的 `Source` 类。
**不做** dex2jar、不做 ASM 字节码改写、不引入 AndroidCompat：这些是「在桌面 JVM 里
假装成 Android」的补丁，在真机上既不需要也会冲突（`android.*` 由系统 bootclasspath 提供）。

复用的是 `jvm-sandbox` 里那份**手写的 `eu.kanade.tachiyomi.**` 接口与 `HttpSource`/
`ParsedHttpSource` 实现**（扩展编译时链接的就是这组类名，加载时由宿主提供，因此宿主
可以直接把它们 cast 成 `Source`，不必像桌面沙盒那样全靠反射）。

### C3 Rust 与扩展宿主之间的调用通道

**决策 D3**：同进程内走**回环 HTTP**，复用 jvm-sandbox 已有的 JSON 契约
（`/health`、`/extensions`、`/sources`、`/reload`、`/source/{id}/...`）。
Rust 侧因此**不需要新的 SourceFetcher 实现** —— `HttpSandboxFetcher` 直接可用，
只是不再由 `SandboxProcess` 去 spawn 一个 JVM，而是连宿主 App 起的端口。

这样做的理由：该契约与它的 Rust 客户端已经被真实扩展验证过；Android 宿主可以脱离
Rust 单独用 `curl` 调试；将来若要省掉这一次回环，可再换成直接 JNI 回调
（`SourceFetcher` 是唯一 seam，替换点只有 `main.rs` 里一处 `Arc::new(...)`）。

### C4 扩展来源与安装/卸载在 Android 上的形态

**决策 D4**：**列表来源**只认系统已安装 —— 不下载、不落盘、不扫描 `extensions/`
目录，扩展列表来自 `PackageManager`；`is_installed`/`version_code` 等元数据来自包的
`PackageInfo`/`ApplicationInfo`，不再来自扩展仓库索引。

**安装/卸载**仍要能用，但**执行者换成系统安装器**，本项目不写入任何 APK 文件：

| 动作 | 桌面（现状） | Android |
|---|---|---|
| 安装 | 下载 APK 写入 `extensions/`，重启沙盒 | 把 APK 落 `cacheDir/apk-import/` 并 `FileProvider` 暴露 → 唤起系统安装器（`ACTION_VIEW` + `application/vnd.android.package-archive`）；用户确认后由 `PackageManager` 装 |
| 卸载 | 删 `extensions/` 下 APK | 唤起系统卸载器（`ACTION_DELETE` / `ACTION_UNINSTALL_PACKAGE`），或 `PackageInstaller.uninstall()` |
| 更新 | 覆盖 APK + 重启沙盒 | 同安装（覆盖安装），系统校验签名一致才允许 |

因此 Android 上 `install`/`uninstall` 是**发起一个系统 Intent / PackageInstaller 会话**，
不是文件操作；完成与否以 `PackageManager` 的广播/轮询为准，随后触发扩展重新发现
（等价 `/reload`）。扩展管理 App 自身装过/卸过的扩展，宿主同样能立刻看到 —— 这是这套
模型比桌面「服务器独占 extensions 目录」更好的地方。

安装入口需要 `REQUEST_INSTALL_PACKAGES` 权限；`targetSdk ≥ 26` 时还需引导用户到
`ACTION_MANAGE_UNKNOWN_APP_SOURCES` 授权本 App 安装未知来源应用。

#### 实测出来的四个坑（都已在代码里避开）

1. **`ACTION_INSTALL_PACKAGE` 已废弃**，实际用 `ACTION_VIEW` +
   `application/vnd.android.package-archive`（配 `FileProvider` 的 `content://`）。
2. **转交时必须显式挑一个"不是自己"的处理者**。本 App 自己也注册了 APK 的
   `VIEW` filter（作为入口），不排除的话系统会弹「打开方式」选择器，用户再点回
   Suwayomi 就原地打转。做法：`queryIntentActivities` 取第一个 `packageName != 自己`
   的 Activity 后 `setClassName` —— manifest 里 `<queries>` 的 VIEW+apk intent
   就是为此让这次查询可见（Android 11+ 包可见性）。
3. **系统安装器是对话框样式的 Activity**，装扩展时宿主 Activity 只 `onPause`、
   不 `onStop`。所以"装完重扫"不能用生命周期闸门判断，改用**指纹比对**：
   App 每次 `onResume` 拿系统已装扩展的 `pkg@versionCode` 指纹与上次扫描时的比较，
   不同才重扫（枚举 `PackageManager` 很便宜，重载 dex 很贵）。副作用是它顺带
   覆盖了从 Mihon / `adb install` 装扩展的情况。
4. **`tachiyomi.extension.nsfw` 的值不统一**：keiyoushi 的扩展写的是整数 `1`。
   `Bundle.getBoolean` 碰到 Integer 不会转换，而是打一条警告后返回默认 `false` ——
   结果是 NSFW 扩展被静默当成 Safe。必须自己判类型（Boolean / Number / String 三种都认）。

另外，导入待安装 APK 时目标文件名必须与源区分开（加时间戳前缀并放进单独子目录）：
源有可能就在 `cacheDir` 里，同名会让 `outputStream()` 先把源截断成 0 字节，复制出
一个空文件，而且是**静默失败**。

## 3. 组件与落点

```
android/                     新：Android 宿主工程（独立 Gradle/AGP，不并入 jvm-sandbox）
  app/                       :app —— Activity(WebView) + Application(JNI 启服)
                             + ExtensionInstaller（唤起系统安装器/卸载器，FileProvider 暴露 APK）
  extension-host/            :extension-host —— 扩展宿主（PackageManager 发现 + ART 加载 + 回环 HTTP）
extension-runtime/           新：jvm-sandbox 与 Android 宿主共享的源码树（无构建脚本）
  eu/kanade/tachiyomi/**     扩展 API 实现（HttpSource/ParsedHttpSource/network/model/Filter…）
  suwayomi/tachidesk/**      rx 桥等
  sandbox/SourceDriver.kt    反射容错驱动（跨 lib 版本字段名差异都在这层兜住）
  sandbox/Router.kt          与平台无关的路由（请求/响应用自有 Http.kt 类型，不再绑 HttpExchange）
crates/suwayomi-server/      lib.rs 抽出启动逻辑；android 模块提供 JNI 入口
  main.rs                    CLI 薄壳（读 env → 调 lib::run）
```

- `jvm-sandbox` 与 `android/extension-host` 各自把自己的平台依赖（AndroidCompat jar /
  Android SDK）加进来，**共享同一份源码树**，避免两份 `eu.kanade.tachiyomi` 漂移。
- Rust 侧新增的环境变量：`SUWAYOMI_SANDBOX_URL`（直接指定扩展宿主基址，Android 宿主
  在建好回环监听后传给 server；桌面也可用它接管已运行的沙盒）。

## 4. 分阶段

| # | 内容 | 状态 |
|---|---|---|
| A1 | 分支与文档（本文） | ✅ |
| A2 | Rust：抽出 `lib::run`、`SUWAYOMI_SANDBOX_URL`、`cfg(target_os="android")` 下不 spawn JVM | ✅ |
| A3 | Rust：cdylib + JNI 入口（start/stop），`aarch64-linux-android` 交叉编译通过 | ✅ |
| A4 | `extension-runtime/` 抽取（jvm-sandbox 改为引用共享源码树，桌面行为不变） | ✅ |
| A5 | Android 宿主：PackageManager 发现扩展 + ART 加载 + 回环 HTTP 契约 | ✅ |
| A6 | Android App：WebView 打开本地 WebUI，Application 启服/停服 | ✅ |
| A7 | Android 安装/卸载：唤起系统安装器（`ACTION_VIEW` + FileProvider）、`ACTION_DELETE` 卸载，完成后重新发现扩展 | ✅ |
| A8 | CI：android-arm64 目标（cargo cross + Gradle assemble），产物命名并入 `pack_mode` | ✅ |
| A9 | 端到端验证：模拟器/真机上启动、WebUI 可访问、已装扩展可搜索/看章节 | ✅（图源联网抓取受环境网络限制，见下） |

A9 的落地证据（模拟器 API 37 / x86_64）：

| 项 | 结果 |
|---|---|
| server 启动 + WebUI | `server listening on http://127.0.0.1:4567`，`/version.txt` 与 WebUI 首页均 200 |
| 扩展入表 | `extension sync at startup: 22 source(s) registered`；GraphQL `extensions` → 1 条、`sources` → 23 条 |
| 扩展**真实执行** | `GET /source/{id}/filters` → 200，13 个 filter（`select`/`text`/`title`/`group`，含 `List<String>` 类型的 `values`）—— 这是扩展自己的 `getFilterList()` 在 ART 里跑出来的 |
| 安装链路 | APK intent → 系统安装器「Update this app?」→ App updated → 回前台自动重扫 22 源 |
| 卸载链路 | 卸载 → 回前台重扫 0 extension → `fetchExtensions` 后 `isInstalled=false` |

**未覆盖**：图源联网抓取（`/source/{id}/manga`）在本机网络下不可达 —— 宿主与宿主机都连不上
`nhentai.com`（`curl` 超时 / `ConnectException`），属环境限制而非代码问题，换一个可达图源即可补测。

A8 的落地：`build.yml` 里新增独立的 `android` job（装 `platforms;android-37` 与钉死版本的
NDK 28.2 → `android/scripts/build-rust.sh` → WebUI 打进 assets → `:app:assembleRelease`），
由 `release.yml` 的 `build_android_arm64` 开关控制，APK 命名并入 `pack_mode` 约定。
release 签名支持从 secret 注入 keystore，没配则回退 debug key。详见 `docs/release.md`。

`ACTION_INSTALL_PACKAGE` 在 API 29 起废弃，且不带 `REQUEST_INSTALL_PACKAGES` 时直接被
`FileUriExposedException` / 系统拒绝；实际用的是 `ACTION_VIEW` +
`application/vnd.android.package-archive` + `FLAG_GRANT_READ_URI_PERMISSION`。

### 扩展在 server 侧如何可见（A5/A6 的必要收尾）

`extension` / `source` 两张表此前**只由仓库索引驱动**（`refresh_stores` upsert），
`sync_sources` 遇到没有索引行的包直接跳过。Android 上扩展来自系统 `PackageManager`，
既没有 `extensions/` 目录也没有仓库索引，于是"宿主里 22 个源、WebUI 扩展页却是空的"。

修法见 `ExtensionStoreService::sync_sources()`：沙盒报上来的扩展元信息现在会
**补建 `extension` 行**（`store_index_url` 留 NULL = 非仓库来源），再注册 source 行；
`/extensions` 与 `/inspect` 两处 JSON 契约相应增加了 `versionCode` / `contentWarning`
（Android 取自 `PackageInfo.longVersionCode` 与 `tachiyomi.extension.nsfw` meta-data，
桌面取自 APK manifest）。并配套：

- `upsert_index` 的 `is_installed` 判定加入"沙盒已加载"，否则 Android 上刷新一次
  仓库就会把已安装扩展冲成未安装（那里原本只看 `extensions/` 目录里的文件）；
- server 启动时同步一次（Android 上这是唯一的入库路径，不能等用户点开扩展页）；
- 沙盒不再报告的扩展回写 `is_installed = FALSE`（Android 走系统安装器卸载时收不到
  回调，只能靠这次同步），但**本地 APK 文件仍在**的桌面行不动，避免与按文件判定的
  `upsert_index` 来回打架。

### 工具链版本（本地实测）

| 组件 | 版本 | 说明 |
|---|---|---|
| Gradle | 9.5.0 | **必须 ≥ 9.4.1** —— AGP 9.2.1 的硬性下限。9.7.0 虽然也满足下限，但下模块级 DSL 访问器会崩（见下方"AGP 9 的坑"） |
| AGP | 9.2.1 | 9.0 起**内置 Kotlin 支持**，不能再单独应用 `org.jetbrains.kotlin.android`（会被直接拒绝） |
| NDK | 28.2.13676358（r28c） | `aarch64-linux-android26-clang` |
| compileSdk / targetSdk / minSdk | 36 / 36 / 26 | minSdk 26 = `java.nio.file` 与 `DelegateLastClassLoader` 的下限 |
| Kotlin | 由 AGP 内置 | 与 jvm-sandbox 的 2.4.0 无关，两端各自编译共享源码 |

### 交叉编译的两个坑（已固化在构建脚本里）

1. **`build.rs` 的图标嵌入必须按「目标平台」判定，不能用 `#[cfg(windows)]`。**
   交叉编译时宿主机也是 Windows，`cfg(windows)` 为真 → winres 会在 Android 目标上
   报 `Can only compile resource file when target_env is gnu or msvc`。
   判据改为 `CARGO_CFG_TARGET_OS == "windows"`。
2. **`lto` 必须在 Android profile 里关掉。** rustc（stable，Windows 宿主）在
   `--target aarch64-linux-android` 下做 thin-LTO 会自己崩掉
   （`0xc0000005 STATUS_ACCESS_VIOLATION`，崩点在 rustc 进程内，与代码无关）。
   故新增 `[profile.android-release]`（继承 release，只把 `lto` 设为 false）。

另有 NDK 目录的一个细节：Windows 上 `aarch64-linux-android26-clang` **无后缀那个文件也存在**
（sh 脚本），但它不能被 `CreateProcess` 执行（os error 193），必须优先选 `.cmd` / `.exe`。

## 5. 产物形态选项（`-core` / `+jre`）

手动触发的构建除架构外还有一组**多选项**（可同时选中，非二选一）：

- `-core`：不打包 JRE，最小构建 —— **默认选中**。
- `+jre`：在 `-core` 产物基础上追加对应架构的 JRE。**该选项需要裁剪 JRE 体积、
  删除未被使用的部分**：用 `scripts/make-jre.sh` 走 jlink，按实测白名单只保留 14 个模块，
  180 MB → 39 MB（解压）、58 MB → 25 MB（压缩）。
- **Android 构建不参与这组选项**：Android 天然不打包 JRE，扩展跑在系统 ART 上。

需要裁剪产物的只有**非 Android 的 `+jre` 构建**；Android 的 APK 里没有 JRE 可裁。

实现与数据见 `docs/release.md` 的「产物形态」与「JRE 裁剪」两节；本地验证脚本
`.workbuddy/verify/ci_pack_check.py`（171 项断言，覆盖各 target × 各形态组合）。

`+jre` 带来的一个约束值得记下来：**jlink 不能跨平台生成运行时**，所以每个 target 的
runner 必须与目标同架构 —— linux-arm64 因此改用 `ubuntu-24.04-arm`（顺带变成原生编译，
不再需要交叉工具链），x64 的 macOS 用 `macos-15-intel`（`macos-13` 已被 GitHub 下线）。

## 6. 明确的非目标

- 不在 Android 上跑 jvm-sandbox（不引入 dex2jar/ASM/AndroidCompat）。
- Android 上不由本项目**静默**安装扩展：一律经系统安装器 / 卸载器，用户可见可撤销。
- 不改桌面端的行为与产物布局（除新增 `SUWAYOMI_SANDBOX_URL` 这一可选入口）。
