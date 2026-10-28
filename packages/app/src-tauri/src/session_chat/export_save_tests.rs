use super::*;

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-export-save-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn path_in(dir: &Path, name: &str) -> String {
    dir.join(name).to_string_lossy().to_string()
}

#[test]
fn the_saved_file_holds_the_page_rust_made_not_what_the_caller_sends() {
    let pages = Pages::default();
    let dir = temp_dir();
    let token = pages.keep("pane-1", "<p>the page</p>").unwrap();
    let written = pages.save(&token, &path_in(&dir, "chat.html")).unwrap();
    assert_eq!(std::fs::read_to_string(written).unwrap(), "<p>the page</p>");
}

#[test]
fn a_token_works_once() {
    let pages = Pages::default();
    let dir = temp_dir();
    let token = pages.keep("pane-1", "<p>x</p>").unwrap();
    pages.save(&token, &path_in(&dir, "a.html")).unwrap();
    let again = pages.save(&token, &path_in(&dir, "b.html")).unwrap_err();
    assert!(again.contains("no longer available"), "{again}");
    assert!(!dir.join("b.html").exists());
}

#[test]
fn an_unknown_token_writes_nothing() {
    let pages = Pages::default();
    let dir = temp_dir();
    pages.keep("pane-1", "<p>x</p>").unwrap();
    assert!(pages.save("not-a-token", &path_in(&dir, "a.html")).is_err());
    assert!(!dir.join("a.html").exists());
}

#[test]
fn a_new_page_for_a_pane_replaces_the_old_one_and_its_token() {
    let pages = Pages::default();
    let dir = temp_dir();
    let old = pages.keep("pane-1", "<p>old</p>").unwrap();
    let new = pages.keep("pane-1", "<p>new</p>").unwrap();
    assert_eq!(pages.count(), 1);
    assert!(pages.save(&old, &path_in(&dir, "a.html")).is_err());
    pages.save(&new, &path_in(&dir, "a.html")).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("a.html")).unwrap(), "<p>new</p>");
}

#[test]
fn only_a_few_pages_are_kept() {
    let pages = Pages::default();
    for n in 0..10 {
        pages.keep(&format!("pane-{n}"), "<p>x</p>").unwrap();
    }
    assert_eq!(pages.count(), KEEP_PAGES);
}

#[test]
fn a_refused_path_keeps_the_token_for_another_try() {
    let pages = Pages::default();
    let dir = temp_dir();
    let token = pages.keep("pane-1", "<p>x</p>").unwrap();
    assert!(pages.save(&token, &path_in(&dir, "notes.txt")).is_err());
    assert!(pages.save(&token, "relative.html").is_err());
    assert_eq!(pages.count(), 1);
    pages.save(&token, &path_in(&dir, "ok.html")).unwrap();
    assert_eq!(pages.count(), 0);
}

#[test]
fn a_relative_path_is_refused() {
    assert!(check_dest("chat.html").is_err());
    assert!(check_dest(r"sub\chat.html").is_err());
    assert!(check_dest("../chat.html").is_err());
    assert!(check_dest("").is_err());
    assert!(check_dest("   ").is_err());
}

#[test]
fn a_path_with_dot_parts_is_refused() {
    let dir = temp_dir();
    let sneaky = format!("{}{}..{}chat.html", dir.display(), std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR);
    assert!(check_dest(&sneaky).is_err());
}

#[test]
fn only_html_names_are_accepted() {
    let dir = temp_dir();
    assert!(check_dest(&path_in(&dir, "a.exe")).is_err());
    assert!(check_dest(&path_in(&dir, "a")).is_err());
    assert!(check_dest(&path_in(&dir, "a.html.exe")).is_err());
    assert!(check_dest(&path_in(&dir, "a.HTML")).is_ok());
    assert!(check_dest(&path_in(&dir, "a.htm")).is_ok());
}

#[test]
fn a_missing_folder_is_refused() {
    let dir = temp_dir();
    assert!(check_dest(&path_in(&dir.join("nope"), "a.html")).is_err());
}

#[test]
fn a_folder_with_an_html_name_is_refused() {
    let dir = temp_dir();
    std::fs::create_dir(dir.join("dir.html")).unwrap();
    let error = check_dest(&path_in(&dir, "dir.html")).unwrap_err();
    assert!(error.contains("not a plain file"), "{error}");
}

#[test]
fn an_existing_plain_file_is_replaced() {
    let pages = Pages::default();
    let dir = temp_dir();
    std::fs::write(dir.join("a.html"), "a much longer old content than the new page has").unwrap();
    let token = pages.keep("pane-1", "<p>x</p>").unwrap();
    pages.save(&token, &path_in(&dir, "a.html")).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("a.html")).unwrap(), "<p>x</p>");
}

#[test]
fn a_link_is_refused() {
    let dir = temp_dir();
    let target = dir.join("target.html");
    std::fs::write(&target, "keep me").unwrap();
    let link = dir.join("link.html");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&target, &link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&target, &link);
    if made.is_err() {
        // Making a link needs a privilege on some Windows set-ups; nothing to check there.
        return;
    }
    let error = check_dest(&link.to_string_lossy()).unwrap_err();
    assert!(error.contains("link"), "{error}");
    let pages = Pages::default();
    let token = pages.keep("p", "<p>x</p>").unwrap();
    assert!(pages.save(&token, &link.to_string_lossy()).is_err());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me");
}

#[test]
fn device_names_are_refused() {
    let dir = temp_dir();
    for name in ["con.html", "NUL.htm", "com1.html", "LPT9.html", "aux.html", "prn.html"] {
        assert!(check_dest(&path_in(&dir, name)).is_err(), "{name}");
    }
    assert!(check_dest(&path_in(&dir, "com10.html")).is_ok());
    assert!(check_dest(&path_in(&dir, "console.html")).is_ok());
}

#[test]
fn a_page_over_the_cap_is_not_kept() {
    let pages = Pages::default();
    let big = "x".repeat(MAX_PAGE_BYTES + 1);
    let error = pages.keep("pane-1", &big).unwrap_err();
    assert!(error.contains("too big"), "{error}");
    assert_eq!(pages.count(), 0);
    assert!(pages.keep("pane-1", &"x".repeat(MAX_PAGE_BYTES)).is_ok());
}

#[test]
fn a_failed_write_keeps_the_page_for_another_try() {
    let pages = Pages::default();
    let dir = temp_dir();
    let token = pages.keep("pane-1", "<p>x</p>").unwrap();
    // The path passes the check, then the place goes away before the write.
    let gone = dir.join("sub");
    std::fs::create_dir(&gone).unwrap();
    let dest = path_in(&gone, "a.html");
    assert!(check_dest(&dest).is_ok());
    std::fs::remove_dir(&gone).unwrap();
    assert!(write_new_or_plain(&PathBuf::from(&dest), b"x").is_err());
    // `save` itself refuses it at the check; the token is still there.
    assert!(pages.save(&token, &dest).is_err());
    assert_eq!(pages.count(), 1);
}
