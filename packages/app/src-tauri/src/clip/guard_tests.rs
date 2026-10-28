//! Guards for the privacy design of Clip It: it is reachable from the app window only, and it never drives a pane.

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `dir`, with its text.
fn sources(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(next) = todo.pop() {
        for entry in std::fs::read_dir(&next).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                todo.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                found.push((path, text));
            }
        }
    }
    assert!(!found.is_empty(), "no sources under {}", dir.display());
    found
}

/// Whether `text` mentions the feature: the word "clip" once the unrelated words `clippy` and `clipboard` are gone.
fn mentions_clip(text: &str) -> bool {
    text.to_lowercase().replace("clippy", "").replace("clipboard", "").contains("clip")
}

#[test]
fn no_bridge_or_tool_catalog_source_mentions_clip() {
    for folder in ["bridge", "tool_registry"] {
        for (path, text) in sources(&src_dir().join(folder)) {
            assert!(!mentions_clip(&text), "{} mentions clip: Clip It must not be a bridge route or an MCP tool", path.display());
        }
    }
}

#[test]
fn catalog_version_is_the_pinned_one() {
    // The same value `tool_registry/tests.rs` pins: adding Clip It must not move it.
    assert_eq!(crate::tool_registry::catalog_version(), "73-caaf894656d938ac");
}

#[test]
fn clip_never_drives_a_pane() {
    let here = Path::new(file!()).file_name().unwrap().to_owned();
    // Built from pieces so this file does not contain the words itself.
    let banned = [["write", "_input"].concat(), ["press", "_key"].concat(), ["answer", "_prompt"].concat()];
    for (path, text) in sources(&src_dir().join("clip")) {
        if path.file_name() == Some(&here) {
            continue;
        }
        for word in &banned {
            assert!(!text.contains(word.as_str()), "{} mentions {word}", path.display());
        }
    }
}
