//! `GET /digest?since=`: the phone's "while you were away" card (logic in `crate::turn_summary::http`).

use super::{Ctx, RouteResult};
use crate::turn_summary::http::{respond, Live};

/// The paired device behind the request's bearer token; `local` for the desktop's own tooling.
fn device_of(headers: &str) -> String {
    crate::mobile_pairing::bearer_token(headers)
        .and_then(|token| crate::mobile_pairing::pairing_store()?.lock().device_id_for_token(&token))
        .unwrap_or_else(|| "local".to_string())
}

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    respond(&Live, crate::turn_summary::last_seen(), cx.method, cx.segments, cx.query, &device_of(cx.headers))
}
