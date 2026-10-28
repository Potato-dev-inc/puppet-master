use super::rules::{mask_pieces, test_ctx, Mode, Utf8Carry};
use super::*;

fn ctx() -> RedactContext {
    test_ctx()
}

#[test]
fn token_shapes_are_masked() {
    let cases: &[(&str, &str)] = &[
        ("key sk-ant-api03-AbCdEf123456 here", "sk-ant"),
        ("ghp_abcdefghijklmnopqrstuvwxyz0123456789", "ghp_abc"),
        ("id AKIAIOSFODNN7EXAMPLE end", "AKIAIOSF"),
        ("AIzaSyA-1234567890abcdefghijklmnopqrstuv", "AIzaSy"),
        ("sk_live_51HabcdefghijKLMNOP", "sk_live"),
        ("glpat-abcdefghij1234567890", "glpat-"),
        ("Authorization: Bearer abcdef1234567890xyz", "abcdef1234"),
        ("Authorization: Basic dXNlcjpwYXNzd29yZA==", "dXNlcjpw"),
        ("postgres://admin:s3cretpass@db.example.test:5432/app", "s3cretpass"),
        ("-----BEGIN RSA PRIVATE KEY-----\nMIIEabc\n-----END RSA PRIVATE KEY-----", "MIIEabc"),
    ];
    for (input, needle) in cases {
        let out = text_with(input, &ctx());
        assert!(!out.as_str().contains(needle), "{input:?} -> {out}");
        assert!(out.as_str().contains(MASK), "{input:?} -> {out}");
    }
}

#[test]
fn dotenv_style_secrets_are_masked_with_their_names_kept() {
    let input = "DATABASE_URL=sqlite:///x.db\nOPENAI_API_KEY=\"abc def ghi 123\"\nexport GITHUB_TOKEN=abcd1234\nPORT=8080\n";
    let out = text_with(input, &ctx());
    assert!(out.as_str().contains("DATABASE_URL=sqlite:///x.db"), "{out}");
    assert!(out.as_str().contains("PORT=8080"), "{out}");
    assert!(out.as_str().contains("OPENAI_API_KEY=[REDACTED]"), "{out}");
    assert!(out.as_str().contains("GITHUB_TOKEN=[REDACTED]"), "{out}");
    assert!(!out.as_str().contains("abc def"), "{out}");
    assert!(!out.as_str().contains("abcd1234"), "{out}");
}

#[test]
fn the_default_strength_keeps_commit_hashes_and_paths_the_strict_one_does_not() {
    let line = "commit 0123456789abcdef0123456789abcdef01234567 in C:\\Users\\you-pc\\Desktop\\x";
    let normal = text_with(line, &ctx());
    assert_eq!(normal.as_str(), line, "transcripts keep what agents read");
    let strict = strict_with(line, &ctx());
    assert!(!strict.as_str().contains("0123456789abcdef0123"), "{strict}");
    assert!(!strict.as_str().to_lowercase().contains("you-pc"), "{strict}");
}

#[test]
fn task_and_disk_words_are_not_mistaken_for_keys() {
    // "sk-" inside a word is not an API key prefix.
    let line = "run the task-runner on the risk-assessment, then ask-me";
    assert_eq!(text_with(line, &ctx()).as_str(), line);
}

#[test]
fn a_secret_split_over_two_chunks_is_still_masked() {
    let key = "sk-ant-api03-SplitAcrossChunks123456";
    let (a, b) = key.split_at(14);
    let pieces = mask_pieces(&["before ", a, b, " after"], &ctx(), Mode::Strict);
    let joined = pieces.concat();
    assert!(!joined.contains("SplitAcross"), "{joined}");
    assert!(joined.starts_with("before ") && joined.ends_with(" after"), "{joined}");
    assert_eq!(pieces.len(), 4, "one output piece per input piece");
    assert!(pieces[1].contains(MASK) && !pieces[2].contains(MASK), "the mask sits where the secret starts: {pieces:?}");
}

#[test]
fn typed_keystrokes_of_a_known_secret_are_masked() {
    let ctx = RedactContext { env_values: vec!["hunter2-secret".into()], home: None };
    let letters: Vec<String> = "my hunter2-secret ok".chars().map(String::from).collect();
    let refs: Vec<&str> = letters.iter().map(String::as_str).collect();
    let joined = mask_pieces(&refs, &ctx, Mode::Strict).concat();
    assert_eq!(joined, "my [REDACTED] ok");
}

#[test]
fn utf8_split_over_chunks_is_decoded_whole() {
    let mut carry = Utf8Carry::default();
    let bytes = "a\u{2588}b".as_bytes(); // a, 3-byte block, b
    let first = carry.push(&bytes[..2]);
    let second = carry.push(&bytes[2..]);
    assert_eq!((first.as_str(), second.as_str()), ("a", "\u{2588}b"));
    assert_eq!(carry.finish(), "");
}

