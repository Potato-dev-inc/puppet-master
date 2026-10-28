//! Issue intake: GitHub reads (SPEC n32 section 3.3). Reads the JSON that `gh api` returns for
//! issues, label events and collaborator permission into small structs. Pure: no I/O, no network.
//! The tests read recorded responses from `fixtures/gh/`, never GitHub.
//!
//! Everything here is data from GitHub, so it is untrusted. A missing or odd field becomes an
//! empty value or `None`. Nothing panics. A label lookup that cannot be read fails closed.

use serde_json::Value;

use super::model::valid_login;

/// Issues per page for the watch poll (`per_page=30`).
pub const ISSUES_PER_PAGE: u32 = 30;
/// Events per page for the label lookup (`per_page=100`).
pub const EVENTS_PER_PAGE: u32 = 100;
/// The label lookup reads at most this many event pages. The caller fails closed past it.
pub const EVENT_PAGES_MAX: u32 = 5;

/// One open issue from the issues list. Pull requests are never in this list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiIssue {
    pub number: u64,
    pub title: String,
    pub body: String,
    /// `user.login`. Empty when GitHub does not say.
    pub author: String,
    pub author_association: String,
    pub url: String,
    pub updated_at: String,
    pub locked: bool,
}

/// The latest `labeled` event for one label on one issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelEvent {
    pub id: u64,
    /// `actor.login`.
    pub actor: String,
    /// `actor.type == "Bot"`, or a login ending in `[bot]`. Either one counts as a bot.
    pub actor_is_bot: bool,
    pub at_ms: u64,
}

/// Parse the issues list. An entry with a `pull_request` key (even `null`) is skipped, and so is
/// an entry without a number. Bad JSON gives an empty list.
pub fn parse_issues(json: &str) -> Vec<ApiIssue> {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    items.iter().filter(|item| item.get("pull_request").is_none()).filter_map(issue_from).collect()
}

