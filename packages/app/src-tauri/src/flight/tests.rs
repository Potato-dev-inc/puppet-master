use super::ring::{Kind, CAPACITY, MAX_EVENTS};
use super::*;
use serde_json::Value;
use std::time::{Duration, Instant};

fn temp_project(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!("pm-flight-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.to_string_lossy().into_owned()
}

/// Parse a cast into (header, events).
fn parse_cast(text: &str) -> (Value, Vec<Value>) {
    let mut lines = text.lines();
    let header: Value = serde_json::from_str(lines.next().expect("header")).expect("header is JSON");
    let events = lines.map(|l| serde_json::from_str::<Value>(l).expect("event line is JSON")).collect();
    (header, events)
}

#[test]
fn ring_keeps_the_newest_bytes_and_drops_events_whose_bytes_are_gone() {
    let ring = Ring::new(80, 24);
    let chunk = vec![b'x'; 4096];
    for _ in 0..(CAPACITY / 4096 + 50) {
        ring.output(&chunk);
    }
    let (held, events) = ring.stats();
    assert!(held <= CAPACITY, "held {held}");
    assert!(held > CAPACITY - 2 * 4096, "the window is nearly full: {held}");
    assert!(events <= CAPACITY / 4096 + 1);
    let snap = ring.snapshot();
    assert!(snap.truncated);
    for e in &snap.events {
        assert_eq!(snap.data(e).len(), 4096, "every kept event is whole");
    }
}

#[test]
fn ring_memory_stops_growing_at_the_cap() {
    let ring = Ring::new(80, 24);
    let chunk = vec![7u8; 1000];
    for _ in 0..(CAPACITY / 1000 + 100) {
        ring.output(&chunk);
    }
    let cap_after_fill = ring.buffer_capacity();
    for _ in 0..20_000 {
        ring.output(&chunk);
    }
    assert_eq!(ring.buffer_capacity(), cap_after_fill, "no allocation once the ring is full");
    assert!(ring.stats().1 <= MAX_EVENTS);
}

#[test]
fn many_tiny_events_are_bounded_by_count() {
    let ring = Ring::new(80, 24);
    for i in 0..(MAX_EVENTS + 500) {
        ring.input(InputTag::User, &[b'a' + (i % 26) as u8]);
    }
    assert_eq!(ring.stats().1, MAX_EVENTS);
}

#[test]
fn a_chunk_bigger_than_the_ring_keeps_its_tail() {
    let ring = Ring::new(80, 24);
    let mut big = vec![b'a'; CAPACITY];
    big.extend_from_slice(b"TAIL");
    ring.output(&big);
    let snap = ring.snapshot();
    let last = snap.events.last().unwrap();
    assert!(snap.data(last).ends_with(b"TAIL"));
}

#[test]
fn the_window_start_size_follows_resizes_that_were_dropped() {
    let ring = Ring::new(80, 24);
    ring.resize(100, 30);
    let chunk = vec![b'y'; 4096];
    for _ in 0..(CAPACITY / 4096 + 10) {
        ring.output(&chunk);
    }
    let snap = ring.snapshot();
    assert!(snap.events.iter().all(|e| e.kind != Kind::Resize), "the resize fell out of the window");
    assert_eq!((snap.base_cols, snap.base_rows), (100, 30));
}

#[test]
fn a_cast_is_valid_asciicast_v2_with_writer_tags_resizes_and_markers() {
    let ring = Ring::new(120, 30);
    ring.output(b"hello \x1b[32mworld\x1b[0m\r\n");
    ring.input(InputTag::RunTask, b"fix the bug");
    ring.input(InputTag::RunTask, b"\r");
    ring.resize(90, 20);
    ring.mark("readiness=Unknown");
    ring.input(InputTag::GateDismiss, b"\x1b");
    let cast = cast::render(&ring.snapshot(), "UNIT", Some("run failed: UNIT"));
    let (header, events) = parse_cast(cast.as_str());
    assert_eq!(header["version"], 2);
    assert_eq!((header["width"].as_u64(), header["height"].as_u64()), (Some(120), Some(30)));
    assert_eq!(header["title"], "UNIT");
    let kinds: Vec<(&str, &str)> =
        events.iter().map(|e| (e[1].as_str().unwrap(), e[2].as_str().unwrap())).collect();
    assert_eq!(
        kinds,
        vec![
            ("o", "hello \u{1b}[32mworld\u{1b}[0m\r\n"),
            ("m", "w=run_task"),
            ("i", "fix the bug"),
            ("i", "\r"),
            ("r", "90x20"),
            ("m", "pm:readiness=Unknown"),
            ("m", "w=gate_dismiss"),
            ("i", "\u{1b}"),
            ("m", "pm:run failed: UNIT"),
        ]
    );
    let times: Vec<f64> = events.iter().map(|e| e[0].as_f64().unwrap()).collect();
    assert!(times.windows(2).all(|w| w[0] <= w[1]), "time never goes back: {times:?}");
    assert_eq!(times[0], 0.0);
}

#[test]
fn the_write_origin_names_the_writer_and_the_bridge_only_refines_a_person_or_api_write() {
    use crate::pty::WriteOrigin;
    assert_eq!(tag_for(&WriteOrigin::User), InputTag::User);
    assert_eq!(tag_for(&WriteOrigin::Api), InputTag::Agent);
    assert_eq!(tag_for(&WriteOrigin::Answer), InputTag::Answer);
    assert_eq!(tag_for(&WriteOrigin::Nudge), InputTag::Nudge);
    assert_eq!(tag_for(&WriteOrigin::Structured), InputTag::RunTask);
    with_tag(InputTag::Phone, || {
        assert_eq!(tag_for(&WriteOrigin::User), InputTag::Phone, "the bridge says a paired phone typed");
        with_tag(InputTag::User, || assert_eq!(tag_for(&WriteOrigin::Api), InputTag::User, "the local app over the bridge"));
        assert_eq!(tag_for(&WriteOrigin::Api), InputTag::Phone, "an inner scope restores the outer one");
        assert_eq!(tag_for(&WriteOrigin::Nudge), InputTag::Nudge, "the bridge never overrides Puppet Master's own keys");
        assert_eq!(tag_for(&WriteOrigin::Answer), InputTag::Answer);
    });
    assert_eq!(tag_for(&WriteOrigin::User), InputTag::User, "restored after the scope");
    assert_eq!(InputTag::for_bridge(true, true), InputTag::Agent);
    assert_eq!(InputTag::for_bridge(false, true), InputTag::Phone);
    assert_eq!(InputTag::for_bridge(false, false), InputTag::User);
}

#[test]
fn registry_tags_input_by_its_writer_and_keeps_closed_panes() {
    use crate::pty::WriteOrigin;
    let id = format!("flight-reg-{}", uuid::Uuid::new_v4());
    let ring = start(&id, 80, 24);
    input_as(&id, tag_for(&WriteOrigin::User), b"typed by the user");
    with_tag(InputTag::Agent, || input_as(&id, tag_for(&WriteOrigin::Api), b"typed by an agent"));
    mark(&id, "note");
    resized(&id, 100, 40);
    let tags: Vec<InputTag> = ring
        .snapshot()
        .events
        .iter()
        .filter(|e| e.kind == Kind::Input)
        .map(|e| InputTag::from_u32(e.aux))
        .collect();
    assert_eq!(tags, vec![InputTag::User, InputTag::Agent]);
    closed(&id);
    assert!(get(&id).is_some(), "a closed pane's recording stays available for a while");
    input_as(&id, InputTag::User, b"after close");
    assert_eq!(get(&id).unwrap().snapshot().events.iter().filter(|e| e.kind == Kind::Input).count(), 2);
}

#[test]
fn a_secret_in_the_recording_is_absent_from_the_export_even_when_split_over_chunks() {
    let secret = "sk-ant-api03-FlightPlantedSecret0123456789";
    let (a, b) = secret.split_at(17);
    let id = format!("flight-secret-{}", uuid::Uuid::new_v4());
    let ring = start(&id, 80, 24);
    ring.output(format!("export KEY={a}").as_bytes());
    ring.output(format!("{b}\r\n").as_bytes());
    ring.input(InputTag::User, format!("echo {secret}\r").as_bytes());
    let (cast, _) = freeze::export(&id, "test").unwrap();
    assert!(!cast.as_str().contains("FlightPlantedSecret"), "{cast}");
    assert!(cast.as_str().contains("[REDACTED]"));
    // The live ring itself is untouched: the terminal the person sees is not redacted.
    let snap = ring.snapshot();
    let raw: Vec<u8> = snap.events.iter().flat_map(|e| snap.data(e).to_vec()).collect();
    assert!(String::from_utf8_lossy(&raw).contains("FlightPlantedSecret"));
}

#[test]
fn freezing_writes_the_cast_to_the_operation_folder() {
    let project = temp_project("freeze");
    let id = format!("flight-freeze-{}", uuid::Uuid::new_v4());
    let ring = start(&id, 80, 24);
    ring.output(b"agent output\r\n");
    let frozen = freeze::freeze_now(&project, "op-frz-1", &id, "PROMPT_NOT_SUBMITTED").unwrap();
    let expected = std::path::Path::new(&project).join(".puppet-master/operations/op-frz-1/flight.cast");
    assert_eq!(std::path::Path::new(&frozen.path), expected);
    let text = std::fs::read_to_string(&expected).unwrap();
    let (header, events) = parse_cast(&text);
    assert_eq!(header["title"], "PROMPT_NOT_SUBMITTED");
    assert!(events.iter().any(|e| e[1] == "o" && e[2].as_str().unwrap().contains("agent output")));
    assert!(freeze::cast_path(&project, "../escape").is_err(), "operation ids cannot climb out of the folder");
    assert_eq!(freeze::list_casts(&project).len(), 1);
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn a_run_that_fails_gets_its_recording_frozen_next_to_its_record() {
    use crate::operations::{self, DelegateWorkRequest, OperationError, OperationStatus, StateSource};
    let project = temp_project("hook");
    let pane = format!("flight-hook-{}", uuid::Uuid::new_v4());
    let ring = start(&pane, 80, 24);
    ring.output(b"the agent exited and PowerShell took over\r\n");
    let request = DelegateWorkRequest {
        project_path: project.clone(),
        task: "inspect".into(),
        agent_type: "codex".into(),
        pane_id: Some(pane.clone()),
        idempotency_key: uuid::Uuid::new_v4().to_string(),
        acceptance_criteria: None,
        task_id: None,
        exclusive: false,
        locks: vec![],
        timeout_ms: 60_000,
        read_only: false,
        keep_pane: false,
        placement: None,
        owner_session_id: None,
        worker_has_mcp_tools: false,
        agent_run_id: Some("flight-hook-run".into()),
        turn_index: 0,
        worker: Default::default(),
        context_policy: None,
        checks: Vec::new(),
    };
    let created = operations::create_operation(request).unwrap().0;
    let cast = std::path::Path::new(&project).join(".puppet-master/operations").join(&created.operation_id).join("flight.cast");
    // Not failed yet: nothing is written.
    assert!(!cast.exists());
    operations::mark_operation_state(
        &project,
        &created.operation_id,
        OperationStatus::Failed,
        StateSource::Native,
        Some("failed".into()),
        None,
        Some(OperationError::new("AGENT_EXITED", "the agent process ended", false)),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cast.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let text = std::fs::read_to_string(&cast).expect("the failure hook froze the ring");
    let (header, events) = parse_cast(&text);
    assert_eq!(header["title"], "AGENT_EXITED");
    assert!(events.iter().any(|e| e[2].as_str().is_some_and(|t| t.contains("PowerShell took over"))));
    assert!(events.iter().any(|e| e[2] == "pm:run failed: AGENT_EXITED"));
    let _ = std::fs::remove_dir_all(&project);
}

#[test]
fn zip_round_trips_with_checksums() {
    let mut zip = zip::ZipWriter::new(Vec::new());
    zip.add("a.txt", b"hello hello hello hello").unwrap();
    zip.add("dir/b.json", "{\"u\":\"caf\u{e9}\"}".as_bytes()).unwrap();
    zip.add("empty.txt", b"").unwrap();
    let bytes = zip.finish().unwrap();
    let files = zip::read_back(&bytes).unwrap();
    assert_eq!(files.len(), 3);
    assert_eq!(files[0], ("a.txt".to_string(), b"hello hello hello hello".to_vec()));
    assert_eq!(files[1].0, "dir/b.json");
    assert!(files[2].1.is_empty());
}

fn bundle_inputs(casts: Vec<bundle::CastFile>, log: Vec<String>) -> bundle::Inputs {
    bundle::Inputs {
        version: serde_json::json!({"app":"Puppet Master","version":"9.9.9","os":"windows"}),
        settings: serde_json::json!({
            "theme": "dark",
            "anthropic_api_key": "sk-ant-api03-SettingsPlanted0123456789",
            "nested": {"access_token": "abcdef123456", "away_minutes": 5},
            "project_path": "C:\\Users\\Ren-pc\\Desktop\\work2\\puppet-master"
        }),
        log_lines: log,
        casts,
        now_ms: 10_000_000,
    }
}

#[test]
fn the_diagnostic_file_lists_its_contents_removes_secrets_and_honours_exclusions() {
    let secret = "sk-ant-api03-BundlePlanted0123456789";
    let cast_text = format!(
        "{{\"version\":2,\"width\":80,\"height\":24,\"timestamp\":1}}\n[0.1,\"o\",\"leaked {secret} here\"]\n"
    );
    let b = bundle::build(bundle_inputs(
        vec![bundle::CastFile { operation_id: "op-1".into(), modified_ms: 9_000_000, text: cast_text }],
        vec![format!("INFO something {secret}"), "WARN other".into()],
    ));
    let preview = b.preview();
    let paths: Vec<&str> = preview.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(paths, ["README.txt", "version.json", "settings.json", "app-log.txt", "recordings/op-1.cast"]);
    assert!(preview.iter().all(|p| p.bytes > 0 && !p.label.is_empty()));
    assert!(preview[4].label.starts_with("Recording of a run that failed"), "{:?}", preview[4]);

    let zip_bytes = b.zip(&["recordings/op-1.cast".to_string()]).unwrap();
    let files = zip::read_back(&zip_bytes).unwrap();
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["manifest.json", "README.txt", "version.json", "settings.json", "app-log.txt"]);
    for (name, data) in &files {
        let text = String::from_utf8_lossy(data);
        assert!(!text.contains("BundlePlanted") && !text.contains("SettingsPlanted"), "{name}: {text}");
        assert!(!text.to_lowercase().contains("ren-pc"), "{name} names the user: {text}");
    }
    let settings: Value = serde_json::from_slice(&files[3].1).unwrap();
    assert_eq!(settings["theme"], "dark");
    assert_eq!(settings["anthropic_api_key"], "[removed]");
    assert_eq!(settings["nested"]["access_token"], "[removed]");
    assert_eq!(settings["nested"]["away_minutes"], 5);

    // With the recording left in, it is there and still redacted.
    let full = zip::read_back(&b.zip(&[]).unwrap()).unwrap();
    let cast = full.iter().find(|(n, _)| n == "recordings/op-1.cast").expect("recording included");
    let (_, events) = parse_cast(&String::from_utf8_lossy(&cast.1));
    assert!(!events[0][2].as_str().unwrap().contains("BundlePlanted"));
}

#[test]
fn the_app_log_tail_keeps_the_newest_lines() {
    use std::io::Write;
    for i in 0..5 {
        applog::TeeWriter.write_all(format!("\u{1b}[32mINFO\u{1b}[0m line {i}\n").as_bytes()).unwrap();
    }
    let tail = applog::tail();
    assert!(tail.iter().any(|l| l == "INFO line 4"), "ansi stripped, newest kept: {:?}", &tail[tail.len().saturating_sub(3)..]);
}

/// Cost of the reader hook. Prints the numbers (run with `--nocapture`); the bound is deliberately
/// loose because this runs in a debug build next to other tests.
#[test]
fn recording_overhead_per_chunk_is_small_next_to_the_work_the_reader_already_does() {
    let chunk: Vec<u8> = (0..4096u32).map(|i| b'a' + (i % 26) as u8).collect();
    let small: Vec<u8> = chunk[..80].to_vec();
    let n = 20_000u32;

    let ring = Ring::new(120, 30);
    let t = Instant::now();
    for _ in 0..n {
        ring.output(&chunk);
    }
    let ring_4k = t.elapsed().as_nanos() as f64 / f64::from(n);

    let ring = Ring::new(120, 30);
    let t = Instant::now();
    for _ in 0..n {
        ring.output(&small);
    }
    let ring_80 = t.elapsed().as_nanos() as f64 / f64::from(n);

    // What the reader does with each chunk anyway: feed the screen emulator.
    let mut parser = vt100::Parser::new(30, 120, 1000);
    let t = Instant::now();
    for _ in 0..n {
        parser.process(&chunk);
    }
    let vt_4k = t.elapsed().as_nanos() as f64 / f64::from(n);

    println!("flight ring: {ring_4k:.0} ns per 4096-byte chunk, {ring_80:.0} ns per 80-byte chunk; vt100 emulator: {vt_4k:.0} ns per 4096-byte chunk; ring/emulator = {:.1}%", ring_4k / vt_4k * 100.0);
    assert!(ring_4k < 50_000.0, "ring too slow: {ring_4k} ns");
    assert!(ring_4k < vt_4k, "recording must cost less than the emulator work next to it");
}

/// Evidence helper: with `PM_FLIGHT_SAMPLE_DIR` set, writes a sample diagnostic zip and cast there so
/// they can be opened with real tools (Expand-Archive, an asciinema player).
#[test]
fn sample_files_for_real_tools() {
    let Ok(dir) = std::env::var("PM_FLIGHT_SAMPLE_DIR") else { return };
    let ring = Ring::new(100, 30);
    ring.output(b"\x1b[1mClaude\x1b[0m> ");
    ring.input(InputTag::RunTask, b"fix the bug\r");
    ring.output(b"working...\r\n");
    ring.mark("readiness=Unknown(blank)");
    let cast = cast::render(&ring.snapshot(), "PROMPT_NOT_SUBMITTED", Some("run failed: PROMPT_NOT_SUBMITTED"));
    std::fs::write(std::path::Path::new(&dir).join("sample.cast"), cast.as_str()).unwrap();
    let b = bundle::build(bundle_inputs(
        vec![bundle::CastFile { operation_id: "op-1".into(), modified_ms: 9_000_000, text: cast.as_str().to_string() }],
        vec!["INFO started".into()],
    ));
    bundle::save(&b, &[], &std::path::Path::new(&dir).join("sample-diagnostic.zip")).unwrap();
}
