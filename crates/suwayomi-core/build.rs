//! Stages the Oliphaunt (embedded PostgreSQL) native runtime artifacts into
//! `OUT_DIR` and emits `OLIPHAUNT_RESOURCES_DIR` for
//! `register_build_resources!()`. See `crates/suwayomi-core/src/db/manager.rs`.
//!
//! Also derives build metadata shared by the whole workspace:
//! - `SUWAYOMI_VERSION_NAME`  — `r{versionCode}` (or injected 3.y.z)
//! - `SUWAYOMI_VERSION_CODE`  — commit count + 3000
//! - `SUWAYOMI_VERSION_COUNT` — commit count
//! - `SUWAYOMI_BUILD_TIME`    — epoch seconds at compile time

use std::process::Command;

fn main() {
    oliphaunt_build::configure();

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
    let version_name = std::env::var("SUWAYOMI_VERSION_NAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("r{version_code}"));

    let build_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    println!("cargo:rustc-env=SUWAYOMI_VERSION_NAME={version_name}");
    println!("cargo:rustc-env=SUWAYOMI_VERSION_CODE={version_code}");
    println!("cargo:rustc-env=SUWAYOMI_VERSION_COUNT={count}");
    println!("cargo:rustc-env=SUWAYOMI_BUILD_TIME={build_time}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SUWAYOMI_VERSION_COUNT");
    println!("cargo:rerun-if-env-changed=SUWAYOMI_VERSION_NAME");
}
