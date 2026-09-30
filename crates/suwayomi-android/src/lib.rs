//! Android 宿主 App 的 JNI 入口（`cdylib`，产物 `libsuwayomi_android.so` 随 APK 的
//! `jniLibs/arm64-v8a/` 分发）。
//!
//! Android 10+ 禁止 App 从私有目录 `exec` 可执行文件，桌面那条「spawn 一个
//! suwayomi-server 子进程」的路走不通；`dlopen`（`System.loadLibrary`）不受限制，
//! 所以整个 server 以库的形式嵌进宿主进程，由 Kotlin 侧调用这里的入口。
//!
//! Kotlin 对应声明：
//!
//! ```kotlin
//! object NativeServer {
//!     init { System.loadLibrary("suwayomi_android") }
//!     external fun start(dataDir: String, webuiDir: String, ip: String, port: Int, sandboxUrl: String): Int
//!     external fun stop(): Int
//!     external fun version(): String
//! }
//! ```
//!
//! 返回值约定：`0` = 已受理（server 在后台线程启动，是否就绪由宿主轮询端口/`/health`），
//! 非 0 = 参数或启动失败（具体原因在 logcat 里，tag `Suwayomi`）。
//!
//! 一个进程只 `start()` 一次：tokio runtime 与关闭通道都是进程级单例。

// 测试代码允许 panic：unwrap / expect / panic! 在断言里是常规写法，
// 逐个改成 `?` 传播只会让失败信息更难读。生产代码不受这条影响
// （`cfg_attr(test, ...)`）。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::todo,
        clippy::indexing_slicing
    )
)]
// JNI 边界本身就是 unsafe 的：入口函数必须 `#[no_mangle]` + `extern "system"`
// 才能被 `System.loadLibrary` 解析到，没有任何 safe 封装可用。整个 crate 就是
// 这一层边界（194 行，不含业务逻辑），所以在 crate 级放行 unsafe。
#![allow(unsafe_code)]

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use jni::JNIEnv;
use jni::objects::{JObject, JString};
use jni::sys::{jint, jstring};

use suwayomi_core::config::ServerConfig;
use suwayomi_db::DbSettings;
use suwayomi_server::{SandboxMode, ServerOptions};

/// 后台 runtime：活到进程结束（App 退出时由系统回收进程）。
static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// `stop()` 用的关闭通道 —— `run()` 收到它就优雅关闭（释放 SQLite 文件锁等）。
static SHUTDOWN: Mutex<Option<tokio::sync::watch::Sender<bool>>> = Mutex::new(None);

/// `Java_org_suwayomi_next_NativeServer_start` —— 启动 server。
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_suwayomi_next_NativeServer_start<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    data_dir: JString<'local>,
    webui_dir: JString<'local>,
    ip: JString<'local>,
    port: jint,
    sandbox_url: JString<'local>,
) -> jint {
    // Rust panic 不能穿过 FFI 边界：默认会 unwind 到 Kotlin，Abort —— 整个 App 挂掉
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        start_inner(&mut env, data_dir, webui_dir, ip, port, sandbox_url)
    }))
    .unwrap_or_else(|_| {
        tracing::error!("panic while starting the server (see the Rust backtrace above)");
        9
    })
}

/// `Java_org_suwayomi_next_NativeServer_stop` —— 请求优雅关闭。
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_suwayomi_next_NativeServer_stop<'local>(
    _env: JNIEnv<'local>,
    _this: JObject<'local>,
) -> jint {
    let guard = match SHUTDOWN.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.as_ref().map_or(1, |tx| {
        // 没人在等（run 已结束）时 send 会失败，忽略即可
        let _ = tx.send(true);
        0
    })
}

/// `Java_org_suwayomi_next_NativeServer_version` —— `r{versionCode}`，给宿主显示用。
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_suwayomi_next_NativeServer_version<'local>(
    env: JNIEnv<'local>,
    _this: JObject<'local>,
) -> jstring {
    env.new_string(suwayomi_server::VERSION).map_or(std::ptr::null_mut(), jni::objects::JString::into_raw)
}

