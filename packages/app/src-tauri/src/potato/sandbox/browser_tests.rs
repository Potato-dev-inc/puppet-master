//! The page browser's checks: web addresses (the places a model must never send the sandbox), action lists
//! rebuilt from the few fields the tool takes, the tool's arguments and the reading of its answer.

use serde_json::{json, Value};

use super::browser::*;
use super::{FakeSandbox, Recorded};

fn refused(url: &str) -> bool {
    check_url(url, false).is_err()
}

#[test]
fn public_web_addresses_pass_and_come_back_unchanged() {
    for url in [
        "https://example.com",
        "http://example.com/path?q=1&r=2#frag",
        "https://sub.example.co.uk:8443/a",
        "HTTPS://Example.COM/",
        "https://example.com.",
        "https://8.8.8.8/",
        "http://93.184.216.34:80/",
        "https://[2606:4700:4700::1111]/",
        "  https://example.com/a  ",
    ] {
        assert_eq!(check_url(url, false).unwrap(), url.trim(), "{url}");
    }
}

#[test]
fn only_http_and_https_are_web_addresses() {
    for url in [
        "file:///etc/passwd",
        "ftp://example.com",
        "javascript:alert(1)",
        "data:text/html,<b>x</b>",
        "chrome://settings",
        "about:blank",
        "view-source:https://example.com",
        "example.com",
        "//example.com",
        "https:example.com",
        "",
        "   ",
        "ws://example.com",
        "mailto:a@b.co",
    ] {
        assert!(refused(url), "{url}");
    }
    assert_eq!(check_url("file:///x", false).unwrap_err().code(), "URL_NOT_ALLOWED");
}

#[test]
fn this_computer_and_private_networks_are_refused_in_every_spelling() {
    for url in [
        "http://localhost/",
        "http://LOCALHOST:3000/",
        "http://app.localhost/",
        "http://127.0.0.1/",
        "http://127.1/",
        "http://127.0.0.1:47801/v1/desktop",
        "http://2130706433/",
        "http://0x7f000001/",
        "http://0x7f.0.0.1/",
        "http://0177.0.0.1/",
        "http://017700000001/",
        "http://0/",
        "http://0.0.0.0/",
        "http://10.0.0.1/",
        "http://172.16.5.4/",
        "http://172.31.255.255/",
        "http://192.168.1.1/",
        "http://169.254.169.254/latest/meta-data/",
        "http://100.64.0.1/",
        "http://198.18.0.1/",
        "http://224.0.0.1/",
        "http://255.255.255.255/",
        "http://240.0.0.1/",
        "http://192.0.0.8/",
        "http://192.0.2.1/",
        "http://[::1]/",
        "http://[::]/",
        "http://[::ffff:127.0.0.1]/",
        "http://[::ffff:7f00:1]/",
        "http://[::ffff:10.0.0.1]/",
        "http://[fe80::1]/",
        "http://[fc00::1]/",
        "http://[fd12:3456::1]/",
        "http://[2001:db8::1]/",
        "http://[64:ff9b::7f00:1]/",
        "http://[2002:7f00:1::]/",
        "http://printer/",
        "http://router.local/",
        "http://nas.lan/",
        "http://wiki.corp/",
        "http://metadata.google.internal/",
        "http://host.docker.internal/",
        "http://127.0.0.1.nip.io/",
        "http://anything.sslip.io/",
        "http://x.localtest.me/",
        "http://intranet/",
        "http://thing.home.arpa/",
        "http://site.test/",
    ] {
        let error = check_url(url, false).unwrap_err();
        assert_eq!(error.code(), "URL_NOT_ALLOWED", "{url}");
    }
}

