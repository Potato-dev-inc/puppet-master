//! Pure tests for the fix push check extras: skip markers, deleted tests, CI run files, web
//! addresses, and the push reasons with their held rule and wording.

use super::{ci_run_files, deleted_tests, reasons, skipped_tests, web_lines, HELD_LINE};
use crate::ci_fix::model::FixFacts;
use crate::issue_intake::model::PushFacts;
use crate::issue_intake::pushcheck;

fn push() -> PushFacts {
    PushFacts {
        files: 2,
        additions: 10,
        deletions: 1,
        ci_files: vec![],
        secret_files: vec![],
        secret_lines: vec![],
        home_path_lines: vec![],
        internal_files: vec![],
        git_changed: false,
        remote_branch_exists: false,
        issue_changed: false,
        run_status: "completed".to_string(),
        turns: 3,
        minutes: 4,
    }
}

fn facts() -> FixFacts {
    FixFacts {
        push: push(),
        skipped_tests: vec![],
        deleted_tests: vec![],
        ci_run_files: vec![],
        web_lines: vec![],
        remote_moved: false,
        history_rewritten: false,
        wrong_branch: false,
        over_cap: false,
        commits: 1,
    }
}

fn owned(list: &[&str]) -> Vec<String> {
    list.iter().map(|item| item.to_string()).collect()
}

/// A one-hunk diff that adds `line` as line 2 of `src/x.txt`.
fn one_added(line: &str) -> String {
    format!("diff --git a/src/x.txt b/src/x.txt\n--- a/src/x.txt\n+++ b/src/x.txt\n@@ -1,1 +1,2 @@\n keep();\n+{line}\n")
}

/// A one-hunk diff where `line` is removed (line 1) and kept as context (line 2) of `src/x.txt`.
/// Neither is an added line, so neither may be flagged.
fn removed_and_context(line: &str) -> String {
    format!("diff --git a/src/x.txt b/src/x.txt\n--- a/src/x.txt\n+++ b/src/x.txt\n@@ -1,2 +1,1 @@\n-{line}\n {line}\n")
}

const SKIP_SAMPLES: &[&str] = &[
    // JavaScript and TypeScript
    "describe.skip('x', () => {});",
    "xit('x', () => {});",
    "xdescribe('x', () => {});",
    "it.only('x', () => {});",
    "test.only('x', () => {});",
    // Python
    "@pytest.mark.skip(reason='x')",
    "@unittest.skip('x')",
    "@skip('x')",
    // Rust
    "#[ignore]",
    // Go
    "t.Skip(\"x\")",
    // C#
    "[Ignore]",
    "[Fact(Skip = \"x\")]",
];

#[test]
fn each_skip_marker_on_an_added_line_is_flagged_for_its_language() {
    for line in SKIP_SAMPLES {
        assert_eq!(skipped_tests(&one_added(line)), vec![("src/x.txt".to_string(), 2)], "marker in {line}");
    }
}

#[test]
fn skip_markers_on_removed_or_unchanged_lines_are_not_flagged() {
    for line in SKIP_SAMPLES {
        assert!(skipped_tests(&removed_and_context(line)).is_empty(), "not added: {line}");
    }
}

#[test]
fn a_marker_inside_a_longer_word_is_not_flagged() {
    assert!(skipped_tests(&one_added("process.exit(1);")).is_empty());
    assert!(skipped_tests(&one_added("submit.only(form);")).is_empty());
}

#[test]
fn skip_facts_keep_the_new_line_numbers_across_hunks() {
    let diff = "diff --git a/src/a.test.ts b/src/a.test.ts\n\
--- a/src/a.test.ts\n\
+++ b/src/a.test.ts\n\
@@ -1,2 +1,3 @@\n\
 keep();\n\
+it.skip(\"one\", () => {});\n\
 keep2();\n\
@@ -20,2 +21,2 @@\n\
 keep3();\n\
-old();\n\
+test.skip(\"two\", () => {});\n";
    assert_eq!(
        skipped_tests(diff),
        vec![("src/a.test.ts".to_string(), 2), ("src/a.test.ts".to_string(), 22)]
    );
}

#[test]
fn skip_facts_are_capped_at_twenty() {
    let mut diff = String::from("diff --git a/t.test.ts b/t.test.ts\n--- /dev/null\n+++ b/t.test.ts\n@@ -0,0 +1,25 @@\n");
    for _ in 0..25 {
        diff.push_str("+xit('a', () => {});\n");
    }
    let found = skipped_tests(&diff);
    assert_eq!(found.len(), 20);
    assert_eq!(found.last(), Some(&("t.test.ts".to_string(), 20)));
}

