//! File checkpoints over HTTP; the request/response logic lives in `crate::checkpoints::http`.

use super::{Ctx, RouteResult};

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    crate::checkpoints::http::respond(crate::checkpoints::live(), cx.method, cx.segments)
}
