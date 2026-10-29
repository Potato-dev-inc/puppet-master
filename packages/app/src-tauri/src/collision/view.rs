//! Collision Alert: the view the windows get from an alert (SPEC-n33 sections 3.2, 3.8 and 5 CA-10).
//!
//! Every string that came from a file or a pane goes through `scrub` first, in this order:
//! 1. `sanitize`: control and bidi or hidden marks are removed.
//! 2. The home folder is written as `~` (both slash styles).
//! 3. The injected `clean` (the live layer passes `redact::text`: known secrets and token shapes).
//! 4. `redact::mask_emails`: an address becomes `<email>`.
//! 5. Whitespace is flattened to one line, then the text is cut (paths 120, names 40).
//!
//! Pane ids are opaque ids, not text, and are not scrubbed. The advice and footer sentences are
//! built from already scrubbed names. The view holds no folder and no process text.

use serde::Serialize;

use crate::push::triggers::sanitize;
use crate::redact::mask_emails;

use super::copy;
use super::model::{Alert, AlertView, FileView, SideView, MAX_FILES_SHOWN, MAX_PATH_CHARS};

/// Pane headings are cut to this many characters (`attention::copy::headings` cuts them the same way).
const MAX_NAME_CHARS: usize = 40;

/// The view of one alert. `clean` is the caller's redaction (the live layer passes `sanitize` plus
/// `redact::text`); it runs on every string, see the module doc.
pub fn view(alert: &Alert, clean: &dyn Fn(&str) -> String) -> AlertView {
    let home = crate::project_path::home_dir().map(|p| p.to_string_lossy().into_owned());
    build(alert, clean, home.as_deref())
}

/// The title, detail and advice of a Ready-lane item on the phone: `(title, sentence)`. The sentence
/// is the detail, then the advice when there is one. The item has no buttons.
pub fn attention_title_sentence(v: &AlertView) -> (String, Option<String>) {
    let parts: Vec<&str> = [v.detail.as_str(), v.advice.as_deref().unwrap_or("")]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    let sentence = (!parts.is_empty()).then(|| parts.join(" "));
    (v.title.clone(), sentence)
}

fn build(alert: &Alert, clean: &dyn Fn(&str) -> String, home: Option<&str>) -> AlertView {
    let scrub = |raw: &str, max: usize| {
        let cleaned = clean(&home_as_tilde(&sanitize(raw), home));
        cut(&flatten(&mask_emails(&cleaned)), max)
    };
    let names: [String; 2] = [
        scrub(&alert.panes[0].label, MAX_NAME_CHARS),
        scrub(&alert.panes[1].label, MAX_NAME_CHARS),
    ];
    let name_refs = [names[0].as_str(), names[1].as_str()];
    let paths: Vec<String> = alert
        .files
        .iter()
        .take(MAX_FILES_SHOWN)
        .map(|file| scrub(&file.path, MAX_PATH_CHARS))
        .collect();

    let title = copy::title(
        name_refs[0],
        name_refs[1],
        alert.level,
        paths.first().map(String::as_str),
        alert.files.len().saturating_sub(1),
    );
    let detail = alert
        .files
        .first()
        .map(|file| copy::detail(&file.how, name_refs, &paths))
        .unwrap_or_default();
    let advice = alert.advice.map(|adv| {
        let (first, other) = if adv.first == 0 { (0, 1) } else { (1, 0) };
        copy::advice_line(alert.relation, adv.why, name_refs[first], name_refs[other])
    });
    let files: Vec<FileView> = alert
        .files
        .iter()
        .take(MAX_FILES_SHOWN)
        .zip(paths)
        .map(|(file, path)| FileView { path, text: copy::file_text(&file.how) })
        .collect();
    let sides = [
        SideView {
            pane_id: alert.panes[0].pane_id.clone(),
            label: names[0].clone(),
            working: alert.sides[0].working,
        },
        SideView {
            pane_id: alert.panes[1].pane_id.clone(),
            label: names[1].clone(),
            working: alert.sides[1].working,
        },
    ];

    AlertView {
        id: alert.id.clone(),
        level: wire(&alert.level),
        relation: wire(&alert.relation),
        title,
        detail,
        advice,
        place: copy::place(alert.relation),
        footer: copy::footer(),
        sides,
        files,
        more: alert.files.len().saturating_sub(MAX_FILES_SHOWN) as u32,
        first_ms: alert.first_ms,
        last_ms: alert.last_ms,
    }
}

/// The serde name of a level or relation (`"clash"`, `"same_folder"`), so the view and the wire
/// format cannot drift apart.
fn wire<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value).ok().and_then(|json| json.as_str().map(str::to_owned)).unwrap_or_default()
}

