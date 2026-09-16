//! Versioning: the release number is derived from the commit count.
//!
//! - internal version code = commit count + 3000  (e.g. 38 commits -> 3038)
//! - version name, two modes:
//!   1. `SUWAYOMI_VERSION_NAME` env (release.yml injects `3.y.z` for the
//!      release/beta channels, `r{versionCode}` for alpha)
//!   2. default `r{versionCode}` (Auto build / local, 38 -> r3038)
//!
//! The count comes from, in priority order:
//!   1. `SUWAYOMI_VERSION_COUNT` (injected by CI — matches `git rev-list --count HEAD`)
//!   2. `git rev-list --count HEAD` (local builds)
//!   3. fallback 38 (no git available)

use std::process::Command;

fn main() {
    let count = std::env::var("SUWAYOMI_VERSION_COUNT")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .or_else(|| {
            Command::new("git")
                .args(["rev-list", "--count", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .and_then(|s| s.trim().parse::<u64>().ok())
        })
        .unwrap_or(38);

    let version_code = count + 3000;
    // release.yml 注入（release/beta: 3.y.z；alpha: r{code}）；Auto build / 本地默认 r{versionCode}
    let version_name = std::env::var("SUWAYOMI_VERSION_NAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("r{version_code}"));

    println!("cargo:rustc-env=SUWAYOMI_VERSION_NAME={version_name}");
    println!("cargo:rustc-env=SUWAYOMI_VERSION_CODE={version_code}");
    println!("cargo:rustc-env=SUWAYOMI_VERSION_COUNT={count}");
    // the count changes on every commit, so always re-run
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SUWAYOMI_VERSION_COUNT");
    println!("cargo:rerun-if-env-changed=SUWAYOMI_VERSION_NAME");

    // Windows: embed the executable icon (generated from assets/images/icon.png).
    //
    // 两道判据缺一不可，各管一件事：
    // - `#[cfg(windows)]` 管**编译**：build script 的 cfg 是**宿主**平台，而 winres 就
    //   挂在 `[target.'cfg(windows)'.build-dependencies]` 下（同样按宿主求值）。少了
    //   它，Linux 宿主上这段代码会被照常编译，直接报 `cannot find crate winres`。
    // - `CARGO_CFG_TARGET_OS` 管**运行**：它是**目标**平台。Windows 宿主交叉编译到
    //   Android 时前者为真而这里必须跳过 —— 否则 winres 会报 "Can only compile
    //   resource file when target_env is gnu or msvc"。
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/images/icon.ico");
        let mut res = winres::WindowsResource::new();
        res.set_icon(&icon.to_string_lossy());
        if let Err(e) = res.compile() {
            panic!("embed icon failed: {e}");
        }
        println!("cargo:rerun-if-changed={}", icon.display());
    }
}
