# 运行时目录与端口

进程自己产生的状态归 `appdata/`，用户数据归 `data/`，两棵树各有唯一来源；端口全部在启动时解析一次。

## 目录树

| 路径 | 写入者 | 内容 |
|---|---|---|
| `<appdata>/cache` | server | 统一缓存根：扩展图标（`extensions/icons`）、图片代理（`images`）、缩略图、扩展仓库索引、本地封面 |
| `<appdata>/logs` | server / 托盘 | `server.log`（托盘托管的会话）、`sandbox.log`（JVM 的 stdout/stderr 重定向）、`tray.log` |
| `<appdata>/db` | server | SQLite 库 `suwayomi.db`、`session.key` |
| `<appdata>/settings` | server + 沙盒 | `trackers.json`（服务端）、`source_<id>.properties`（沙盒写的源偏好） |
| `<appdata>/extensions/apk` | 扩展安装流程 | 被沙盒扫描加载的扩展 APK |
| `<appdata>/extensions/bin` | 沙盒 | dex2jar 产物，不参与加载扫描 |
| `<data>/downloads` | server | CBZ 等下载产物；被 `downloadsPath` 设置整体替换 |
| `<data>/local` | server | 本地图源；被 `localSourcePath` 设置整体替换 |
| `<data>/autobackup` | server | 定时备份落点；被 `backupPath` 设置整体替换 |

- Rust 侧的唯一定义点是 `crates/suwayomi-core/src/config/paths.rs` 的 `AppPaths`（子目录名字面量也在那里）。
- **两侧各有一套同名子路径**，改动必须同步：沙盒按 `SUWAYOMI_APPDATA_DIR` 自行派生 `extensions/apk`、`extensions/bin`、`settings`（`ext-runtime/src/main/kotlin/sandbox/Main.kt`）；对不上就各写一半。
- `appdata` 根只有 `SUWAYOMI_APPDATA_DIR` 一个旋钮，它下面的子目录**没有各自的环境变量**，全由根派生（`SUWAYOMI_DATA_DIR` 同理只给 data 根）。
- 库在 appdata 下、不在 data 下：挪 `data/` 不会动到库与设置。

## 目录树模板

`assets/templates/directory/` 是这棵树的**唯一清单**（每个叶目录一个 `.gitkeep`），四处复制它、再滤掉 `.gitkeep`：

| 消费点 | 做法 |
|---|---|
| `.github/workflows/build.yml` | `cp -R assets/templates/directory/. "$STAGE/"` + `find "$STAGE" -name .gitkeep -delete` |
| `build.bat` | `xcopy /e /i /y "assets\templates\directory\*" "%STAGE%\"` + `del /s /q "%STAGE%\*.gitkeep"` |
| `Dockerfile` | `COPY assets/templates/directory/data /data/`、`COPY .../appdata /data/appdata/` + `find /data -name .gitkeep -delete` |
| `.workbuddy/verify/run_latest_server.py` | 部署本地实例前同样按模板准备目录树 |

- 加一个子目录只改模板一处，四处不再各自维护 mkdir 列表；副本之间的偏差是静默的（少一层目录要等首启现造才发现）。
- `.gitkeep` 必须滤掉：它只是 Git 的占位，进了产物就是无主的垃圾文件。
- `.gitignore` 的 `data/` 与 `extensions` 是任意层级模式，会连模板一起吃掉 —— 两处 `!assets/templates/directory/...` 例外不能删（少一条会有半棵树进不了 Git，`git status` 只显示被跟踪的那半）。
- `.dockerignore` 的同名规则是 `/` 前缀的根层匹配，不能改成 `**/` 形式，否则构建上下文里的模板会被一起排掉、`COPY` 直接失败。

## 端口

| 端口 | 归属 | 定义点 | 说明 |
|---|---|---|---|
| **4567** | server HTTP | `ServerConfig::default().port`（`suwayomi-core/src/config.rs`）；`SUWAYOMI_PORT` 覆盖 | 与上游一致的默认值 |
| **4568** | 沙盒（ext-runtime JVM） | `DEFAULT_SANDBOX_PORT`（`suwayomi-domain/src/source/sandbox.rs`）；`SUWAYOMI_SANDBOX_PORT` 覆盖 | 托盘恒传一个 ≠ server 的值 |
| 4569 | 沙盒兜底默认 | `ext-runtime/.../sandbox/Main.kt` | 只有裸跑 `ext-runtime.jar` 又不传 env 时用到；Rust 侧总会显式传值，所以与 4568 不一致也不影响发布形态 |
| 4567…4567+32 | 托盘自动档的嗅探范围 | `PORT_SNIFF_TRIES`（Suwayomi-tray） | 从默认端口起找首个能绑的 |
| 4901 | server 自顺延跳点 | `suwayomi-server/src/lib.rs` 的监听循环 | bind 报 `10013` 且端口 ≤ 4900 时直接跳 4901，跳过整段 Hyper-V 保留区；其余失败逐格 +1，最多上探 50 步 |
| **4570** | Android 扩展宿主 | `SuwayomiApp.kt` 的 `EXTENSION_HOST_PORT` | 桌面形态没有这个端口 |

开发侧端口（不属于交付物，全部只出现在 `.workbuddy/verify/` 与 CI 里）：

| 端口 | 用途 |
|---|---|
| 4599 / 4601 | ext-lab 全量试验台 / 单包探针沙盒（`ext_probe.py`） |
| 18899 | 连接日志调试代理（`connlog_proxy.py`），经 `SUWAYOMI_SANDBOX_PROXY` 接给沙盒 |
| 8123 | 直跑 server 的验证实例（`run_verify.sh`） |
| 9223 | 托盘 WebView2 远程调试（`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`） |
| 18090 | OCI 冒烟容器映射的宿主端口（`build.yml`）、MSI 安装冒烟 |
| 9569 / 9624 / 45711 | seed 服务 / 排序种子 / 旧仓库索引假服务 |

## 端口规则

- **只有 server 与沙盒两个端口是运行时可变的**，都在启动前解析一次（托盘的 `resolve_ports`）。设置里写死的端口用不了就**拒绝启动**：静默换端口会让按原端口配的防火墙与端口转发失效，界面上还看不出服务搬去了哪。
- **沙盒端口恒不等于 server 端口**：沙盒启动时会清掉端口占用者，撞上等于 server 被自己拉起的沙盒杀掉。
- 自动档最多上探 32 格；挑不出沙盒端口时退回默认值让沙盒自己顺延 —— 扩展不可用不该挡住书架与阅读。
- 报告"服务在哪个端口"一律读运行态（server 实际监听的那个），不读设置里的值：自动档下两者经常不同。
- 托盘的「服务器地址」只影响 **WebUI 打开的目标**，不改变 server 的监听地址；它同时被加进 WebView 的允许源，否则指向远端 server 时 WebUI 自身的跳转会被判成外部链接、丢给系统浏览器。
