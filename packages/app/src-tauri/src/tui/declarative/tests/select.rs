use serde_json::{json, Value};

use crate::tui::declarative::schema::Selector;
use crate::tui::declarative::select::{parse_json, walk, within_depth, CompiledSelector, MAX_DEPTH, MAX_LINE_BYTES};

/// Build a `Selector` from TOML text (the same table users write).
fn sel(toml_text: &str) -> CompiledSelector {
    let parsed: Selector = toml::from_str(toml_text).unwrap_or_else(|e| panic!("bad selector {toml_text}: {e}"));
    CompiledSelector::compile(&parsed, "test").unwrap()
}

fn run(selector: &str, docs: &[Value]) -> Option<String> {
    sel(selector).select(docs.iter())
}

struct Case {
    name: &'static str,
    selector: &'static str,
    docs: Vec<Value>,
    want: Option<&'static str>,
}

fn case(name: &'static str, selector: &'static str, docs: Vec<Value>, want: Option<&'static str>) -> Case {
    Case { name, selector, docs, want }
}

#[test]
fn table_30_cases() {
    let kilo_stream = vec![
        json!({"type":"step_start","sessionID":"ses_1"}),
        json!({"type":"text","sessionID":"ses_1","part":{"text":"first"}}),
        json!({"type":"text","sessionID":"ses_1","part":{"text":"final answer"}}),
        json!({"type":"step_finish","sessionID":"ses_1","part":{"reason":"stop"}}),
    ];
    let cases = vec![
        case("where string eq, last", r#"where = { type = "text" }
path = "part.text""#, kilo_stream.clone(), Some("final answer")),
        case("pick first", r#"where = { type = "text" }
path = "part.text"
pick = "first""#, kilo_stream.clone(), Some("first")),
        case("pick join default separator", r#"where = { type = "text" }
path = "part.text"
pick = "join""#, kilo_stream.clone(), Some("first\nfinal answer")),
        case("pick join custom separator", r#"where = { type = "text" }
path = "part.text"
pick = "join"
join_with = " | ""#, kilo_stream.clone(), Some("first | final answer")),
        case("exists key", r#"where = { "~sessionID" = true }
path = "sessionID"
pick = "first""#, kilo_stream.clone(), Some("ses_1")),
        case("exists false means absent", r#"where = { "~sessionID" = false }
path = "type""#, vec![json!({"type":"a","sessionID":"x"}), json!({"type":"b"})], Some("b")),
        case("two conditions (dotted key)", r#"where = { type = "step_finish", "part.reason" = "stop" }
path = "sessionID""#, kilo_stream.clone(), Some("ses_1")),
        case("second condition fails", r#"where = { type = "step_finish", "part.reason" = "length" }
path = "sessionID""#, kilo_stream.clone(), None),
        case("whole object when path absent", r#"where = { type = "error" }"#, vec![json!({"type":"error","code":7})], Some(r#"{"code":7,"type":"error"}"#)),
        case("integer where", r#"where = { code = 7 }
path = "msg""#, vec![json!({"code":7,"msg":"seven"}), json!({"code":8,"msg":"eight"})], Some("seven")),
        case("integer where matches integral float", r#"where = { code = 7 }
path = "msg""#, vec![json!({"code":7.0,"msg":"seven"})], Some("seven")),
        case("bool where", r#"where = { ok = true }
path = "msg""#, vec![json!({"ok":false,"msg":"no"}), json!({"ok":true,"msg":"yes"})], Some("yes")),
        case("type mismatch never matches", r#"where = { code = "7" }
path = "msg""#, vec![json!({"code":7,"msg":"seven"})], None),
        case("where_regex", r#"where_regex = { type = "^step_" }
path = "type"
pick = "first""#, kilo_stream.clone(), Some("step_start")),
        case("where_regex on number stringifies", r#"where_regex = { n = "^4" }
path = "msg""#, vec![json!({"n":42,"msg":"hit"})], Some("hit")),
        case("path with array index", r#"where = { type = "m" }
path = "items.1.name""#, vec![json!({"type":"m","items":[{"name":"a"},{"name":"b"}]})], Some("b")),
        case("array index out of range", r#"where = { type = "m" }
path = "items.5.name""#, vec![json!({"type":"m","items":[{"name":"a"}]})], None),
        case("missing path", r#"where = { type = "m" }
path = "no.such.path""#, vec![json!({"type":"m"})], None),
        case("null value is no value", r#"path = "v""#, vec![json!({"v":null})], None),
        case("number path stringified", r#"path = "n""#, vec![json!({"n":12})], Some("12")),
        case("max_by numeric", r#"where = { type = "t" }
pick = "max_by"
max_by = "score"
path = "name""#, vec![json!({"type":"t","score":1,"name":"low"}), json!({"type":"t","score":9,"name":"high"}), json!({"type":"t","score":3,"name":"mid"})], Some("high")),
        case("max_by skips non numeric", r#"pick = "max_by"
max_by = "score"
path = "name""#, vec![json!({"score":"x","name":"bad"}), json!({"score":2,"name":"ok"})], Some("ok")),
        case("max_by without key", r#"pick = "max_by"
path = "name""#, vec![json!({"name":"a"})], None),
        case("regex group value", r#"path = "line"
regex = 'model=(?P<value>\S+)'"#, vec![json!({"line":"x model=gpt-5 y"})], Some("gpt-5")),
        case("regex group 1 fallback", r#"path = "line"
regex = 'id: (\d+)'"#, vec![json!({"line":"id: 123"})], Some("123")),
        case("regex without match is None", r#"path = "line"
regex = 'zzz(\d+)'"#, vec![json!({"line":"id: 123"})], None),
        case("trim default", r#"path = "t""#, vec![json!({"t":"  padded \n"})], Some("padded")),
        case("trim off", r#"path = "t"
trim = false"#, vec![json!({"t":"  padded "})], Some("  padded ")),
        case("empty after trim is None", r#"path = "t""#, vec![json!({"t":"   "})], None),
        case("non-object documents never match where", r#"where = { type = "x" }
path = "type""#, vec![json!("str"), json!(5), json!([1, 2]), json!(null)], None),
        case("no documents", r#"path = "x""#, vec![], None),
    ];
    assert!(cases.len() >= 30, "{} cases", cases.len());
    for c in cases {
        assert_eq!(run(c.selector, &c.docs).as_deref(), c.want, "case: {}", c.name);
    }
}

#[test]
fn deeply_nested_json_is_rejected_not_overflowed() {
    for depth in [MAX_DEPTH, MAX_DEPTH + 1, 120, 5_000, 200_000] {
        let text = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
        let parsed = parse_json(&text);
        if depth <= MAX_DEPTH {
            assert!(parsed.is_some(), "depth {depth} within cap");
        } else {
            assert!(parsed.is_none(), "depth {depth} over cap");
        }
    }
    let nested = (0..MAX_DEPTH + 10).fold(json!({"type": "x"}), |acc, _| json!({ "a": acc }));
    assert!(!within_depth(&nested, MAX_DEPTH));
    assert_eq!(run(r#"path = "a""#, &[nested]), None, "over-deep value never matches");
}

#[test]
fn one_mib_line_is_handled() {
    let pad = "x".repeat(MAX_LINE_BYTES - 40);
    let text = format!(r#"{{"type":"text","part":{{"text":"{pad}"}}}}"#);
    assert!(text.len() <= MAX_LINE_BYTES);
    let value = parse_json(&text).expect("a line at the cap parses");
    let got = run(r#"where = { type = "text" }
path = "part.text""#, &[value]);
    assert_eq!(got.map(|s| s.len()), Some(pad.len()));
}

#[test]
fn garbage_text_parses_to_none() {
    for text in ["", "not json", "{", "{\"a\":", "}{", "\u{0}", "nul", "[1,2", "{'a':1}"] {
        assert!(parse_json(text).is_none(), "{text:?}");
    }
}

#[test]
fn walk_handles_dots_indices_and_scalars() {
    let v = json!({"a":{"b":[10,{"c":"deep"}]},"n":1});
    assert_eq!(walk(&v, "a.b.1.c"), Some(&json!("deep")));
    assert_eq!(walk(&v, "a.b.0"), Some(&json!(10)));
    assert_eq!(walk(&v, ""), Some(&v));
    assert_eq!(walk(&v, "n.x"), None);
    assert_eq!(walk(&v, "a.b.x"), None);
    assert_eq!(walk(&v, "a..b"), None);
}

#[test]
fn bad_regex_surfaces_a_def_error_with_path() {
    let parsed: Selector = toml::from_str("regex = \"(oops\"").unwrap();
    let err = CompiledSelector::compile(&parsed, "headless.output.result").unwrap_err();
    assert_eq!((err.code, err.path.as_str()), ("AGENT_DEF_BAD_REGEX", "headless.output.result.regex"));
    let parsed: Selector = toml::from_str("where_regex = { a = \"(?:[a-z]{100}){100}\" }").unwrap();
    let err = CompiledSelector::compile(&parsed, "s").unwrap_err();
    assert_eq!(err.code, "AGENT_DEF_REGEX_TOO_LARGE");
}

#[test]
fn line_selection_for_plain_text() {
    let s = sel(r#"regex = '^result: (.*)$'
pick = "last""#);
    let text = "noise\nresult: one\nmore noise\nresult:  two \n\n";
    assert_eq!(s.select_lines(text.lines()).as_deref(), Some("two"));
    let first = sel("pick = \"first\"");
    assert_eq!(first.select_lines("\n\nalpha\nbeta".lines()).as_deref(), Some("alpha"));
    assert_eq!(s.select_lines("nothing here".lines()), None);
}
