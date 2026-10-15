//! `GET /tamper/check`: the read-only test-change check. The logic lives in `crate::tamper::http`.

use super::{Ctx, RouteResult};
use crate::bridge::http::query_param;

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    crate::tamper::http::respond(cx.method, cx.segments, &|name| query_param(cx.query, name))
}
