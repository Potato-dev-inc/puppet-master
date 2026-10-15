use super::KILO;
use crate::tui::declarative::output::OutputParser;
use crate::tui::declarative::parse_definition;
use crate::tui::declarative::schema::HeadlessTable;

fn kilo_headless() -> HeadlessTable {
    parse_definition(KILO).unwrap().headless.unwrap()
}

fn kilo() -> OutputParser {
    OutputParser::compile(&kilo_headless()).unwrap()
}

const STREAM: &str = concat!(
    "{\"type\":\"step_start\",\"sessionID\":\"ses_abc\"}\n",
    "{\"type\":\"text\",\"sessionID\":\"ses_abc\",\"part\":{\"text\":\"draft\"}}\n",
    "{\"type\":\"text\",\"sessionID\":\"ses_abc\",\"part\":{\"text\":\"the answer\"}}\n",
    "{\"type\":\"step_finish\",\"sessionID\":\"ses_abc\",\"part\":{\"reason\":\"stop\"}}\n",
);

fn with_output(f: impl FnOnce(&mut HeadlessTable)) -> OutputParser {
    let mut h = kilo_headless();
    f(&mut h);
    OutputParser::compile(&h).unwrap()
}

fn selector(toml_text: &str) -> crate::tui::declarative::schema::Selector {
    toml::from_str(toml_text).unwrap()
}

#[test]
fn kilo_stream_extracts_everything() {
    let p = kilo();
    assert_eq!(p.extract_result(STREAM, 0).as_deref(), Some("the answer"));
    assert_eq!(p.extract_provider_session(STREAM).as_deref(), Some("ses_abc"));
    assert!(p.result_ready(STREAM));
    assert!(!p.result_ready(&STREAM[..STREAM.len() - 90]), "no done marker yet");
    assert_eq!(p.error_message(STREAM), None);
    assert_eq!(p.extract_model(STREAM), None);
}

#[test]
fn nonzero_exit_returns_none() {
    let p = kilo();
    for code in [1, 2, -1, 137] {
        assert_eq!(p.extract_result(STREAM, code), None, "exit {code}");
    }
    assert_eq!(p.extract_result("{}", 1), None);
}

#[test]
fn garbage_and_unmatched_output_is_none() {
    let p = kilo();
    assert_eq!(p.extract_result("not json at all", 0), None);
    assert_eq!(p.extract_result("", 0), None);
    assert_eq!(p.extract_result("{\"type\":\"other\"}\n", 0), None);
    assert_eq!(p.extract_provider_session("not json"), None);
    assert!(!p.result_ready("garbage"));
}

#[test]
fn error_selector_reports_message_and_marks_ready() {
    let p = kilo();
    let out = "{\"type\":\"error\",\"error\":{\"data\":{\"message\":\"rate limited\"}}}\n";
    assert_eq!(p.error_message(out).as_deref(), Some("rate limited"));
    assert!(p.result_ready(out), "an error line also ends the turn (HDL-5)");
    // exit code 0 with an error line: the result is still absent
    assert_eq!(p.extract_result(out, 0), None);
}

#[test]
fn ansi_stripped_and_garbage_counted() {
    let p = kilo();
    let noisy = format!(
        "\u{1b}[2K\u{1b}[1Gbanner line\n{}\u{1b}[0mnot json either\n{}",
        "{\"type\":\"text\",\"part\":{\"text\":\"\u{1b}[32mgreen\u{1b}[0m\"}}\n",
        "{\"type\":\"step_finish\",\"part\":{\"reason\":\"stop\"}}\n"
    );
    // ANSI inside the JSON string is stripped too (the escape byte is not valid raw in JSON, so
    // the stripping has to happen before parsing; here it is the literal ESC inside the text)
    assert_eq!(p.unparseable_lines(&noisy), 2, "banner line and the trailing non-json line");
    assert!(p.result_ready(&noisy));
    let counted = (0..7).map(|i| format!("garbage {i}\n")).collect::<String>() + STREAM;
    assert_eq!(p.unparseable_lines(&counted), 7);
    assert_eq!(p.extract_result(&counted, 0).as_deref(), Some("the answer"));
    // strip_ansi = false leaves escapes: the ESC byte makes that JSON line unparseable
    let raw = with_output(|h| h.output.as_mut().unwrap().strip_ansi = false);
    assert_eq!(raw.unparseable_lines(&noisy), 3);
}

#[test]
fn max_output_bytes_enforced() {
    let p = with_output(|h| h.max_output_bytes = 65_536);
    let mut big = String::from("{\"type\":\"text\",\"part\":{\"text\":\"early\"}}\n");
    big.push_str(&"{\"type\":\"noise\"}\n".repeat(10_000));
    big.push_str("{\"type\":\"text\",\"part\":{\"text\":\"late\"}}\n");
    assert!(big.len() > 65_536);
    assert_eq!(p.extract_result(&big, 0).as_deref(), Some("early"), "text past the cap is never read");
    // everything within the cap is read
    assert_eq!(kilo().extract_result(&big, 0).as_deref(), Some("late"));
    // truncation never splits a multi-byte character
    let wide = "\u{e9}".repeat(70_000);
    assert_eq!(p.extract_result(&wide, 0), None);
}

