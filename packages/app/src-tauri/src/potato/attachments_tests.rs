//! Attachments: what is kept, what is refused, and the one text message the brain gets.

use std::path::PathBuf;

use base64::Engine as _;

use super::attachments::*;
use super::store::MAX_MESSAGE_CHARS;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-attach-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn text_file(name: &str, body: &str) -> Incoming {
    Incoming { name: name.into(), kind: "text".into(), data: b64(body.as_bytes()) }
}

const PNG: [u8; 12] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 1];

fn png_file(name: &str) -> Incoming {
    Incoming { name: name.into(), kind: "image".into(), data: b64(&PNG) }
}

#[test]
fn a_name_loses_its_path_and_odd_characters_and_keeps_its_end() {
    assert_eq!(safe_name("C:\\Users\\me\\notes.txt"), "notes.txt");
    assert_eq!(safe_name("../../etc/passwd"), "passwd");
    assert_eq!(safe_name(".env"), "env");
    assert_eq!(safe_name("a<b>:c?.md"), "a_b__c_.md");
    assert_eq!(safe_name(""), "file");
    assert_eq!(safe_name("///"), "file");
    let long = format!("{}.txt", "x".repeat(200));
    let cut = safe_name(&long);
    assert_eq!(cut.chars().count(), 60);
    assert!(cut.ends_with(".txt"));
}

#[test]
fn a_picture_is_known_by_its_first_bytes_not_by_its_name() {
    assert_eq!(image_ext(&PNG), Some("png"));
    assert_eq!(image_ext(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("jpg"));
    assert_eq!(image_ext(b"GIF89a.."), Some("gif"));
    assert_eq!(image_ext(b"RIFF\x01\x00\x00\x00WEBPVP8 "), Some("webp"));
    assert_eq!(image_ext(b"hello"), None);
    assert_eq!(image_ext(b""), None);
}

#[test]
fn saves_files_in_the_bot_folder_and_names_them_there() {
    let root = temp("save");
    let saved = save_all(&root, "nimbus", &[text_file("notes.txt", "hello there"), png_file("shot.jpeg")], 1700).unwrap();
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0].rel, "attachments/1700-1-notes.txt");
    assert_eq!(saved[0].kind, Kind::Text);
    assert_eq!(saved[0].text.as_deref(), Some("hello there"));
    // The picture takes its real extension, not the one the page claimed.
    assert_eq!(saved[1].name, "shot.png");
    assert_eq!(saved[1].kind, Kind::Image);
    let base = root.join("brain").join("nimbus");
    assert_eq!(std::fs::read_to_string(base.join(&saved[0].rel)).unwrap(), "hello there");
    assert_eq!(std::fs::read(base.join(&saved[1].rel)).unwrap(), PNG.to_vec());
}

#[test]
fn refuses_a_bad_bot_name_too_many_files_and_big_files_and_saves_nothing() {
    let root = temp("refuse");
    let one = [text_file("a.txt", "a")];
    assert_eq!(save_all(&root, "../evil", &one, 1).unwrap_err().code, "POTATO_BOT_NOT_FOUND");
    let five: Vec<Incoming> = (0..5).map(|i| text_file(&format!("{i}.txt"), "x")).collect();
    assert!(save_all(&root, "nimbus", &five, 1).unwrap_err().message.contains("at most 4"));
    let big = text_file("big.txt", &"x".repeat(MAX_FILE_BYTES + 1));
    assert!(save_all(&root, "nimbus", &[big], 1).unwrap_err().message.contains("bigger than 2.0 MB"));
    // Three 1.5 MB files are each fine and together too big.
    let part = "y".repeat(1_500_000);
    let three = [text_file("a.txt", &part), text_file("b.txt", &part), text_file("c.txt", &part)];
    assert!(save_all(&root, "nimbus", &three, 1).unwrap_err().message.contains("together"));
    // A good file next to a bad one: neither is kept.
    let mixed = [text_file("ok.txt", "fine"), Incoming { name: "x.bin".into(), kind: "text".into(), data: b64(&[1, 0, 2, 3]) }];
    assert!(save_all(&root, "nimbus", &mixed, 1).is_err());
    assert!(!root.join("brain").join("nimbus").join(FOLDER).exists());
}