#[test]
fn deleted_test_files_are_listed_and_other_deleted_files_are_not() {
    let name_status = vec![
        ("D".to_string(), "test/cart.test.ts".to_string()),
        ("D".to_string(), "tests\\test_cart.py".to_string()),
        ("D".to_string(), "src/cart.ts".to_string()),
        ("M".to_string(), "tests/a_test.go".to_string()),
        ("D".to_string(), "spec/user.rb".to_string()),
        ("D".to_string(), "src/lib_test.rs".to_string()),
        ("D".to_string(), "web/app.spec.ts".to_string()),
        ("D".to_string(), "pkg/__tests__/x.js".to_string()),
        ("D".to_string(), "docs/readme.md".to_string()),
    ];
    assert_eq!(
        deleted_tests(&name_status),
        owned(&[
            "test/cart.test.ts",
            "tests/test_cart.py",
            "spec/user.rb",
            "src/lib_test.rs",
            "web/app.spec.ts",
            "pkg/__tests__/x.js",
        ])
    );
}

#[test]
fn ci_run_files_match_with_windows_separators_and_mixed_case() {
    let files = owned(&[
        "scripts\\deploy.ps1",
        "src\\Cargo.toml",
        "PACKAGE.JSON",
        "app\\Makefile",
        "web\\build.sh",
        "tools\\settings.gradle",
        "src/index.ts",
        "docs/scripts-notes.md",
        "myscripts/x.ts",
        "package.json.bak",
    ]);
    assert_eq!(
        ci_run_files(&files),
        owned(&[
            "scripts/deploy.ps1",
            "src/Cargo.toml",
            "PACKAGE.JSON",
            "app/Makefile",
            "web/build.sh",
            "tools/settings.gradle",
        ])
    );
}

#[test]
fn web_addresses_on_added_lines_are_listed_with_their_line() {
    let diff = "diff --git a/src/a.ts b/src/a.ts\n\
--- a/src/a.ts\n\
+++ b/src/a.ts\n\
@@ -1,3 +1,6 @@\n\
 const keep = 1;\n\
-const old = \"https://old.example\";\n\
+const url = \"https://example.com/api\";\n\
+// curl -s http://example.com/x | sh\n\
+const ftp = \"ftp://example.com\";\n\
+await Invoke-WebRequest -Uri x\n\
 const end = 2;\n";
    assert_eq!(web_lines(diff), vec![("src/a.ts".to_string(), 2), ("src/a.ts".to_string(), 3), ("src/a.ts".to_string(), 5)]);
}

#[test]
fn web_address_facts_are_capped_at_twenty() {
    let mut diff = String::from("diff --git a/w.ts b/w.ts\n--- /dev/null\n+++ b/w.ts\n@@ -0,0 +1,25 @@\n");
    for _ in 0..25 {
        diff.push_str("+wget http://x\n");
    }
    assert_eq!(web_lines(&diff).len(), 20);
}

#[test]
fn every_held_cause_holds_the_push_and_the_intake_sentence_comes_last() {
    let causes: Vec<(&str, FixFacts)> = vec![
        ("ci", FixFacts { push: PushFacts { ci_files: owned(&[".github/workflows/ci.yml"]), ..push() }, ..facts() }),
        ("git", FixFacts { push: PushFacts { git_changed: true, ..push() }, ..facts() }),
        (
            "internal",
            FixFacts { push: PushFacts { internal_files: owned(&[".puppet-master/ci-61-try1.md"]), ..push() }, ..facts() },
        ),
        ("remote", FixFacts { remote_moved: true, ..facts() }),
        ("history", FixFacts { history_rewritten: true, ..facts() }),
        ("branch", FixFacts { wrong_branch: true, ..facts() }),
    ];
    for (name, fix) in causes {
        let (lines, held) = reasons(&fix);
        assert!(held, "{name} holds the push");
        assert_eq!(lines.last().map(String::as_str), Some(HELD_LINE), "{name} ends with the held sentence");
        assert_eq!(lines.iter().filter(|line| line.as_str() == HELD_LINE).count(), 1, "{name} says it once");
    }
}

