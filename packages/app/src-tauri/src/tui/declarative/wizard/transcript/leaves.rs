//! Small helpers over one parsed JSON line: its string leaves with their JSON pointers, and the normal form two
//! texts are compared in.

use serde_json::Value;

/// Deepest object the walk goes into, and most array items it looks at.
const MAX_DEPTH: usize = 8;
const MAX_ITEMS: usize = 16;

/// One string in a line.
#[derive(Debug, Clone)]
pub struct Leaf {
    pub pointer: String,
    /// The last object key on the way (`content` for the items of `/message/content`, `text` for `.../0/text`).
    pub key: String,
    pub value: String,
    /// The pointer passes through an array.
    pub in_array: bool,
}

pub fn escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// Every string of `value`, depth first.
pub fn leaves(value: &Value) -> Vec<Leaf> {
    let mut out = Vec::new();
    walk(value, String::new(), String::new(), false, 0, &mut out);
    out
}

fn walk(value: &Value, pointer: String, key: String, in_array: bool, depth: usize, out: &mut Vec<Leaf>) {
    if depth > MAX_DEPTH {
        return;
    }
    match value {
        Value::String(text) => out.push(Leaf { pointer, key, value: text.clone(), in_array }),
        Value::Object(map) => {
            for (name, item) in map {
                walk(item, format!("{pointer}/{}", escape(name)), name.clone(), in_array, depth + 1, out);
            }
        }
        Value::Array(items) => {
            for (at, item) in items.iter().take(MAX_ITEMS).enumerate() {
                walk(item, format!("{pointer}/{at}"), key.clone(), true, depth + 1, out);
            }
        }
        _ => {}
    }
}

/// The pointer of the field that holds a message: cut before the first array index, so a list of parts
/// (`/message/content/0/text`) becomes the list (`/message/content`), which the reader joins.
pub fn text_pointer(pointer: &str) -> String {
    let mut kept = Vec::new();
    for part in pointer.split('/').skip(1) {
        if !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) {
            break;
        }
        kept.push(part);
    }
    if kept.is_empty() {
        String::new()
    } else {
        format!("/{}", kept.join("/"))
    }
}

/// Lower case with every run of white space as one space: the form texts are compared in.
pub fn norm(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}