fn issue_from(item: &Value) -> Option<ApiIssue> {
    Some(ApiIssue {
        number: item.get("number").and_then(Value::as_u64)?,
        title: text_of(item, "title"),
        body: text_of(item, "body"),
        author: item.pointer("/user/login").and_then(Value::as_str).unwrap_or("").to_string(),
        author_association: text_of(item, "author_association"),
        url: text_of(item, "html_url"),
        updated_at: text_of(item, "updated_at"),
        locked: item.get("locked").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn text_of(item: &Value, key: &str) -> String {
    item.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// The newest `labeled` or `unlabeled` event for `label` (case-insensitive) in the events list.
/// The newest one must be a `labeled` event. If it is an `unlabeled` one, the label is not on the
/// issue any more and the answer is None. A matching event that cannot be read also gives None
/// (fail closed). Events are ordered by time, then by id.
pub fn latest_labeled(events_json: &str, label: &str) -> Option<LabelEvent> {
    let Ok(Value::Array(events)) = serde_json::from_str::<Value>(events_json) else {
        return None;
    };
    let wanted = label.trim();
    // (time, id, the event when it is a `labeled` one)
    let mut newest: Option<(u64, u64, Option<LabelEvent>)> = None;
    for event in &events {
        let kind = event.get("event").and_then(Value::as_str).unwrap_or("");
        if kind != "labeled" && kind != "unlabeled" {
            continue;
        }
        let name = event.pointer("/label/name").and_then(Value::as_str).unwrap_or("").trim();
        if !name.eq_ignore_ascii_case(wanted) {
            continue;
        }
        let id = event.get("id").and_then(Value::as_u64)?;
        let at_ms = event.get("created_at").and_then(Value::as_str).and_then(parse_ms)?;
        let login = event.pointer("/actor/login").and_then(Value::as_str)?.trim();
        if login.is_empty() {
            return None;
        }
        let bot = login.ends_with("[bot]") || event.pointer("/actor/type").and_then(Value::as_str) == Some("Bot");
        let found = (kind == "labeled").then(|| LabelEvent { id, actor: login.to_string(), actor_is_bot: bot, at_ms });
        let is_newer = match &newest {
            None => true,
            Some((at, last_id, _)) => (at_ms, id) > (*at, *last_id),
        };
        if is_newer {
            newest = Some((at_ms, id, found));
        }
    }
    newest.and_then(|(_, _, found)| found)
}

fn parse_ms(at: &str) -> Option<u64> {
    let ms = chrono::DateTime::parse_from_rfc3339(at).ok()?.timestamp_millis();
    u64::try_from(ms).ok()
}

/// The role of a collaborator from `repos/{o}/{r}/collaborators/{login}/permission`: `role_name`
/// when GitHub sends it, else `permission`, lowercase. An error body has neither and gives None,
/// which the caller treats as refused.
pub fn parse_permission(json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(json).ok()?;
    ["role_name", "permission"].iter().find_map(|key| {
        let role = value.get(*key)?.as_str()?.trim();
        (!role.is_empty()).then(|| role.to_ascii_lowercase())
    })
}

/// The login from `gh api user`. Only a login that passes `valid_login` is returned.
pub fn parse_login(json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(json).ok()?;
    let login = value.get("login")?.as_str()?.trim();
    valid_login(login).then(|| login.to_string())
}

/// The issues list path for the watch poll. The label is trimmed and percent-encoded.
pub fn issues_path(owner_repo: &str, label: &str) -> String {
    format!(
        "repos/{owner_repo}/issues?labels={}&state=open&sort=updated&direction=desc&per_page={ISSUES_PER_PAGE}",
        encode_query(label.trim())
    )
}

/// One page of the events list of one issue, for the label lookup.
pub fn events_path(owner_repo: &str, n: u64, page: u32) -> String {
    format!("repos/{owner_repo}/issues/{n}/events?per_page={EVENTS_PER_PAGE}&page={page}")
}

/// Percent-encode everything except the unreserved characters (RFC 3986).
fn encode_query(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ISSUES: &str = include_str!("fixtures/gh/issues.json");
    const ISSUES_WITH_PR: &str = include_str!("fixtures/gh/issues_with_pr.json");
    const EVENTS_LABELED: &str = include_str!("fixtures/gh/events_labeled.json");
    const EVENTS_RELABELED: &str = include_str!("fixtures/gh/events_relabeled.json");
    const EVENTS_BOT: &str = include_str!("fixtures/gh/events_bot.json");
    const PERMISSION_WRITE: &str = include_str!("fixtures/gh/permission_write.json");
    const PERMISSION_TRIAGE: &str = include_str!("fixtures/gh/permission_triage.json");
    const USER: &str = include_str!("fixtures/gh/user.json");

    fn ms(at: &str) -> u64 {
        chrono::DateTime::parse_from_rfc3339(at).unwrap().timestamp_millis() as u64
    }

    #[test]
    fn pull_requests_are_skipped() {
        let numbers: Vec<u64> = parse_issues(ISSUES_WITH_PR).iter().map(|issue| issue.number).collect();
        assert_eq!(numbers, vec![44]);
    }

    #[test]
    fn issues_read_their_fields_and_a_null_body_is_empty() {
        let issues = parse_issues(ISSUES);
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].number, 42);
        assert_eq!(issues[0].title, "Totals are wrong on the invoice page");
        assert_eq!(issues[0].author, "you");
        assert_eq!(issues[0].author_association, "OWNER");
        assert_eq!(issues[0].url, "https://github.com/octo-org/billing/issues/42");
        assert_eq!(issues[0].updated_at, "2026-10-27T09:00:00Z");
        assert!(issues[0].body.contains("discount"));
        assert!(!issues[0].locked);
        assert_eq!(issues[1].author, "someone");
        assert_eq!(issues[1].body, "");
        assert!(issues[1].locked);
    }

    #[test]
    fn bad_json_gives_no_issues() {
        assert!(parse_issues("not json").is_empty());
        assert!(parse_issues(r#"{"number":1}"#).is_empty());
        assert!(parse_issues(r#"[{"title":"no number"}]"#).is_empty());
    }

    #[test]
    fn latest_labeled_reads_the_newest_matching_event() {
        let ev = latest_labeled(EVENTS_LABELED, "puppet").unwrap();
        assert_eq!(ev.id, 1001);
        assert_eq!(ev.actor, "you");
        assert!(!ev.actor_is_bot);
        assert_eq!(ev.at_ms, ms("2026-10-27T10:00:00Z"));
    }

    #[test]
    fn label_match_ignores_case() {
        assert_eq!(latest_labeled(EVENTS_LABELED, "PUPPET").map(|ev| ev.id), Some(1001));
        let other = latest_labeled(EVENTS_LABELED, "ui").map(|ev| (ev.id, ev.actor));
        assert_eq!(other, Some((1003, "someone".to_string())));
    }

    #[test]
    fn relabel_after_unlabel_uses_the_last_labeled_event() {
        let ev = latest_labeled(EVENTS_RELABELED, "puppet").unwrap();
        assert_eq!((ev.id, ev.actor.as_str()), (2003, "someone"));
        assert_eq!(ev.at_ms, ms("2026-10-27T10:00:00Z"));
    }

    #[test]
    fn last_event_unlabeled_means_no_labeler() {
        let json = r#"[
          {"id":1,"event":"labeled","label":{"name":"puppet"},"actor":{"login":"you","type":"User"},"created_at":"2026-10-27T10:00:00Z"},
          {"id":2,"event":"unlabeled","label":{"name":"puppet"},"actor":{"login":"you","type":"User"},"created_at":"2026-10-27T11:00:00Z"}
        ]"#;
        assert_eq!(latest_labeled(json, "puppet"), None);
    }

    #[test]
    fn unreadable_matching_event_fails_closed() {
        let no_actor = r#"[{"id":1,"event":"labeled","label":{"name":"puppet"},"created_at":"2026-10-27T10:00:00Z"}]"#;
        let bad_time = r#"[{"id":1,"event":"labeled","label":{"name":"puppet"},"actor":{"login":"you"},"created_at":"soon"}]"#;
        assert_eq!(latest_labeled(no_actor, "puppet"), None);
        assert_eq!(latest_labeled(bad_time, "puppet"), None);
        assert_eq!(latest_labeled("not json", "puppet"), None);
    }

    #[test]
    fn bot_actor_is_marked() {
        let ev = latest_labeled(EVENTS_BOT, "puppet").unwrap();
        assert_eq!(ev.actor, "octo-org-ci[bot]");
        assert!(ev.actor_is_bot);
    }

    #[test]
    fn permission_reads_role_name_then_permission() {
        assert_eq!(parse_permission(PERMISSION_WRITE).as_deref(), Some("write"));
        assert_eq!(parse_permission(PERMISSION_TRIAGE).as_deref(), Some("triage"));
        assert_eq!(parse_permission(r#"{"permission":"write","role_name":"maintain"}"#).as_deref(), Some("maintain"));
        assert_eq!(parse_permission(r#"{"permission":"admin"}"#).as_deref(), Some("admin"));
    }

    #[test]
    fn an_error_body_has_no_permission() {
        assert_eq!(parse_permission(r#"{"message":"Not Found"}"#), None);
        assert_eq!(parse_permission("oops"), None);
    }

    #[test]
    fn login_comes_from_gh_api_user() {
        assert_eq!(parse_login(USER), Some("you".to_string()));
        assert_eq!(parse_login(r#"{"login":"bad_name"}"#), None);
        assert_eq!(parse_login(r#"{"message":"Requires authentication"}"#), None);
    }

    #[test]
    fn paths_encode_the_label_and_name_the_page() {
        assert_eq!(
            issues_path("octo-org/billing", "puppet"),
            "repos/octo-org/billing/issues?labels=puppet&state=open&sort=updated&direction=desc&per_page=30"
        );
        assert_eq!(
            issues_path("octo-org/billing", " puppet: ready "),
            "repos/octo-org/billing/issues?labels=puppet%3A%20ready&state=open&sort=updated&direction=desc&per_page=30"
        );
        assert_eq!(events_path("octo-org/billing", 42, 2), "repos/octo-org/billing/issues/42/events?per_page=100&page=2");
    }
}
