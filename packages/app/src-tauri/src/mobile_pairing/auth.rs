//! Bridge request authorization for paired mobile devices (public paths, bearer tokens, loopback bypass).

use super::*;

/// Strip a leading `bridge` segment when a reverse proxy forwards `/bridge/*` to the
/// embedded bridge without rewriting the path (common with custom domains).
pub fn normalize_bridge_segments<'a>(segments: &'a [&'a str]) -> &'a [&'a str] {
    match segments.first() {
        Some(&"bridge") if segments.len() > 1 => &segments[1..],
        _ => segments,
    }
}

pub fn is_public_bridge_path(segments: &[&str], method: &str) -> bool {
    let segments = normalize_bridge_segments(segments);
    (segments == ["health"] && method == "GET")
        || (segments == ["pair"] && method == "POST")
        || (segments.len() == 3
            && segments[0] == "pair"
            && segments[1] == "session"
            && method == "GET")
}

pub fn header_value<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim().eq_ignore_ascii_case(name) {
            Some(value.trim())
        } else {
            None
        }
    })
}

pub fn bearer_token(headers: &str) -> Option<String> {
    let auth = header_value(headers, "Authorization")?;
    auth.strip_prefix("Bearer ")
        .or_else(|| auth.strip_prefix("bearer "))
        .map(|t| t.trim().to_string())
}

pub fn requires_pairing_token(headers: &str, peer_loopback: bool) -> bool {
    if header_value(headers, "X-PM-Proxied").is_some() {
        return true;
    }
    !peer_loopback
}

pub fn authorize_bridge_request(
    headers: &str,
    peer_loopback: bool,
    segments: &[&str],
    method: &str,
) -> Result<(), (u16, serde_json::Value)> {
    let segments = normalize_bridge_segments(segments);
    if is_public_bridge_path(segments, method) {
        return Ok(());
    }

    let needs_token = requires_pairing_token(headers, peer_loopback);
    if !needs_token {
        return Ok(());
    }

    let token = bearer_token(headers).ok_or_else(|| {
        (
            401,
            serde_json::json!({
                "error": "pairing_required",
                "hint": "Scan the desktop QR code to pair this device before remote control.",
            }),
        )
    })?;

    let store = pairing_store()
        .ok_or_else(|| (503, serde_json::json!({ "error": "pairing_unavailable" })))?;
    let valid = store.lock().validate_bearer_token(&token);
    if valid {
        Ok(())
    } else {
        Err((401, serde_json::json!({ "error": "invalid_token" })))
    }
}
