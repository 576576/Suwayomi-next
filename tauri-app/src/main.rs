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
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
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
    /// 自定义工作数据目录（空 = 默认 base/data）
    data_dir: Option<String>,
    /// 启动时自动打开 WebUI（默认开）
    open_webui: bool,
    /// 有 Electron 壳时优先用 Electron 打开 WebUI（默认开）
    prefer_electron: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { port: 8090, data_dir: None, open_webui: true, prefer_electron: true }
    }
}

struct AppState {
    port: AtomicU16,
    data_dir: Mutex<PathBuf>,
    server: Mutex<Option<Child>>,
    /// Electron 壳进程（_wElectron 产物启动的桌面窗口），退出托盘时一并关闭
    electron: Mutex<Option<Child>>,
    /// true = 「隐藏托盘」退出：只退出托盘进程，server 保持后台运行；
    /// Drop 时跳过一切 server 清理（不 POST shutdown、不强杀、不 wait）。
    keep_server_on_exit: AtomicBool,
}

impl Drop for AppState {
    fn drop(&mut self) {
        if self.keep_server_on_exit.load(Ordering::Relaxed) {
            // 隐藏托盘：server 继续运行。`Child` drop 只释放句柄不杀进程，
            // server 的日志文件句柄由 server 自身持有，不受影响。
            return;
        }
        // 退出托盘：通知 server 优雅关闭（若还在运行），但**不等待**——托盘
        // 进程必须立即退出（视觉上立刻消失）。server 收到 shutdown 请求后在
        // 后台自行收尾（pg_ctl stop postgres、杀 JVM 沙盒），完成后进程自然
        // 退出；即使异常残留，下次启动的 stale-postmaster 自愈会兜底。
        let port = self.port.load(Ordering::Relaxed);
        if server_running(port) {
            request_graceful_shutdown(port);
        }
        // Electron 壳随托盘退出一并关闭（只释放句柄不阻塞）
        if let Some(mut c) = self.electron.lock().unwrap().take() {
            let _ = c.kill();
        }
        // 只释放 Child 句柄，不 kill / 不 wait：server 作为独立进程继续完成
        // 优雅关闭（Windows 父进程退出不会自动终止子进程）。
        if let Some(child) = self.server.lock().unwrap().take() {
            drop(child);
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
                // 发布布局：server 在 bin/ 子目录（suwayomi-server.exe）；旧布局同目录
                for cand in [
                    dir.join("bin").join("suwayomi-server.exe"),
                    dir.join("bin").join("suwayomi-server"),
                    dir.join("suwayomi-server.exe"),
                    dir.join("suwayomi-server"),
                    dir.join("suwayomi.exe"),
                    dir.join("suwayomi"),
                ] {
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

/// 是否已有 suwayomi-server 在运行。双检测兜底：
/// 1) 进程名匹配（sysinfo 枚举，个别情况下可能漏判/竞态）；
/// 2) 端口探测——server 就绪后端口必然可连，作为最终判定。
/// 两者任一命中即视为「已有实例」，避免重复 spawn。
fn server_running(port: u16) -> bool {
    use sysinfo::{ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let by_name = sys.processes().values().any(|p| {
        let n = p.name().to_string_lossy();
        SERVER_PROC_NAMES.iter().any(|x| n == *x)
    });
    if by_name {
        return true;
    }
    TcpStream::connect(("127.0.0.1", port)).is_ok()
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

/// 请求 server 优雅关闭（POST /api/v1/shutdown，loopback）。server 收到后会
/// 走 graceful shutdown：Db drop → oliphaunt `pg_ctl stop` 停 postgres 子进程、
/// JVM 沙盒子进程随 Drop 终止——所有进程干净退出，不留 postgres/java 残留。
fn request_graceful_shutdown(port: u16) {
    use std::io::{Read, Write};
    tray_log(&format!("[tray] requesting graceful shutdown on port {port}"));
    if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
        let req = format!(
            "POST /api/v1/shutdown HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(req.as_bytes());
        let mut buf = [0u8; 256];
        let _ = stream.read(&mut buf);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    }
}

/// 优雅停掉 server：先请求 shutdown，等待进程自然退出（graceful），超时再强杀
/// 兜底（强杀会残留 oliphaunt postgres 子进程，server 下次启动时自愈清理）。
fn stop_server_gracefully(port: u16) {
    if !server_running(port) {
        return;
    }
    request_graceful_shutdown(port);
    for _ in 0..12 {
        std::thread::sleep(Duration::from_millis(500));
        if !server_running(port) {
            tray_log("[tray] server exited gracefully");
            return;
        }
    }
    tray_log("[tray] graceful shutdown timed out; force-killing server");
    kill_server_processes();
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
        // 日志目录（server + JVM 沙盒输出统一落位）
        .env("SUWAYOMI_LOGS_DIR", logs.clone())
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

// ---- Electron 桌面壳（_wElectron 产物）----

/// Electron 运行时：_wElectron 产物内置 `electron/electron.exe`（解压后的
/// electron v44.1.0 win32-x64 完整目录）。普通产物无此目录 → 回退系统浏览器。
fn electron_exe() -> Option<PathBuf> {
    let cand = base_dir().join("electron").join("electron.exe");
    if cand.is_file() { Some(cand) } else { None }
}

/// 打开 WebUI：设置了「有 Electron 时优先使用」且存在 Electron 壳 → 启动
/// Electron 窗口（通过 SUWAYOMI_WEBUI_URL 环境变量传入本地 server 地址；
/// electron.exe 无参启动会自动加载 resources/app 里的应用入口）；否则用
/// 系统默认浏览器。
fn launch_webui(state: &AppState, prefer_electron: bool, port: u16) {
    if prefer_electron {
        if let Some(exe) = electron_exe() {
            tray_log(&format!("[tray] launching electron shell: {}", exe.display()));
            match Command::new(&exe)
                .env("SUWAYOMI_WEBUI_URL", webui_url(port))
                .spawn()
            {
                Ok(child) => {
                    *state.electron.lock().unwrap() = Some(child);
                    return;
                }
                Err(e) => tray_log(&format!("[tray] electron spawn failed: {e}")),
            }
        }
    }
    tray_log("[tray] opening webui in system browser");
    let _ = open::that(webui_url(port));
}

/// 关闭 Electron 壳：先杀本进程持有的 Child；再兜底枚举命令行指向本发布
/// 目录 electron\ 的 electron.exe 进程（防外部启动/句柄丢失）。
fn kill_electron_processes() {
    use sysinfo::{ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let base = base_dir().join("electron");
    let base_str = base.to_string_lossy().to_lowercase();
    let ids: Vec<_> = sys
        .processes()
        .values()
        .filter(|p| {
            let n = p.name().to_string_lossy();
            n.eq_ignore_ascii_case("electron.exe")
                && p.cmd().iter().any(|c| {
                    let s = c.to_string_lossy().to_lowercase();
                    s.contains(&base_str) || s.contains("suwayomi")
                })
        })
        .map(|p| p.pid())
        .collect();
    for id in ids {
        tray_log(&format!("[tray] killing electron pid {id}"));
        if let Some(p) = sys.process(id) {
            let _ = p.kill();
        }
    }
}

fn kill_owned_electron(state: &AppState) {
    if let Some(mut c) = state.electron.lock().unwrap().take() {
        let _ = c.kill();
    }
    kill_electron_processes();
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
    prefer_electron: bool,
    webui_url: String,
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> SettingsView {
    let port = state.port.load(Ordering::Relaxed);
    let d = state.data_dir.lock().unwrap().clone();
    let loaded = load_settings();
    let over = loaded.data_dir.unwrap_or_default();
    SettingsView {
        port,
        data_dir: d.display().to_string(),
        data_dir_override: over,
        open_webui: loaded.open_webui,
        prefer_electron: loaded.prefer_electron,
        webui_url: webui_url(port),
    }
}

#[tauri::command]
fn save_settings(
    state: State<AppState>,
    port: u16,
    data_dir: Option<String>,
    open_webui: bool,
    prefer_electron: bool,
) -> Result<String, String> {
    let port = port.max(1).min(65535);
    let clean = data_dir.map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
    let settings = Settings { port, data_dir: clean.clone(), open_webui, prefer_electron };
    save_settings_file(&settings).map_err(|e| format!("写入设置失败: {e}"))?;

    // 新工作目录：创建（含三个子目录）
    let new_data = data_dir_of(&settings);
    ensure_data_dirs(&new_data);
    std::fs::create_dir_all(&new_data).map_err(|e| format!("创建数据目录失败: {e}"))?;

    // 端口/目录变更：优雅结束旧 server 并以新配置重启（不残留 postgres）
    stop_server_gracefully(state.port.load(Ordering::Relaxed));
    // Electron 窗口指向旧端口/旧目录，一并关闭（用户可重新打开 WebUI）
    kill_owned_electron(&state);
    let mut guard = state.server.lock().unwrap();
    if let Some(mut c) = guard.take() {
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
            let running = server_running(port);
            tray_log(&format!("[tray] setup: server_running={running}"));
            let (server, ready) = if running {
                tray_log("[tray] server already running; not starting another");
                (None, false)
            } else {
                let s = spawn_server(&data, port);
                tray_log(&format!("[tray] spawn_server result: {}", s.is_some()));
                if s.is_none() {
                    tray_log("[tray] WARN: server binary not found (set SUWAYOMI_BIN or place suwayomi-server next to this exe)");
                }
                let ready = wait_ready(port, Duration::from_secs(20));
                (s, ready)
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
            // 以不可见状态创建——启动时完全不闪现，托盘「设置」菜单才 show()
            .visible(false)
            .build()?;

            // 系统托盘：启动Suwayomi（运行中则禁用）→ 打开WebUI → 数据目录 → 设置 → 隐藏托盘 → 退出
            let start_item = MenuItem::with_id(app, "start_suwayomi", "启动 Suwayomi", true, None::<&str>)?;
            let open_webui = MenuItem::with_id(app, "open_webui", "打开 WebUI", true, None::<&str>)?;
            let open_data = MenuItem::with_id(app, "open_data", "打开数据目录", true, None::<&str>)?;
            let settings_item = MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?;
            let hide_tray = MenuItem::with_id(app, "hide_tray", "隐藏托盘", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&start_item, &open_webui, &open_data, &settings_item, &hide_tray, &quit],
            )?;

            // 根据当前 server 状态刷新「启动 Suwayomi」项
            let running = server_running(port);
            let _ = start_item.set_enabled(!running);
            let _ = start_item.set_text(if running { "Suwayomi 运行中" } else { "启动 Suwayomi" });

            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().expect("default window icon").clone())
                .menu(&menu)
                .tooltip("Suwayomi")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "start_suwayomi" => {
                        let st = app.state::<AppState>();
                        let port = st.port.load(Ordering::Relaxed);
                        if server_running(port) {
                            return;
                        }
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
                        let port = st.port.load(Ordering::Relaxed);
                        let loaded = load_settings();
                        launch_webui(&st, loaded.prefer_electron, port);
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
                    "hide_tray" => {
                        // 只退出托盘进程，server 保持后台运行（下次打开
                        // suwayomi.exe 时会检测到已有实例而跳过重复启动）。
                        tray_log("[tray] hide_tray: keeping server running, exiting tray");
                        let st = app.state::<AppState>();
                        st.keep_server_on_exit.store(true, Ordering::Relaxed);
                        app.exit(0);
                    }
                    "quit" => {
                        // 托盘立刻关闭（视觉上不等待）：只发优雅关闭请求，
                        // server 在后台自行收尾（pg_ctl stop postgres、杀 JVM
                        // 沙盒）后退出；AppState::drop 同样不阻塞。Electron 壳
                        // 窗口随退出一并关闭。
                        let st = app.state::<AppState>();
                        let port = st.port.load(Ordering::Relaxed);
                        request_graceful_shutdown(port);
                        kill_owned_electron(&st);
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            let started = server.is_some();
            app.manage(AppState {
                port: AtomicU16::new(port),
                data_dir: Mutex::new(data),
                server: Mutex::new(server),
                electron: Mutex::new(None),
                keep_server_on_exit: AtomicBool::new(false),
            });

            // 启动时自动打开 WebUI（默认开启，设置里可关；有 Electron 则优先
            // 用 Electron 壳打开）——放在 manage 之后，launch_webui 需要 state。
            if started && settings.open_webui && ready {
                let st = app.state::<AppState>();
                launch_webui(&st, settings.prefer_electron, port);
            }
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