#[test]
fn refuses_empty_files_bad_base64_binary_and_a_picture_that_is_not_one() {
    let root = temp("kinds");
    let empty = Incoming { name: "e.txt".into(), kind: "text".into(), data: String::new() };
    assert!(save_all(&root, "nimbus", &[empty], 1).unwrap_err().message.contains("empty"));
    let junk = Incoming { name: "j.txt".into(), kind: "text".into(), data: "***not base64***".into() };
    assert!(save_all(&root, "nimbus", &[junk], 1).unwrap_err().message.contains("could not be read"));
    let exe = Incoming { name: "run.txt".into(), kind: "text".into(), data: b64(b"MZ\x90\x00\x03\x00") };
    assert!(save_all(&root, "nimbus", &[exe], 1).unwrap_err().message.contains("not a text file"));
    let fake = Incoming { name: "cat.png".into(), kind: "image".into(), data: b64(b"just words") };
    assert!(save_all(&root, "nimbus", &[fake], 1).unwrap_err().message.contains("not a picture"));
    let latin1 = Incoming { name: "l.txt".into(), kind: "text".into(), data: b64(&[0x68, 0xE9, 0x6C, 0x6C, 0x6F]) };
    assert!(save_all(&root, "nimbus", &[latin1], 1).is_err());
}

#[test]
fn forget_removes_only_what_was_saved_there() {
    let root = temp("forget");
    let saved = save_all(&root, "nimbus", &[text_file("a.txt", "keep me?")], 5).unwrap();
    let path = root.join("brain").join("nimbus").join(&saved[0].rel);
    assert!(path.exists());
    forget(&root, "nimbus", &saved);
    assert!(!path.exists());
    // A name outside the folder is ignored.
    let outside = root.join("brain").join("nimbus").join("keep.txt");
    std::fs::write(&outside, "x").unwrap();
    let sneaky = Saved { name: "keep.txt".into(), rel: "../keep.txt".into(), kind: Kind::Text, bytes: 1, text: None };
    forget(&root, "nimbus", &[sneaky]);
    assert!(outside.exists());
}

#[test]
fn the_message_has_the_words_a_line_per_file_and_the_text_of_a_small_text_file() {
    let root = temp("compose");
    let saved = save_all(&root, "nimbus", &[text_file("todo.md", "- milk\n- eggs"), png_file("pic.png")], 9).unwrap();
    let message = compose("  look at these  ", &saved).unwrap();
    assert!(message.starts_with("look at these\n\n[Attached file: todo.md (13 B), saved as attachments/9-1-todo.md]"));
    assert!(message.contains("[Attached image: pic.png (12 B), saved as attachments/9-2-pic.png]"));
    assert!(message.contains("todo.md:\n```\n- milk\n- eggs\n```"));
    // The picture's bytes are never in the message.
    assert!(!message.contains("PNG"));
}

#[test]
fn only_files_and_no_words_still_makes_a_message() {
    let root = temp("only");
    let saved = save_all(&root, "nimbus", &[png_file("a.png")], 1).unwrap();
    let message = compose("", &saved).unwrap();
    assert!(message.starts_with("[Attached image: a.png"));
}

#[test]
fn a_long_text_file_is_cut_to_fit_and_says_so_and_the_message_never_passes_the_cap() {
    let root = temp("cut");
    let body = "line of text\n".repeat(900);
    let saved = save_all(&root, "nimbus", &[text_file("log.txt", &body)], 2).unwrap();
    let message = compose(&"w".repeat(500), &saved).unwrap();
    assert!(message.chars().count() <= MAX_MESSAGE_CHARS);
    assert!(message.contains("(Cut here. The whole file is saved.)"));
    assert!(message.contains("log.txt:\n```\nline of text"));
}

#[test]
fn a_file_that_leaves_no_room_is_named_but_not_quoted_and_too_many_words_are_refused() {
    let root = temp("room");
    let saved = save_all(&root, "nimbus", &[text_file("a.txt", &"z".repeat(5000))], 3).unwrap();
    let almost_full = "w".repeat(MAX_MESSAGE_CHARS - 120);
    let message = compose(&almost_full, &saved).unwrap();
    assert!(message.chars().count() <= MAX_MESSAGE_CHARS);
    assert!(message.contains("saved as attachments/3-1-a.txt]"));
    assert!(!message.contains("```"));
    let err = compose(&"w".repeat(MAX_MESSAGE_CHARS), &saved).unwrap_err();
    assert_eq!(err.code, "POTATO_ATTACHMENT");
    assert!(err.message.contains("Shorten"));
}

#[test]
fn a_quote_that_contains_backticks_gets_a_longer_fence() {
    let root = temp("fence");
    let saved = save_all(&root, "nimbus", &[text_file("c.md", "before\n```js\nlet a = 1;\n```\nafter")], 4).unwrap();
    let message = compose("", &saved).unwrap();
    assert!(message.contains("c.md:\n````\nbefore"));
    assert!(message.trim_end().ends_with("after\n````"));
}

#[test]
fn size_words_are_short() {
    assert_eq!(size_word(300), "300 B");
    assert_eq!(size_word(2048), "2 KB");
    assert_eq!(size_word(1_572_864), "1.5 MB");
}
