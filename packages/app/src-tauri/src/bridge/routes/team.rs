//! `/team/*`: Team cards for the person's own phone and desktop (`crate::team::phone`). Gated by the
//! bridge like every route (bearer + device caps for a phone); teammates never reach these.

use super::{Ctx, RouteResult};

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    let segments = crate::mobile_pairing::normalize_bridge_segments(cx.segments);
    if segments.first() != Some(&"team") {
        return None;
    }
    let caller = match crate::mobile_pairing::authorize_caller(cx.headers, cx.peer_loopback, cx.segments, cx.method) {
        Ok(caller) => caller,
        Err(refused) => return Some(Err(refused)),
    };
    crate::team::phone::handle(cx.method, segments, &cx.request_body, &caller, cx.session.is_some())
}
