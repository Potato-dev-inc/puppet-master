//! `companion_recall`: one bridge request with the arguments untouched. The app decides who may ask
//! (`bridge/routes/companion_mcp.rs`): the shim only forwards, with the session and pane headers it
//! always sends.

use super::super::*;
use std::sync::mpsc::Receiver;

pub(super) fn dispatch(name: &str, args: &Value, _cancel_rx: Option<&Receiver<()>>) -> Option<Result<String, String>> {
    match name {
        "companion_recall" => Some(bridge_request("POST", "/companion/recall", Some(args.clone()))),
        _ => None,
    }
}