#[test]
fn tricks_in_the_address_are_refused() {
    for url in [
        "https://user:pass@example.com/",
        "https://example.com@127.0.0.1/",
        "https://example.com\\@127.0.0.1/",
        "https://example.com\\.evil.com/",
        "https://example.com/\u{7}",
        "https://exam ple.com/",
        "https://example.com/a b",
        "https://example.com/\u{0}",
        "https://127。0。0。1/",
        "https://ex\u{e4}mple.com/",
        "https://example.com:0/",
        "https://example.com:99999/",
        "https://example.com:80a/",
        "https://example.com:/",
        "https://%31%32%37.0.0.1/",
        "https://[::1]x/",
        "https://[::1/",
        "https://1.2.3.4.5/",
        "https://256.1.1.1/",
        "https://1.2.3.4abc/",
        "https://.example.com/",
        "https://exa..mple.com/",
        "https:///example.com",
        "https://",
    ] {
        assert!(refused(url), "{url}");
    }
    let long = format!("https://example.com/{}", "a".repeat(MAX_URL_CHARS));
    assert!(refused(&long));
}

#[test]
fn a_page_inside_the_sandbox_is_opened_only_when_the_caller_says_so() {
    assert!(refused("http://127.0.0.1:8000/test.html"));
    assert_eq!(check_url("http://127.0.0.1:8000/test.html", true).unwrap(), "http://127.0.0.1:8000/test.html");
    assert!(check_url("http://localhost:8000/", true).is_ok());
    // ... but it is still a web address: no other scheme, no name and password, no junk
    assert!(check_url("file:///etc/passwd", true).is_err());
    assert!(check_url("http://user:pw@127.0.0.1/", true).is_err());
    assert!(check_url("http://127.0.0.1:99999/", true).is_err());
    assert!(check_url("http://exa mple/", true).is_err());
}

#[test]
fn a_launch_argument_is_a_web_address_or_a_plain_path() {
    assert!(check_launch_arg("https://example.com", false).is_ok());
    assert!(check_launch_arg("http://127.0.0.1/", false).is_err());
    assert!(check_launch_arg("file:///etc/passwd", false).is_err());
    assert!(check_launch_arg("/home/pm/notes.txt", false).is_ok());
    assert!(check_launch_arg("notes.txt", false).is_ok());
    assert!(check_launch_arg("-x", false).is_err());
    assert!(check_launch_arg("", false).is_err());
    assert!(check_launch_arg("a\u{0}b", false).is_err());
}

fn act(list: Value) -> Result<Vec<Value>, BrowserError> {
    clean_actions(&list)
}

#[test]
fn an_action_list_is_rebuilt_from_the_fields_the_tool_takes() {
    let clean = act(json!([
        { "kind": "click", "ref": "e1", "text": "ignored", "origin": "https://x.example", "roots": ["/"], "field": "password" },
        { "kind": "fill", "ref": "e2", "text": "hello", "secret": "mine", "purchase": "p1" },
        { "kind": "type", "ref": "e3", "text": "123456" },
        { "kind": "upload", "ref": "e4", "paths": ["/home/pm/a.png"], "roots": ["/etc"] },
    ]))
    .unwrap();
    assert_eq!(clean[0], json!({ "kind": "click", "ref": "e1" }));
    assert_eq!(clean[1], json!({ "kind": "fill", "ref": "e2", "text": "hello" }));
    assert_eq!(clean[2], json!({ "kind": "type", "ref": "e3", "text": "123456" }));
    assert_eq!(clean[3], json!({ "kind": "upload", "ref": "e4", "paths": ["/home/pm/a.png"] }));
}

#[test]
fn the_kinds_that_are_gone_are_refused() {
    for kind in ["fill_secret", "fill_card", "fill_pay_token", "select", "press", "scroll", "", "Click"] {
        let error = act(json!([{ "kind": kind, "ref": "e1", "text": "x" }])).unwrap_err();
        assert_eq!(error.code(), "BAD_ACTIONS", "{kind}");
    }
}

#[test]
fn an_action_list_has_one_to_twenty_four_actions_each_with_a_ref() {
    let one = json!({ "kind": "click", "ref": "e1" });
    assert!(act(json!([])).is_err());
    assert!(act(json!({})).is_err());
    assert!(act(json!("click")).is_err());
    assert!(act(json!([one.clone()])).is_ok());
    assert!(act(Value::Array(vec![one.clone(); 24])).is_ok());
    assert!(act(Value::Array(vec![one.clone(); 25])).is_err());
    assert!(act(json!([1])).is_err());
    assert!(act(json!([{ "kind": "click" }])).is_err());
    assert!(act(json!([{ "kind": "click", "ref": "" }])).is_err());
    assert!(act(json!([{ "kind": "click", "ref": "e 1" }])).is_err());
    assert!(act(json!([{ "kind": "click", "ref": "e".repeat(MAX_REF_CHARS + 1) }])).is_err());
    assert!(act(json!([{ "kind": "click", "ref": 5 }])).is_err());
}

