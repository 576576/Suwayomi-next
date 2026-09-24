//! Server 可执行文件入口。
//!
//! 只做「进程外壳」的事：版本号、单实例互斥、日志初始化、从 env/CLI 组装
//! [`ServerOptions`]，然后交给 [`suwayomi_server::run`]。启动逻辑本身在 lib.rs，
//! Android 宿主 App 走 JNI 调同一个 `run`（见 crates/suwayomi-android 与
//! docs/migration/ANDROID_IMPL.md）。

// release 无控制台窗口（隐藏启动在真实系统上可能被安全软件拦截）；日志由父进程重定向
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use suwayomi_server::{
    SandboxMode, ServerOptions, VERSION, config_from_env, init_logging, resolve_data_dir, resolve_sandbox_jar,
    resolve_webui_dir,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // `-v` / `--version`：打印版本信息后退出（替代打包产物里的 VERSION.txt）
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    if cli_args.iter().any(|a| a == "-v" || a == "--version") {
        println!("Suwayomi {VERSION}");
        println!("{}", env!("CARGO_PKG_REPOSITORY"));
        return Ok(());
    }

    // 单实例：命名互斥体，已运行则退出（避免多开）
    #[cfg(windows)]
    let _instance_guard = {
        use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let name: Vec<u16> = "SuwayomiServerSingleInstance"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            let already = h.is_null() || GetLastError() == ERROR_ALREADY_EXISTS;
            if already {
                if !h.is_null() {
                    CloseHandle(h);
                }
                eprintln!("suwayomi-server 已在运行（单实例），本实例退出");
                return Ok(());
            }
            h
        }
    };
    #[cfg(not(windows))]
    let _instance_guard = ();

    init_logging("info");

    // 扩展来源：`SUWAYOMI_SANDBOX_URL`（已运行的扩展宿主，如 Android 宿主 App）优先，
    // 其次本地 ext-runtime.jar（见 resolve_sandbox_jar 的发布布局），都没有则不接扩展。
    let sandbox = match std::env::var("SUWAYOMI_SANDBOX_URL") {
        Ok(url) if !url.trim().is_empty() => SandboxMode::External { base_url: url },
        _ => match resolve_sandbox_jar() {
            Some(jar) => SandboxMode::Spawn {
                // 默认 8091：避开 Windows Hyper-V 动态保留区 4501-4900
                port: std::env::var("SUWAYOMI_SANDBOX_PORT").unwrap_or_else(|_| "8091".into()),
                jar,
            },
            None => SandboxMode::Disabled,
        },
    };

    let options = ServerOptions {
        config: config_from_env(),
        data_dir: resolve_data_dir(),
        webui_dir: resolve_webui_dir(),
        // `None` → DbSettings::from_env()（SUWAYOMI_DB_BACKEND / SUWAYOMI_DATABASE_URL /
        // SUWAYOMI_SQLITE_PATH）；Android 宿主显式传 SQLite 路径（App 里没有 env）
        db: None,
        sandbox,
        // 桌面没有「宿主」：靠 Ctrl+C 与 POST /api/v1/shutdown 关闭
        shutdown: None,
    };
    suwayomi_server::run(options).await
}
