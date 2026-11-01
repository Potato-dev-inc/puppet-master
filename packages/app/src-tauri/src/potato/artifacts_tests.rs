//! The Artifacts store: versions by name, filters, previews, "Save as file", delete, download, and the refusals
//! (a bad bot id, a file too big, a damaged index). Every test works in its own temp folder.

use std::path::PathBuf;

use super::artifacts::*;
use super::store::{FromKind, Message, MsgKind};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-artifacts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn filter(bot: Option<&str>, kind: Option<Kind>, search: Option<&str>) -> Filter {
    Filter { bot: bot.map(str::to_string), kind, search: search.map(str::to_string) }
}

#[test]
fn a_saved_file_keeps_its_metadata_and_its_bytes() {
    let root = temp("saved");
    let saved = put(&root, "nimbus", "Plan.md", b"# Plan\nhi", 1_700).unwrap();
    assert_eq!(saved.name, "Plan.md");
    assert_eq!(saved.bot, "nimbus");
    assert_eq!(saved.kind, Kind::Markdown);
    assert_eq!(saved.size, 9, "# Plan\\nhi is nine bytes");
    assert_eq!(saved.created, 1_700);
    assert_eq!(saved.version, 1);
    assert!(saved.id.starts_with("a-") && saved.id.len() == 14);
    assert_eq!(saved.file, format!("{}.md", saved.id), "the file is named by its id, never by the name");
    let on_disk = std::fs::read(root.join("artifacts").join("nimbus").join(&saved.file)).unwrap();
    assert_eq!(on_disk, b"# Plan\nhi");

    let index: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("artifacts/nimbus/index.json")).unwrap()).unwrap();
    assert_eq!(index["v"], 1);
    assert_eq!(index["items"][0]["kind"], "markdown");
    assert_eq!(index["items"][0]["bot"], "nimbus");
}

#[test]
fn the_same_name_is_a_new_version_and_the_list_shows_the_newest() {
    let root = temp("versions");
    let first = put(&root, "nimbus", "resume.pdf", b"%PDF-1", 1).unwrap();
    let second = put(&root, "nimbus", "Resume.PDF", b"%PDF-2", 2).unwrap();
    let other = put(&root, "nimbus", "notes.txt", b"n", 3).unwrap();
    assert_eq!((first.version, second.version, other.version), (1, 2, 1));
    assert_ne!(first.id, second.id, "each version keeps its own file");
    assert_eq!(std::fs::read(root.join("artifacts/nimbus").join(&first.file)).unwrap(), b"%PDF-1", "the old version stays");

    let listed = list(&root, &Filter::default()).unwrap();
    assert_eq!(listed.len(), 2, "one row per name");
    let resume = listed.iter().find(|i| i.artifact.name.eq_ignore_ascii_case("resume.pdf")).unwrap();
    assert_eq!(resume.artifact.id, second.id);
    assert_eq!(resume.versions, 2);
    assert_eq!(listed[0].artifact.id, other.id, "newest first");
}

#[test]
fn the_list_filters_by_bot_kind_and_name() {
    let root = temp("filters");
    put(&root, "nimbus", "chart.png", &[0x89, 0x50], 10).unwrap();
    put(&root, "nimbus", "Cover letter.md", b"hi", 20).unwrap();
    put(&root, "tango", "cover-letter.html", b"<p>x</p>", 30).unwrap();
    put(&root, "tango", "data.bin", b"\x00", 40).unwrap();

    assert_eq!(list(&root, &filter(Some("nimbus"), None, None)).unwrap().len(), 2);
    assert_eq!(list(&root, &filter(None, Some(Kind::Image), None)).unwrap().len(), 1);
    assert_eq!(list(&root, &filter(None, Some(Kind::Html), None)).unwrap()[0].artifact.name, "cover-letter.html");
    let found = list(&root, &filter(None, None, Some("  COVER "))).unwrap();
    assert_eq!(found.len(), 2, "search is part of the name, without regard to case");
    assert!(list(&root, &filter(None, None, Some("nothing"))).unwrap().is_empty());
    assert_eq!(list(&root, &filter(Some("nobody"), None, None)).unwrap().len(), 0);
}

#[test]
fn an_empty_or_missing_folder_lists_nothing() {
    let root = temp("empty");
    assert!(list(&root, &Filter::default()).unwrap().is_empty());
    assert!(list(&root, &filter(Some("nimbus"), None, None)).unwrap().is_empty());
}

