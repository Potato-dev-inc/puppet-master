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
