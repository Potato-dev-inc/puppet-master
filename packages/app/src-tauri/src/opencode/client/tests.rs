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
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "{:?}",
        start.elapsed()
    );
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
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
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

// ---- a refused connection is retried; anything that may have arrived is not ----

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[test]
fn a_refused_connection_is_a_connect_failure_and_an_http_error_is_not() {
    let port = free_port();
    let error = ureq::post(&format!("http://127.0.0.1:{port}/x")).timeout(Duration::from_secs(2)).call().unwrap_err();
    assert!(is_connect_failure(&error.to_string()), "{error}");
    assert!(is_connect_failure("opencode prompt_async: Connection Failed: Connect error"));
    // The request may have arrived in each of these: it must never be sent twice.
    assert!(!is_connect_failure("opencode prompt_async: timed out reading response"));
    assert!(!is_connect_failure("opencode prompt_async: connection reset by peer"));
    assert!(!is_connect_failure("opencode create session: HTTP 500 connection refused upstream"));
}

#[test]
fn the_retry_waits_the_given_pauses_then_returns_the_last_error() {
    let mut slept = Vec::new();
    let mut tries = 0;
    let delays = [Duration::from_millis(1), Duration::from_millis(2)];
    let result: Result<(), String> = retry_never_arrived(&delays, |d| slept.push(d), |e: &String| e == "refused", || {
        tries += 1;
        Err("refused".to_string())
    });
    assert_eq!(result, Err("refused".to_string()));
    assert_eq!((tries, slept.len()), (3, 2), "the first try plus one per pause");
}

#[test]
fn the_retry_stops_at_once_on_an_error_that_may_have_arrived_or_on_success() {
    let mut tries = 0;
    let result: Result<(), String> = retry_never_arrived(&[Duration::ZERO; 3], |_| {}, |e: &String| e == "refused", || {
        tries += 1;
        Err("timed out".to_string())
    });
    assert!(result.is_err());
    assert_eq!(tries, 1);
    let mut tries = 0;
    let ok: Result<u8, String> = retry_never_arrived(&[Duration::ZERO; 3], |_| {}, |e: &String| e == "refused", || {
        tries += 1;
        if tries < 3 { Err("refused".into()) } else { Ok(7) }
    });
    assert_eq!((ok, tries), (Ok(7), 3));
}

#[test]
fn a_prompt_sent_while_the_server_is_still_coming_up_gets_through() {
    use std::io::{Read, Write};
    let port = free_port();
    // The server binds half a second AFTER the first try: the old code failed the run there.
    let server = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        let listener = std::net::TcpListener::bind(("127.0.0.1", port)).expect("bind the same port");
        let (mut stream, _) = listener.accept().unwrap();
        // Read the whole request (headers and body) before answering, or the close resets the connection.
        let mut buf = [0u8; 4096];
        let mut seen = Vec::new();
        stream.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
            seen.extend_from_slice(&buf[..n]);
            if String::from_utf8_lossy(&seen).contains("\"hello\"") {
                break;
            }
        }
        let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
    });
    let result = prompt_async(&format!("http://127.0.0.1:{port}"), "ses_x", None, "hello", None);
    server.join().unwrap();
    assert_eq!(result, Ok(()));
}

#[test]
fn a_server_that_never_comes_up_still_fails_with_the_connection_error() {
    let port = free_port();
    let started = Instant::now();
    let error = prompt_async(&format!("http://127.0.0.1:{port}"), "ses_x", None, "hello", None).unwrap_err();
    assert_eq!(error.status, 0);
    assert!(is_connect_failure(&error.body), "{}", error.body);
    // Bounded: four tries (Windows takes ~2 s to refuse each) and 2.5 s of pauses, never the 30 s request timeout per try.
    assert!(started.elapsed() < Duration::from_secs(25), "{:?}", started.elapsed());
}
