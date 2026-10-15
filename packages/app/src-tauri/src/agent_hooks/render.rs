//! Render PM's hook entries for a vendor config from the agent's `[hooks]` table (§2.4). Pure:
//! the same table, exe path and agent id always give the same bytes (Codex trusts a hook by a hash
//! of its definition, so a changed byte means every user re-trusts; see the golden test).

use serde_json::{json, Map, Value};

use crate::tui::declarative::hooks::{HooksLayout, HooksTable};

/// The emitter's arguments. Not configurable: always `hook <agent id>` (§2.3).
pub fn emitter_args(agent: &str) -> [String; 2] {
    ["hook".to_string(), agent.to_string()]
}

/// The exe for a shell-string command. Quoted only when it holds whitespace, so the common case is
/// a bare path every shell runs as is.
pub fn exe_for_shell(exe: &str) -> String {
    if exe.chars().any(char::is_whitespace) {
        format!("\"{exe}\"")
    } else {
        exe.to_string()
    }
}

fn fill(template: &str, exe: &str, agent: &str) -> Value {
    if template == "{args}" {
        return json!(emitter_args(agent));
    }
    Value::String(
        template
            .replace("{exe_quoted}", &exe_for_shell(exe))
            .replace("{args_joined}", &emitter_args(agent).join(" "))
            .replace("{exe}", exe),
    )
}

fn toml_to_json(value: &toml::Value, exe: &str, agent: &str) -> Value {
    match value {
        toml::Value::String(s) => fill(s, exe, agent),
        toml::Value::Integer(i) => json!(i),
        toml::Value::Float(f) => json!(f),
        toml::Value::Boolean(b) => json!(b),
        toml::Value::Datetime(d) => json!(d.to_string()),
        toml::Value::Array(items) => Value::Array(items.iter().map(|v| toml_to_json(v, exe, agent)).collect()),
        toml::Value::Table(t) => Value::Object(t.iter().map(|(k, v)| (k.clone(), toml_to_json(v, exe, agent))).collect()),
    }
}

/// One rendered hook entry (the same for every event).
pub fn entry(scheme: &HooksTable, exe: &str, agent: &str) -> Value {
    toml_to_json(&toml::Value::Table(scheme.entry.clone()), exe, agent)
}

/// PM's matcher groups per vendor event, in the table's order: `{Event: [{"matcher"?, "hooks"}]}`.
pub fn groups(scheme: &HooksTable, exe: &str, agent: &str) -> Map<String, Value> {
    let entry = entry(scheme, exe, agent);
    let mut out: Map<String, Value> = Map::new();
    for row in &scheme.events {
        let mut group = Map::new();
        if !row.matcher.is_empty() {
            group.insert("matcher".into(), json!(row.matcher));
        }
        group.insert("hooks".into(), json!([entry.clone()]));
        match out.get_mut(&row.vendor) {
            Some(Value::Array(list)) => list.push(Value::Object(group)),
            _ => {
                out.insert(row.vendor.clone(), json!([Value::Object(group)]));
            }
        }
    }
    out
}

/// The whole config document holding only PM's hooks (`layout = "event_groups"`).
pub fn document(scheme: &HooksTable, root_key: &str, layout: HooksLayout, exe: &str, agent: &str) -> Value {
    match layout {
        HooksLayout::EventGroups => json!({ root_key: Value::Object(groups(scheme, exe, agent)) }),
    }
}

/// True for an entry PM wrote for `agent`: it runs `hook <agent>` through a puppet-master-mcp exe.
/// This is how PM finds its own entries again; every other entry is the user's.
pub fn is_pm_entry(entry: &Value, agent: &str) -> bool {
    let command = entry.get("command").and_then(Value::as_str).unwrap_or_default();
    let names_emitter = command.to_ascii_lowercase().contains("puppet-master-mcp");
    let args_match = entry.get("args").and_then(Value::as_array).is_some_and(|args| {
        args.iter().map(|a| a.as_str().unwrap_or_default()).eq(emitter_args(agent).iter().map(String::as_str))
    });
    let joined = format!(" {}", emitter_args(agent).join(" "));
    names_emitter && (args_match || command.trim_end().ends_with(&joined))
}

/// A matcher group made only of PM entries.
pub fn is_pm_group(group: &Value, agent: &str) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| !hooks.is_empty() && hooks.iter().all(|h| is_pm_entry(h, agent)))
}
