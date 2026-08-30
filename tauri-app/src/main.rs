//! Suwayomi 桌面壳（Tauri 2）——只负责托盘与设置窗口
//!
//! - 无头启动 `suwayomi-server` 子进程；启动时静默驻托盘（不弹设置窗口）
//! - 发布布局（exe 同级）：
//!     suwayomi.exe + suwayomi-server.exe + jre/jvm-sandbox.jar + webui/ + data/
//!     data/ 为工作数据目录（autobackup/downloads/local，可在设置中更换）；
//!     pglite-data/ 与 extensions/ 由 server 自动建在发布根目录；日志在 logs/
//! - 系统托盘菜单：启动 Suwayomi / 打开 WebUI / 打开数据目录 / 设置 / 退出
//! - 设置窗口（暗黑主题）：端口、工作目录、启动打开 WebUI、打开数据目录

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::Write;
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
    /// 自定义工作数据目录（空 = base/data）
    data_dir: Option<String>,
    /// 启动时自动打开 WebUI（默认开）
    open_webui: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { port: 8090, data_dir: None, open_webui: true }
    }
}

struct AppState {
    port: AtomicU16,
    data_dir: Mutex<PathBuf>,
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

/// 工作数据目录解析：设置里自定义目录优先，否则 base/data
fn data_dir_of(s: &Settings) -> PathBuf {
    match s.data_dir.as_deref() {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d.trim()),
        _ => base_dir().join("data"),
    }
}

/// settings.json 位于发布根目录（数据目录本身可更换，不能存在数据目录里）
fn settings_path() -> PathBuf {
    base_dir().join("settings.json")
}

fn load_settings() -> Settings {
    let text = std::fs::read_to_string(settings_path()).ok()
        // 兼容旧位置（data/settings.json）
        .or_else(|| std::fs::read_to_string(base_dir().join("data").join("settings.json")).ok());
    text.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save_settings_file(settings: &Settings) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(settings).expect("serialize settings");
    std::fs::write(settings_path(), json)
}

/// Locate the headless server binary:
/// 1. `SUWAYOMI_BIN` env
/// 2. next to this tray executable (`suwayomi-server.exe` / `suwayomi-server`)
/// 3. on `PATH`
fn find_server_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SUWAYOMI_BIN") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    match std::env::current_exe() {
        Ok(exe) => {
            tray_log(&format!("[tray] current_exe: {}", exe.display()));
            if let Some(dir) = exe.parent() {
                for name in ["suwayomi-server.exe", "suwayomi-server", "suwayomi.exe", "suwayomi"] {
                    let cand = dir.join(name);
                    tray_log(&format!("[tray] find_server_bin: cand {} is_file={}", cand.display(), cand.is_file()));
                    if cand.is_file() {
                        return Some(cand);
                    }
                }
            }
        }
        Err(e) => tray_log(&format!("[tray] current_exe error: {e}")),
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in ["suwayomi-server.exe", "suwayomi-server", "suwayomi.exe", "suwayomi"] {
                let cand = dir.join(name);
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    None
}

const SERVER_PROC_NAMES: [&str; 2] = ["suwayomi-server.exe", "suwayomi-server"];

