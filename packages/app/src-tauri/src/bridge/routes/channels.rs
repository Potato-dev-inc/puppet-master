//! The phone's two Telegram routes (logic in `crate::channels`):
//! - `GET /channels/telegram`: `{on, connected, botUsername}` and nothing else (no chat, no names).
//! - `POST /channels/telegram/off`: switches the channel off. There is NO route that turns it on,
//!   pairs a chat or changes what it may do: that is only the person at the PC.
//!
//! Both need a paired phone's bearer token (D-12). A plain loopback program, an agent's MCP session
//! and the companion are refused, whatever they send.

use serde_json::json;

use super::{Ctx, RouteResult};

pub(super) fn handle(cx: &Ctx) -> Option<RouteResult> {
    if cx.segments.first() != Some(&"channels") {
        return None;
    }
    match (cx.method, cx.segments) {
        ("GET", ["channels", "telegram"]) => Some(read(cx)),
        ("POST", ["channels", "telegram", "off"]) => Some(turn_off(cx)),
        _ => Some(Err((404, json!({"error": "unknown_channels_route"})))),
    }
}

fn refused() -> super::super::errors::HttpResponse {
    (403, json!({"error": "paired_phone_only", "code": "PAIRED_PHONE_ONLY"}))
}

fn unavailable() -> super::super::errors::HttpResponse {
    (503, json!({"error": "telegram_unavailable"}))
}

fn read(cx: &Ctx) -> RouteResult {
    paired_phone(cx.headers, cx.session).ok_or_else(refused)?;
    let view = match crate::channels::current() {
        Some(rt) => rt.phone_view(),
        // Not started: the same shape, everything off.
        None => crate::channels::config::phone_view(&crate::channels::config::TelegramConfig::default(), false),
    };
    Ok((200, serde_json::to_value(view).unwrap_or_else(|_| json!({}))))
}

fn turn_off(cx: &Ctx) -> RouteResult {
    let device = paired_phone(cx.headers, cx.session).ok_or_else(refused)?;
    let rt = crate::channels::current().ok_or_else(unavailable)?;
    match rt.turn_off_from_phone(&device) {
        Ok(was_on) => Ok((200, json!({"ok": true, "wasOn": was_on}))),
        Err(_) => Err((500, json!({"error": "telegram_off_failed"}))),
    }
}

/// The paired device that is calling: it holds a live device token and is NOT an agent's MCP session.
fn paired_phone(headers: &str, session: Option<&str>) -> Option<String> {
    if session.is_some() {
        return None;
    }
    let token = crate::mobile_pairing::bearer_token(headers)?;
    let store = crate::mobile_pairing::pairing_store()?;
    let guard = store.lock();
    let id = guard.device_id_for_token(&token)?;
    guard.is_active_device(&id).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::paired_phone;
    use crate::mobile_pairing::{init_pairing_store, pairing_store, Caps, PairRequestBody};

    fn pair() -> (String, String) {
        if pairing_store().is_none() {
            let dir = std::env::temp_dir().join(format!("pm-channels-route-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let _ = init_pairing_store(dir.join("pairing.json"));
        }
        let store = pairing_store().unwrap();
        let mut guard = store.lock();
        let s = guard.create_pairing_session_with_caps("http://127.0.0.1:1".into(), Caps::all());
        let paired = guard
            .pair_device(PairRequestBody { pairing_code: s.pairing_code, device_name: "Anna's phone".into(), device_public_key: s.server_public_key })
            .unwrap();
        (paired.device_id, paired.device_token)
    }

    #[test]
    fn only_a_paired_phone_bearer_may_use_the_telegram_routes() {
        let (id, token) = pair();
        let bearer = format!("Authorization: Bearer {token}\r\nHost: pm.example.com\r\n");
        assert_eq!(paired_phone(&bearer, None), Some(id));
        // An agent's MCP session is never a phone, even holding a real token.
        assert_eq!(paired_phone(&bearer, Some("mcp-session-1")), None);
        // A plain loopback program has no bearer; a made-up bearer is not a device.
        assert_eq!(paired_phone("Host: 127.0.0.1\r\nX-PM-Local-Secret: s\r\n", None), None);
        assert_eq!(paired_phone("Authorization: Bearer not-a-device\r\n", None), None);
        assert_eq!(paired_phone("", None), None);
    }
}