#[test]
fn text_and_paths_have_limits() {
    assert!(act(json!([{ "kind": "fill", "ref": "e1" }])).is_err());
    assert!(act(json!([{ "kind": "fill", "ref": "e1", "text": 5 }])).is_err());
    assert!(act(json!([{ "kind": "fill", "ref": "e1", "text": "a".repeat(MAX_TEXT_CHARS) }])).is_ok());
    assert!(act(json!([{ "kind": "fill", "ref": "e1", "text": "a".repeat(MAX_TEXT_CHARS + 1) }])).is_err());
    assert!(act(json!([{ "kind": "type", "ref": "e1", "text": "a\u{0}b" }])).is_err());
    for paths in [json!([]), json!("/a"), json!(["a/b"]), json!(["/a/../etc"]), json!(["/a\u{0}"]), json!([5]), Value::Array(vec![json!("/a"); 9]), json!(["/".repeat(MAX_PATH_CHARS + 1)])] {
        assert!(act(json!([{ "kind": "upload", "ref": "e1", "paths": paths }])).is_err(), "{paths}");
    }
    assert!(act(json!([{ "kind": "upload", "ref": "e1" }])).is_err());
}

#[test]
fn the_arguments_are_words_for_the_tool_and_stay_small() {
    let args = navigate_args("https://example.com/a?b=\"c\"").unwrap();
    assert_eq!(args[0], "navigate");
    assert_eq!(serde_json::from_str::<Value>(&args[1]).unwrap(), json!({ "url": "https://example.com/a?b=\"c\"" }));
    assert_eq!(snapshot_args(), vec!["snapshot".to_string()]);
    let actions = act(json!([{ "kind": "fill", "ref": "e1", "text": "say \"hi\"\nnow" }])).unwrap();
    let args = act_args(&actions).unwrap();
    assert_eq!(args[0], "act");
    assert_eq!(serde_json::from_str::<Value>(&args[1]).unwrap()["actions"][0]["text"], "say \"hi\"\nnow");
    // 24 long texts are more than the 24 KB a call may carry
    let big = act(Value::Array((0..24).map(|i| json!({ "kind": "fill", "ref": format!("e{i}"), "text": "x".repeat(MAX_TEXT_CHARS) })).collect())).unwrap();
    assert_eq!(act_args(&big).unwrap_err().code(), "BAD_ACTIONS");
}

#[test]
fn the_answer_is_the_last_json_line_and_a_bad_answer_points_at_the_desktop() {
    assert_eq!(parse_reply("{\"ok\":true,\"url\":\"https://example.com\"}\n")["url"], "https://example.com");
    assert_eq!(parse_reply("warning: something\n{\"ok\":true}\n\n")["ok"], true);
    for raw in ["", "not json", "[1,2]", "{broken", "\"text\""] {
        let value = parse_reply(raw);
        assert_eq!(value["ok"], false, "{raw}");
        assert_eq!(value["fallback"], "computer_act");
    }
    let big = format!("{{\"ok\":true,\"text\":\"{}\"}}", "x".repeat(MAX_REPLY_CHARS + 10));
    let cut = parse_reply(&big);
    assert_eq!(cut["truncated"], true);
    assert_eq!(cut["ok"], true);
    assert_eq!(cut["text"].as_str().unwrap().chars().count(), MAX_REPLY_CHARS);
}

#[test]
fn run_hands_the_words_to_the_sandbox_and_reads_what_it_printed() {
    let fake = FakeSandbox::new();
    fake.set_browser_reply("{\"ok\":true,\"title\":\"Shop\"}");
    let args = navigate_args("https://example.com").unwrap();
    let reply = run(&fake, &args).unwrap();
    assert_eq!(reply["title"], "Shop");
    assert_eq!(fake.calls().last().unwrap(), &Recorded::Browser(args));
    assert!(fake.is_running(), "a page call starts the sandbox like any use of the screen");
}
