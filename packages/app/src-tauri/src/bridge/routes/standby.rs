//! `GET /standby`: read-only state of the warm standby pool (list of standbys with their key,
//! age and health, plus what is held back and why). Settings are changed by the app, not here.

use super::{Ctx, RouteResult};

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    match (cx.method, cx.segments) {
        ("GET", ["standby"]) => Some(Ok((
            200,
            serde_json::to_value(crate::standby::status()).unwrap_or_default(),
        ))),
        _ => None,
    }
}
