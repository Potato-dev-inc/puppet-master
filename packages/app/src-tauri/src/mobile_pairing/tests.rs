use super::*;

#[test]
fn public_paths_accept_bridge_prefix() {
    let segments = ["bridge", "health"];
    assert!(is_public_bridge_path(&segments, "GET"));

    let segments = ["bridge", "pair", "session", "CODE12"];
    assert!(is_public_bridge_path(&segments, "GET"));

    let segments = ["bridge", "pair"];
    assert!(is_public_bridge_path(&segments, "POST"));
}

#[test]
fn proxied_bridge_prefix_does_not_bypass_pairing() {
    let segments = ["bridge", "panes"];
    let err = authorize_bridge_request("X-PM-Proxied: 1\r\n", true, &segments, "GET")
        .expect_err("panes should require auth");
    assert_eq!(err.0, 401);
}

#[test]
fn local_only_caller_requires_loopback_and_no_proxy_header() {
    assert!(is_local_only_caller("Host: 127.0.0.1\r\n", true));
    assert!(!is_local_only_caller("X-PM-Proxied: 1\r\n", true));
    assert!(!is_local_only_caller("Host: example.com\r\n", false));
}

#[test]
fn invite_route_is_not_public() {
    let segments = ["pair", "invite"];
    assert!(!is_public_bridge_path(&segments, "POST"));
}

#[test]
fn push_act_is_public_but_every_other_push_route_needs_a_bearer() {
    // The Allow/Deny tap carries its own one-shot token instead of a bearer.
    assert!(is_public_bridge_path(&["push", "act"], "POST"));
    assert!(is_public_bridge_path(&["bridge", "push", "act"], "POST"));
    assert!(!is_public_bridge_path(&["push", "act"], "GET"));
    for (method, path) in [("GET", "key"), ("POST", "subscribe"), ("DELETE", "subscribe"), ("POST", "presence")] {
        let err = authorize_bridge_request("X-PM-Proxied: 1\r\n", true, &["push", path], method)
            .expect_err("push routes require a paired device");
        assert_eq!(err.0, 401, "{method} /push/{path}");
    }
}
