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
    println!("cargo:rerun-if-changed=../../../.git/HEAD");
    println!("cargo:rerun-if-changed=src");
    tauri_build::build();
}