#[test]
fn json_single_takes_the_object_or_the_last_json_line() {
    let h = |h: &mut HeadlessTable| {
        let out = h.output.as_mut().unwrap();
        out.format = crate::tui::declarative::schema::OutputFormat::JsonSingle;
        out.result = Some(selector("path = \"result\""));
        out.session = Some(selector("path = \"session_id\""));
        out.model = Some(selector("path = \"model\""));
        out.done = None;
        out.error = Some(selector("where = { is_error = true }\npath = \"result\""));
    };
    let p = with_output(h);
    let compact = "{\"result\":\"hi\",\"session_id\":\"s1\",\"model\":\"m1\"}";
    assert_eq!(p.extract_result(compact, 0).as_deref(), Some("hi"));
    assert_eq!(p.extract_provider_session(compact).as_deref(), Some("s1"));
    assert_eq!(p.extract_model(compact).as_deref(), Some("m1"));
    // earlier lines are not JSON
    let noisy = format!("warming up...\nplugin loaded\n{compact}\n");
    assert_eq!(p.extract_result(&noisy, 0).as_deref(), Some("hi"));
    assert_eq!(p.unparseable_lines(&noisy), 2);
    // pretty printed after noise
    let pretty = "banner\n{\n  \"result\": \"multi\",\n  \"session_id\": \"s2\"\n}\n";
    assert_eq!(p.extract_result(pretty, 0).as_deref(), Some("multi"));
    assert_eq!(p.extract_provider_session(pretty).as_deref(), Some("s2"));
    // is_error object reports its message and counts as ready
    let failed = "{\"is_error\":true,\"result\":\"boom\"}";
    assert_eq!(p.error_message(failed).as_deref(), Some("boom"));
    assert!(p.result_ready(failed));
    assert_eq!(p.extract_result("totally not json", 0), None);
}

#[test]
fn plain_text_whole_text_or_regex_lines() {
    let whole = with_output(|h| {
        let out = h.output.as_mut().unwrap();
        out.format = crate::tui::declarative::schema::OutputFormat::PlainText;
        out.result = None;
        out.session = None;
        out.done = None;
        out.error = None;
    });
    assert_eq!(whole.extract_result("\u{1b}[1mhello\u{1b}[0m world\n\n", 0).as_deref(), Some("hello world"));
    assert_eq!(whole.extract_result("hello", 3), None);
    assert_eq!(whole.extract_result("  \n", 0), None);
    assert_eq!(whole.unparseable_lines("a\nb\nc"), 0);

    let lines = with_output(|h| {
        let out = h.output.as_mut().unwrap();
        out.format = crate::tui::declarative::schema::OutputFormat::PlainText;
        out.result = Some(selector("regex = '^answer: (.*)$'"));
        out.session = Some(selector("regex = 'session (?P<value>[a-z0-9]+)'\npick = \"first\""));
        out.done = Some(selector("regex = '^done$'"));
        out.error = None;
    });
    let text = "session abc123 started\nthinking\nanswer: 42\ndone\n";
    assert_eq!(lines.extract_result(text, 0).as_deref(), Some("42"));
    assert_eq!(lines.extract_provider_session(text).as_deref(), Some("abc123"));
    assert!(lines.result_ready(text));
    assert!(!lines.result_ready("session abc123 started\n"));
}

#[test]
fn rejected_call_warning_returns_the_configured_code() {
    let p = with_output(|h| {
        h.output.as_mut().unwrap().rejected =
            Some(selector("where = { type = \"tool_rejected\" }\nwarning_code = \"TOOL_CALL_REJECTED\""));
    });
    let out = format!("{STREAM}{{\"type\":\"tool_rejected\"}}\n");
    assert_eq!(p.rejected_call_warning(&out), Some("TOOL_CALL_REJECTED"));
    assert_eq!(p.rejected_call_warning(STREAM), None);
    assert_eq!(kilo().rejected_call_warning(&out), None, "no rejected selector configured");
}

#[test]
fn compile_needs_an_output_table_and_good_regexes() {
    let mut h = kilo_headless();
    h.output = None;
    assert_eq!(OutputParser::compile(&h).unwrap_err().code, "AGENT_DEF_INCONSISTENT");
    let mut h = kilo_headless();
    h.output.as_mut().unwrap().result = Some(selector("regex = \"(bad\""));
    let err = OutputParser::compile(&h).unwrap_err();
    assert_eq!((err.code, err.path.as_str()), ("AGENT_DEF_BAD_REGEX", "headless.output.result.regex"));
}
