//! `/missions/*`: the desktop's and the phone's mission routes. The logic lives in
//! `crate::missions::routes`.

use super::{Ctx, RouteResult};
use crate::bridge::errors::operation_http_error;
use crate::bridge::http::query_param;

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    if cx.segments.first() != Some(&"missions") {
        return None;
    }
    let project = query_param(cx.query, "project_path");
    let by = if cx.session.is_none() && crate::mobile_pairing::requires_pairing_token(cx.headers, cx.peer_loopback) { "device:phone" } else { "you" };
    let result = crate::missions::routes::respond(cx.method, cx.segments, project.as_deref(), &cx.request_body, cx.app, cx.session, by)?;
    Some(result.map(|value| (200, value)).map_err(operation_http_error))
}
