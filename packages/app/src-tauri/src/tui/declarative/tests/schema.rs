use super::{KILO, MINIMAL};
use crate::tui::declarative::schema::{Kind, OutputFormat, Pick, Scalar};
use crate::tui::declarative::{parse_definition, DefError};

fn err(text: &str) -> DefError {
    match parse_definition(text) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e,
    }
}

#[test]
fn kilo_example_parses() {
    let def = parse_definition(KILO).expect("kilo parses");
    assert_eq!(def.agent.id, "kilo");
    assert_eq!(def.agent.kind, Kind::Cli);
    assert_eq!(def.binary.names, ["kilo", "kilocode"]);
    let headless = def.headless.as_ref().unwrap();
    assert_eq!(headless.task_separator.as_deref(), Some("--"));
    assert_eq!(headless.optional.len(), 3);
    let output = headless.output.as_ref().unwrap();
    assert_eq!(output.format, OutputFormat::JsonLines);
    let result = output.result.as_ref().unwrap();
    assert_eq!(result.where_eq.get("type"), Some(&Scalar::Str("text".into())));
    assert_eq!(result.pick, Pick::Last);
    let session = output.session.as_ref().unwrap();
    assert_eq!(session.where_eq.get("~sessionID"), Some(&Scalar::Bool(true)));
    assert_eq!(session.pick, Pick::First);
    let tui = def.tui.as_ref().unwrap();
    assert_eq!(tui.clear_input_keys.as_deref(), Some("\u{15}"));
    assert_eq!(tui.prompt[0].choices[1].key.as_deref(), Some("escape"));
    assert_eq!(def.context.smartness, 6);
}

#[test]
fn minimal_example_parses() {
    let def = parse_definition(MINIMAL).expect("minimal parses");
    assert!(def.headless.is_none());
    assert_eq!(def.agent.icon_or_default(), "MY");
    assert_eq!(def.binary.windows_extensions, ["exe", "cmd", "bat"]);
    assert_eq!(def.binary.version_args, ["--version"]);
    assert!(def.features.dispatchable);
    assert_eq!(def.context.strengths, ["implementation"]);
}

#[test]
fn unknown_field_rejected_with_path() {
    let top = err(&format!("bogus = 1\n{MINIMAL}"));
    assert_eq!((top.code, top.path.as_str()), ("AGENT_DEF_UNKNOWN_FIELD", "bogus"));

    let nested = err(&MINIMAL.replace("label = \"My Coder\"", "label = \"My Coder\"\nlabl = \"typo\""));
    assert_eq!((nested.code, nested.path.as_str()), ("AGENT_DEF_UNKNOWN_FIELD", "agent.labl"));
    assert!(nested.line.is_some());

    let deep = err(&KILO.replace("pick = \"last\"", "pick = \"last\"\nnope = true"));
    assert_eq!(deep.code, "AGENT_DEF_UNKNOWN_FIELD");
    assert_eq!(deep.path, "headless.output.result.nope");
}

#[test]
fn wrong_type_rejected_with_path() {
    let e = err(&MINIMAL.replace("experimental = true", "experimental = \"yes\""));
    assert_eq!((e.code, e.path.as_str()), ("AGENT_DEF_INVALID_TOML", "agent.experimental"));

    let e = err(&MINIMAL.replace("names = [\"mycoder\"]", "names = \"mycoder\""));
    assert_eq!((e.code, e.path.as_str()), ("AGENT_DEF_INVALID_TOML", "binary.names"));

    let e = err(&KILO.replace("format = \"json_lines\"", "format = \"xml\""));
    assert_eq!((e.code, e.path.as_str()), ("AGENT_DEF_INVALID_TOML", "headless.output.format"));
}

#[test]
fn bad_schema_version_rejected() {
    for text in [
        MINIMAL.replace("schema_version = 1", "schema_version = 2"),
        MINIMAL.replace("schema_version = 1", "schema_version = \"1\""),
        MINIMAL.replace("schema_version = 1", ""),
    ] {
        let e = err(&text);
        assert_eq!((e.code, e.path.as_str()), ("AGENT_DEF_SCHEMA_UNSUPPORTED", "schema_version"));
        assert!(e.message.contains("supported versions: 1"), "{}", e.message);
    }
}

#[test]
fn invalid_toml_reports_line_and_column() {
    let e = err("schema_version = 1\n[agent\nid = 1");
    assert_eq!(e.code, "AGENT_DEF_INVALID_TOML");
    assert_eq!(e.line, Some(2));
    assert!(e.col.is_some());
}

#[test]
fn unknown_field_inside_array_table_uses_dotted_index() {
    let text = MINIMAL.replace("kind = \"command_approval\"", "kind = \"command_approval\"\nwat = 1");
    let e = err(&text);
    assert_eq!((e.code, e.path.as_str()), ("AGENT_DEF_UNKNOWN_FIELD", "tui.prompt.0.wat"));
}
