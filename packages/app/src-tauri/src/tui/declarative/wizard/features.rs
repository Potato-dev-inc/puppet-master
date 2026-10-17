//! Facts the wizard MEASURED are written into the draft by the wizard, not left to the drafter's guess.
//! Today that is one: `[features] reprints_on_resize`, from the guided capture's resize measurement
//! (`visible::capture::ResizeProbe`). A drafter that wrote the key itself is overruled, and the key is written
//! either way (`false` is a fact too) so the definition says what was measured.

const KEY: &str = "reprints_on_resize";

/// `text` with `[features] reprints_on_resize = <measured>`. `None` (not measured) leaves the text alone.
pub fn with_reprints_on_resize(text: &str, measured: Option<bool>) -> String {
    let Some(value) = measured else { return text.to_string() };
    let line = format!("{KEY} = {value}");
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let lines: Vec<&str> = text.lines().collect();
    let header = |l: &&str| {
        let t = l.trim();
        t == "[features]" || t.starts_with("[features]") && t[10..].trim_start().starts_with('#')
    };
    let Some(start) = lines.iter().position(header) else {
        let mut out = text.trim_end().to_string();
        out.push_str(eol);
        out.push_str(eol);
        out.push_str("[features]");
        out.push_str(eol);
        out.push_str(&line);
        out.push_str(eol);
        return out;
    };
    let end = lines[start + 1..].iter().position(|l| l.trim_start().starts_with('[')).map_or(lines.len(), |at| start + 1 + at);
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 1);
    for (i, l) in lines.iter().enumerate() {
        let inside = i > start && i < end;
        if inside && l.trim_start().starts_with(KEY) && l.trim_start()[KEY.len()..].trim_start().starts_with('=') {
            continue;
        }
        out.push((*l).to_string());
        if i == start {
            out.push(line.clone());
        }
    }
    let mut joined = out.join(eol);
    if text.ends_with('\n') {
        joined.push_str(eol);
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_table_is_added_at_the_end() {
        let out = with_reprints_on_resize("[agent]\nid = \"x\"\n", Some(true));
        assert!(out.ends_with("\n[features]\nreprints_on_resize = true\n"), "{out}");
        assert!(toml::from_str::<toml::Table>(&out).is_ok());
    }

    #[test]
    fn an_existing_table_gets_the_key_and_the_drafters_own_is_overruled() {
        let draft = "[agent]\nid = \"x\"\n\n[features]\ndispatchable = true\nreprints_on_resize = false # my guess\n\n[tui]\nargv = []\n";
        let out = with_reprints_on_resize(draft, Some(true));
        assert_eq!(out.matches(KEY).count(), 1, "{out}");
        let table: toml::Table = toml::from_str(&out).expect("valid toml");
        assert_eq!(table["features"]["reprints_on_resize"].as_bool(), Some(true));
        assert_eq!(table["features"]["dispatchable"].as_bool(), Some(true));
        assert!(table.contains_key("tui"));
    }

    #[test]
    fn not_measured_changes_nothing() {
        let draft = "[agent]\nid = \"x\"\n";
        assert_eq!(with_reprints_on_resize(draft, None), draft);
    }

    #[test]
    fn false_is_written_too_and_a_second_pass_changes_nothing() {
        let once = with_reprints_on_resize("[features]\nreprints_on_resize = true\n", Some(false));
        assert_eq!(once, "[features]\nreprints_on_resize = false\n");
        assert_eq!(with_reprints_on_resize(&once, Some(false)), once);
    }
}
