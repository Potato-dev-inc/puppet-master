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
        // An Allow/Deny tap from a notification: authorised by its own one-shot token, not a bearer.
        || (segments == ["push", "act"] && method == "POST")
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

/// Request headers that only a proxy, tunnel or CDN adds. The desktop app's own callers (webview,
/// CLI, MCP shim, Vite dev server) never send any of these to the bridge, so seeing one on a
/// loopback connection means a remote client reached us through a forwarder on this machine.
const REMOTE_INDICATOR_HEADERS: &[&str] = &[
    // our own PWA proxy and the Vite dev proxy
    "X-PM-Proxied",
    // Cloudflare (cloudflared tunnels)
    "Cf-Connecting-Ip",
    "Cf-Connecting-Ipv6",
    "Cf-Ray",
    "Cf-Visitor",
    "Cf-Ipcountry",
    "Cdn-Loop",
    "True-Client-Ip",
    // generic reverse proxies
    "X-Forwarded-For",
    "X-Forwarded-Host",
    "X-Forwarded-Proto",
    "X-Forwarded-Port",
    "X-Real-Ip",
    "Forwarded",
];

/// True when the request carries any header a forwarder adds.
pub fn has_remote_indicators(headers: &str) -> bool {
    REMOTE_INDICATOR_HEADERS
        .iter()
        .any(|name| header_value(headers, name).is_some())
}

fn is_loopback_host(value: &str) -> bool {
    let value = value.trim();
    let host = match value.strip_prefix('[') {
        // bracketed IPv6, with or without a port
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => match value.matches(':').count() {
            0 => value,
            1 => value.split(':').next().unwrap_or(""),
            _ => value, // bare IPv6
        },
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// True when a `Host` header names something other than this machine. A browser page reaching the
/// bridge by DNS rebinding, or a reverse proxy pointed straight at the bridge port, arrives from
/// loopback but carries its own hostname. No `Host` at all (HTTP/1.0 tooling) is not suspicious.
fn names_foreign_host(headers: &str) -> bool {
    headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(key, value)| {
            key.trim().eq_ignore_ascii_case("host") && !is_loopback_host(value)
        })
    })
}

/// True when an `Origin` header says the request was made by a web page that is not the desktop
/// app: any site open in the user's browser can fetch `http://127.0.0.1:<port>` (the bridge sends
/// `Access-Control-Allow-Origin: *`), so a loopback peer alone does not make it the app. The app's
/// own webview (`tauri://localhost`, `http(s)://tauri.localhost`) and the dev server on localhost
/// are loopback origins; CLI and MCP clients send no `Origin`.
fn names_foreign_origin(headers: &str) -> bool {
    headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(key, value)| {
            if !key.trim().eq_ignore_ascii_case("origin") {
                return false;
            }
            let value = value.trim();
            let authority = value.split_once("://").map(|(_, rest)| rest);
            match authority {
                // `null` (sandboxed frames, file://) and anything unparsable is foreign
                None => true,
                Some(rest) => !is_loopback_host(rest.split('/').next().unwrap_or("")),
            }
        })
    })
}

/// Whether the caller must present a paired-device bearer token. Fails closed: only a loopback
/// peer with no sign of having been forwarded (see `REMOTE_INDICATOR_HEADERS`), no foreign `Host` and no
/// foreign `Origin` is treated as the desktop app itself.
pub fn requires_pairing_token(headers: &str, peer_loopback: bool) -> bool {
    !peer_loopback
        || has_remote_indicators(headers)
        || names_foreign_host(headers)
        || names_foreign_origin(headers)
}

/// True only for a request made on this machine by the app's own tooling: a loopback peer that
/// did not come through the PWA proxy. Gates routes that must never be reached remotely.
pub fn is_local_only_caller(headers: &str, peer_loopback: bool) -> bool {
    !requires_pairing_token(headers, peer_loopback)
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
