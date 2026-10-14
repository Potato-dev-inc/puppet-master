//! Encrypt a payload for one subscription and POST it to the browser push service.
//!
//! Encryption (RFC 8291 aes128gcm) and VAPID signing (RFC 8292) come from the `web-push-native`
//! crate (MIT OR Apache-2.0, pure Rust: RustCrypto p256/aes-gcm/hkdf, no OpenSSL, builds on
//! Windows). HTTP goes through `ureq`, which the app already uses.

use super::store::Subscription;
use super::vapid::VapidKeys;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
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

/// The body of a push message. The two texts a person reads (title and body) are `Redacted` by
/// construction; the rest is ids and the one-time action token, which the phone needs verbatim.
#[derive(Debug, Clone)]
pub struct PushPayload(Vec<u8>);

impl PushPayload {
    pub fn new(
        title: &crate::redact::Redacted<String>,
        body: &crate::redact::Redacted<String>,
        extra: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        let mut map = extra;
        map.insert("title".into(), serde_json::Value::String(title.as_str().to_string()));
        map.insert("body".into(), serde_json::Value::String(body.as_str().to_string()));
        Self(serde_json::Value::Object(map).to_string().into_bytes())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// A payload of fixed bytes, for tests of the encryption and the request shape.
    #[cfg(test)]
    pub(crate) fn raw_for_tests(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }
}

impl std::ops::Deref for PushPayload {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
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
    payload: &PushPayload,
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
    let request = builder.build(payload.as_bytes().to_vec()).map_err(|e| e.to_string())?;
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

/// Addresses a push request must never reach: this machine, the LAN, link-local (cloud metadata),
/// CGNAT, multicast and unspecified. Checked on the resolved IPs at send time.
pub fn ip_is_forbidden(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (64..=127).contains(&o[1])) // 100.64.0.0/10 CGNAT
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return ip_is_forbidden(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
        }
    }
}

/// Resolves with the OS resolver, then drops every forbidden address. ureq connects to exactly
/// the addresses returned here, so the checked IP is the IP used (no second lookup to race).
struct GuardedResolver;

impl ureq::Resolver for GuardedResolver {
    fn resolve(&self, netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
        let all: Vec<SocketAddr> = netloc.to_socket_addrs()?.collect();
        let allowed: Vec<SocketAddr> = all.into_iter().filter(|a| !ip_is_forbidden(a.ip())).collect();
        if allowed.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "push host resolves to a forbidden address"));
        }
        Ok(allowed)
    }
}

pub fn send(request: &Outgoing) -> Outcome {
    // No redirects: a push service answers 201 or an error, never a redirect to follow (a redirect
    // could point at http:// or an internal host). Resolution is guarded, see GuardedResolver.
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .redirects(0)
        .resolver(GuardedResolver)
        .build();
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
        let out = build_request(&sub, &vapid, "mailto:a@b.example", &PushPayload::raw_for_tests(payload), true).unwrap();
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
        let out = build_request(&sub, &vapid, "mailto:a@b.example", &PushPayload::raw_for_tests(b"{}"), false).unwrap();
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
    fn forbidden_addresses_cover_loopback_private_linklocal_cgnat_and_mapped() {
        for bad in [
            "127.0.0.1", "127.8.8.8", "10.0.0.1", "172.16.5.5", "192.168.1.1", "169.254.169.254",
            "0.0.0.0", "0.1.2.3", "100.64.0.1", "100.127.255.255", "224.0.0.1", "255.255.255.255",
            "::1", "::", "fe80::1", "fc00::1", "fd12:3456::1", "ff02::1", "::ffff:127.0.0.1", "::ffff:10.1.1.1",
        ] {
            assert!(ip_is_forbidden(bad.parse().unwrap()), "{bad} must be forbidden");
        }
        for good in ["142.250.80.10", "17.253.144.10", "100.63.0.1", "100.128.0.1", "2607:f8b0:4004::1", "::ffff:8.8.8.8"] {
            assert!(!ip_is_forbidden(good.parse().unwrap()), "{good} must be allowed");
        }
    }

    #[test]
    fn resolver_refuses_a_name_that_points_at_loopback() {
        use ureq::Resolver;
        // "localhost" resolves to loopback on every machine: the guard must drop it.
        let err = GuardedResolver.resolve("localhost:443").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn redirects_are_not_followed() {
        // A server that answers 302 to an internal address: send() must report a failure, not follow.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            for stream in listener.incoming().take(2) {
                let mut s = stream.unwrap();
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                // Read the whole request (head + 1-byte body) so closing never resets the client.
                let mut seen: Vec<u8> = Vec::new();
                let mut chunk = [0u8; 512];
                loop {
                    let head_end = seen.windows(4).position(|w| w == b"\r\n\r\n");
                    if head_end.is_some_and(|p| seen.len() > p + 4) {
                        break;
                    }
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => seen.extend_from_slice(&chunk[..n]),
                    }
                }
                let reply = "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/internal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = s.write_all(reply.as_bytes());
            }
        });
        // Use a plain ureq agent with the same redirect setting through the public classify path.
        let agent = ureq::AgentBuilder::new().redirects(0).build();
        let result = agent.post(&format!("http://127.0.0.1:{port}/x")).send_bytes(b"x");
        let code = match result {
            Ok(resp) => resp.status(),
            Err(ureq::Error::Status(code, _)) => code,
            Err(other) => panic!("unexpected transport error {other:?}"),
        };
        assert_eq!(classify_status(code), Outcome::Failed(302));
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1, "the redirect target must not be contacted");
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
