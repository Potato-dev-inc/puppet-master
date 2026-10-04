use super::*;

#[test]
fn wait_for_health_polls_fast_and_survives_a_hung_request() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for (n, stream) in listener.incoming().enumerate() {
            let mut stream = stream.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            match n {
                0 => {
                    let _ = stream.write_all(
                        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
                1 => held.push(stream), // never answers: must hit the per-request timeout
                _ => {
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    );
                    break;
                }
            }
        }
    });
    let start = Instant::now();
    wait_for_health(&format!("http://127.0.0.1:{port}"), Duration::from_secs(10)).unwrap();
    // hung request is cut at ~1s, not 30s
    assert!(start.elapsed() < Duration::from_secs(4), "{:?}", start.elapsed());
}

#[test]
fn abort_gives_up_quickly_on_a_server_that_never_answers() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let held: Vec<_> = listener.incoming().take(2).filter_map(Result::ok).collect();
        std::thread::sleep(Duration::from_secs(40));
        drop(held);
    });
    let start = Instant::now();
    let result = abort_session(&format!("http://127.0.0.1:{port}"), "ses_x", None);
    assert!(result.is_err());
    // cancel_agent must answer well inside the MCP shim's read timeout
    assert!(start.elapsed() < Duration::from_secs(10), "{:?}", start.elapsed());
}

#[test]
fn normalizes_base_url() {
    assert_eq!(
        normalize_base_url("http://127.0.0.1:4096/"),
        "http://127.0.0.1:4096"
    );
}

#[test]
fn detects_rate_limit_status_codes() {
    assert!(is_rate_limit_status(429, ""));
    assert!(is_rate_limit_status(402, ""));
    assert!(!is_rate_limit_status(500, ""));
}

#[test]
fn detects_rate_limit_body_keywords() {
    assert!(is_rate_limit_status(400, "Rate Limit exceeded"));
    assert!(is_rate_limit_status(400, "quota exhausted"));
    assert!(is_rate_limit_status(400, "usage cap"));
    assert!(is_rate_limit_status(400, "Too Many Requests"));
    assert!(!is_rate_limit_status(400, "invalid session"));
}

#[test]
fn with_directory_query_encodes_windows_paths() {
    let url = with_directory_query(
        "http://127.0.0.1:4097/api/session/ses_test/model",
        Some(r"C:\repo\packages\app"),
    );
    assert!(url.contains("directory=C%3A%5Crepo%5Cpackages%5Capp"));
}

#[test]
fn switch_session_model_body_uses_v2_shape() {
    let body = switch_session_model_body(&OpenCodeModelRef {
        provider_id: "openrouter".into(),
        model_id: "z-ai/glm-5.2".into(),
    });
    assert_eq!(
        body["model"],
        json!({ "id": "z-ai/glm-5.2", "providerID": "openrouter", "variant": "default" })
    );
}

#[test]
fn prompt_async_body_includes_model_when_set() {
    let body = prompt_async_body(
        "hello",
        Some(&OpenCodeModelRef {
            provider_id: "anthropic".into(),
            model_id: "claude-sonnet-4".into(),
        }),
    );
    assert_eq!(
        body["model"],
        json!({ "providerID": "anthropic", "modelID": "claude-sonnet-4" })
    );
    assert_eq!(body["parts"][0]["text"], "hello");
}

#[test]
fn prompt_async_body_omits_model_when_none() {
    let body = prompt_async_body("hello", None);
    assert!(body.get("model").is_none());
}

#[test]
fn model_matches_filter_respects_provider_and_model() {
    let model = OpenCodeModelRef {
        provider_id: "opencode-go".into(),
        model_id: "glm-5.2".into(),
    };
    assert!(model_matches_filter(
        &model,
        Some("opencode-go"),
        Some("glm-5.2")
    ));
    assert!(model_matches_filter(&model, None, Some("glm-5.2")));
    assert!(!model_matches_filter(
        &model,
        Some("openrouter"),
        Some("glm-5.2")
    ));
    assert!(!model_matches_filter(
        &model,
        Some("opencode-go"),
        Some("deepseek")
    ));
}
