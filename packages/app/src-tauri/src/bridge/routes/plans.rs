//! `GET /plans?month=2026-10`: "Your plans" for the paired phone (read only). It returns the same report the
//! desktop page reads (`plans_report::plans_report`): one row per plan, with turns, time, files and limits.
//! The report has no folder, no pane id and no chat text. Reading it needs `watch` (`mobile_pairing/caps.rs`),
//! and an MCP session is refused here as well as by the gate, the same as the companion routes.

use serde_json::{json, Value};

use super::{Ctx, RouteResult};
use crate::bridge::http::query_param;
use crate::plans_report::model::PlansReport;
use crate::plans_report::month::YearMonth;

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    if cx.segments != ["plans"] {
        return None;
    }
    Some(respond(cx.method, cx.query, cx.session, &crate::plans_report::plans_report))
}

/// The route without the Tauri context: `report` builds the report (the live one in the app, a fixed one in tests).
fn respond(
    method: &str,
    query: &str,
    session: Option<&str>,
    report: &dyn Fn(Option<&str>) -> Result<PlansReport, String>,
) -> RouteResult {
    if method != "GET" {
        return Err((404, json!({ "code": "NOT_FOUND", "error": "not found", "message": "No such plans route." })));
    }
    if session.is_some_and(|s| !s.trim().is_empty()) {
        let message = "plans can only be read from the app and the paired phone, not MCP sessions";
        return Err((403, json!({ "code": "PLANS_LOCAL_ONLY", "error": message, "message": message, "recoverable": false })));
    }
    let month = query_param(query, "month").map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
    if let Some(text) = month.as_deref() {
        if YearMonth::parse(text).is_none() {
            let message = format!("month must look like 2026-10, got {text}");
            return Err((400, json!({ "code": "INVALID_ARGUMENT", "error": message, "message": message })));
        }
    }
    let built = report(month.as_deref()).map_err(|error| (500, json!({ "code": "INTERNAL", "error": error.clone(), "message": error })))?;
    let value: Value = serde_json::to_value(built).map_err(|e| (500, json!({ "code": "INTERNAL", "error": e.to_string() })))?;
    Ok((200, crate::redact::json(value).into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobile_pairing::{authorize_bridge_request, route_need, Cap, Caps, Need};
    use crate::plans_report::aggregate::Labels;
    use crate::plans_report::model::{FilesFact, TurnFact};
    use crate::plans_report::{build, Inputs};
    use chrono::{TimeZone, Utc};

    const MIN: u64 = 60_000;

    /// A fixed report: one turn in October 2026 under a work account, the clock on 28 October.
    fn fixed_report(month: Option<&str>) -> Result<PlansReport, String> {
        let agent = |id: &str| if id == "alpha" { "Alpha Code".to_string() } else { id.to_string() };
        let account = |id: &str| (id == "acct-work").then(|| "Work".to_string());
        let at = |d: u32| Utc.with_ymd_and_hms(2026, 10, d, 9, 0, 0).unwrap().timestamp_millis() as u64;
        let inputs = Inputs {
            turns: vec![TurnFact {
                agent_id: "alpha".into(),
                account: Some("acct-work".into()),
                started_ms: at(3),
                busy_ms: 42 * MIN,
                files: Some(FilesFact { count: 2, added: 30, deleted: 5 }),
                merged: true,
            }],
            limits: vec![],
            limits_since_ms: None,
            unreadable_records: 0,
        };
        let now = Utc.with_ymd_and_hms(2026, 10, 28, 12, 0, 0).unwrap().timestamp_millis() as u64;
        let labels = Labels { agent: &agent, account: &account };
        Ok(build(&inputs, month.and_then(YearMonth::parse), now, &Utc, &labels))
    }

    fn never_read(_: Option<&str>) -> Result<PlansReport, String> {
        panic!("a refused request must not read the plans")
    }

    #[test]
    fn the_phone_gets_the_report_for_the_month_in_the_shape_the_page_reads() {
        let (status, body) = respond("GET", "month=2026-10", None, &fixed_report).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body["month"]["id"], "2026-10");
        assert_eq!(body["month"]["label"], "October 2026");
        assert_eq!(body["months"][0]["id"], "2026-10");
        let row = &body["rows"][0];
        assert_eq!(row["agentId"], "alpha");
        assert_eq!(row["agentLabel"], "Alpha Code");
        assert_eq!(row["accountLabel"], "Work");
        assert_eq!(row["turns"], 1);
        assert_eq!(row["busyMs"], 42 * MIN);
        assert_eq!(row["files"]["knownTurns"], 1);
        assert_eq!(row["files"]["changed"], 2);
        assert_eq!(row["merged"], 1);
        assert_eq!(row["limits"]["hits"], 0);
        assert_eq!(body["totals"]["turns"], 1);
        assert_eq!(body["totals"]["filesChanged"], 2);
        assert!(body["notes"].as_array().is_some_and(|notes| !notes.is_empty()));
        // No folder, no pane and no chat text leave with the report.
        let text = body.to_string();
        for banned in ["\"folder", "paneId", "pane_id", "\"task\"", "\"transcript\"", "C:\\"] {
            assert!(!text.contains(banned), "{banned} in {text}");
        }
    }

    #[test]
    fn no_month_means_this_month_and_a_bad_month_is_refused_before_anything_is_read() {
        let (_, body) = respond("GET", "", None, &fixed_report).unwrap();
        assert_eq!(body["month"]["id"], "2026-10");
        let (status, error) = respond("GET", "month=2026-13", None, &never_read).unwrap_err();
        assert_eq!((status, error["code"].as_str()), (400, Some("INVALID_ARGUMENT")));
    }

    #[test]
    fn an_mcp_session_is_refused_and_reads_nothing() {
        let (status, error) = respond("GET", "", Some("mcp-session-1"), &never_read).unwrap_err();
        assert_eq!((status, error["code"].as_str()), (403, Some("PLANS_LOCAL_ONLY")));
    }

    #[test]
    fn only_the_method_get_is_served() {
        let (status, _) = respond("POST", "", None, &never_read).unwrap_err();
        assert_eq!(status, 404);
    }

    #[test]
    fn a_paired_phone_needs_a_token_and_the_watch_cap() {
        // A phone that comes through the loopback proxy, or a remote peer, has to present a token.
        let (status, body) = authorize_bridge_request("X-PM-Proxied: 1\r\n", true, &["plans"], "GET").unwrap_err();
        assert_eq!((status, body["error"].as_str()), (401, Some("pairing_required")));
        let (status, _) = authorize_bridge_request("", false, &["plans"], "GET").unwrap_err();
        assert_eq!(status, 401);
        // The class of the route: reading needs `watch`; a device that can only answer, type or control cannot read it.
        assert_eq!(route_need("GET", &["plans"]), Need::Cap(Cap::Watch));
        assert!(!Caps::none().with(Cap::Answer, true).satisfies(route_need("GET", &["plans"])));
        assert!(!Caps::none().with(Cap::Type, true).with(Cap::Control, true).satisfies(route_need("GET", &["plans"])));
        assert!(Caps::none().with(Cap::Watch, true).satisfies(route_need("GET", &["plans"])));
        // A write to the route fails closed into control.
        assert_eq!(route_need("POST", &["plans"]), Need::Cap(Cap::Control));
    }
}