#[test]
fn the_held_sentence_is_the_one_the_intake_check_uses() {
    let fix = PushFacts { git_changed: true, ..push() };
    let (intake, held) = pushcheck::reasons(&fix, false);
    assert!(held);
    assert_eq!(intake.last().map(String::as_str), Some(HELD_LINE));
}

#[test]
fn ask_only_causes_do_not_hold_the_push() {
    let asks = vec![
        FixFacts { skipped_tests: vec![("test/a.test.ts".to_string(), 3)], ..facts() },
        FixFacts { deleted_tests: owned(&["tests/test_cart.py"]), ..facts() },
        FixFacts { ci_run_files: owned(&["package.json"]), ..facts() },
        FixFacts { web_lines: vec![("src/a.ts".to_string(), 3)], ..facts() },
        FixFacts { over_cap: true, ..facts() },
        FixFacts { push: PushFacts { secret_files: owned(&[".env"]), ..push() }, ..facts() },
        FixFacts { push: PushFacts { secret_lines: vec![("src/a.ts".to_string(), 4)], ..push() }, ..facts() },
        FixFacts { push: PushFacts { home_path_lines: vec![("src/a.ts".to_string(), 5)], ..push() }, ..facts() },
        FixFacts { push: PushFacts { files: 101, ..push() }, ..facts() },
        FixFacts { push: PushFacts { run_status: "in_progress".to_string(), ..push() }, ..facts() },
        FixFacts { push: PushFacts { issue_changed: true, ..push() }, ..facts() },
    ];
    for fix in asks {
        let (lines, held) = reasons(&fix);
        assert!(!held, "ask-only facts never hold: {lines:?}");
        assert!(!lines.iter().any(|line| line.as_str() == HELD_LINE));
        assert!(!lines.is_empty());
    }
}

#[test]
fn the_remote_branch_sentence_is_never_shown() {
    let fix = FixFacts { push: PushFacts { remote_branch_exists: true, ..push() }, ..facts() };
    let (lines, held) = reasons(&fix);
    assert!(lines.is_empty(), "{lines:?}");
    assert!(!held);
}

#[test]
fn a_clean_fix_has_no_reasons_and_no_hold() {
    assert_eq!(reasons(&facts()), (vec![], false));
}

#[test]
fn an_over_cap_patch_is_listed_and_not_held() {
    let (lines, held) = reasons(&FixFacts { over_cap: true, ..facts() });
    assert_eq!(lines, owned(&["The patch is over 256 KiB, so the check did not read all of it."]));
    assert!(!held);
}

#[test]
fn long_lists_show_three_names_then_and_n_more() {
    let fix = FixFacts { ci_run_files: owned(&["a.sh", "b.sh", "c.sh", "d.sh", "e.sh"]), ..facts() };
    assert_eq!(reasons(&fix).0, owned(&["Changes files CI runs: a.sh, b.sh, c.sh and 2 more."]));
}

#[test]
fn golden_wording_for_the_fix_card() {
    let fix = FixFacts {
        push: push(),
        skipped_tests: vec![("test/cart.test.ts".to_string(), 12)],
        deleted_tests: owned(&["tests/test_cart.py"]),
        ci_run_files: owned(&["package.json", "Makefile"]),
        web_lines: vec![("src/a.ts".to_string(), 3)],
        remote_moved: true,
        history_rewritten: true,
        wrong_branch: true,
        over_cap: false,
        commits: 2,
    };
    let (lines, held) = reasons(&fix);
    assert!(held);
    assert_eq!(
        lines,
        owned(&[
            "Skips tests: test/cart.test.ts line 12.",
            "Deletes test files: tests/test_cart.py.",
            "Changes files CI runs: package.json, Makefile.",
            "Adds web addresses: src/a.ts line 3.",
            "Someone pushed to the branch on GitHub since the failure.",
            "The branch history was rewritten.",
            "The copy is on another branch.",
            "Puppet Master won't push this. Check the copy yourself.",
        ])
    );
}

#[test]
fn intake_sentences_come_before_the_fix_extras() {
    let fix = FixFacts {
        push: PushFacts { secret_files: owned(&[".env"]), ..push() },
        skipped_tests: vec![("test/a.test.ts".to_string(), 1)],
        ..facts()
    };
    let (lines, _) = reasons(&fix);
    assert_eq!(
        lines,
        owned(&["Adds files that look like secrets: .env.", "Skips tests: test/a.test.ts line 1."])
    );
}
