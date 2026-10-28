//! Point and Fix tests for source: a page hint names a file only when that file really exists inside a folder.
//! Fixtures are made under the system temp folder. The guard removes only the files and folders the test
//! created (one by one, never recursively), and only when they are empty.

use crate::point::source::resolve;
use crate::point::{SourceHint, SourceHow};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// A temp folder with files the test wrote. Dropping it removes what the test created.
struct Fixture {
    root: PathBuf,
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let root = std::env::temp_dir().join(format!("pm-point-source-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir(&root).expect("make the test folder");
        Fixture { root: root.clone(), files: Vec::new(), dirs: vec![root] }
    }

    /// Write a small file at `rel` (under the root), making the folders it needs.
    fn file(&mut self, rel: &str) -> PathBuf {
        let path = self.root.join(rel);
        let mut missing = Vec::new();
        let mut dir = path.parent().expect("a parent folder").to_path_buf();
        while !dir.exists() {
            missing.push(dir.clone());
            dir = dir.parent().expect("a parent folder").to_path_buf();
        }
        for made in missing.iter().rev() {
            std::fs::create_dir(made).expect("make a folder");
            self.dirs.push(made.clone());
        }
        std::fs::write(&path, "export const Card = () => null;\n").expect("write a file");
        self.files.push(path.clone());
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for file in self.files.iter().rev() {
            let _ = std::fs::remove_file(file);
        }
        for dir in self.dirs.iter().rev() {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

fn hint(file: &str, how: SourceHow, line: Option<u32>) -> SourceHint {
    SourceHint { file: file.to_string(), line, column: None, how }
}

/// The app's file, as a forward-slash absolute path (drive, then the folders, then `src/Card.tsx`).
fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn found(hint: &SourceHint, roots: &[&Path]) -> Option<(PathBuf, String, Option<u32>)> {
    resolve(hint, roots).map(|(root, source)| (root, source.rel_path, source.line))
}

#[test]
fn accepts_each_form_of_a_page_path() {
    let mut fx = Fixture::new("forms");
    let card = fx.file("src/Card.tsx");
    let root = fx.root.clone();
    let roots: [&Path; 1] = [&root];
    let forms = vec![
        card.to_string_lossy().to_string(),
        "/src/Card.tsx".to_string(),
        "src/Card.tsx".to_string(),
        "./src/Card.tsx".to_string(),
        "webpack-internal:///(app-pages-browser)/./src/Card.tsx".to_string(),
        "webpack-internal:///./src/Card.tsx".to_string(),
        "http://localhost:5173/src/Card.tsx?t=123".to_string(),
        "http://127.0.0.1:5173/src/Card.tsx#L12".to_string(),
        "https://localhost:5173/src/Card.tsx".to_string(),
        "http://localhost:5173/src/%43ard.tsx".to_string(),
    ];
    for form in forms {
        let got = found(&hint(&form, SourceHow::DataAttr, Some(12)), &roots);
        assert_eq!(got, Some((root.clone(), "src/Card.tsx".to_string(), Some(12))), "form: {form}");
    }
}

#[cfg(windows)]
#[test]
fn accepts_vite_fs_and_file_urls_on_windows() {
    let mut fx = Fixture::new("winforms");
    let card = fx.file("src/Card.tsx");
    let root = fx.root.clone();
    let roots: [&Path; 1] = [&root];
    let forms = vec![
        format!("/@fs/{}", slash_path(&card)),
        format!("file:///{}", slash_path(&card)),
        slash_path(&card),
    ];
    for form in forms {
        let got = found(&hint(&form, SourceHow::VueFile, None), &roots);
        assert_eq!(got, Some((root.clone(), "src/Card.tsx".to_string(), None)), "form: {form}");
    }
}

#[cfg(windows)]
#[test]
fn windows_drive_letter_casing_does_not_matter() {
    let mut fx = Fixture::new("casing");
    let card = fx.file("src/Card.tsx");
    let root = fx.root.clone();
    let roots: [&Path; 1] = [&root];
    let text = card.to_string_lossy().to_string();
    let mut flipped = text.clone();
    flipped.replace_range(0..1, &text[0..1].to_ascii_lowercase());
    let mut upper = text.clone();
    upper.replace_range(0..1, &text[0..1].to_ascii_uppercase());
    for form in [flipped, upper] {
        let got = found(&hint(&form, SourceHow::Stack, None), &roots);
        assert_eq!(got, Some((root.clone(), "src/Card.tsx".to_string(), None)), "form: {form}");
    }
}

#[test]
fn a_missing_file_is_none() {
    let mut fx = Fixture::new("missing");
    fx.file("src/Card.tsx");
    let root = fx.root.clone();
    let roots: [&Path; 1] = [&root];
    assert_eq!(found(&hint("src/Missing.tsx", SourceHow::DataAttr, Some(3)), &roots), None);
    assert_eq!(found(&hint("http://localhost:5173/src/Missing.tsx", SourceHow::DataAttr, None), &roots), None);
}

#[test]
fn a_folder_is_not_a_file() {
    let mut fx = Fixture::new("folder");
    fx.file("src/Card.tsx");
    let root = fx.root.clone();
    let roots: [&Path; 1] = [&root];
    assert_eq!(found(&hint("src", SourceHow::Stack, None), &roots), None);
}

#[test]
fn a_file_outside_the_roots_is_none() {
    let mut inside = Fixture::new("inside");
    inside.file("src/Card.tsx");
    let mut outside = Fixture::new("outside");
    let secret = outside.file("Secret.tsx");
    let roots: [&Path; 1] = [&inside.root];
    assert_eq!(found(&hint(&secret.to_string_lossy(), SourceHow::DataAttr, Some(1)), &roots), None);
    assert_eq!(found(&hint(&slash_path(&secret), SourceHow::DataAttr, Some(1)), &roots), None);
}

#[test]
fn a_path_with_dot_dot_is_none() {
    let mut inside = Fixture::new("escape");
    inside.file("src/Card.tsx");
    let mut outside = Fixture::new("escaped");
    outside.file("Secret.tsx");
    let outside_name = outside.root.file_name().expect("a folder name").to_string_lossy().to_string();
    let roots: [&Path; 1] = [&inside.root];
    let escapes = [
        format!("../{outside_name}/Secret.tsx"),
        format!("src/../../{outside_name}/Secret.tsx"),
        format!("/src/../../{outside_name}/Secret.tsx"),
        format!("http://localhost:5173/%2e%2e/{outside_name}/Secret.tsx"),
        format!("http://localhost:5173/src/..%2f..%2f{outside_name}/Secret.tsx"),
    ];
    for escape in escapes {
        assert_eq!(found(&hint(&escape, SourceHow::DataAttr, None), &roots), None, "escape: {escape}");
    }
}

#[test]
fn a_home_folder_path_outside_the_roots_is_none() {
    let mut inside = Fixture::new("home");
    inside.file("src/Card.tsx");
    let roots: [&Path; 1] = [&inside.root];
    // Never named, even when it does not exist: the page path is only a hint.
    let text = r"C:\Users\you\other\x.tsx";
    assert_eq!(found(&hint(text, SourceHow::DataAttr, Some(4)), &roots), None);
    assert_eq!(found(&hint("/Users/you/other/x.tsx", SourceHow::Stack, None), &roots), None);
}

#[test]
fn a_stack_hint_never_keeps_a_line() {
    let mut fx = Fixture::new("stack");
    fx.file("src/Card.tsx");
    let root = fx.root.clone();
    let roots: [&Path; 1] = [&root];
    let stack = found(&hint("/src/Card.tsx", SourceHow::Stack, Some(42)), &roots);
    assert_eq!(stack, Some((root.clone(), "src/Card.tsx".to_string(), None)));
    let react = found(&hint("/src/Card.tsx", SourceHow::ReactDebugSource, Some(42)), &roots);
    assert_eq!(react, Some((root.clone(), "src/Card.tsx".to_string(), Some(42))));
    let zero = found(&hint("/src/Card.tsx", SourceHow::SvelteMeta, Some(0)), &roots);
    assert_eq!(zero, Some((root, "src/Card.tsx".to_string(), None)));
}

#[test]
fn the_first_root_that_holds_the_file_wins() {
    let mut first = Fixture::new("first");
    first.file("src/Other.tsx");
    let mut second = Fixture::new("second");
    second.file("src/Card.tsx");
    let roots: [&Path; 2] = [&first.root, &second.root];
    let got = found(&hint("src/Card.tsx", SourceHow::DataAttr, Some(9)), &roots);
    assert_eq!(got, Some((second.root.clone(), "src/Card.tsx".to_string(), Some(9))));
}

#[test]
fn odd_hints_are_none() {
    let mut fx = Fixture::new("odd");
    fx.file("src/Card.tsx");
    let root = fx.root.clone();
    let roots: [&Path; 1] = [&root];
    let odd = [
        String::new(),
        "   ".to_string(),
        "src/Card.tsx:stream".to_string(),
        "src/Card\0.tsx".to_string(),
        "ftp://localhost/src/Card.tsx".to_string(),
        "javascript:alert(1)".to_string(),
        "http://localhost:5173/src/%ZZ.tsx".to_string(),
        "http://localhost:5173".to_string(),
        "x".repeat(2000),
    ];
    for text in odd {
        assert_eq!(found(&hint(&text, SourceHow::DataAttr, Some(1)), &roots), None, "hint: {text:?}");
    }
}

#[test]
fn no_roots_is_none() {
    let mut fx = Fixture::new("noroots");
    fx.file("src/Card.tsx");
    let roots: [&Path; 0] = [];
    assert_eq!(found(&hint("src/Card.tsx", SourceHow::DataAttr, None), &roots), None);
}
