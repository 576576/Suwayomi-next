//! Suwayomi 桌面壳（Tauri 2）
//!
//! - 无头启动 `suwayomi`（server 二进制）子进程，数据落在系统应用数据目录
//!   （Windows: %APPDATA%\Suwayomi，内含 pglite-data/ 与 server.log）
//! - 主窗口：WebView 加载 http://127.0.0.1:{port} 的 WebUI
//! - 系统托盘（对齐 Suwayomi SystemTray.kt 并扩展）：
//!   打开 WebUI / 打开数据目录 / 退出
//! - 关闭窗口驻留托盘；退出托盘菜单时结束 server 子进程

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

struct AppState {
    port: u16,
    data_dir: PathBuf,
    server: Mutex<Option<Child>>,
}

impl Drop for AppState {
    fn drop(&mut self) {
        if let Some(mut child) = self.server.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Locate the headless server binary:
/// 1. `SUWAYOMI_BIN` env
/// 2. next to this tray executable (`suwayomi.exe` / `suwayomi`)
/// 3. on `PATH`
fn find_server_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SUWAYOMI_BIN") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["suwayomi.exe", "suwayomi"] {
                let cand = dir.join(name);
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in ["suwayomi.exe", "suwayomi"] {
                let cand = dir.join(name);
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    None
}

/// Poll TCP until the server accepts connections (it may take a while to
/// boot the embedded PGlite database).
fn wait_ready(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

fn spawn_server(data_dir: &PathBuf, port: u16) -> Option<Child> {
    let bin = find_server_bin()?;
    let log = data_dir.join("server.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .ok()?;
    let child = Command::new(&bin)
        .current_dir(data_dir)
        .env("SUWAYOMI_PORT", port.to_string())
        .env("SUWAYOMI_PGLITE_DATA_DIR", data_dir.join("pglite-data"))
        .stdout(Stdio::from(log_file.try_clone().ok()?))
        .stderr(Stdio::from(log_file))
        .spawn()
        .ok()?;
    eprintln!("[tray] spawned server {:?} on port {port}", bin);
    Some(child)
}

fn webui_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let port: u16 = std::env::var("SUWAYOMI_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8090);

            let data_dir = app.path().app_data_dir().expect("resolve app data dir");
            std::fs::create_dir_all(&data_dir)
                .expect("create app data dir");

            let server = spawn_server(&data_dir, port);
            if server.is_none() {
                eprintln!("[tray] WARN: server binary not found (set SUWAYOMI_BIN or place suwayomi next to this exe)");
            }

            // 主窗口：等 server 就绪后加载 WebUI
            let window = WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External(webui_url(port).parse().expect("parse webui url")),
            )
            .title("Suwayomi")
            .inner_size(1280.0, 800.0)
            .min_inner_size(800.0, 600.0)
            .build()?;

            if wait_ready(port, Duration::from_secs(20)) {
                window.show()?;
            } else {
                // server 未能就绪：仍显示窗口（页面会显示连接失败），托盘可重试
                window.show()?;
                eprintln!("[tray] WARN: server not ready on port {port} after 20s");
            }

            // 系统托盘
            let open_webui = MenuItem::with_id(app, "open_webui", "打开 WebUI", true, None::<&str>)?;
            let open_data = MenuItem::with_id(app, "open_data", "打开数据目录", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_webui, &open_data, &quit])?;

            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().expect("default window icon").clone())
                .menu(&menu)
                .tooltip("Suwayomi")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open_webui" => {
                        let st = app.state::<AppState>();
                        let _ = open::that(webui_url(st.port));
                    }
                    "open_data" => {
                        let st = app.state::<AppState>();
                        let _ = open::that(&st.data_dir);
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            app.manage(AppState {
                port,
                data_dir,
                server: Mutex::new(server),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭窗口 = 隐藏到托盘，不退出
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Suwayomi tray");
}
