use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// `<short git sha>[-dirty]@<unix seconds>`, so a running bridge can say which build it is.
fn git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn build_id() -> String {
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
    // Ask git where those are: in a `git worktree` checkout `.git` is a file, HEAD lives in the worktree's
    // own git dir and the refs in the shared one. A watched path that does not exist counts as changed on
    // every build, which re-stamped the id and recompiled this crate on every `cargo test` in a worktree.
    let git_dir = git(&["rev-parse", "--path-format=absolute", "--git-dir"]).unwrap_or_else(|| "../../../.git".into());
    let common_dir = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]).unwrap_or_else(|| git_dir.clone());
    // Only paths that exist: a fresh clone may have no `packed-refs`, and a packed branch no loose ref
    // file (then its folder is watched, which a commit creating the file changes).
    let watch_existing = |path: String| {
        if std::path::Path::new(&path).exists() {
            println!("cargo:rerun-if-changed={path}");
            true
        } else {
            false
        }
    };
    watch_existing(format!("{git_dir}/HEAD"));
    watch_existing(format!("{common_dir}/packed-refs"));
    if let Ok(head) = std::fs::read_to_string(format!("{git_dir}/HEAD")) {
        if let Some(reference) = head.trim().strip_prefix("ref: ") {
            let file = format!("{common_dir}/{reference}");
            if !watch_existing(file.clone()) {
                if let Some(folder) = std::path::Path::new(&file).parent() {
                    watch_existing(folder.to_string_lossy().into_owned());
                }
            }
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
        // `tauri.dev.local.json` is a local, gitignored override: watched only when it is there (a missing
        // path would rerun this script, and recompile the crate, on every debug build).
        watch_existing(path.to_string());
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
