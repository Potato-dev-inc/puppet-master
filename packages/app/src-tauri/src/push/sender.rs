//! Encrypt a payload for one subscription and POST it to the browser push service.
//!
//! Encryption (RFC 8291 aes128gcm) and VAPID signing (RFC 8292) come from the `web-push-native`
//! crate (MIT OR Apache-2.0, pure Rust: RustCrypto p256/aes-gcm/hkdf, no OpenSSL, builds on
//! Windows). HTTP goes through `ureq`, which the app already uses.

use super::store::Subscription;
use super::vapid::VapidKeys;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::time::Duration;
use web_push_native::{p256::PublicKey, Auth, WebPushBuilder};

/// A stale "needs you" is worse than none: let the push service drop it after an hour.
const TTL: Duration = Duration::from_secs(3600);

/// The VAPID `sub` claim. Push services want a contact; override with PUPPET_MASTER_VAPID_SUBJECT.
pub fn vapid_subject() -> String {
    std::env::var("PUPPET_MASTER_VAPID_SUBJECT")
        .ok()
        .filter(|s| s.starts_with("mailto:") || s.starts_with("https://"))
        .unwrap_or_else(|| "mailto:admin@puppet-master.example".to_string())
}

/// A ready-to-send push request, as plain data.
#[derive(Debug, Clone)]
pub struct Outgoing {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub fn build_request(
    sub: &Subscription,
    vapid: &VapidKeys,
    subject: &str,
    payload: &[u8],
    urgent: bool,
) -> Result<Outgoing, String> {
    let ua_public = PublicKey::from_sec1_bytes(
        &URL_SAFE_NO_PAD.decode(&sub.p256dh).map_err(|e| format!("p256dh: {e}"))?,
    )
    .map_err(|e| format!("p256dh: {e}"))?;
    let auth_bytes = URL_SAFE_NO_PAD.decode(&sub.auth).map_err(|e| format!("auth: {e}"))?;
    if auth_bytes.len() != 16 {
        return Err("auth must be 16 bytes".into());
    }
    let builder = WebPushBuilder::new(
        sub.endpoint.parse().map_err(|e| format!("endpoint: {e}"))?,
        ua_public,
        Auth::clone_from_slice(&auth_bytes),
    )
    .with_valid_duration(TTL)
    .with_vapid(vapid.pair(), subject);
    let request = builder.build(payload.to_vec()).map_err(|e| e.to_string())?;
    let url = request.uri().to_string();
    let mut headers: Vec<(String, String)> = request
        .headers()
        .iter()
        .filter_map(|(name, value)| Some((name.as_str().to_string(), value.to_str().ok()?.to_string())))
        .collect();
    headers.push(("Urgency".into(), if urgent { "high" } else { "normal" }.into()));
    Ok(Outgoing { url, headers, body: request.into_body() })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Delivered,
    /// 404 / 410: the subscription is gone for good.
    Gone,
    /// Anything else: logged, not retried.
    Failed(u16),
    Unreachable,
}

pub fn classify_status(status: u16) -> Outcome {
    match status {
        200..=299 => Outcome::Delivered,
        404 | 410 => Outcome::Gone,
        other => Outcome::Failed(other),
    }
}

pub fn send(request: &Outgoing) -> Outcome {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build();
    let mut call = agent.post(&request.url);
    for (name, value) in &request.headers {
        call = call.set(name, value);
    }
    match call.send_bytes(&request.body) {
        Ok(resp) => classify_status(resp.status()),
        Err(ureq::Error::Status(code, _)) => classify_status(code),
        Err(_) => Outcome::Unreachable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::store::test_subscription;
    use web_push_native::jwt_simple::algorithms::{ECDSAP256PublicKeyLike, ES256PublicKey};
    use web_push_native::jwt_simple::claims::NoCustomClaims;

    fn keys(name: &str) -> VapidKeys {
        let path = std::env::temp_dir()
            .join(format!("pm-push-sender-{name}-{}", std::process::id()))
            .join("vapid.json");
        VapidKeys::load_or_create(&path).unwrap()
    }

    #[test]
    fn payload_round_trips_through_encryption() {
        let (sub, ua_secret, auth) = test_subscription("https://fcm.googleapis.com/fcm/send/abc");
        let vapid = keys("roundtrip");
        let payload = br#"{"title":"Claude Code needs you","body":"Run command: ls"}"#;
        let out = build_request(&sub, &vapid, "mailto:a@b.example", payload, true).unwrap();
        assert_eq!(out.url, "https://fcm.googleapis.com/fcm/send/abc");
        assert_ne!(out.body, payload.to_vec(), "the body must be encrypted");
        let plain = web_push_native::decrypt(out.body.clone(), &ua_secret, &Auth::clone_from_slice(&auth)).unwrap();
        assert_eq!(plain, payload.to_vec());
    }

    #[test]
    fn decrypts_the_rfc_8291_appendix_a_message() {
        // The ciphertext, receiver key and auth secret of RFC 8291 section 5 / appendix A.
        let ciphertext = URL_SAFE_NO_PAD
            .decode("DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN")
            .unwrap();
        let ua_private = web_push_native::p256::SecretKey::from_slice(
            &URL_SAFE_NO_PAD.decode("q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94").unwrap(),
        )
        .unwrap();
        let auth = URL_SAFE_NO_PAD.decode("BTBZMqHH6r4Tts7J_aSIgg").unwrap();
        let plain = web_push_native::decrypt(ciphertext, &ua_private, &Auth::clone_from_slice(&auth)).unwrap();
        assert_eq!(plain, b"When I grow up, I want to be a watermelon");
    }

    #[test]
    fn vapid_header_is_signed_by_our_key_for_the_push_service_origin() {
        let (sub, _, _) = test_subscription("https://updates.push.services.mozilla.com/wpush/v2/xyz");
        let vapid = keys("vapid-header");
        let out = build_request(&sub, &vapid, "mailto:a@b.example", b"{}", false).unwrap();
        let header = |name: &str| out.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
        let auth = header("authorization").expect("authorization header");
        let rest = auth.strip_prefix("vapid ").unwrap();
        let token = rest.split(", ").find_map(|p| p.strip_prefix("t=")).unwrap();
        let key = rest.split(", ").find_map(|p| p.strip_prefix("k=")).unwrap();
        assert_eq!(key, vapid.public_key_b64url());
        let public = ES256PublicKey::from_bytes(&URL_SAFE_NO_PAD.decode(key).unwrap()).unwrap();
        let claims = public.verify_token::<NoCustomClaims>(token, None).unwrap();
        assert_eq!(claims.audiences.unwrap().into_string().unwrap(), "https://updates.push.services.mozilla.com");
        assert_eq!(claims.subject.as_deref(), Some("mailto:a@b.example"));
        assert_eq!(header("content-encoding").as_deref(), Some("aes128gcm"));
        assert_eq!(header("ttl").as_deref(), Some("3600"));
        assert_eq!(header("urgency").as_deref(), Some("normal"));
    }

    #[test]
    fn push_service_statuses_map_to_outcomes() {
        assert_eq!(classify_status(201), Outcome::Delivered);
        assert_eq!(classify_status(410), Outcome::Gone);
        assert_eq!(classify_status(404), Outcome::Gone);
        assert_eq!(classify_status(403), Outcome::Failed(403));
        assert_eq!(classify_status(429), Outcome::Failed(429));
    }
}
