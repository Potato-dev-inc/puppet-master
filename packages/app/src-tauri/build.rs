use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// `<short git sha>[-dirty]@<unix seconds>`, so a running bridge can say which build it is.
fn build_id() -> String {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let sha = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "nogit".into());
    let dirty = git(&["status", "--porcelain"]).is_some_and(|out| !out.is_empty());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    format!("{sha}{}@{now}", if dirty { "-dirty" } else { "" })
}

fn main() {
    println!("cargo:rustc-env=PM_BUILD_ID={}", build_id());
    // HEAD only changes on a branch switch; a commit moves the branch ref (or packed-refs), so
    // watch those too, otherwise a new commit keeps reporting the old build id.
    println!("cargo:rerun-if-changed=../../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../../.git/packed-refs");
    if let Ok(head) = std::fs::read_to_string("../../../.git/HEAD") {
        if let Some(reference) = head.trim().strip_prefix("ref: ") {
            println!("cargo:rerun-if-changed=../../../.git/{reference}");
        }
    }
    println!("cargo:rerun-if-changed=src");
    // Anything else that changes what a dev rebuild produces must also re-stamp the id, or live
    // evidence cannot tell which code ran: manifests, Tauri config, capabilities, the tests dir,
    // and the frontend sources.
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "tauri.conf.json",
        "tauri.dev.local.json",
        "capabilities",
        "resources",
        "tests",
        "../src",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    // A release build (the app and the MCP exe people retest) must carry a fresh build id every
    // time, even when nothing changed. A rerun-if-changed path that does not exist is always
    // considered stale, so cargo re-runs this script (and re-stamps the id) on every release
    // build. Debug builds keep the cheaper rule above so `cargo test` is not recompiled each run;
    // set PM_FORCE_NEW_BUILD_ID=1 to force it there too.
    println!("cargo:rerun-if-env-changed=PM_FORCE_NEW_BUILD_ID");
    let release = std::env::var("PROFILE").is_ok_and(|profile| profile == "release");
    if release || std::env::var_os("PM_FORCE_NEW_BUILD_ID").is_some() {
        println!("cargo:rerun-if-changed=.pm-always-new-build-id");
    }
    // The end-to-end tests (`tests/fake_tui_e2e.rs`, feature `dev-fake-tui`) link the whole app. Like
    // the app they need the Common Controls v6 manifest, or the exe does not load.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
        && std::env::var_os("CARGO_FEATURE_DEV_FAKE_TUI").is_some()
    {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_tui_e2e.manifest");
        println!("cargo:rerun-if-changed=tests/fake_tui_e2e.manifest");
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::build();
}
