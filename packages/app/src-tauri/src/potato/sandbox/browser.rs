//! The page browser of the sandbox: what the bot's `browser_navigate`, `browser_snapshot` and `browser_act`
//! turn into, and what is checked before any of it reaches the sandbox.
//!
//! The tool (`pm-sbx-page-browser`) lives in the guest and drives the Chrome that is showing on the sandbox
//! screen. This file does not start it. It builds the arguments (`navigate {"url":...}`, `snapshot`,
//! `act {"actions":[...]}`) and hands them to `Sandbox::browser`, which runs the tool as the `pm` user through
//! the one `wsl.exe` door (`wsl.rs`). That keeps the gate rules: no second place builds a `wsl.exe` command, and
//! the arguments travel as plain argv words, never through a shell.
//!
//! What is checked here, because the model wrote it:
//!
//! - a URL is `http` or `https` only, with no user name or password in it, no backslash, no control character,
//!   at most 2048 characters, and a host that is a public name or address. Not `localhost`, not a name with no dot
//!   (an office-network name), not `.local`, `.internal` and the like, not a private, loopback, link-local,
//!   shared-address or reserved address in ANY spelling (`2130706433`, `0x7f.1`, `[::ffff:127.0.0.1]`), and not
//!   a name that only exists to point at one. A page inside the sandbox (a test page) is allowed only when the
//!   caller says so, which only a test or a dev build can;
//! - an action list has 1 to 24 actions. Each action is REBUILT from the few fields the tool takes (`kind`,
//!   `ref`, `text`, `paths`), so a field the model adds (`origin` for a saved login, `roots` for upload folders,
//!   anything else) never reaches the tool. Kinds are `click`, `fill`, `type` and `upload`;
//! - the whole argument is at most 24 KB.
//!
//! The answer of the tool is one JSON line. It is read forgivingly: a reply that is not JSON becomes a plain
//! failure that points the bot at the desktop tools.

use std::net::{Ipv4Addr, Ipv6Addr};

use serde_json::{json, Map, Value};

use super::{Sandbox, SbxResult};

pub const MAX_URL_CHARS: usize = 2048;
pub const MAX_ACTIONS: usize = 24;
pub const MAX_TEXT_CHARS: usize = 2000;
pub const MAX_ARGS_BYTES: usize = 24 * 1024;
pub const MAX_REF_CHARS: usize = 64;
pub const MAX_PATHS: usize = 8;
pub const MAX_PATH_CHARS: usize = 300;
/// The answer handed to the brain is cut to this many characters of JSON text.
pub const MAX_REPLY_CHARS: usize = 40_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrowserError {
    /// The URL was refused; the text says why in a plain sentence.
    Url(&'static str),
    /// The actions were refused.
    Actions(String),
}

impl BrowserError {
    pub fn code(&self) -> &'static str {
        match self {
            BrowserError::Url(_) => "URL_NOT_ALLOWED",
            BrowserError::Actions(_) => "BAD_ACTIONS",
        }
    }

    pub fn message(&self) -> String {
        match self {
            BrowserError::Url(why) => (*why).to_string(),
            BrowserError::Actions(why) => why.clone(),
        }
    }
}

// ---- URLs ----------------------------------------------------------------------------------------

const NOT_WEB: &str = "Only web addresses that start with http or https can be opened.";
const NOT_PUBLIC: &str = "That address is on a private network or this computer, so it is not opened. Use a public web address.";
const BAD_URL: &str = "That is not a web address the browser can open.";

/// Name endings that are never on the public internet (or point at the local network).
const LOCAL_SUFFIXES: [&str; 11] =
    ["localhost", "local", "internal", "intranet", "lan", "home", "corp", "localdomain", "home.arpa", "invalid", "test"];

/// Services that answer every name with an address the caller picks, so a public-looking name can point inside.
const POINTER_DOMAINS: [&str; 7] = ["nip.io", "sslip.io", "xip.io", "localtest.me", "lvh.me", "vcap.me", "traefik.me"];

fn ipv4_is_public(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0
        || (a == 100 && (64..=127).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (18..=19).contains(&b))
        || a >= 240)
}

fn ipv6_is_public(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return ipv4_is_public(v4);
    }
    let first = ip.segments()[0];
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
        || (first & 0xffc0) == 0xfec0
        || (first == 0x2001 && ip.segments()[1] == 0x0db8)
        // NAT64 and 6to4 and Teredo carry an IPv4 address inside; refuse them rather than unpack them.
        || (first == 0x0064 && ip.segments()[1] == 0xff9b)
        || first == 0x2002
        || (first == 0x2001 && ip.segments()[1] == 0))
}

/// One part of an address in the forms a browser reads: decimal, `0x` hex or leading-zero octal.
fn ipv4_part(part: &str) -> Option<u64> {
    if part.is_empty() {
        return None;
    }
    let (digits, radix) = if let Some(hex) = part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
        (hex, 16)
    } else if part.len() > 1 && part.starts_with('0') {
        (&part[1..], 8)
    } else {
        (part, 10)
    };
    if digits.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(digits, radix).ok()
}