/// 是否已有 suwayomi-server 进程在运行
fn server_running() -> bool {
    use sysinfo::{ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    sys.processes().values().any(|p| {
        let n = p.name().to_string_lossy();
        SERVER_PROC_NAMES.iter().any(|x| n == *x)
    })
}

/// 杀掉所有 suwayomi-server 进程（含外部启动的）
fn kill_server_processes() {
    use sysinfo::{ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let ids: Vec<_> = sys
        .processes()
        .values()
        .filter(|p| {
            let n = p.name().to_string_lossy();
            SERVER_PROC_NAMES.iter().any(|x| n == *x)
        })
        .map(|p| p.pid())
        .collect();
    for id in ids {
        if let Some(p) = sys.process(id) {
            let _ = p.kill();
        }
    }
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

/// 工作数据目录（不存在时创建）。pglite-data 不在此列——
/// server 启动时会在 data 同级（发布根目录）自动创建。
fn ensure_data_dirs(data: &PathBuf) {
    for d in ["autobackup", "downloads", "local"] {
        let _ = std::fs::create_dir_all(data.join(d));
    }
}

fn spawn_server(data: &PathBuf, port: u16) -> Option<Child> {
    let bin = find_server_bin()?;
    // 日志统一放发布根目录 logs/（不混入 data/ 工作数据目录）
    let logs = base_dir().join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("server.log"))
        .ok()?;
    let child = Command::new(&bin)
        .current_dir(data)
        .env("SUWAYOMI_PORT", port.to_string())
        // pglite 数据目录 = 发布根目录，由 server 自动创建
        .env("SUWAYOMI_PGLITE_DATA_DIR", base_dir().join("pglite-data"))
        // 扩展安装目录 = 发布根目录（与 data/ 同级，也不是 cwd=data 下）
        .env("SUWAYOMI_EXTENSIONS_DIR", base_dir().join("extensions"))
        .env("SUWAYOMI_WEBUI_DIR", base_dir().join("webui"))
        .stdout(Stdio::from(log_file.try_clone().ok()?))
        .stderr(Stdio::from(log_file))
        .spawn()
        .ok()?;
    eprintln!("[tray] spawned server {:?} on port {port}", bin);
    Some(child)
}

/// 调试日志：写入 logs/tray.log（release GUI 无控制台，eprintln 不可见）
fn tray_log(msg: &str) {
    let dir = base_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("tray.log"))
    {
        let _ = writeln!(f, "{msg}");
    }
}

fn webui_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

// ---- 设置窗口 commands ----

#[derive(Serialize)]
struct SettingsView {
    port: u16,
    /// 当前实际生效的工作目录路径
    data_dir: String,
    /// 设置里的自定义目录（空 = 默认 base/data）
    data_dir_override: String,
    open_webui: bool,
    webui_url: String,
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> SettingsView {
    let port = state.port.load(Ordering::Relaxed);
    let d = state.data_dir.lock().unwrap().clone();
    let over = load_settings().data_dir.unwrap_or_default();
    SettingsView {
        port,
        data_dir: d.display().to_string(),
        data_dir_override: over,
        open_webui: load_settings().open_webui,
        webui_url: webui_url(port),
    }
}

#[tauri::command]
fn save_settings(
    state: State<AppState>,
    port: u16,
    data_dir: Option<String>,
    open_webui: bool,
) -> Result<String, String> {
    let port = port.max(1).min(65535);
    let clean = data_dir.map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
    let settings = Settings { port, data_dir: clean.clone(), open_webui };
    save_settings_file(&settings).map_err(|e| format!("写入设置失败: {e}"))?;

    // 新工作目录：创建（含三个子目录）
    let new_data = data_dir_of(&settings);
    ensure_data_dirs(&new_data);
    std::fs::create_dir_all(&new_data).map_err(|e| format!("创建数据目录失败: {e}"))?;

    // 端口/目录变更：结束旧 server 并以新配置重启
    let mut guard = state.server.lock().unwrap();
    if let Some(mut c) = guard.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    let child = spawn_server(&new_data, port)
        .ok_or_else(|| "server 启动失败（找不到 suwayomi-server 可执行文件）".to_string())?;
    *guard = Some(child);
    let _ = wait_ready(port, Duration::from_secs(20));

    state.port.store(port, Ordering::Relaxed);
    *state.data_dir.lock().unwrap() = new_data;
    Ok(webui_url(port))
}

#[tauri::command]
fn open_data_dir(state: State<AppState>) -> Result<(), String> {
    let d = state.data_dir.lock().unwrap().clone();
    open::that(d).map_err(|e| e.to_string())
}

#[tauri::command]
fn webui_url_cmd(state: State<AppState>) -> String {
    webui_url(state.port.load(Ordering::Relaxed))
}

/// 设置页前端（编译期内联；纯 cargo build 不嵌入 frontendDist 资产，
/// 故通过自定义协议提供，避免设置窗口白屏）。
const SETTINGS_HTML: &str = include_str!("../frontend/index.html");

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            open_data_dir,
            webui_url_cmd
        ])
        .register_uri_scheme_protocol("settings", |_ctx, _req| {
            tauri::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .body(SETTINGS_HTML.as_bytes().to_vec())
                .expect("build settings page response")
        })
        .setup(|app| {
            let settings = load_settings();
            let port = settings.port;
            let data = data_dir_of(&settings);
            ensure_data_dirs(&data);
            // 先检测：已有 server 实例（外部启动）则不再重复拉起
            let running = server_running();
            tray_log(&format!("[tray] setup: server_running={running}"));
            let server = if running {
                tray_log("[tray] server already running; not starting another");
                None
            } else {
                let s = spawn_server(&data, port);
                tray_log(&format!("[tray] spawn_server result: {}", s.is_some()));
                if s.is_none() {
                    tray_log("[tray] WARN: server binary not found (set SUWAYOMI_BIN or place suwayomi-server next to this exe)");
                }
                let ready = wait_ready(port, Duration::from_secs(20));
                // 启动时自动打开 WebUI（默认开启，设置里可关）
                if s.is_some() && settings.open_webui && ready {
                    tray_log("[tray] opening webui at startup");
                    let _ = open::that(webui_url(port));
                }
                s
            };

            // 设置窗口：静默创建（build 后隐藏，托盘菜单唤起）；暗黑主题跟随
            let _settings_window = WebviewWindowBuilder::new(
                app,
                "settings",
                WebviewUrl::CustomProtocol(
                    tauri::Url::parse("settings://localhost/index.html").expect("parse settings url"),
                ),
            )
            .title("Suwayomi 设置")
            .inner_size(480.0, 380.0)
            .resizable(false)
            .theme(Some(tauri::Theme::Dark))
            .build()?;
            // 启动不显示设置窗口——静默驻托盘
            let _ = _settings_window.hide();

            // 系统托盘：启动Suwayomi（运行中则禁用）→ 打开WebUI → 数据目录 → 设置 → 退出
            let start_item = MenuItem::with_id(app, "start_suwayomi", "启动 Suwayomi", true, None::<&str>)?;
            let open_webui = MenuItem::with_id(app, "open_webui", "打开 WebUI", true, None::<&str>)?;
            let open_data = MenuItem::with_id(app, "open_data", "打开数据目录", true, None::<&str>)?;
            let settings_item = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&start_item, &open_webui, &open_data, &settings_item, &quit],
            )?;

            // 根据当前 server 状态刷新「启动 Suwayomi」项
            let running = server_running();
            let _ = start_item.set_enabled(!running);
            let _ = start_item.set_text(if running { "Suwayomi 运行中" } else { "启动 Suwayomi" });

            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().expect("default window icon").clone())
                .menu(&menu)
                .tooltip("Suwayomi")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "start_suwayomi" => {
                        if server_running() {
                            return;
                        }
                        let st = app.state::<AppState>();
                        let port = st.port.load(Ordering::Relaxed);
                        let mut guard = st.server.lock().unwrap();
                        if guard.is_none() {
                            let d = st.data_dir.lock().unwrap().clone();
                            *guard = spawn_server(&d, port);
                            let _ = wait_ready(port, Duration::from_secs(20));
                        }
                        drop(guard);
                        if let Some(menu) = app.menu() {
                            if let Some(kind) = menu.get("start_suwayomi") {
                                if let Some(item) = kind.as_menuitem() {
                                    let _ = item.set_enabled(false);
                                    let _ = item.set_text("Suwayomi 运行中");
                                }
                            }
                        }
                    }
                    "open_webui" => {
                        let st = app.state::<AppState>();
                        let _ = open::that(webui_url(st.port.load(Ordering::Relaxed)));
                    }
                    "open_data" => {
                        let st = app.state::<AppState>();
                        let d = st.data_dir.lock().unwrap().clone();
                        let _ = open::that(d);
                    }
                    "settings" => {
                        if let Some(w) = app.get_webview_window("settings") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => {
                        kill_server_processes();
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            app.manage(AppState {
                port: AtomicU16::new(port),
                data_dir: Mutex::new(data),
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
