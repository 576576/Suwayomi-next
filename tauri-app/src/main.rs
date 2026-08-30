//! Suwayomi 桌面壳（Tauri 2）——只负责托盘与设置窗口
//!
//! - 无头启动 `suwayomi`（server 二进制）子进程
//! - 发布布局（exe 同级）：
//!     suwayomi.exe + suwayomi_launch.bat + webui/ + data/
//!     data/ 为工作数据目录：autobackup / downloads / local / pglite-data
//!     （不存在时自动创建）
//! - 系统托盘菜单：打开 WebUI / 打开数据目录 / 设置 / 退出
//! - 设置窗口：端口（保存后重启 server）、打开数据目录、WebUI 地址

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

#[derive(Serialize, Deserialize, Clone)]
struct Settings {
    port: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self { port: 8090 }
    }
}

struct AppState {
    port: AtomicU16,
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

/// 发布布局根目录 = 本 exe 同级
fn base_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn data_dir() -> PathBuf {
    base_dir().join("data")
}

fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

fn load_settings() -> Settings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
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

/// Poll TCP until the server accepts connections.
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

/// 工作数据目录（不存在时创建）
fn ensure_data_dirs(data: &PathBuf) {
    for d in ["autobackup", "downloads", "local", "pglite-data"] {
        let _ = std::fs::create_dir_all(data.join(d));
    }
}

fn spawn_server(data: &PathBuf, port: u16) -> Option<Child> {
    let bin = find_server_bin()?;
    let log = data.join("server.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .ok()?;
    let child = Command::new(&bin)
        .current_dir(data)
        .env("SUWAYOMI_PORT", port.to_string())
        .env("SUWAYOMI_PGLITE_DATA_DIR", data.join("pglite-data"))
        .env("SUWAYOMI_WEBUI_DIR", base_dir().join("webui"))
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

// ---- 设置窗口 commands ----

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    Settings { port: state.port.load(Ordering::Relaxed) }
}

#[tauri::command]
fn save_settings(state: State<AppState>, port: u16) -> Result<String, String> {
    let port = port.max(1).min(65535);
    let settings = Settings { port };
    std::fs::write(settings_path(), serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    // 端口变更：结束旧 server 并以新端口重启
    let mut guard = state.server.lock().unwrap();
    if let Some(mut c) = guard.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    let child = spawn_server(&state.data_dir, port).ok_or_else(|| "server 启动失败（找不到 suwayomi 可执行文件）".to_string())?;
    *guard = Some(child);
    let _ = wait_ready(port, Duration::from_secs(20));
    state.port.store(port, Ordering::Relaxed);
    Ok(webui_url(port))
}

#[tauri::command]
fn open_data_dir() -> Result<(), String> {
    open::that(data_dir()).map_err(|e| e.to_string())
}

#[tauri::command]
fn webui_url_cmd(state: State<AppState>) -> String {
    webui_url(state.port.load(Ordering::Relaxed))
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            open_data_dir,
            webui_url_cmd
        ])
        .setup(|app| {
            let settings = load_settings();
            let port = settings.port;
            let data = data_dir();
            ensure_data_dirs(&data);
            let server = spawn_server(&data, port);
            if server.is_none() {
                eprintln!("[tray] WARN: server binary not found (set SUWAYOMI_BIN or place suwayomi next to this exe)");
            }
            let _ = wait_ready(port, Duration::from_secs(20));

            // 设置窗口（默认隐藏，托盘菜单唤起）
            let _settings_window = WebviewWindowBuilder::new(
                app,
                "settings",
                WebviewUrl::App("index.html".into()),
            )
            .title("Suwayomi 设置")
            .inner_size(440.0, 320.0)
            .resizable(false)
            .build()?;

            // 系统托盘
            let open_webui = MenuItem::with_id(app, "open_webui", "打开 WebUI", true, None::<&str>)?;
            let open_data = MenuItem::with_id(app, "open_data", "打开数据目录", true, None::<&str>)?;
            let settings_item = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_webui, &open_data, &settings_item, &quit])?;

            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().expect("default window icon").clone())
                .menu(&menu)
                .tooltip("Suwayomi")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open_webui" => {
                        let st = app.state::<AppState>();
                        let _ = open::that(webui_url(st.port.load(Ordering::Relaxed)));
                    }
                    "open_data" => {
                        let _ = open::that(data_dir());
                    }
                    "settings" => {
                        if let Some(w) = app.get_webview_window("settings") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            app.manage(AppState {
                port: AtomicU16::new(port),
                data_dir: data,
                server: Mutex::new(server),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // 设置窗口关闭 = 隐藏（驻托盘）
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Suwayomi tray");
}
