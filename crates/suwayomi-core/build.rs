//! Stages the Oliphaunt (embedded PostgreSQL) native runtime artifacts into
//! `OUT_DIR` and emits `OLIPHAUNT_RESOURCES_DIR` for
//! `register_build_resources!()`. See `crates/suwayomi-core/src/db/manager.rs`.
//!
//! Also derives build metadata shared by the whole workspace:
//! - `SUWAYOMI_VERSION_NAME`  — `r{versionCode}` (or injected 3.y.z)
//! - `SUWAYOMI_VERSION_CODE`  — commit count + 3000
//! - `SUWAYOMI_VERSION_COUNT` — commit count
//! - `SUWAYOMI_BUILD_TIME`    — epoch seconds at compile time
//! - `SUWAYOMI_BUILD_TYPE`    — release channel: alpha / beta / release

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

    // 发布通道：CI 注入 SUWAYOMI_BUILD_TYPE（alpha / beta / release）。
    // 注意必须走编译期常量——运行时 env::var 在用户机器上读不到 CI 的变量；
    // 版本名在 release.yml 里是干净的 3.y.z / r{code}（通道后缀只出现在 tag 与
    // 文件名上），所以未注入时只能退回 release。
    let build_type = std::env::var("SUWAYOMI_BUILD_TYPE")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| {
            if version_name.contains("-alpha") {
                "alpha".to_string()
            } else if version_name.contains("-beta") {
                "beta".to_string()
            } else {
                "release".to_string()
            }
        });

    let build_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    println!("cargo:rustc-env=SUWAYOMI_VERSION_NAME={version_name}");
    println!("cargo:rustc-env=SUWAYOMI_VERSION_CODE={version_code}");
    println!("cargo:rustc-env=SUWAYOMI_VERSION_COUNT={count}");
    println!("cargo:rustc-env=SUWAYOMI_BUILD_TIME={build_time}");
    println!("cargo:rustc-env=SUWAYOMI_BUILD_TYPE={build_type}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SUWAYOMI_VERSION_COUNT");
    println!("cargo:rerun-if-env-changed=SUWAYOMI_VERSION_NAME");
    println!("cargo:rerun-if-env-changed=SUWAYOMI_BUILD_TYPE");
}