#[test]
fn known_secrets_come_from_env_the_credential_store_and_registrations() {
    use crate::secrets::{backend, SecretBackend};
    let keyring_secret = "kr-PlantedKeyringValue-7788";
    backend().set("openrouter_api_key", keyring_secret).unwrap();
    known::register("registered-RuntimeToken-4455");
    std::env::set_var("PM_REDACT_TEST_SERVICE_TOKEN", "env-PlantedEnvValue-9911");
    let out = text("a kr-PlantedKeyringValue-7788 b");
    assert!(!out.as_str().contains("PlantedKeyringValue"), "{out}");
    let out = text("x registered-RuntimeToken-4455 y");
    assert!(!out.as_str().contains("RuntimeToken"), "{out}");
    let out = text("x env-PlantedEnvValue-9911 y");
    assert!(!out.as_str().contains("PlantedEnvValue"), "{out}");
    backend().delete("openrouter_api_key").unwrap();
    std::env::remove_var("PM_REDACT_TEST_SERVICE_TOKEN");
    known::clear_registered();
}

#[test]
fn fanout_redacts_json_fields_but_not_the_terminal_or_structure() {
    let key = "sk-ant-api03-FanoutSecret0123456789";
    let chat = format!("event: chat\ndata: {{\"text\":\"use {key} now\",\"id\":\"op-0123456789abcdef0123456789abcdef01234567\"}}\n\n");
    let out = fanout::sse_frame(chat);
    assert!(!out.contains("FanoutSecret"), "{out}");
    assert!(out.ends_with("\n\n") && out.starts_with("event: chat\ndata: "), "{out}");
    let body = out.strip_prefix("event: chat\ndata: ").unwrap().trim_end();
    let value: serde_json::Value = serde_json::from_str(body).expect("still valid JSON");
    assert_eq!(value["id"], "op-0123456789abcdef0123456789abcdef01234567", "ids survive");
    // The live terminal is the person's own view: untouched.
    let terminal = format!("event: terminal\ndata: {{\"pane_id\":\"p\",\"b64\":\"{key}\"}}\n\n");
    assert_eq!(fanout::sse_frame(terminal.clone()), terminal);
    // An event with nothing secret keeps its exact bytes.
    let plain = "event: pane-activity\ndata: {\"pane_id\":\"p\",\"activity\":{\"label\":\"Reading files\"}}\n\n".to_string();
    assert_eq!(fanout::sse_frame(plain.clone()), plain);
}

#[test]
fn compose_cannot_leak_through_quoted_parts() {
    let red = text("token: abcdefgh1234");
    let line = compose(&[Part::Lit("["), Part::Quoted(&red), Part::Lit("]")]);
    assert!(!line.as_str().contains("abcdefgh"), "{line}");
    assert!(line.as_str().starts_with("[\"") && line.as_str().ends_with("\"]"));
}

#[test]
fn email_addresses_become_a_placeholder_and_host_paths_stay() {
    let cases: &[(&str, &str)] = &[
        ("someone@example.com (Nimbus Starter Quota)", "<email> (Nimbus Starter Quota)"),
        ("│ Signed in as first.last+tag@mail.example.co.uk │", "│ Signed in as <email> │"),
        ("Contact: a_b@x-y.example.org.", "Contact: <email>."),
        ("clone git@github.com:org/repo.git", "clone <email>:org/repo.git"),
        ("account someone@exam…", "account <email>"),
        ("account someone@exam...", "account <email>"),
        ("josé@exemple.fr et 用户@例子.中国", "<email> et <email>"),
    ];
    for (input, want) in cases {
        assert_eq!(super::mask_emails(input), *want, "{input:?}");
    }
    // not email-shaped: a dotless host, a scope, a bare at-sign
    for same in ["ssh deploy@buildhost", "npm i @scope/pkg", "a @ b", "price 5 @ 3.50", "no at sign here"] {
        assert_eq!(super::mask_emails(same), same);
    }
}

#[test]
fn strict_mode_loses_the_user_name_in_a_wsl_home_path() {
    // `\wsl.localhost\<distro>\home\<name>` and `\wsl$\...` (either slash): the name is masked, the rest stays.
    let line = r"open \wsl.localhost\Ubuntu\home\you\proj\a.rs and //wsl$/Ubuntu-Test/home/someone/x";
    let strict = strict_with(line, &ctx());
    assert!(!strict.as_str().contains("you\\"), "{strict}");
    assert!(!strict.as_str().contains("someone"), "{strict}");
    assert!(strict.as_str().contains(r"\wsl.localhost\Ubuntu\home\USER\proj\a.rs"), "{strict}");
    // Transcripts keep paths (the default strength), as they do for `C:\Users\<name>`.
    assert_eq!(text_with(line, &ctx()).as_str(), line);
}
