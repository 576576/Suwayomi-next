//! Suwayomi 桌面壳（Tauri 2）：驻托盘，拉起 suwayomi-server，用系统 WebView
//! （Win WebView2 / Linux WebKitGTK / macOS WKWebView）打开 WebUI/设置窗口；
//! WebView 不可用时回退系统浏览器。无图形会话（Linux）降级为前台跑 server。
//! 发布布局（exe 同级）：bin/suwayomi-server(.exe) + bin/jvm-sandbox.jar +
//! webui/ + data/(工作数据) + extensions/ + pglite-data/。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::io::Write;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sysinfo::{ProcessesToUpdate, System};
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
    /// 用窗口（系统 WebView）打开 WebUI，否则系统浏览器。保留旧 JSON key
    /// `prefer_electron` 兼容既有 settings.json。
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
    /// true = 「隐藏托盘」退出：托盘退出、server 保持后台，Drop 不做任何清理。
    keep_server_on_exit: AtomicBool,
}

impl Drop for AppState {
    fn drop(&mut self) {
        if self.keep_server_on_exit.load(Ordering::Relaxed) {
            return;
        }
        // 退出托盘：通知 server 优雅关闭但不等待（托盘须立即消失）；server
        // 自行收尾（停 postgres、杀 JVM 沙盒），异常残留由下次启动自愈兜底。
        let port = self.port.load(Ordering::Relaxed);
        if server_running(port) {
            request_graceful_shutdown(port);
        }
        if let Some(child) = self.server.lock().unwrap().take() {
            drop(child); // 只放句柄不 kill/wait：Windows 父进程退出不杀子进程
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

/// 定位 server 二进制：`SUWAYOMI_BIN` → exe 同级/`bin/` → PATH
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
                // 候选绝不含托盘自身（suwayomi(.exe)，同目录）：server 缺失时
                // 会把托盘自己当 server 反复 spawn（fork 炸弹）；覆盖用 SUWAYOMI_BIN。
                for cand in [
                    dir.join("bin").join("suwayomi-server.exe"),
                    dir.join("bin").join("suwayomi-server"),
                    dir.join("suwayomi-server.exe"),
                    dir.join("suwayomi-server"),
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
            for name in ["suwayomi-server.exe", "suwayomi-server"] {
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

/// 是否已有 suwayomi-server 在运行：进程名匹配或端口可连，任一命中即视为已有。
fn server_running(port: u16) -> bool {
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

/// 请求 server 优雅关闭（POST loopback /api/v1/shutdown：停 postgres、杀 JVM 沙盒）
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

/// 优雅停 server：请求 shutdown 后等进程退出，超时强杀兜底（强杀残留的
/// postgres 子进程由 server 下次启动自愈清理）。
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

/// 数据子目录（autobackup/downloads/local）不存在时创建
fn ensure_data_dirs(data: &PathBuf) {
    for d in ["autobackup", "downloads", "local"] {
        let _ = std::fs::create_dir_all(data.join(d));
    }
}

/// `inherit_stdio=true` 前台模式直接继承控制台；否则输出落 cache/logs/server.log
fn spawn_server(data: &PathBuf, port: u16, inherit_stdio: bool) -> Option<Child> {
    let bin = find_server_bin()?;
    let logs = base_dir().join("cache").join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("server.log"))
        .ok()?;
    let mut command = Command::new(&bin);
    command
        .current_dir(data)
        .env("SUWAYOMI_PORT", port.to_string())
        .env("SUWAYOMI_PGLITE_DATA_DIR", base_dir().join("pglite-data"))
        .env("SUWAYOMI_EXTENSIONS_DIR", base_dir().join("extensions"))
        // server cwd=data，不传 env 时 local_source_root 会落到 data/data/local
        .env("SUWAYOMI_LOCAL_SOURCE_DIR", base_dir().join("data").join("local"))
        .env("SUWAYOMI_DATA_DIR", base_dir().join("data"))
        .env("SUWAYOMI_LOGS_DIR", logs.clone())
        .env("SUWAYOMI_WEBUI_DIR", base_dir().join("webui"));

    if !inherit_stdio {
        let _ = command.stdout(Stdio::from(log_file.try_clone().ok()?)).stderr(Stdio::from(log_file));
    }

    let child = command.spawn().ok()?;
    if inherit_stdio {
        eprintln!("[tray] spawned server {:?} on port {port}", bin);
    }
    Some(child)
}

/// 调试日志：写入 cache/logs/tray.log（release GUI 无控制台，eprintln 不可见）
fn tray_log(msg: &str) {
    let dir = base_dir().join("cache").join("logs");
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

/// 打开/聚焦 WebUI 窗口（label "webui"，已存在→show+focus，销毁后重建）。
/// 返回 false = 系统 WebView 不可用，调用方应回退系统浏览器。
fn open_webui_window(app: &tauri::AppHandle, port: u16) -> bool {
    if let Some(w) = app.get_webview_window("webui") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        tray_log("[tray] webui window exists; focusing existing window");
        return true;
    }

    match WebviewWindowBuilder::new(
        app,
        "webui",
        WebviewUrl::External(tauri::Url::parse(&webui_url(port)).expect("parse webui url")),
    )
    .title("Suwayomi")
    .inner_size(1280.0, 860.0)
    .build()
    {
        Ok(w) => {
            // 不 set_icon：标题栏与任务栏共用 WM_SETICON，set 后任务栏也会变
            let _ = w.show();
            let _ = w.set_focus();
            tray_log(&format!("[tray] opened webui window: {}", webui_url(port)));
            true
        }
        Err(e) => {
            tray_log(&format!(
                "[tray] system webview unavailable ({e}); falling back to system browser"
            ));
            false
        }
    }
}

/// 打开 WebUI：设置开启且 WebView 窗口可用 → 窗口，否则系统浏览器
fn launch_webui(app: &tauri::AppHandle, port: u16) {
    let settings = load_settings();
    if settings.prefer_electron && open_webui_window(app, port) {
        return;
    }
    tray_log(&format!("[tray] opening webui in system browser: {}", webui_url(port)));
    let _ = open::that(webui_url(port));
}

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
    app: tauri::AppHandle,
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

    // 端口/目录变更：优雅停旧 server 再按新配置重启；关掉旧端口的 WebUI 窗口
    stop_server_gracefully(state.port.load(Ordering::Relaxed));
    if let Some(w) = app.get_webview_window("webui") {
        let _ = w.close();
    }
    let mut guard = state.server.lock().unwrap();
    if let Some(mut c) = guard.take() {
        let _ = c.wait();
    }
    let child = spawn_server(&new_data, port, false)
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

/// 设置页前端：编译期内联，经自定义协议提供（纯 cargo build 不嵌 frontendDist）
const SETTINGS_HTML: &str = include_str!("../frontend/index.html");

/// Linux 是否有图形会话（DISPLAY/WAYLAND_DISPLAY）。无会话时直接跑 GTK 会
/// panic，启动前探测并降级为前台 server 模式。
#[cfg(target_os = "linux")]
fn has_graphical_session() -> bool {
    ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|k| std::env::var(k).is_ok_and(|v| !v.trim().is_empty()))
}

/// 无图形会话降级：沿用 settings 的端口/数据目录前台跑 server（等价于直接
/// 跑 bin/suwayomi-server），不返回。
#[cfg(target_os = "linux")]
fn run_server_foreground() -> ! {
    let settings = load_settings();
    let port = settings.port;
    let data = data_dir_of(&settings);
    ensure_data_dirs(&data);

    if server_running(port) {
        eprintln!("suwayomi-server already running on port {port}; nothing to do");
        std::process::exit(1);
    }

    let Some(mut child) = spawn_server(&data, port, true) else {
        eprintln!(
            "suwayomi-server binary not found.\n\
             Place it in ./bin/ or set SUWAYOMI_BIN=/path/to/suwayomi-server"
        );
        std::process::exit(1);
    };

    if !wait_ready(port, Duration::from_secs(20)) {
        tray_log("[tray] foreground server did not become ready within 20s");
    }
    eprintln!("suwayomi-server ready on {} (Ctrl-C to stop)", webui_url(port));

    match child.wait() {
        Ok(status) => std::process::exit(status.code().unwrap_or(0)),
        Err(e) => {
            eprintln!("failed to wait for server: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "linux")]
fn maybe_run_headless() {
    if !has_graphical_session() {
        run_server_foreground();
    }
}

#[cfg(not(target_os = "linux"))]
fn maybe_run_headless() {}

/// 编译期内联 PNG（8-bit RGBA/RGB）解码为 tauri Image（托盘图标用）
fn decode_png_icon(data: &[u8]) -> tauri::image::Image<'static> {
    use png::ColorType;
    let mut reader = png::Decoder::new(data).read_info().expect("tray icon: png info");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("tray icon: png frame");
    let (w, h) = (info.width, info.height);
    let len = (w as usize) * (h as usize);
    let rgba = match (info.color_type, info.bit_depth) {
        (ColorType::Rgba, png::BitDepth::Eight) => buf[..len * 4].to_vec(),
        (ColorType::Rgb, png::BitDepth::Eight) => {
            let rgb = &buf[..len * 3];
            let mut out = Vec::with_capacity(len * 4);
            for px in rgb.chunks_exact(3) {
                out.extend_from_slice(px);
                out.push(255);
            }
            out
        }
        other => panic!("tray icon: unsupported png format {other:?}"),
    };
    tauri::image::Image::new_owned(rgba, w, h)
}

fn main() {
    // 必须早于 tauri::Builder：无图形会话时 GTK 初始化会直接 panic，来不及兜底
    maybe_run_headless();

    tauri::Builder::default()
        // 单实例：重复启动时第二实例直接退出，聚焦已有窗口
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray_log("[tray] single-instance: second launch detected, focusing existing window");
            if let Some(w) = app.get_webview_window("webui") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            } else if let Some(w) = app.get_webview_window("settings") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
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
            let (server, _ready) = if running {
                tray_log("[tray] server already running; not starting another");
                (None, false)
            } else {
                let s = spawn_server(&data, port, false);
                tray_log(&format!("[tray] spawn_server result: {}", s.is_some()));
                if s.is_none() {
                    tray_log("[tray] WARN: server binary not found (set SUWAYOMI_BIN or place suwayomi-server next to this exe)");
                }
                let ready = wait_ready(port, Duration::from_secs(20));
                (s, ready)
            };

            // 设置窗口：不可见创建（托盘菜单唤起）；WebView 不可用仅告警不中断
            let _settings_built = match WebviewWindowBuilder::new(
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
            .build()
            {
                Ok(_) => true,
                Err(e) => {
                    tray_log(&format!(
                        "[tray] settings window unavailable (no system webview?): {e}"
                    ));
                    false
                }
            };

            // 托盘菜单：启动/重启 → 打开 WebUI → 数据目录 → 设置 → 隐藏托盘 → 退出
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

            // 根据当前 server 状态刷新「启动/重启 Suwayomi」项（始终可点：
            // 运行中 = 重启，未运行 = 启动）
            let running = server_running(port);
            let _ = start_item.set_enabled(true);
            let _ = start_item.set_text(if running { "重启 Suwayomi" } else { "启动 Suwayomi" });

            // 托盘小图标用专用 tray.png（「坐」放大版，小尺寸可读）；不用
            // default_window_icon/exe ICO（缩放会糊）。窗口/任务栏仍走 exe ICO。
            let tray_icon = decode_png_icon(include_bytes!("../icons/tray.png"));
            let _tray = TrayIconBuilder::with_id("main")
                .icon(tray_icon)
                .menu(&menu)
                .tooltip("Suwayomi")
                .show_menu_on_left_click(false)
                // 单击托盘图标 = 打开 WebUI（与菜单「打开 WebUI」行为一致）
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        let st = app.state::<AppState>();
                        let port = st.port.load(Ordering::Relaxed);
                        launch_webui(app, port);
                    }
                })
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "start_suwayomi" => {
                        let st = app.state::<AppState>();
                        let port = st.port.load(Ordering::Relaxed);
                        if server_running(port) {
                            // 重启：优雅停（内部已等进程退出）后重新拉起
                            tray_log("[tray] restart: stopping running server");
                            stop_server_gracefully(port);
                            std::thread::sleep(Duration::from_secs(2));
                        }
                        let mut guard = st.server.lock().unwrap();
                        if let Some(child) = guard.take() {
                            drop(child);
                        }
                        let d = st.data_dir.lock().unwrap().clone();
                        *guard = spawn_server(&d, port, false);
                        drop(guard);
                        let _ = wait_ready(port, Duration::from_secs(25));
                    }
                    "open_webui" => {
                        let st = app.state::<AppState>();
                        let port = st.port.load(Ordering::Relaxed);
                        launch_webui(app, port);
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
                        } else {
                            // 无 WebView 引擎（精简系统）：设置窗口不可用
                            tray_log("[tray] settings window not available (no system webview)");
                        }
                    }
                    "hide_tray" => {
                        // 托盘退出、server 保持后台运行
                        tray_log("[tray] hide_tray: keeping server running, exiting tray");
                        let st = app.state::<AppState>();
                        st.keep_server_on_exit.store(true, Ordering::Relaxed);
                        app.exit(0);
                    }
                    "quit" => {
                        // 托盘立即退出：只发优雅关闭请求，server 后台自行收尾
                        let st = app.state::<AppState>();
                        let port = st.port.load(Ordering::Relaxed);
                        request_graceful_shutdown(port);
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
                keep_server_on_exit: AtomicBool::new(false),
            });

            // 启动即打开 WebUI（设置可关）：本托盘拉起或 server 已在运行都要开
            if settings.open_webui && (started || running) {
                launch_webui(app.handle(), port);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // settings 关闭=隐藏（驻托盘）；webui 关闭=真销毁（下次打开重建）
            if window.label() == "settings" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Suwayomi tray");
}
