//! Pure tests for the push check: path rules, the added-line scan, the fingerprint and the reasons
//! wording. The real git reads are in `pushcheck_git_tests.rs`.

use super::model::{PushFacts, AUTO_PUSH_MAX_FILES};
use super::pushcheck::{
    ci_paths, diff_totals, fingerprint_of, git_fingerprint, internal_paths, reasons, secret_paths, scan_added,
};

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|p| p.to_string()).collect()
}

fn clean() -> PushFacts {
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

// ------------------------------------------------------------------------------- path rules

#[test]
fn every_ci_file_and_folder_is_flagged_with_either_separator_at_any_depth() {
    let flagged = [
        ".github/workflows/ci.yml",
        ".github\\workflows\\release.yml",
        ".GitHub/dependabot.yml",
        "packages/x/.github/workflows/y.yml",
        ".gitlab-ci.yml",
        "azure-pipelines.yml",
        "Jenkinsfile",
        ".circleci/config.yml",
        ".buildkite/pipeline.yml",
        "bitbucket-pipelines.yml",
        ".travis.yml",
    ];
    let all = paths(&flagged);
    assert_eq!(ci_paths(&all), all, "every CI path is flagged");
}

#[test]
fn ordinary_files_are_not_ci() {
    let plain = paths(&["src/github.rs", "docs/ci-notes.md", ".github-notes.txt", "src/Jenkinsfile.md"]);
    assert!(ci_paths(&plain).is_empty());
}

#[test]
fn secret_names_are_flagged_and_ordinary_names_are_not() {
    let secrets = [
        ".env",
        "app/.env.local",
        "keys/server.pem",
        "tls/x.key",
        "certs/a.p12",
        "a.pfx",
        "id_rsa",
        ".ssh/id_ed25519.pub",
        ".npmrc",
        ".pypirc",
        "config/credentials.json",
        "credentials-prod.json",
        "sub\\.env",
    ];
    assert_eq!(secret_paths(&paths(&secrets)), paths(&secrets));
    let plain = paths(&["src/env.rs", "environment.md", "key.rs", "keyboard.ts", ".envrc", "envelope.txt"]);
    assert!(secret_paths(&plain).is_empty());
}

#[test]
fn puppet_master_folder_is_internal_at_any_depth_with_either_separator() {
    let internal = paths(&[".puppet-master/issue-42.md", ".puppet-master\\issue-42.md", "sub/.puppet-master/x"]);
    assert_eq!(internal_paths(&internal), internal);
    let other = paths(&["puppet-master/x.md", ".puppet-masterx/x", "src/a.rs"]);
    assert!(internal_paths(&other).is_empty());
}

// ------------------------------------------------------------------------- the added-line scan

/// Two hunks in `src/a.txt` (the second one has a `+++` line inside it, which is content), then a
/// new file `b.env`. A removed line and a context line hold secret-looking text: neither counts.
const DIFF: &str = "\
diff --git a/src/a.txt b/src/a.txt
index 111..222 100644
--- a/src/a.txt
+++ b/src/a.txt
@@ -1,4 +1,5 @@
 keep one
-OLD_KEY=zzz
+API_KEY=abc123
 keep two
+plain add
 keep three
@@ -10,1 +11,3 @@ fn x
 ctx
+++ inside hunk: looks like a header
+C:\\Users\\you\\Desktop
diff --git a/b.env b/b.env
new file mode 100644
--- /dev/null
+++ b/b.env
@@ -0,0 +1,2 @@
+TOKEN=zzz
+ok
";

fn key_hit(line: &str) -> bool {
    line.contains("KEY=") || line.contains("TOKEN=")
}

#[test]
fn scan_flags_only_added_lines_with_the_right_new_line_numbers_across_hunks() {
    let (secrets, homes) = scan_added(DIFF, &key_hit, Some("C:\\Users\\you"));
    assert_eq!(secrets, vec![("src/a.txt".to_string(), 2), ("b.env".to_string(), 1)]);
    assert_eq!(homes, vec![("src/a.txt".to_string(), 13)]);
}

#[test]
fn a_plus_plus_plus_line_inside_a_hunk_is_content_so_the_counts_hold() {
    assert_eq!(diff_totals(DIFF), (6, 1));
}

#[test]
fn home_folder_match_ignores_slash_direction_and_case() {
    let diff = "@@ -0,0 +1,1 @@\n+see c:/users/YOU/notes\n";
    let (_, homes) = scan_added(diff, &|_| false, Some("C:\\Users\\you\\"));
    assert_eq!(homes, vec![(String::new(), 1)]);
}

#[test]
fn no_home_folder_means_no_home_lines() {
    let (_, homes) = scan_added(DIFF, &key_hit, None);
    assert!(homes.is_empty());
}

#[test]
fn each_list_keeps_at_most_twenty_entries() {
    let mut diff = String::from("+++ b/many.txt\n@@ -0,0 +1,25 @@\n");
    for _ in 0..25 {
        diff.push_str("+SECRET_KEY=1\n");
    }
    let (secrets, _) = scan_added(&diff, &key_hit, None);
    assert_eq!(secrets.len(), 20);
    assert_eq!(secrets.last(), Some(&("many.txt".to_string(), 20)));
}

#[test]
fn an_empty_or_odd_diff_gives_nothing_and_does_not_panic() {
    assert_eq!(scan_added("", &key_hit, Some("C:/Users/you")), (vec![], vec![]));
    assert_eq!(diff_totals("@@ -1,9 +1,9 @@\n+x\n"), (1, 0), "a cut-short hunk still counts the lines it has");
    assert_eq!(diff_totals("+not in a hunk\n-nor this\n"), (0, 0));
}

// ----------------------------------------------------------------------------- fingerprint

const CONFIG: &str = "global\tfile:/home/x/.gitconfig\tuser.name=Me\n\
local\tfile:.git/config\tcore.fsmonitor=false\n\
local\tfile:.git/config\tremote.origin.url=https://github.com/acme/billing.git\n\
command\tcommand line:\tGIT_CONFIG_COUNT=2\n";

fn hooks() -> Vec<(String, String)> {
    vec![
        (".git/hooks/pre-push".to_string(), "aa".to_string()),
        (".husky/pre-commit".to_string(), "bb".to_string()),
    ]
}

#[test]
fn fingerprint_ignores_line_order_of_config_and_hooks() {
    let base = git_fingerprint(CONFIG, &hooks());
    let mut reversed = hooks();
    reversed.reverse();
    let shuffled: String = CONFIG.lines().rev().map(|line| format!("{line}\n")).collect();
    assert_eq!(git_fingerprint(&shuffled, &reversed), base);
}

#[test]
fn fingerprint_changes_with_a_local_config_line_but_not_with_a_global_one() {
    let base = git_fingerprint(CONFIG, &hooks());
    let fsmonitor_on = CONFIG.replace("core.fsmonitor=false", "core.fsmonitor=true");
    assert_ne!(git_fingerprint(&fsmonitor_on, &hooks()), base);
    let url_swapped = CONFIG.replace("acme/billing", "evil/billing");
    assert_ne!(git_fingerprint(&url_swapped, &hooks()), base);
    let global_changed = CONFIG.replace("user.name=Me", "user.name=Other");
    assert_eq!(git_fingerprint(&global_changed, &hooks()), base, "global settings are not the agent's");
}

#[test]
fn fingerprint_changes_with_a_hook_edit_or_a_new_hook_file() {
    let base = git_fingerprint(CONFIG, &hooks());
    let mut edited = hooks();
    edited[0].1 = "cc".to_string();
    assert_ne!(git_fingerprint(CONFIG, &edited), base);
    let mut added = hooks();
    added.push((".git/hooks/post-commit".to_string(), "dd".to_string()));
    assert_ne!(git_fingerprint(CONFIG, &added), base);
}

#[test]
fn fingerprint_of_a_snapshot_matches_the_plain_call() {
    let snapshot = (CONFIG.to_string(), hooks());
    assert_eq!(fingerprint_of(&snapshot), git_fingerprint(CONFIG, &hooks()));
}

// --------------------------------------------------------------------------- reasons

#[test]
fn a_clean_run_with_asking_off_needs_no_card() {
    assert_eq!(reasons(&clean(), false), (vec![], false));
}

#[test]
fn asking_before_each_push_gives_one_sentence_and_is_not_held() {
    let (lines, held) = reasons(&clean(), true);
    assert_eq!(lines, vec!["You asked Puppet Master to check each push first."]);
    assert!(!held);
}

#[test]
fn an_issue_that_changed_after_the_yes_is_a_reason_but_not_a_hold() {
    let mut facts = clean();
    facts.issue_changed = true;
    let (lines, held) = reasons(&facts, false);
    assert_eq!(lines, ["The issue changed after you said yes."]);
    assert!(!held, "the person may still push");
    // Asking is on too: both lines, the change first.
    let (lines, held) = reasons(&facts, true);
    assert_eq!(lines, ["The issue changed after you said yes.", "You asked Puppet Master to check each push first."]);
    assert!(!held);
    // An old saved record without the field reads as not changed.
    let mut value = serde_json::to_value(clean()).unwrap();
    value.as_object_mut().unwrap().remove("issue_changed");
    let old: PushFacts = serde_json::from_value(value).unwrap();
    assert!(!old.issue_changed);
}

#[test]
fn forced_reasons_say_what_was_found() {
    let mut facts = clean();
    facts.ci_files = paths(&[".github/workflows/x.yml"]);
    facts.secret_files = paths(&[".env"]);
    facts.secret_lines = vec![("src/a.txt".to_string(), 7)];
    facts.home_path_lines = vec![("notes.md".to_string(), 3)];
    facts.run_status = "timed out".to_string();
    let (lines, held) = reasons(&facts, false);
    assert_eq!(
        lines,
        vec![
            "Changes CI or workflow files: .github/workflows/x.yml.",
            "Adds files that look like secrets: .env.",
            "Adds lines that look like a key or token: src/a.txt line 7.",
            "Adds lines with your home folder path: notes.md line 3.",
            "The run did not finish (status: timed out).",
        ]
    );
    assert!(!held, "these are forced cards, not held pushes");
}

#[test]
fn more_than_the_file_cap_is_forced_and_the_cap_itself_is_not() {
    let mut facts = clean();
    facts.files = AUTO_PUSH_MAX_FILES as u32;
    assert!(reasons(&facts, false).0.is_empty());
    facts.files = AUTO_PUSH_MAX_FILES as u32 + 1;
    let (lines, _) = reasons(&facts, false);
    assert_eq!(lines, vec![format!("Changes {} files, more than {}.", AUTO_PUSH_MAX_FILES + 1, AUTO_PUSH_MAX_FILES)]);
}

#[test]
fn long_lists_show_three_names_then_how_many_more() {
    let mut facts = clean();
    facts.secret_files = paths(&["a.env", "b.pem", "c.key", "d.p12", "e.pfx"]);
    let (lines, _) = reasons(&facts, false);
    assert_eq!(lines, vec!["Adds files that look like secrets: a.env, b.pem, c.key and 2 more."]);
}

#[test]
fn git_changed_remote_branch_and_internal_files_are_held_with_the_sentence() {
    let held_line = "Puppet Master won't push this. Check the copy yourself.";

    let mut git = clean();
    git.git_changed = true;
    let (lines, held) = reasons(&git, true);
    assert!(held);
    assert!(lines.contains(&"The git settings or hooks changed while the agent worked.".to_string()));
    assert_eq!(lines.last().map(String::as_str), Some(held_line));

    let mut remote = clean();
    remote.remote_branch_exists = true;
    let (lines, held) = reasons(&remote, false);
    assert!(held);
    assert!(lines.contains(&"A branch with this name already exists on GitHub.".to_string()));

    let mut internal = clean();
    internal.internal_files = paths(&[".puppet-master/issue-42.md"]);
    let (lines, held) = reasons(&internal, false);
    assert!(held);
    assert!(lines.contains(&"Includes Puppet Master's own files: .puppet-master/issue-42.md.".to_string()));
    assert_eq!(lines.last().map(String::as_str), Some(held_line));
}