fn start_inner(
    env: &mut JNIEnv<'_>,
    data_dir: JString<'_>,
    webui_dir: JString<'_>,
    ip: JString<'_>,
    port: jint,
    sandbox_url: JString<'_>,
) -> jint {
    suwayomi_server::init_logging("info");

    let data_dir = match read_string(env, &data_dir) {
        Some(s) => PathBuf::from(s),
        None => return 2,
    };
    let webui_dir = match read_string(env, &webui_dir) {
        Some(s) => PathBuf::from(s),
        None => return 2,
    };
    let ip = read_string(env, &ip).unwrap_or_else(|| "127.0.0.1".to_owned());
    let sandbox_url = read_string(env, &sandbox_url).unwrap_or_default();
    if port <= 0 || port > u16::MAX as jint {
        tracing::error!("invalid port {port}");
        return 3;
    }

    // 数据目录必须存在：SQLite 会在这里建库，backups/downloads 也挂在下面
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        tracing::error!("cannot create data dir {}: {e}", data_dir.display());
        return 4;
    }

    // Android 没有环境变量可设，appdata 根必须显式钉住：否则默认推导会退化成相对
    // 路径 `appdata`，而进程 CWD 是 `/`，写 `/appdata/…` 恒失败。宿主传进来的
    // 应用私有目录就是这里唯一的可写根。
    let appdata_dir = data_dir.clone();

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    {
        let mut guard = match SHUTDOWN.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if guard.is_some() {
            tracing::warn!("server already started in this process; ignoring the second start()");
            return 5;
        }
        *guard = Some(shutdown_tx);
    }

    let options = ServerOptions {
        config: ServerConfig {
            ip,
            port,
            // `database_type` 是 Kotlin 时代遗留的对外字段（WebUI 设置页会显示），
            // 与真实后端无关；这里与桌面 config_from_env() 保持一致。
            database_type: suwayomi_core::config::DatabaseType::Rusqlite,
            ..ServerConfig::default()
        },
        data_dir: data_dir.clone(),
        // 宿主没有环境变量：appdata 根直接给出（缓存 / 库 / 设置 / 扩展都挂在它下面）
        appdata_dir: Some(appdata_dir),
        webui_dir,
        // Android 上没有环境变量可用，数据库设置显式给出：固定在**启动时传入的**应用私有
        // 目录下（不走 suwayomi-db 的目录解析，因此不会随 WebUI「存储位置」设置漂移）。
        // 设置里的 dataDir 只改运行期数据目录（下载/本地图源/自动备份），库的位置不变。
        db: Some(DbSettings::sqlite(data_dir.join("suwayomi.db"))),
        // Android 不用桌面沙盒 jar：由宿主 App 同进程提供扩展宿主（回环 HTTP）
        sandbox: if sandbox_url.trim().is_empty() {
            SandboxMode::Disabled
        } else {
            SandboxMode::External { base_url: sandbox_url }
        },
        shutdown: Some(shutdown_rx),
    };

    // 建 runtime 会失败（IO 线程池起不来等）。以前这里 `expect` 直接崩进程，
    // 而 JNI 崩溃在 Android 上表现为整个 App 闪退且抓不到 Java 栈；改成返回错误码，
    // 宿主 App 能按 6 处理（与下面的 4/5 同一套约定）。
    let runtime = match RUNTIME.get() {
        Some(r) => r,
        None => match tokio::runtime::Builder::new_multi_thread().enable_all().thread_name("suwayomi").build() {
            Ok(r) => RUNTIME.get_or_init(|| r),
            Err(e) => {
                tracing::error!("cannot create tokio runtime: {e}");
                return 6;
            }
        },
    };
    runtime.spawn(async move {
        if let Err(e) = suwayomi_server::run(options).await {
            tracing::error!("server exited with error: {e}");
        } else {
            tracing::info!("server stopped");
        }
    });
    0
}

fn read_string(env: &mut JNIEnv<'_>, s: &JString<'_>) -> Option<String> {
    match env.get_string(s) {
        Ok(v) => Some(v.into()),
        Err(e) => {
            tracing::error!("cannot read JNI string: {e}");
            None
        }
    }
}
