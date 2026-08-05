//! Named PTY key sequences (mirrors packages/app/src/lib/tui-autopilot.ts).

pub fn sequence(key_name: &str) -> Result<String, String> {
    let lower = key_name.trim().to_lowercase();
    if let Some(rest) = lower.strip_prefix("ctrl+") {
        return match rest {
            "c" => Ok("\x03".into()),
            "d" => Ok("\x04".into()),
            "z" => Ok("\x1a".into()),
            other => Err(format!("unsupported ctrl key: ctrl+{other}")),
        };
    }
    match lower.as_str() {
        "enter" | "return" => Ok("\r".into()),
        "escape" | "esc" => Ok("\x1b".into()),
        "tab" => Ok("\t".into()),
        "space" => Ok(" ".into()),
        "up" => Ok("\x1b[A".into()),
        "down" => Ok("\x1b[B".into()),
        "right" => Ok("\x1b[C".into()),
        "left" => Ok("\x1b[D".into()),
        "home" => Ok("\x1b[H".into()),
        "end" => Ok("\x1b[F".into()),
        "pageup" => Ok("\x1b[5~".into()),
        "pagedown" => Ok("\x1b[6~".into()),
        "y" => Ok("y".into()),
        "n" => Ok("n".into()),
        "yes" => Ok("y".into()),
        "no" => Ok("n".into()),
        other => Err(format!(
            "unknown key \"{other}\". Known: enter, escape, tab, space, up, down, left, right, home, end, pageup, pagedown, y, n, yes, no, ctrl+c, ctrl+d, ctrl+z"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::sequence;

    #[test]
    fn maps_navigation_keys() {
        assert_eq!(sequence("enter").unwrap(), "\r");
        assert_eq!(sequence("down").unwrap(), "\x1b[B");
        assert_eq!(sequence("yes").unwrap(), "y");
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(sequence("frobnicate").is_err());
    }
}
