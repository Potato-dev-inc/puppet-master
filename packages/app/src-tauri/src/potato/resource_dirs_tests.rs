//! The bundled folder lookup: the installed layout first, the plain one second, never a folder without its marker.

use std::path::PathBuf;

use super::resource_dirs::{candidates, find};

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