/// The address a browser would connect to for a host written as 1 to 4 numbers. `None`: not that form.
fn ipv4_from_numbers(host: &str) -> Option<Result<Ipv4Addr, ()>> {
    let last = host.trim_end_matches('.').rsplit('.').next()?;
    let numeric = last.bytes().next().is_some_and(|b| b.is_ascii_digit());
    if !numeric {
        return None;
    }
    let parts: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    if parts.len() > 4 {
        return Some(Err(()));
    }
    let mut numbers = Vec::new();
    for part in &parts {
        match ipv4_part(part) {
            Some(n) => numbers.push(n),
            None => return Some(Err(())),
        }
    }
    let last_n = *numbers.last()?;
    let head = &numbers[..numbers.len() - 1];
    if head.iter().any(|n| *n > 255) {
        return Some(Err(()));
    }
    let room = 256u64.pow(5 - numbers.len() as u32);
    if last_n >= room {
        return Some(Err(()));
    }
    let mut value = last_n;
    for (i, n) in head.iter().enumerate() {
        value += n * 256u64.pow(3 - i as u32);
    }
    Some(Ok(Ipv4Addr::from(value as u32)))
}

fn host_is_public(host: &str) -> Result<(), &'static str> {
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        let ip: Ipv6Addr = inner.parse().map_err(|_| BAD_URL)?;
        return if ipv6_is_public(ip) { Ok(()) } else { Err(NOT_PUBLIC) };
    }
    if host.is_empty() || !host.is_ascii() || host.contains(['%', '[', ']', ':']) {
        return Err(BAD_URL);
    }
    let name = host.trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty() || name.starts_with('.') || name.contains("..") {
        return Err(BAD_URL);
    }
    if let Some(parsed) = ipv4_from_numbers(&name) {
        return match parsed {
            Ok(ip) if ipv4_is_public(ip) => Ok(()),
            Ok(_) => Err(NOT_PUBLIC),
            Err(()) => Err(BAD_URL),
        };
    }
    if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_') {
        return Err(BAD_URL);
    }
    if !name.contains('.') {
        return Err(NOT_PUBLIC);
    }
    if LOCAL_SUFFIXES.iter().any(|s| name == *s || name.ends_with(&format!(".{s}"))) {
        return Err(NOT_PUBLIC);
    }
    if POINTER_DOMAINS.iter().any(|d| name == *d || name.ends_with(&format!(".{d}"))) {
        return Err(NOT_PUBLIC);
    }
    Ok(())
}

/// Checks a URL the model wrote and returns it as it will be sent. `allow_local`: a page inside the sandbox
/// itself may be opened (a test or dev build only; the caller decides).
pub fn check_url(raw: &str, allow_local: bool) -> Result<String, BrowserError> {
    let url = raw.trim();
    if url.is_empty() || url.chars().count() > MAX_URL_CHARS {
        return Err(BrowserError::Url(BAD_URL));
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace() || c == '\\') {
        return Err(BrowserError::Url(BAD_URL));
    }
    let lower = url.to_ascii_lowercase();
    let rest = if let Some(rest) = lower.strip_prefix("https:") {
        rest
    } else if let Some(rest) = lower.strip_prefix("http:") {
        rest
    } else {
        return Err(BrowserError::Url(NOT_WEB));
    };
    let Some(after) = rest.strip_prefix("//") else { return Err(BrowserError::Url(BAD_URL)) };
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let authority = &after[..end];
    if authority.contains('@') {
        return Err(BrowserError::Url("An address with a name or password in it is not opened."));
    }
    let (host, port) = match authority.strip_prefix('[') {
        Some(tail) => {
            let close = tail.find(']').ok_or(BrowserError::Url(BAD_URL))?;
            let after_host = &tail[close + 1..];
            let port = match after_host {
                "" => None,
                p if p.starts_with(':') => Some(&p[1..]),
                _ => return Err(BrowserError::Url(BAD_URL)),
            };
            (format!("[{}]", &tail[..close]), port)
        }
        None => match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (authority.to_string(), None),
        },
    };
    if let Some(port) = port {
        let ok = !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n));
        if !ok {
            return Err(BrowserError::Url(BAD_URL));
        }
    }
    if !allow_local {
        host_is_public(&host).map_err(BrowserError::Url)?;
    } else if host.is_empty() || !host.is_ascii() {
        return Err(BrowserError::Url(BAD_URL));
    }
    Ok(url.to_string())
}

