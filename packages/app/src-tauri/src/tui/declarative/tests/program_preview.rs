//! The Approve dialog shows the program that will run in front of the arguments.

use std::fs;
use std::path::PathBuf;

use serde_json::json;

use super::KILO;
use crate::tui::declarative::parse_definition;
use crate::tui::declarative::resolve::ResolveError;
use crate::tui::declarative::service::{program_json, validate_text_in};

const GENERIC: &str = r#"
schema_version = 1
[agent]
id = "mycli"
label = "My CLI"
kind = "generic"
experimental = true
[binary]
names = ["mycli-fixture-not-installed"]
"#;

/// A folder holding only a fake `kilo` program; removed file by file when the test ends.
struct Bin {
    dir: PathBuf,
    file: PathBuf,
}

impl Bin {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pm-program-preview-{}-{tag}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join(if cfg!(windows) { "kilo.exe" } else { "kilo" });
        fs::write(&file, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self { dir, file }
    }
}

impl Drop for Bin {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.file);
        let _ = fs::remove_dir(&self.dir);
    }
}

#[test]
fn the_review_payload_names_the_program_that_will_run() {
    let bin = Bin::new("found");
    let v = validate_text_in(KILO, "user", &["claude"], &[bin.dir.clone()]).unwrap();
    let program = &v["program"];
    assert_eq!(program["found"], json!(true));
    assert_eq!(program["name"], json!("kilo"));
    assert_eq!(program["path"], json!(bin.file.to_string_lossy()));
    assert!(program["note"].is_null());
    assert_eq!(program["tui_argv_has_program"], json!(false));
    // the arguments stay exactly as they were: the program is a separate field
    assert_eq!(v["rendered"]["headless_argv"][0], json!("run"));
}

#[test]
fn a_program_that_is_not_installed_keeps_its_name_and_says_so() {
    let empty = std::env::temp_dir().join(format!("pm-program-preview-{}-empty", std::process::id()));
    fs::create_dir_all(&empty).unwrap();
    let v = validate_text_in(KILO, "user", &["claude"], &[empty.clone()]).unwrap();
    let _ = fs::remove_dir(&empty);
    let program = &v["program"];
    assert_eq!(program["found"], json!(false));
    assert_eq!(program["path"], json!(null));
    assert_eq!(program["display"], json!("kilo"));
    assert_eq!(program["note"], json!("not found on this PC"));
}

#[test]
fn a_shim_without_the_program_is_not_shown_as_found() {
    let def = parse_definition(KILO).unwrap();
    let v = program_json(&def, Err(ResolveError::ShimNeedsNative { shim: PathBuf::from("kilo.cmd") }));
    assert_eq!(v["found"], json!(false));
    assert_eq!(v["display"], json!("kilo"));
    assert!(v["note"].as_str().unwrap().contains("shim"));
}

#[test]
fn the_user_folder_is_shown_as_a_tilde() {
    let Some(home) = crate::project_path::home_dir() else { return };
    let def = parse_definition(KILO).unwrap();
    let path = home.join("tools").join("kilo.exe");
    let v = program_json(&def, Ok(path.clone()));
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(v["display"], json!(format!("~{sep}tools{sep}kilo.exe")));
    // the real path is still there for anything that needs it
    assert_eq!(v["path"], json!(path.to_string_lossy()));
    let outside = if cfg!(windows) { PathBuf::from(r"Z:\elsewhere\kilo.exe") } else { PathBuf::from("/opt/elsewhere/kilo") };
    let v = program_json(&def, Ok(outside.clone()));
    assert_eq!(v["display"], json!(outside.display().to_string()));
}

#[test]
fn a_generic_agents_visible_command_already_starts_with_its_program() {
    let v = validate_text_in(GENERIC, "user", &["claude"], &[]).unwrap();
    assert_eq!(v["rendered"]["tui_argv"], json!(["mycli-fixture-not-installed"]));
    assert_eq!(v["program"]["tui_argv_has_program"], json!(true));
}

#[test]
fn an_invalid_definition_has_no_program() {
    let v = validate_text_in("not a definition", "user", &["claude"], &[]).unwrap();
    assert!(v["program"].is_null());
}
