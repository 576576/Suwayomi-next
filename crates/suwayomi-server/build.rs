//! Versioning: the release number is derived from the commit count.
//!
//! - internal version code = commit count + 3000  (e.g. 38 commits -> 3038)
//! - external version name  = 3.{count/100}.{(count/10)%10}  (38 -> 3.0.3, 100 -> 3.1.0)
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
    let version_name = format!("3.{}.{}", count / 100, (count / 10) % 10);

    println!("cargo:rustc-env=SUWAYOMI_VERSION_NAME={version_name}");
    println!("cargo:rustc-env=SUWAYOMI_VERSION_CODE={version_code}");
    println!("cargo:rustc-env=SUWAYOMI_VERSION_COUNT={count}");
    // the count changes on every commit, so always re-run
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SUWAYOMI_VERSION_COUNT");

    // Windows: embed the executable icon (generated from assets/images/icon.png).
    #[cfg(windows)]
    {
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
