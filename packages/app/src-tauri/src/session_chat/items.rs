//! Turns the lines read from a session file into chat items, using one CLI's format.

use super::model::{ChatItem, SessionFormat};

/// Items for each line, in order. Lines the format does not know give nothing.
pub fn read_lines(format: &dyn SessionFormat, lines: &[(u64, String)]) -> Vec<ChatItem> {
    lines.iter().flat_map(|(offset, line)| format.parse_line(line, *offset)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads lines like `u:hello` as a user message and ignores everything else.
    struct FakeFormat;

    impl SessionFormat for FakeFormat {
        fn parse_line(&self, line: &str, offset: u64) -> Vec<ChatItem> {
            match line.strip_prefix("u:") {
                Some(text) => vec![ChatItem::User {
                    id: format!("{offset}:0"),
                    at: None,
                    text: text.to_string(),
                }],
                None => Vec::new(),
            }
        }
    }

    #[test]
    fn read_lines_flat_maps_each_line_in_order() {
        let lines = vec![
            (0, "u:hello".to_string()),
            (9, "noise".to_string()),
            (16, "u:again".to_string()),
            (22, "u:x".to_string()),
        ];
        let items = read_lines(&FakeFormat, &lines);
        let ids: Vec<&str> = items.iter().map(|item| item.id()).collect();
        assert_eq!(ids, vec!["0:0", "16:0", "22:0"]);
        assert_eq!(
            items[1],
            ChatItem::User { id: "16:0".into(), at: None, text: "again".into() }
        );
    }

    #[test]
    fn read_lines_of_nothing_is_empty() {
        assert!(read_lines(&FakeFormat, &[]).is_empty());
    }
}