/// Rewrites the user's home folder as `~`. A home shorter than 3 characters is left alone (as
/// `redact::rules` does). Matching ignores ASCII case and needs the home to end at a separator or at
/// the end of the text, so `C:\Users\you-pc` is not read as `C:\Users\you`.
fn home_as_tilde(text: &str, home: Option<&str>) -> String {
    let Some(home) = home.map(|h| h.trim_end_matches(['\\', '/'])).filter(|h| h.len() >= 3) else {
        return text.to_string();
    };
    let forward = home.replace('\\', "/");
    let backward = home.replace('/', "\\");
    let mut out = replace_home(text, home);
    if forward != home {
        out = replace_home(&out, &forward);
    }
    if backward != home {
        out = replace_home(&out, &backward);
    }
    out
}

fn replace_home(text: &str, home: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let needle = home.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (at, _) in lower.match_indices(&needle) {
        let end = at + needle.len();
        let ends_here = matches!(lower.as_bytes().get(end).copied(), None | Some(b'\\') | Some(b'/'));
        if !ends_here {
            continue;
        }
        out.push_str(&text[last..at]);
        out.push('~');
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

/// One line: every run of whitespace (newlines, tabs, line separators) becomes one space.
fn flatten(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `max` characters, ending in `...` when cut.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redact::{self, RedactContext};
    use super::super::model::{Advice, AlertId, FileOverlap, How, Level, PaneFact, PaneRun, Relation, SideFacts, Span, Why};

    fn pane(id: &str, label: &str, cwd: &str) -> PaneFact {
        PaneFact {
            pane_id: id.to_string(),
            agent_id: "fake-agent-kind".to_string(),
            label: label.to_string(),
            cwd: cwd.to_string(),
            run: PaneRun::Working,
            created_ms: 1,
        }
    }

    fn file(path: &str, how: How) -> FileOverlap {
        FileOverlap { path: path.to_string(), how }
    }

    fn side(pane_id: &str, working: bool, files: u32) -> SideFacts {
        SideFacts { pane_id: pane_id.to_string(), working, files, started_ms: 1 }
    }

    /// A copies alert of `fake_a` (pane-1, working) and `fake_b` (pane-2, stopped).
    fn alert_with(relation: Relation, level: Level, files: Vec<FileOverlap>, advice: Option<Advice>) -> Alert {
        let mut sides = [side("pane-1", true, 4), side("pane-2", false, 10)];
        sides[0].started_ms = 2;
        Alert {
            id: AlertId::of(&["pane-1", "pane-2"]),
            relation,
            level,
            panes: [
                pane("pane-1", "fake_a", "D:/work/fake-proj-root"),
                pane("pane-2", "fake_b", "D:/work/fake-proj-root-2"),
            ],
            sides,
            files,
            advice,
            first_ms: 5,
            last_ms: 9,
            misses: 0,
            hidden: false,
        }
    }

    fn clash_copies(how: How) -> Alert {
        alert_with(Relation::Copies, Level::Clash, vec![file("src/auth.ts", how)], None)
    }

    /// The advice of a card whose stopped side is `fake_b` (`first: 1`).
    fn stopped_b() -> Option<Advice> {
        Some(Advice { first: 1, why: Why::AlreadyDone })
    }

    fn plain(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn clash_copies_view_has_the_card_words() {
        let mut alert = clash_copies(How::Lines(Span { start: 40, end: 80 }));
        alert.advice = stopped_b();
        let v = view(&alert, &|s: &str| plain(s));
        assert_eq!(v.id, alert.id);
        assert_eq!(v.level, "clash");
        assert_eq!(v.relation, "copies");
        assert_eq!(v.title, "fake_a and fake_b both changed src/auth.ts");
        assert_eq!(v.detail, "Lines 40-80 overlap.");
        assert_eq!(v.place, "In separate copies.");
        assert_eq!(
            v.advice.as_deref(),
            Some("Finish fake_b first. It has stopped working. Look at its changes and merge them first. Then fake_a can build on it.")
        );
        assert_eq!(v.footer, "Nothing was paused. You decide.");
        assert_eq!(v.files, vec![FileView { path: "src/auth.ts".into(), text: "Lines 40-80".into() }]);
        assert_eq!(v.more, 0);
        assert_eq!((v.first_ms, v.last_ms), (5, 9));
    }

    #[test]
    fn sides_keep_pane_ids_and_working_flags_in_order() {
        let alert = clash_copies(How::Apart);
        let v = view(&alert, &|s: &str| plain(s));
        assert_eq!(v.sides[0].pane_id, "pane-1");
        assert_eq!(v.sides[0].label, "fake_a");
        assert!(v.sides[0].working);
        assert_eq!(v.sides[1].pane_id, "pane-2");
        assert_eq!(v.sides[1].label, "fake_b");
        assert!(!v.sides[1].working);
    }

    #[test]
    fn same_file_view_has_no_advice_and_is_quiet() {
        let alert = alert_with(Relation::Copies, Level::SameFile, vec![file("src/auth.ts", How::Apart)], None);
        let v = view(&alert, &|s: &str| plain(s));
        assert_eq!(v.level, "same_file");
        assert_eq!(v.title, "fake_a and fake_b both changed src/auth.ts");
        assert_eq!(v.detail, "Different places in the file. No clash yet.");
        assert_eq!(v.advice, None);
    }

    #[test]
    fn same_folder_view_lists_the_shared_files_and_names_the_stopped_side() {
        let files = vec![file("src/a.ts", How::Shared), file("src/b.ts", How::Shared)];
        let advice = Some(Advice { first: 1, why: Why::AlreadyDone });
        let alert = alert_with(Relation::SameFolder, Level::SharedFolder, files, advice);
        let v = view(&alert, &|s: &str| plain(s));
        assert_eq!(v.level, "shared_folder");
        assert_eq!(v.relation, "same_folder");
        assert_eq!(v.title, "fake_a and fake_b are working in the same folder");
        assert_eq!(v.place, "In the same folder.");
        assert_eq!(
            v.detail,
            "They can overwrite each other. Changed while both worked: src/a.ts, src/b.ts."
        );
        assert_eq!(
            v.advice.as_deref(),
            Some("fake_b has stopped working. Ask fake_a to read the file again before it changes more.")
        );
        assert_eq!(v.files.len(), 2);
    }

    #[test]
    fn copies_advice_sentences_follow_the_three_reasons() {
        let started = Some(Advice { first: 0, why: Why::StartedFirst });
        let smaller = Some(Advice { first: 0, why: Why::SmallerChange });
        for (advice, expected) in [
            (started, "Finish fake_a first. It started first. Merge it first. Then fake_b can build on it."),
            (
                smaller,
                "Finish fake_a first. It has changed fewer files, so it is quicker to finish. Merge it first. Then fake_b can build on it.",
            ),
        ] {
            let alert = alert_with(Relation::Copies, Level::Clash, vec![file("src/auth.ts", How::Binary)], advice);
            let v = view(&alert, &|s: &str| plain(s));
            assert_eq!(v.advice.as_deref(), Some(expected));
        }
    }

    #[test]
    fn every_how_reaches_the_detail_with_the_names() {
        let cases = [
            (How::LinesUnknown, "The change is big, so lines were not compared."),
            (How::BothNew, "Both made a new file with this name."),
            (How::DeleteVsEdit { deleted_by: 1 }, "fake_b deleted it. fake_a changed it."),
            (How::Binary, "Both changed a file that is not text."),
            (How::DifferentStart, "They started from different versions, so lines were not compared."),
        ];
        for (how, expected) in cases {
            let v = view(&clash_copies(how), &|s: &str| plain(s));
            assert_eq!(v.detail, expected);
        }
    }

    #[test]
    fn several_files_show_five_and_count_the_rest() {
        let files: Vec<FileOverlap> = (0..7).map(|n| file(&format!("src/f{n}.ts"), How::BothNew)).collect();
        let alert = alert_with(Relation::Copies, Level::Clash, files, None);
        let v = view(&alert, &|s: &str| plain(s));
        assert_eq!(v.files.len(), 5);
        assert_eq!(v.more, 2);
        assert_eq!(v.title, "fake_a and fake_b both changed src/f0.ts and 6 more files");
    }

    #[test]
    fn a_long_path_is_cut_to_120_characters_and_a_long_name_to_40() {
        let long_path = format!("src/{}.ts", "x".repeat(200));
        let mut alert = alert_with(Relation::Copies, Level::Clash, vec![file(&long_path, How::BothNew)], None);
        alert.panes[0].label = "n".repeat(60);
        let v = view(&alert, &|s: &str| plain(s));
        assert_eq!(v.files[0].path.chars().count(), 120);
        assert!(v.files[0].path.ends_with("..."));
        assert_eq!(v.sides[0].label.chars().count(), 40);
        assert!(v.sides[0].label.ends_with("..."));
    }

    #[test]
    fn names_with_braces_are_not_templates() {
        let mut alert = alert_with(Relation::Copies, Level::Clash, vec![file("src/{b}.ts", How::BothNew)], None);
        alert.panes[0].label = "{a}".to_string();
        alert.panes[1].label = "{0}".to_string();
        let v = view(&alert, &|s: &str| plain(s));
        assert_eq!(v.title, "{a} and {0} both changed src/{b}.ts");
        assert_eq!(v.sides[0].label, "{a}");
        assert_eq!(v.sides[1].label, "{0}");
    }

    #[test]
    fn the_injected_clean_runs_on_every_string_from_a_file_or_a_pane() {
        use std::cell::RefCell;
        let seen: RefCell<Vec<String>> = RefCell::new(Vec::new());
        let planted_path = "src/SECRET-7\u{202e}.ts\nnew line.ts";
        let mut alert = alert_with(Relation::Copies, Level::Clash, vec![file(planted_path, How::BothNew)], None);
        alert.panes[0].label = "fake_a SECRET-7".to_string();
        let clean = |raw: &str| {
            seen.borrow_mut().push(raw.to_string());
            raw.replace("SECRET-7", "[masked]")
        };
        let v = view(&alert, &clean);
        let calls = seen.borrow();
        assert!(calls.iter().any(|c| c.contains("fake_a")), "clean never saw the pane heading");
        assert!(calls.iter().any(|c| c.contains("SECRET-7.ts")), "clean never saw the path");
        let json = serde_json::to_string(&v).unwrap();
        assert!(!json.contains("SECRET-7"), "a planted secret reached the view: {json}");
        assert!(!json.contains('\u{202e}'), "a bidi mark reached the view");
        assert!(!v.title.contains('\n') && !v.files[0].path.contains('\n'), "a newline reached the view");
        assert_eq!(v.files[0].path, "src/[masked].ts new line.ts");
        assert_eq!(v.sides[0].label, "fake_a [masked]");
    }

    #[test]
    fn e_mails_in_names_are_masked() {
        let alert = alert_with(
            Relation::Copies,
            Level::Clash,
            vec![file("notes/alice@example.com", How::BothNew)],
            None,
        );
        let v = view(&alert, &|s: &str| plain(s));
        assert_eq!(v.files[0].path, "notes/<email>");
        assert!(!serde_json::to_string(&v).unwrap().contains("alice@example.com"));
    }

    #[test]
    fn the_real_redaction_masks_a_known_secret_in_a_path() {
        let ctx = RedactContext { env_values: vec!["hunter2-secret".into()], home: None };
        let alert = alert_with(
            Relation::Copies,
            Level::Clash,
            vec![file("config/hunter2-secret.env", How::BothNew)],
            None,
        );
        let v = view(&alert, &|s: &str| redact::text_with(s, &ctx).into_inner());
        assert!(!v.files[0].path.contains("hunter2-secret"), "{}", v.files[0].path);
        assert!(v.files[0].path.starts_with("config/"));
    }

    #[test]
    fn the_home_folder_is_written_as_tilde_in_both_slash_styles() {
        let home = Some(r"C:\Users\you");
        assert_eq!(home_as_tilde(r"C:\Users\you\src\a.ts", home), r"~\src\a.ts");
        assert_eq!(home_as_tilde("C:/Users/you/src/a.ts", home), "~/src/a.ts");
        assert_eq!(home_as_tilde(r"c:\users\YOU\a.ts", home), r"~\a.ts");
        assert_eq!(home_as_tilde(r"C:\Users\you-pc\a.ts", home), r"C:\Users\you-pc\a.ts");
        assert_eq!(home_as_tilde(r"C:\Users\you", home), "~");
        assert_eq!(home_as_tilde("src/a.ts", home), "src/a.ts");
        assert_eq!(home_as_tilde(r"C:\Users\you\a.ts", None), r"C:\Users\you\a.ts");
        assert_eq!(home_as_tilde(r"ab\x", Some("ab")), r"ab\x");
    }

    #[test]
    fn a_home_path_in_a_name_reaches_the_view_as_tilde() {
        let alert = alert_with(
            Relation::Copies,
            Level::Clash,
            vec![file(r"C:\Users\you\proj\a.ts", How::BothNew)],
            None,
        );
        let v = view_home_for_test(&alert, Some(r"C:\Users\you"));
        assert_eq!(v.files[0].path, r"~\proj\a.ts");
    }

    fn view_home_for_test(alert: &Alert, home: Option<&str>) -> AlertView {
        build(alert, &|s: &str| s.to_string(), home)
    }

    #[test]
    fn the_view_holds_no_folder_and_no_process_text() {
        let alert = clash_copies(How::Apart);
        let json = serde_json::to_string(&view(&alert, &|s: &str| plain(s))).unwrap();
        assert!(!json.contains("fake-proj-root"), "a folder reached the view");
        assert!(!json.contains("fake-agent-kind"), "a process or agent kind reached the view");
        assert!(!json.contains("D:/work"), "a folder reached the view");
        assert!(!json.contains("created"), "a pane time reached the view");
    }

    #[test]
    fn the_phone_sentence_is_detail_then_advice() {
        let mut alert = clash_copies(How::Lines(Span { start: 40, end: 80 }));
        alert.advice = stopped_b();
        let v = view(&alert, &|s: &str| plain(s));
        let (title, sentence) = attention_title_sentence(&v);
        assert_eq!(title, "fake_a and fake_b both changed src/auth.ts");
        assert_eq!(
            sentence.as_deref(),
            Some("Lines 40-80 overlap. Finish fake_b first. It has stopped working. Look at its changes and merge them first. Then fake_a can build on it.")
        );
    }

    #[test]
    fn the_phone_sentence_without_advice_is_the_detail_alone() {
        let alert = alert_with(Relation::Copies, Level::SameFile, vec![file("src/a.ts", How::Apart)], None);
        let v = view(&alert, &|s: &str| plain(s));
        let (_, sentence) = attention_title_sentence(&v);
        assert_eq!(sentence.as_deref(), Some("Different places in the file. No clash yet."));
    }

    #[test]
    fn the_phone_sentence_is_none_when_there_is_no_text() {
        let mut v = view(&clash_copies(How::Apart), &|s: &str| plain(s));
        v.detail = String::new();
        v.advice = None;
        assert_eq!(attention_title_sentence(&v).1, None);
    }

    #[test]
    fn view_serde_names_match_the_ui_contract() {
        // The UI contract is `CollisionView`, `CollisionSide` and `CollisionFile` in src/lib/collision.ts.
        let source = include_str!("../../../src/lib/collision.ts");
        let mut alert = clash_copies(How::Lines(Span { start: 40, end: 80 }));
        alert.advice = Some(Advice { first: 0, why: Why::StartedFirst });
        let json = serde_json::to_value(view(&alert, &|s: &str| plain(s))).unwrap();
        assert_same_keys(&json, &ts_interface_keys(source, "CollisionView"));
        assert_same_keys(&json["sides"][0], &ts_interface_keys(source, "CollisionSide"));
        assert_same_keys(&json["files"][0], &ts_interface_keys(source, "CollisionFile"));
        for level in ["clash", "same_file", "shared_folder"] {
            assert!(source.contains(&format!("'{level}'")), "the UI lacks the level {level}");
        }
        for relation in ["copies", "same_folder"] {
            assert!(source.contains(&format!("'{relation}'")), "the UI lacks the relation {relation}");
        }
    }

    fn assert_same_keys(json: &serde_json::Value, expected: &[String]) {
        let mut got: Vec<String> = json.as_object().expect("object").keys().cloned().collect();
        got.sort();
        let mut want = expected.to_vec();
        want.sort();
        assert_eq!(got, want);
    }

    /// The field names of `export interface <name> { ... }` in a TypeScript file (two-space indent).
    fn ts_interface_keys(source: &str, name: &str) -> Vec<String> {
        let head = format!("export interface {name} {{");
        let body = source.split(&head).nth(1).and_then(|rest| rest.split("\n}").next()).unwrap_or("");
        body.lines()
            .filter_map(|line| {
                if !line.starts_with("  ") || line.starts_with("   ") {
                    return None;
                }
                let text = line.trim_start();
                let key: String = text.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                let after = &text[key.len()..];
                (!key.is_empty() && (after.starts_with(':') || after.starts_with('?'))).then_some(key)
            })
            .collect()
    }

    #[test]
    fn the_view_holds_only_the_wire_fields() {
        let alert = clash_copies(How::Apart);
        let json = serde_json::to_value(view(&alert, &|s: &str| plain(s))).unwrap();
        let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "advice", "detail", "files", "firstMs", "footer", "id", "lastMs", "level", "more", "place", "relation",
                "sides", "title"
            ]
        );
    }

    #[test]
    fn view_names_no_agent() {
        // C12: the code above the tests names no agent. Labels come from the pane facts only.
        let source = include_str!("view.rs");
        let code = source.split("#[cfg(test)]").next().unwrap_or("");
        let lower = code.to_ascii_lowercase();
        for word in ["claude", "codex", "cursor", "opencode", "agy"] {
            assert!(!lower.contains(word), "generic code names an agent: {word}");
        }
    }
}