#[test]
fn bot_ids_and_names_cannot_point_outside_the_folder() {
    let root = temp("escape");
    for bad in ["", ".", "..", "../x", "a/b", "a\\b", "c:x"] {
        assert_eq!(put(&root, bad, "f.txt", b"x", 1).unwrap_err().code, "POTATO_ARTIFACT_INVALID", "bot {bad:?}");
        assert!(list(&root, &filter(Some(bad), None, None)).is_err(), "list {bad:?}");
    }
    let saved = put(&root, "nimbus", "../../../../evil.txt", b"x", 1).unwrap();
    assert_eq!(saved.name, "evil.txt");
    assert!(root.join("artifacts/nimbus").join(&saved.file).exists());
    assert!(!root.join("evil.txt").exists());
}

#[test]
fn a_file_too_big_is_refused_and_nothing_is_written() {
    let root = temp("big");
    let big = vec![0u8; MAX_SAVE_BYTES + 1];
    assert_eq!(put(&root, "nimbus", "big.bin", &big, 1).unwrap_err().code, "POTATO_ARTIFACT_TOO_BIG");
    assert!(!root.join("artifacts").exists(), "the check comes before any folder is made");
}

#[test]
fn a_damaged_index_is_an_error_and_is_never_overwritten() {
    let root = temp("damaged");
    let dir = root.join("artifacts/nimbus");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.json"), "{ damaged").unwrap();
    assert!(list(&root, &Filter::default()).is_err());
    assert!(put(&root, "nimbus", "a.txt", b"x", 1).is_err());
    assert_eq!(std::fs::read_to_string(dir.join("index.json")).unwrap(), "{ damaged");
}

#[test]
fn an_index_from_a_newer_version_is_refused() {
    let root = temp("newer");
    let dir = root.join("artifacts/nimbus");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.json"), r#"{"v":99,"items":[]}"#).unwrap();
    let err = list(&root, &Filter::default()).unwrap_err();
    assert!(err.message.contains("newer"), "{err}");
}

#[test]
fn the_preview_shows_text_markdown_and_html_as_text_and_images_as_a_data_address() {
    let root = temp("preview");
    let text = put(&root, "nimbus", "todo.txt", b"buy milk", 1).unwrap();
    let md = put(&root, "nimbus", "todo.md", b"# hi", 2).unwrap();
    let html = put(&root, "nimbus", "page.html", b"<script>x</script>", 3).unwrap();
    let png = put(&root, "nimbus", "pic.PNG", &[1, 2, 3], 4).unwrap();
    let pdf = put(&root, "nimbus", "doc.pdf", b"%PDF", 5).unwrap();
    let other = put(&root, "nimbus", "tool.exe", b"MZ", 6).unwrap();

    assert_eq!(open(&root, "nimbus", &text.id).unwrap().text.as_deref(), Some("buy milk"));
    assert_eq!(open(&root, "nimbus", &md.id).unwrap().text.as_deref(), Some("# hi"));
    let html_preview = open(&root, "nimbus", &html.id).unwrap();
    assert_eq!(html_preview.text.as_deref(), Some("<script>x</script>"), "the page shows it inside a sandbox, never runs it here");
    let image = open(&root, "nimbus", &png.id).unwrap();
    assert_eq!(image.image.as_deref(), Some("data:image/png;base64,AQID"));
    let pdf_preview = open(&root, "nimbus", &pdf.id).unwrap();
    assert!(pdf_preview.text.is_none() && pdf_preview.image.is_none(), "PDF: download or show in folder only");
    let other_preview = open(&root, "nimbus", &other.id).unwrap();
    assert!(other_preview.text.is_none() && other_preview.image.is_none());
}

#[test]
fn a_long_text_preview_is_cut_and_says_so() {
    let root = temp("long");
    let body = "a".repeat(MAX_TEXT_PREVIEW_BYTES + 10);
    let saved = put(&root, "nimbus", "long.txt", body.as_bytes(), 1).unwrap();
    let preview = open(&root, "nimbus", &saved.id).unwrap();
    assert!(preview.truncated);
    assert_eq!(preview.text.unwrap().len(), MAX_TEXT_PREVIEW_BYTES);
}

#[test]
fn the_preview_lists_every_version_newest_first() {
    let root = temp("versions-preview");
    let v1 = put(&root, "nimbus", "plan.md", b"one", 1).unwrap();
    let v2 = put(&root, "nimbus", "plan.md", b"two", 2).unwrap();
    let preview = open(&root, "nimbus", &v1.id).unwrap();
    assert_eq!(preview.artifact.id, v1.id, "the version asked for");
    assert_eq!(preview.versions.iter().map(|v| v.version).collect::<Vec<_>>(), vec![2, 1]);
    assert_eq!(preview.versions[0].id, v2.id);
    assert_eq!(preview.text.as_deref(), Some("one"));
}

#[test]
fn an_unknown_or_missing_file_says_so() {
    let root = temp("missing");
    assert_eq!(open(&root, "nimbus", "a-000000000000").unwrap_err().code, "POTATO_ARTIFACT_NOT_FOUND");
    assert_eq!(open(&root, "nimbus", "../x").unwrap_err().code, "POTATO_ARTIFACT_NOT_FOUND");
    let saved = put(&root, "nimbus", "gone.txt", b"x", 1).unwrap();
    std::fs::remove_file(root.join("artifacts/nimbus").join(&saved.file)).unwrap();
    assert_eq!(open(&root, "nimbus", &saved.id).unwrap_err().code, "POTATO_ARTIFACT_NOT_FOUND");
}

#[test]
fn a_message_from_the_chat_is_saved_as_markdown_with_its_text() {
    let root = temp("from-message");
    let message = Message::new(5, FromKind::Bot, Some("Nimbus"), MsgKind::Text, "## Weekly plan\n- one\n- two");
    let saved = save_message(&root, "nimbus", &message, None, 9).unwrap();
    assert_eq!(saved.name, "Weekly plan.md");
    assert_eq!(saved.kind, Kind::Markdown);
    assert_eq!(saved.created, 9);
    assert_eq!(open(&root, "nimbus", &saved.id).unwrap().text.as_deref(), Some("## Weekly plan\n- one\n- two"));

    let named = save_message(&root, "nimbus", &message, Some("plan"), 10).unwrap();
    assert_eq!(named.name, "plan.md", "a name without an extension gets .md");
    assert_eq!(named.version, 1);
    let again = save_message(&root, "nimbus", &message, Some("plan.md"), 11).unwrap();
    assert_eq!(again.version, 2, "the same name is the next version");
}

#[test]
fn only_a_plain_chat_message_can_be_saved() {
    let root = temp("kinds");
    let card = Message::new(1, FromKind::Bot, None, MsgKind::HelperCard, "Start a helper?");
    assert_eq!(save_message(&root, "nimbus", &card, None, 1).unwrap_err().code, "POTATO_ARTIFACT_INVALID");
    assert!(list(&root, &Filter::default()).unwrap().is_empty());
}

#[test]
fn delete_removes_one_version_and_its_file() {
    let root = temp("delete");
    let v1 = put(&root, "nimbus", "plan.md", b"one", 1).unwrap();
    let v2 = put(&root, "nimbus", "plan.md", b"two", 2).unwrap();
    delete(&root, "nimbus", &v2.id).unwrap();
    assert!(!root.join("artifacts/nimbus").join(&v2.file).exists());
    let left = list(&root, &Filter::default()).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].artifact.id, v1.id, "the old version is still there");
    assert_eq!(left[0].versions, 1);
    assert_eq!(delete(&root, "nimbus", &v2.id).unwrap_err().code, "POTATO_ARTIFACT_NOT_FOUND", "a second delete says it is gone");
}

#[test]
fn download_copies_the_file_and_keeps_the_original() {
    let root = temp("download");
    let saved = put(&root, "nimbus", "chart.png", &[9, 8, 7], 1).unwrap();
    let dest = root.join("picked").join("my chart.png");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    export(&root, "nimbus", &saved.id, &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), vec![9, 8, 7]);
    assert!(root.join("artifacts/nimbus").join(&saved.file).exists());
}

#[test]
fn many_saves_in_a_row_keep_every_record() {
    let root = temp("many");
    for i in 0..5 {
        put(&root, "nimbus", &format!("f{i}.txt"), b"x", i).unwrap();
    }
    assert_eq!(list(&root, &Filter::default()).unwrap().len(), 5);
}
