//! The bundled folder lookup: the installed layout first, the plain one second, never a folder without its marker.

use std::path::PathBuf;

use super::resource_dirs::{candidates, find, without_verbatim_prefix};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-resource-dirs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_installed_layout_comes_first() {
    assert_eq!(candidates("potato-sandbox"), ["resources/potato-sandbox".to_string(), "potato-sandbox".to_string()]);
}

#[test]
fn a_folder_with_its_marker_is_found_in_either_layout() {
    let root = temp("either");
    std::fs::create_dir_all(root.join("resources/potato-sandbox/recipes")).unwrap();
    let found = find("potato-sandbox", "recipes", |rel| Some(root.join(rel))).unwrap();
    assert_eq!(found, root.join("resources/potato-sandbox"));

    let plain = temp("plain");
    std::fs::create_dir_all(plain.join("potato-sandbox/recipes")).unwrap();
    assert_eq!(find("potato-sandbox", "recipes", |rel| Some(plain.join(rel))).unwrap(), plain.join("potato-sandbox"));
    let _ = (std::fs::remove_dir_all(&root), std::fs::remove_dir_all(&plain));
}

#[test]
fn a_folder_without_its_marker_is_not_used() {
    let root = temp("half");
    std::fs::create_dir_all(root.join("resources/potato-sandbox")).unwrap();
    std::fs::create_dir_all(root.join("potato-sandbox/recipes")).unwrap();
    // The installed layout is empty (a half copy), so the plain one answers.
    assert_eq!(find("potato-sandbox", "recipes", |rel| Some(root.join(rel))).unwrap(), root.join("potato-sandbox"));
    assert!(find("potato-sandbox", "viewer", |rel| Some(root.join(rel))).is_none());
    assert!(find("potato-sandbox", "recipes", |_| None).is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_extended_length_prefix_is_dropped_so_node_can_run_the_script() {
    let plain = |s: &str| without_verbatim_prefix(std::path::Path::new(s));
    assert_eq!(plain(r"\\?\C:\app\resources\potato-brain"), PathBuf::from(r"C:\app\resources\potato-brain"));
    assert_eq!(plain(r"\\?\UNC\host\share\x"), PathBuf::from(r"\\host\share\x"));
    // nothing else is touched
    assert_eq!(plain(r"C:\app\x"), PathBuf::from(r"C:\app\x"));
    assert_eq!(plain("/usr/lib/app"), PathBuf::from("/usr/lib/app"));
    assert_eq!(plain(r"\\?\Volume{1234}\x"), PathBuf::from(r"\\?\Volume{1234}\x"));
}

#[test]
fn a_folder_that_tauri_names_with_the_prefix_is_handed_on_without_it() {
    let root = temp("verbatim");
    std::fs::create_dir_all(root.join("resources/potato-brain")).unwrap();
    std::fs::write(root.join("resources/potato-brain/brain.mjs"), "").unwrap();
    // On Windows the resolver's answer is the canonical (extended-length) spelling of the same folder.
    let handed = std::fs::canonicalize(&root).unwrap();
    let found = find("potato-brain", "brain.mjs", |rel| Some(handed.join(rel))).unwrap();
    assert!(!found.to_string_lossy().starts_with(r"\\?\"), "{found:?}");
    assert!(found.join("brain.mjs").is_file());
    let _ = std::fs::remove_dir_all(&root);
}
