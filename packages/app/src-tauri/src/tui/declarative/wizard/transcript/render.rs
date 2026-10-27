//! The `[session_chat]` text for a mapping, written the way DEFINITION-FORMAT.md 6.1 shows it.

use super::infer::Mapping;

/// The comment above the table: the person reads it in the file, and in Settings > Agents before approving.
pub const MARK: &str = "# found by the wizard: check before approving";

fn q(text: &str) -> String {
    toml::Value::String(text.to_string()).to_string()
}

fn list(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|i| q(i)).collect::<Vec<_>>().join(", "))
}

/// One pointer as a string, several as a list.
fn pointers(items: &[String]) -> String {
    match items {
        [one] => q(one),
        many => list(many),
    }
}

/// The table for `mapping` whose transcript folder is `dir` (a template). `with_tool` leaves the tool fields out when
/// they did not pass the check.
pub fn render(dir: &str, mapping: &Mapping, with_tool: bool) -> String {
    let mut out = String::new();
    out.push_str(MARK);
    out.push_str("\n[session_chat]\nformat = \"jsonl\"\n");
    out.push_str(&format!("dir = {}\nfile_glob = \"*.jsonl\"\npick = \"newest_since_start\"\n\n", q(dir)));
    out.push_str("[session_chat.map]\n");
    out.push_str(&format!("role = {}\ntext = {}\n", q(&mapping.role), pointers(&mapping.text)));
    for (key, value) in [("timestamp", &mapping.timestamp), ("model", &mapping.model)] {
        if let Some(pointer) = value {
            out.push_str(&format!("{key} = {}\n", q(pointer)));
        }
    }
    if let (true, Some(tool)) = (with_tool, &mapping.tool) {
        out.push_str(&format!("tool_name = {}\n", q(&tool.name)));
        for (key, value) in [("tool_input", &tool.input), ("tool_id", &tool.id)] {
            if let Some(pointer) = value {
                out.push_str(&format!("{key} = {}\n", q(pointer)));
            }
        }
        if let (Some(result_for), Some(output)) = (&tool.result_for, &tool.output) {
            out.push_str(&format!("tool_result_for = {}\ntool_output = {}\n", q(result_for), q(output)));
            if let Some(error) = &tool.error {
                out.push_str(&format!("tool_error = {}\n", q(error)));
            }
        }
    }
    out.push_str("\n[session_chat.values]\n");
    out.push_str(&format!("user = {}\nassistant = {}\n", list(&mapping.user_values), list(&mapping.assistant_values)));
    if with_tool && !mapping.tool_values.is_empty() {
        out.push_str(&format!("tool = {}\n", list(&mapping.tool_values)));
    }
    out
}
