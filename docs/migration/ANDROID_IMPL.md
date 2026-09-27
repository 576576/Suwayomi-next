# Android arm64 实现

> 状态：进行中。本文是这套实现的决策与施工记录，随实现推进更新。
> 前置：数据库双后端（`84b8f3a`）已完成并合并进 `main`。
>
> ⚠️ **后记（剥离后）**：文中提到的 `jvm-sandbox/` 与 `extension-runtime/` 两个目录
> 已合并为独立仓库 [576576/Suwayomi-ext-runtime](https://github.com/576576/Suwayomi-ext-runtime)
> （`jvm-sandbox` 改名 `ext-runtime`，共享源码树并入其中）。本仓库不再持有它们的源码，
> 桌面侧下载 `ext-runtime-<V>.jar`、Android 侧下载 `-shared-sources` 包展开后编译。
> 目录树一节已同步更新，其余段落保留当时的原始记录。

## 1. 目标

给一套能在 Android（arm64）上跑的构建：**扩展不再经桌面沙盒（ext-runtime）**，而是直接使用
**系统上已安装的 Mihon/Tachiyomi 扩展**（与 Mihon 自身加载扩展的方式一致）。

与桌面端的根本差别：

| | 桌面（现状） | Android |
|---|---|---|
| server 进程 | 独立可执行文件 | 与宿主 App **同进程**（Rust 以 cdylib + JNI 嵌入） |
| 扩展来源 | 服务器下载 APK 落盘 `extensions/` | **系统已安装的扩展应用**（`PackageManager` 发现） |
| 扩展执行 | 子进程 JVM 沙盒：APK→dex2jar→ASM 修字节码→`URLClassLoader.defineClass` | 宿主 App 进程的 **ART** 内直接加载扩展 APK 的 dex |
| Android 框架 | `android-compat/` 里的 AndroidCompat stub 模拟 | 真实 Android 框架（系统提供） |
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

复用的是 ext-runtime 共享源码树里那份**手写的 `eu.kanade.tachiyomi.**` 接口与 `HttpSource`/
`ParsedHttpSource` 实现**（扩展编译时链接的就是这组类名，加载时由宿主提供，因此宿主
可以直接把它们 cast 成 `Source`，不必像桌面沙盒那样全靠反射）。

### C3 Rust 与扩展宿主之间的调用通道

**决策 D3**：同进程内走**回环 HTTP**，复用 ext-runtime 已有的 JSON 契约
（`/health`、`/extensions`、`/sources`、`/reload`、`/icon/{pkg}`、`/source/{id}/...`）。
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
android/                     Android 宿主工程（独立 Gradle/AGP，不并入主工程）
  app/                       :app —— Activity(WebView) + Application(JNI 启服)
                             + ExtensionInstaller（唤起系统安装器/卸载器，FileProvider 暴露 APK）
                             + DirectoryPicker（选目录，SAF + 全盘写权限）
                             + FileChooser（选文件，WebUI 的 <input type="file">）
                             + DownloadSaver（存文件，WebUI 的 link.download）
  extension-host/            :extension-host —— 扩展宿主（PackageManager 发现 + ART 加载 + 回环 HTTP）
  build/ext-runtime-src/     由 android/scripts/fetch-ext-runtime-src.sh 下载展开，:extension-host 的源目录
    eu/kanade/tachiyomi/**   扩展 API 实现（HttpSource/ParsedHttpSource/network/model/Filter…）
    suwayomi/tachidesk/**    rx 桥等
    sandbox/SourceDriver.kt  反射容错驱动（跨 lib 版本字段名差异都在这层兜住）
    sandbox/Router.kt        与平台无关的路由（请求/响应用自有 Http.kt 类型，不再绑 HttpExchange）
crates/suwayomi-server/      lib.rs 抽出启动逻辑；android 模块提供 JNI 入口
  main.rs                    CLI 薄壳（读 env → 调 lib::run）
```

- ext-runtime 与 `android/extension-host` 各自把自己的平台依赖（AndroidCompat jar /
  Android SDK）加进来，**共享同一份源码树**，避免两份 `eu.kanade.tachiyomi` 漂移。
- Rust 侧新增的环境变量：`SUWAYOMI_SANDBOX_URL`（直接指定扩展宿主基址，Android 宿主
  在建好回环监听后传给 server；桌面也可用它接管已运行的沙盒）。

## 4. 分阶段

| # | 内容 | 状态 |
|---|---|---|
| A1 | 文档（本文） | ✅ |
| A2 | Rust：抽出 `lib::run`、`SUWAYOMI_SANDBOX_URL`、`cfg(target_os="android")` 下不 spawn JVM | ✅ |
| A3 | Rust：cdylib + JNI 入口（start/stop），`aarch64-linux-android` 交叉编译通过 | ✅ |
| A4 | `extension-runtime/` 抽取（jvm-sandbox 改为引用共享源码树，桌面行为不变）；后已随 jvm-sandbox 一并迁出到 ext-runtime 仓库 | ✅ |
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

A8 的落地：`build.yml` 里新增独立的 `android` job（装 `platforms;android-37.0` 与钉死版本的
NDK 28.2 → `android/scripts/build-rust.sh` → WebUI 打进 assets → `:app:assembleRelease`），
由 `release.yml` 的 Android 开关控制（**后续演进**：单一 `build_android_arm64` 已换成
`build_android_arm64` + `build_android_x64` 两个开关，再收成 `android_targets` 矩阵）。
release 签名支持从 secret 注入 keystore，没配则回退 debug key。详见 `docs/release.md`。

> 平台包名是 `platforms;android-37.0` 而非 `platforms;android-37`：API 36.1 起 Google 改为
> 「次版本号」命名，`36.1` / `37.0` / `37.1` / `37.2` 各自是独立包，裸 `android-37` 不存在。

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

### 扩展图标（`GET /icon/{pkg}`）

契约里多了 `/icon/{pkg}`：沙盒端返回 `{"mime":"image/png","data":"<base64>"}`，
REST 侧的 `/api/v1/extension/icon/{pkg}` 按 **磁盘缓存 → 沙盒 → 仓库索引 `icon_url`**
取值，三条路都要求拿到**真的图片字节**（PNG/JPEG/WebP 魔数）。

沙盒排在 `icon_url` 之前，是因为后者对系统装进来的扩展**恒为死链**：
`extension.icon_url` 的**列默认值**是个早已 404 的占位 URL
（`migrations/0001_schema_baseline.sql`），而这些扩展没有仓库索引行去盖掉默认值。
真图就在 APK 里（Android 取自 `PackageManager`，桌面取自 APK 的 `android:icon`）。

不校验字节的后果比"缺一张图"更麻烦：死链的 404 正文会被当成图标写进磁盘缓存，下次
请求"命中缓存"再喂一遍。注意 WebUI 的 Service Worker 对 `/extension/icon/` 配了
**1 年**的 `CacheFirst` —— 改图标逻辑前要先清掉 CacheStorage，否则会误判成没修好。

配套修的一个静默失败：缓存根的默认推导在 Android 上会退化成相对路径 `cache`，
而进程 CWD 是 `/`，`create_dir_all("/cache/…")` 一律权限失败（调用方普遍 `let _ =`）。
现在 JNI 入口经 `ServerOptions::cache_dir = Some(data_dir/cache)` 显式钉住，
由 `AppPaths` 注入到各服务（没有进程级单例，见 RUST_STYLE_AUDIT.md §7 阶段 4-1）。

### 选文件（`<input type="file">`）：WebView 不会自己弹选择器

WebUI 的「恢复备份」是点一个**隐藏的** `<input type="file">`
（`Suwayomi-WebUI/src/features/backup/screens/Backup.tsx` 里 `inputRef.current?.click()`）。
浏览器里这一步由浏览器自己弹选择器；**WebView 里不会** —— WebView 把「弹出文件选择器」
外包给宿主，只有宿主实现了 `WebChromeClient.onShowFileChooser` 才会弹。此前 `:app`
只设了 `WebViewClient`、没有 `WebChromeClient`，于是这个按钮按下去**静默无反应**：
不抛异常、不回调、logcat 里也什么都没有（唯一线索是「点了没反应」本身）。

修法落点 `android/app/src/main/kotlin/org/suwayomi/next/FileChooser.kt`，与「编辑存储位置」
的 `DirectoryPicker` 分开：那个选**目录**（SAF tree URI → 真实路径，还先要全盘写权限，
因为写盘的是同进程的 Rust 库），是跨两次 Activity 跳转的状态机；这个选**文件**，选完把
`content://` 交回 WebView 即可（读盘由 WebView 自己按 URI 做，不需要任何存储权限）。

四个实现要点，都有各自的坑：

- **用 `FileChooserParams.createIntent()`，不要自己拼 `ACTION_GET_CONTENT`。** 平台已经按
  `<input accept=… multiple>` 补上了 `CATEGORY_OPENABLE`（少了它，某些 provider 会给出
  宿主打不开的 URI）与 `EXTRA_MIME_TYPES`。
- **不要加 MIME 过滤。** `.tachibk` 没有注册过的 MIME 类型，一过滤反而会把用户要选的
  那个备份文件从选择器里藏起来（WebUI 那个 input 本来也没写 `accept`）。
- **回调必须恰好回一次。** `ValueCallback` 不被回调时，WebView 会把这次请求永久挂起 ——
  之后再点同一个 `<input type="file">` 都不会有反应，页面刷新前救不回来。所以
  「构不出 Intent」「没有 DocumentsUI」「用户取消」每一条分支都要走到 `onReceiveValue`。
  反过来，`onDestroy` 时**只丢引用、不回话**：WebView 马上就被 `destroy()` 了。
- **请求码分段**：`DirectoryPicker` 占 `0x51xx`、`FileChooser` 占 `0x5201`、`DownloadSaver`
  占 `0x5202`，避免两边将来各加一个请求码时撞车（`MainActivity.onActivityResult` 依次问三家）。
- **跳转期间 Activity 被重建**（切深浅色会走 `uiMode` 重建，见 `AndroidManifest` 的
  `configChanges` 注释）时，新实例的 `callback` 是空的，这次选择被静默丢弃 —— 用户重点一次
  即可，不值得为它引入 `onRetainCustomNonConfigurationInstance`。

### 创建备份（`link.download` + `link.click()`）

同一类缺口的另一半：「创建备份」拿到 `createBackup.url`（`/api/v1/backup/export/file`）后
用 `link.download` + `link.click()` 触发一次下载。浏览器里浏览器自己存盘；WebView 同样把
「下载」外包给宿主 —— 只有宿主设了 `setDownloadListener` 才会被通知，否则也是
**静默无反应**（不抛异常、不回调、logcat 一行都没有）。

修法落点 `android/app/src/main/kotlin/org/suwayomi/next/DownloadSaver.kt`：
`onDownloadStart` → `ACTION_CREATE_DOCUMENT`（SAF 的「另存为」）让用户选落点 →
拿到目标 `content://` 后**再回服务端取一次文件**写进去（下载是宿主发起的第二次请求，
所以 cookie 与响应码都得自己管）。

三个坑：

- **必须带上 WebView 的 cookie。** 下载不会自动继承 WebView 的会话。设置页开了认证时
  裸请求只拿到 302 登录页，照写不误的话落下来的是一个 HTML、后缀却是 `.tachibk`。
  所以 `instanceFollowRedirects = false` 并断言 `HTTP 200` —— 拿到 302 就如实报错。
  （`/api/v1/backup/export/file` 不在 `auth.rs` 的 `token_query_allowed` 白名单里，
  只能靠 cookie。）
- **备份其实被生成了两次。** mutation 里已经生成过一次，GET 时路由又跑一遍 `create_backup`。
  桌面浏览器走的是同一条路（既有行为，本次未改）；真要省掉，得让 mutation 返回一次性
  token 而不是直接给路径。
- **文件名从 `Content-Disposition` 取。** 服务端发的是
  `attachment; filename="org.suwayomi.next_<日期>_<时刻>.tachibk"`（与 autobackup / Mihon
  命名一致），正好喂给 SAF 的 `EXTRA_TITLE`；取不到才退到 URL 末段。
- **失败要把 SAF 建出来的空壳删掉。** `ACTION_CREATE_DOCUMENT` 是先建文件再让我们写，
  写砸了（认证挑战、网络断、存储满）就会留一个半截的 `.tachibk` —— 它**看着像一份真备份**，
  比"什么都没发生"更坏。所以失败分支统一走 `DocumentsContract.deleteDocument`（删不掉就
  算了，至少 Toast 要说清）。

与「选文件」的分工值得记一笔：那个把 `content://` 交回 **WebView**，读盘由 WebView 按 URI 做，
宿主不需要任何存储权限；这个由**宿主自己**读盘再写盘，所以失败模式也不同 —— 必须自己判定
响应码，不能只看「文件写出来了没有」。

`DownloadSaver` 只处理**宿主自己 server 的**下载：外链在 `shouldOverrideUrlLoading` 里就
交给系统浏览器了。另外备份页那句「也可拖放备份文件到此」在触屏上没有意义，已改为不提案
拖放的措辞（WebUI 仓 `src/i18n/locales/zh-Hans.po`）。

### 工具链版本（本地实测）

| 组件 | 版本 | 说明 |
|---|---|---|
| Gradle | 9.5.0 | **必须 ≥ 9.4.1** —— AGP 9.2.1 的硬性下限。9.7.0 虽然也满足下限，但下模块级 DSL 访问器会崩（见下方"AGP 9 的坑"） |
| AGP | 9.2.1 | 9.0 起**内置 Kotlin 支持**，不能再单独应用 `org.jetbrains.kotlin.android`（会被直接拒绝） |
| NDK | 28.2.13676358（r28c） | `aarch64-linux-android26-clang` |
| compileSdk / targetSdk / minSdk | **37** / 36 / 26 | compileSdk 必须 37：okhttp 5.x 的 Android 变体 `okhttp-android` 的 AAR metadata 就要 37，36 会在 `checkDebugAarMetadata` 直接失败（CI 因此装的是 `platforms;android-37.0`）。minSdk 26 = `java.nio.file` 与 `DelegateLastClassLoader` 的下限 |
| Kotlin | 由 AGP 内置 | 与 ext-runtime 的 2.4.0 无关，两端各自编译共享源码 |

本机**跑不了** `./gradlew`：compileSdk 是 37，而本机只装了 android-28/34 的 platform、没有
cmdline-tools，`:extension-host` 又还要 ext-runtime 的共享源码（下载产物）。所以改完 Kotlin
先在本地过一遍 `.workbuddy/verify/android_kotlin_check.sh` —— 它用 Gradle 缓存里现成的
`kotlin-compiler-embeddable` 加本机 `android.jar`，对 `app` 模块的**真实源码**做一次类型检查
（只对 `sandbox.ExtensionHost` 与 `androidx.core.content.FileProvider` 打桩，且断言桩与真货
同形，真签名一改就红）。抓得住「用错 API / 类型不匹配 / `override` 没对上签名」，抓不住
Android 运行时行为与资源链接 —— 后者只能靠 CI 的 `assembleRelease` 与真机。
同目录下还有 `.workbuddy/verify/android_kt_comment_check.py`：注释长度门禁（每块 ≤ 3 行）。
（`.workbuddy/` 是 gitignore 的，这两个脚本都不入库。）

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

### 实现要点归档（从代码注释搬来）

`android/**/*.kt` 的注释硬约束是**每块内容行 ≤ 3**（由 `.workbuddy/verify/android_kt_comment_check.py`
把关，不入库）：代码里只留结论，读起来一眼能扫过；理由与推演一律写在这一节。

**`DirectoryPicker`：为什么除了 SAF 还要「所有文件访问」。** SAF 的授权是**按 URI** 的 ——
它能让你用 `ContentResolver` 读写那个 URI，但对**普通 POSIX 路径**没有任何效力。而真正写盘的
是同进程里的 Rust 库（`std::fs`），它只认路径。所以只挑目录是不够的：目录挑得动，一落盘就失败。
API 30+ 走 `MANAGE_EXTERNAL_STORAGE`（`Environment.isExternalStorageManager()`），以下走
`WRITE_EXTERNAL_STORAGE`；顺序必须是先要权限、再挑目录。

**`MainActivity`：为什么用裸 `android.app.Activity`。** 界面就是一个全屏 WebView，没有任何
AppCompat 特性依赖 —— 少一层依赖就少一层体积与启动开销（与 `:app` 刻意不引 Material/Compose
同一个理由）。另外两处与生命周期相关的取舍：① 回前台时补一次扩展重扫 + 「所有文件访问」
判断，因为那个系统授权页不一定把结果回给 `onActivityResult`（各家 ROM 行为不一）；
② `onRenderProcessGone` 返回 `true` 并**整套重建** WebView —— 渲染进程被杀时 `reload()`
救不回来，而返回（默认的）`false` 会让系统直接杀掉整个 App；重建期间靠 `loadGeneration`
代号丢弃过期的等待结果，免得旧任务覆盖新 WebView。

**`SuwayomiApp`：启动顺序不能换。** 扩展宿主 → WebUI 解压 → `NativeServer.start()`。
扩展宿主必须**早于** server：server 启动时要连它做 `/health` 探测并拉 `/extensions`，
晚起会得到「一个扩展都没有」的空目录（server 不会自己重试）。退到后台**不主动停 server**：
Android 按需回收进程，`onTerminate` 根本不可靠；要停只能走设置页的「退出」→ `shutdown()`。

**`NativeServer`：JNI 符号名是约定，不是配置。** 导出符号由
`Java_org_suwayomi_next_NativeServer_*` 规则生成，与 `crates/suwayomi-android/src/lib.rs`
一一对应 —— 类名/包名/方法名任何一处不一致都会变成 `UnsatisfiedLinkError`。`load()` 不写在
`object` 的 `init` 里，是为了把 `loadLibrary` 的失败包成可上报的错误（`lastError`），
而不是让它变成静态初始化异常。

**`DalvikExtensionClassLoader`：为什么必须 delegate-last。** `PathClassLoader`（dalvik 默认）
是**父优先**：把 `android.*` 交给 bootclasspath 是对的，对扩展自己的类也只是「先问父、
父没有才回落」，行为上等价。真正需要子优先的场景是：**宿主自己也带了一份同名的第三方库**
（okhttp 等），扩展内置的那份应当优先，否则版本错配。API 27 才有
`DelegateLastClassLoader`，minSdk 26 因此需要一个 backport（`PathClassLoader` + 手写
`loadClass` 的「已加载 → boot → 自己 → 父」顺序，与 Mihon 一致）。

**`SimpleHttpServer`：为什么自己写一个。** Android 的 bootclasspath 里**没有**
`com.sun.net.httpserver`（那是 JDK 的 `jdk.httpserver` 模块），桌面宿主那套用不了。自己写的
这个只够本项目用：GET/POST + 按 `Content-Length` 读定长 body（`/inspect` 需要）、
keep-alive（Rust 侧 reqwest 的连接池默认复用连接，一次请求一条连接会白搭一次 TCP 往返）、
以及**只绑 127.0.0.1** —— 这是同进程通道，绝不能对外暴露扩展接口。契约与桌面完全一致
（同一个 `sandbox.Router`），所以 Rust 侧的 `HttpSandboxFetcher` 不需要知道对端是 JVM 还是 ART。

**`WebUiInstaller`：`version.txt` 是唯一判据。** 与桌面端 server 同口径（桌面也只读 WebUI
根目录的 `version.txt`）。zip 里那份结尾带换行，直接比会永远不相等 —— 表现是每次启动都重解
一遍 40MB，所以两边都 `trim` 后再比。解压是流式的（不把 40MB zip 读进内存），并防 zip slip。

**`PackageManagerRegistry`：为什么不做签名校验。** 与 Mihon 的取舍不同：这里没有
`TrustExtension` 那套数据库，也不过滤 NSFW —— 使用者就是服务器自己，而「能不能装」已经由
系统安装器把过关。

## 5. 产物形态选项（`-core` / `+jre`）

> **本节描述的选项组合已被取代**（在此留作施工记录）：`-core` 这个后缀后来被废除 ——
> 它就是"不带 JRE 的基线包"的代号，而给必然发生的事加后缀没有信息量。现在「形态」是
> **五个正交开关**（`pack_core` / `pack_jre` / `pack_msi` / `pack_exe` / `pack_oci`），产物名里完全
> 不带形态后缀；`make-jre.sh` 也已随沙盒搬到 Suwayomi-ext-runtime。当前规则以
> `docs/release.md` 的「产物形态」一节为准。

手动触发的构建除架构外还有一组**多选项**（可同时选中，非二选一）：

- `-core`：不打包 JRE，最小构建 —— **默认选中**。
- `+jre`：在 `-core` 产物基础上追加对应架构的 JRE。**该选项需要裁剪 JRE 体积、
  删除未被使用的部分**：用 `scripts/make-jre.sh` 走 jlink，按实测白名单只保留 14 个模块，
  180 MB → 39 MB（解压）、58 MB → 25 MB（压缩）。
- **Android 构建不参与这组选项**：Android 天然不打包 JRE，扩展跑在系统 ART 上。

需要裁剪产物的只有**非 Android 的 `+jre` 构建**；Android 的 APK 里没有 JRE 可裁。

实现与数据见 `docs/release.md` 的「产物形态」与「JRE 裁剪」两节；形态开关的本地校验
脚本是 `.workbuddy/verify/release_inputs_check.py`（真跑 prep 的 `out` 步骤，覆盖各开关
组合与两道守卫，项数随改动增长）。

`+jre` 带来的一个约束值得记下来：**jlink 不能跨平台生成运行时**，所以每个 target 的
runner 必须与目标同架构 —— linux-arm64 因此改用 `ubuntu-24.04-arm`（顺带变成原生编译，
不再需要交叉工具链），x64 的 macOS 用 `macos-15-intel`（`macos-13` 已被 GitHub 下线）。

## 6. 明确的非目标

- 不在 Android 上跑桌面沙盒 ext-runtime（不引入 dex2jar/ASM/AndroidCompat）。
- Android 上不由本项目**静默**安装扩展：一律经系统安装器 / 卸载器，用户可见可撤销。
- 不改桌面端的行为与产物布局（除新增 `SUWAYOMI_SANDBOX_URL` 这一可选入口）。
