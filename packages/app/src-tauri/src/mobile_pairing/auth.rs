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

/// True for an `Origin` value of the desktop app itself: its webview (`tauri://localhost`,
/// `http(s)://tauri.localhost`) or a dev server on this machine. These are the only origins the
/// bridge answers with CORS headers.
pub fn is_app_origin(origin: &str) -> bool {
    origin
        .trim()
        .split_once("://")
        .is_some_and(|(_, rest)| is_loopback_host(rest.split('/').next().unwrap_or("")))
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

/// Who is behind an authorized request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// The desktop app or its own tools on this machine (no token needed; never limited by caps).
    Local,
    /// A public route (`/health`, pairing, the push Allow/Deny tap).
    Public,
    /// A paired device, with what it may do right now.
    Device { device_id: String, caps: Caps },
}

impl Caller {
    pub fn may(&self, cap: Cap) -> bool {
        match self {
            Caller::Local => true,
            Caller::Public => false,
            Caller::Device { caps, .. } => caps.has(cap),
        }
    }
}

/// Who receives an event stream: the fanout filters per viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Viewer {
    Local,
    Device(String),
}

impl Viewer {
    /// `None`: the device is gone (revoked or expired) and its stream should close. Otherwise
    /// whether this payload goes to it (terminal bytes need `watch`).
    pub fn may_receive(&self, payload: &str) -> Option<bool> {
        self.view(payload).map(|view| view.is_some())
    }

    /// `None`: the device is gone (revoked or expired) and its stream should close. Otherwise what
    /// this viewer gets for the payload: nothing, the payload, or the payload cut down to what its
    /// caps allow (`caps::project`).
    pub fn view<'a>(&self, payload: &'a str) -> Option<Option<std::borrow::Cow<'a, str>>> {
        match self {
            Viewer::Local => Some(Some(std::borrow::Cow::Borrowed(payload))),
            Viewer::Device(id) => stream_caps(id).map(|caps| project(caps, payload)),
        }
    }
}

/// The viewer of an already-authorized stream request.
pub fn viewer_for(caller: &Caller) -> Viewer {
    match caller {
        Caller::Device { device_id, .. } => Viewer::Device(device_id.clone()),
        Caller::Local | Caller::Public => Viewer::Local,
    }
}

pub fn authorize_bridge_request(
    headers: &str,
    peer_loopback: bool,
    segments: &[&str],
    method: &str,
) -> Result<(), (u16, serde_json::Value)> {
    authorize_caller(headers, peer_loopback, segments, method).map(|_| ())
}

/// The bridge's one authorization decision: public route, the local app, or a paired device whose
/// token is valid, unexpired and whose caps cover the route ([`route_need`]). A device that lacks
/// the cap gets 403 `not_allowed` naming it; an expired one 401 `device_expired`.
pub fn authorize_caller(
    headers: &str,
    peer_loopback: bool,
    segments: &[&str],
    method: &str,
) -> Result<Caller, (u16, serde_json::Value)> {
    let segments = normalize_bridge_segments(segments);
    if is_public_bridge_path(segments, method) {
        return Ok(Caller::Public);
    }

    let needs_token = requires_pairing_token(headers, peer_loopback);
    if !needs_token {
        return Ok(Caller::Local);
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
    let check = store.lock().check_token(&token);
    match check {
        TokenCheck::Unknown => Err((401, serde_json::json!({ "error": "invalid_token" }))),
        TokenCheck::Expired { .. } => Err((
            401,
            serde_json::json!({
                "error": "device_expired",
                "hint": "This phone was signed out after 30 days without use. Pair again from your PC.",
            }),
        )),
        TokenCheck::Valid { device_id, caps } => {
            let need = route_need(method, segments);
            if caps.satisfies(need) {
                return Ok(Caller::Device { device_id, caps });
            }
            let needs = need_name(need);
            events::emit(events::SecurityEvent::CapDenied {
                device_id,
                method: method.to_string(),
                route: format!("/{}", segments.join("/")),
                needs: needs.clone(),
            });
            Err((
                403,
                serde_json::json!({
                    "error": "not_allowed",
                    "code": "DEVICE_NOT_ALLOWED",
                    "needs": needs,
                    "hint": "This phone is not allowed to do that. You can change what it may do on your PC: Settings > Phone > Devices.",
                }),
            ))
        }
    }
}

fn need_name(need: Need) -> String {
    match need {
        Need::Device => "device".into(),
        Need::Cap(cap) => cap.as_str().into(),
        Need::AnyOf(a, b) => format!("{}|{}", a.as_str(), b.as_str()),
    }
}

/// Whether the caller behind these headers may use `cap` right now (re-checks the token, so a
/// change of caps or a revoke applies at once). For checks inside an open connection, e.g. the
/// WebSocket `input` frame.
pub fn caller_may(headers: &str, peer_loopback: bool, cap: Cap) -> bool {
    if !requires_pairing_token(headers, peer_loopback) {
        return true;
    }
    let Some(token) = bearer_token(headers) else { return false };
    let Some(store) = pairing_store() else { return false };
    let check = store.lock().check_token(&token);
    matches!(check, TokenCheck::Valid { caps, .. } if caps.has(cap))
}