/// A `focus` address or a guest path: a word like `https:...` is a web address and is checked as one; anything
/// else is a path inside the sandbox (no leading dash, no NUL).
pub fn check_launch_arg(raw: &str, allow_local: bool) -> Result<String, BrowserError> {
    let arg = raw.trim();
    let scheme_end = arg.find(':');
    let has_scheme = scheme_end.is_some_and(|i| i > 1 && arg[..i].bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.')) && arg.as_bytes()[0].is_ascii_alphabetic());
    if has_scheme {
        return check_url(arg, allow_local);
    }
    if arg.is_empty() || arg.starts_with('-') || arg.chars().any(char::is_control) || arg.chars().count() > MAX_TEXT_CHARS {
        return Err(BrowserError::Actions("That address or path cannot be used.".to_string()));
    }
    Ok(arg.to_string())
}

// ---- actions -------------------------------------------------------------------------------------

fn bad(why: impl Into<String>) -> BrowserError {
    BrowserError::Actions(why.into())
}

fn clean_text(value: &Value, what: &str) -> Result<String, BrowserError> {
    let text = value.as_str().ok_or_else(|| bad(format!("{what} must be text.")))?;
    if text.chars().count() > MAX_TEXT_CHARS || text.contains('\0') {
        return Err(bad(format!("{what} is too long (2000 characters at most).")));
    }
    Ok(text.to_string())
}

/// Rebuilds the model's action list from the fields the tool takes. See the file header.
pub fn clean_actions(raw: &Value) -> Result<Vec<Value>, BrowserError> {
    let list = raw.as_array().ok_or_else(|| bad("actions must be a list."))?;
    if list.is_empty() || list.len() > MAX_ACTIONS {
        return Err(bad("Give 1 to 24 actions."));
    }
    let mut out = Vec::with_capacity(list.len());
    for (i, step) in list.iter().enumerate() {
        let n = i + 1;
        let obj = step.as_object().ok_or_else(|| bad(format!("Action {n} must be an object.")))?;
        let kind = obj.get("kind").and_then(Value::as_str).unwrap_or("");
        if !["click", "fill", "type", "upload"].contains(&kind) {
            return Err(bad(format!("Action {n}: the kind must be click, fill, type or upload.")));
        }
        let reference = obj.get("ref").and_then(Value::as_str).unwrap_or("");
        if reference.is_empty() || reference.chars().count() > MAX_REF_CHARS || reference.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(bad(format!("Action {n}: give the element ref from browser_snapshot.")));
        }
        let mut action = Map::new();
        action.insert("kind".into(), json!(kind));
        action.insert("ref".into(), json!(reference));
        match kind {
            "click" => {}
            "fill" | "type" => {
                let text = obj.get("text").ok_or_else(|| bad(format!("Action {n}: {kind} needs text.")))?;
                action.insert("text".into(), json!(clean_text(text, "The text")?));
            }
            _ => {
                let paths = obj.get("paths").and_then(Value::as_array).ok_or_else(|| bad(format!("Action {n}: upload needs paths.")))?;
                if paths.is_empty() || paths.len() > MAX_PATHS {
                    return Err(bad(format!("Action {n}: attach 1 to 8 files.")));
                }
                let mut clean = Vec::new();
                for path in paths {
                    let path = path.as_str().unwrap_or("");
                    let ok = path.starts_with('/')
                        && path.chars().count() <= MAX_PATH_CHARS
                        && !path.chars().any(char::is_control)
                        && !path.split('/').any(|part| part == "..");
                    if !ok {
                        return Err(bad(format!("Action {n}: a file path is an absolute path inside the sandbox, without '..'.")));
                    }
                    clean.push(json!(path));
                }
                action.insert("paths".into(), Value::Array(clean));
            }
        }
        out.push(Value::Object(action));
    }
    Ok(out)
}

// ---- arguments for the tool ----------------------------------------------------------------------

fn with_json(command: &str, body: &Value) -> Result<Vec<String>, BrowserError> {
    let text = body.to_string();
    if text.len() > MAX_ARGS_BYTES {
        return Err(bad("That is too much to send in one call. Split it into smaller calls."));
    }
    Ok(vec![command.to_string(), text])
}

pub fn navigate_args(url: &str) -> Result<Vec<String>, BrowserError> {
    with_json("navigate", &json!({ "url": url }))
}

pub fn snapshot_args() -> Vec<String> {
    vec!["snapshot".to_string()]
}

/// `actions` must come from `clean_actions`.
pub fn act_args(actions: &[Value]) -> Result<Vec<String>, BrowserError> {
    with_json("act", &json!({ "actions": actions }))
}

// ---- the answer ----------------------------------------------------------------------------------

fn failure(why: &str) -> Value {
    json!({ "ok": false, "error": why, "fallback": "computer_act" })
}

/// Reads what the tool printed: one JSON object (the last line that is one). Anything else is a failure that
/// points at the desktop tools. A reply over `MAX_REPLY_CHARS` is cut and says so.
pub fn parse_reply(raw: &str) -> Value {
    let found = raw.trim().lines().rev().map(str::trim).filter(|l| l.starts_with('{')).find_map(|l| serde_json::from_str::<Value>(l).ok()).filter(Value::is_object);
    let Some(value) = found else {
        return failure("The page tool gave an answer that could not be read.");
    };
    let text = value.to_string();
    if text.chars().count() <= MAX_REPLY_CHARS {
        return value;
    }
    let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false);
    json!({ "ok": ok, "truncated": true, "text": text.chars().take(MAX_REPLY_CHARS).collect::<String>() })
}

/// Runs one page-tool command in the sandbox and reads its answer.
pub fn run(sandbox: &dyn Sandbox, args: &[String]) -> SbxResult<Value> {
    Ok(parse_reply(&sandbox.browser(args)?))
}
