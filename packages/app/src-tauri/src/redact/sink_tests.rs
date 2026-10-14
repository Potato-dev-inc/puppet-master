//! The grep test: plant known secrets (credential store, environment, and a token shape), run every
//! code path that saves or sends text, then search everything those paths produced.

use crate::secrets::{backend, SecretBackend};
use std::path::Path;

const KEYRING_SECRET: &str = "plain-keyring-secret-ZXCV1234";
const ENV_SECRET: &str = "plain-env-secret-QWER5678";
const SHAPE_SECRET: &str = "sk-ant-api03-GrepTestSecret0123456789";
const NEEDLES: [&str; 3] = ["ZXCV1234", "QWER5678", "GrepTestSecret"];

fn every_file(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            every_file(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn leaked(label: &str, text: &str) -> Vec<String> {
    NEEDLES.iter().filter(|n| text.contains(**n)).map(|n| format!("{label} contains {n}")).collect()
}

fn dirty_text() -> String {
    format!("deploy with {KEYRING_SECRET} and {ENV_SECRET} and token {SHAPE_SECRET} done")
}

#[test]
fn a_planted_secret_is_absent_from_every_sink() {
    backend().set("anthropic_api_key", KEYRING_SECRET).unwrap();
    std::env::set_var("PM_SINK_GREP_SERVICE_TOKEN", ENV_SECRET);

    let dir = std::env::temp_dir().join(format!("pm-sink-grep-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let project = dir.to_string_lossy().into_owned();
    let mut found: Vec<String> = Vec::new();

    // 1. transcript
    crate::agent_runs::transcript::append(&project, "op-grep", "stdout", &crate::redact::text(&dirty_text()));
    let page = crate::agent_runs::transcript::read(&project, "op-grep", 0).unwrap();
    found.extend(leaked("transcript page", &serde_json::to_string(&page).unwrap()));

    // 2. turn summary ring file
    {
        let mut summary = crate::turn_summary::model::assemble(crate::turn_summary::model::Facts {
            pane_id: "p-grep".into(),
            agent: "Agent".into(),
            turn: 1,
            started_at_ms: 1,
            ended_at_ms: 2_000,
            files: None,
            approvals: vec![crate::turn_summary::model::Approval {
                at_ms: 1,
                prompt_id: "pid".into(),
                prompt: dirty_text(),
                kind: "command".into(),
                decision: "allow".into(),
            }],
            end_status: "waiting_input".into(),
            end_prompt: Some(dirty_text()),
            receipt: None,
        });
        summary.sentence.push_str(&dirty_text());
        let mut store = crate::turn_summary::store::Store::default();
        store.record(&dir, crate::redact::scrub(summary)).unwrap();
    }

    // 3. flight recording: frozen file and export
    let pane = format!("grep-pane-{}", uuid::Uuid::new_v4());
    let ring = crate::flight::start(&pane, 80, 24);
    ring.output(dirty_text().as_bytes());
    ring.input(crate::flight::InputTag::User, dirty_text().as_bytes());
    ring.mark(&dirty_text());
    crate::flight::freeze::freeze_now(&project, "op-grep", &pane, "GREP").unwrap();
    let (export, _) = crate::flight::freeze::export(&pane, &dirty_text()).unwrap();
    found.extend(leaked("flight export", export.as_str()));

    // 4. diagnostic file
    let bundle = crate::flight::bundle::build(crate::flight::bundle::gather(
        &project,
        serde_json::json!({"theme": "dark", "note": dirty_text(), "anthropic_api_key": KEYRING_SECRET}),
    ));
    for (name, data) in crate::flight::zip::read_back(&bundle.zip(&[]).unwrap()).unwrap() {
        found.extend(leaked(&format!("diagnostic file {name}"), &String::from_utf8_lossy(&data)));
    }

    // 5. what a phone is sent
    let frame = format!("event: operation\ndata: {}\n\n", serde_json::json!({"result": dirty_text()}));
    found.extend(leaked("phone event", &crate::redact::fanout::sse_frame(frame)));

    // Everything that landed on disk.
    let mut files = Vec::new();
    every_file(&dir, &mut files);
    assert!(files.len() >= 3, "the paths wrote files: {files:?}");
    for path in files {
        found.extend(leaked(&path.display().to_string(), &String::from_utf8_lossy(&std::fs::read(&path).unwrap())));
    }

    // Control: the ring itself (the live terminal) still holds the real text, so the search can find it.
    let snap = ring.snapshot();
    let raw: Vec<u8> = snap.events.iter().flat_map(|e| snap.data(e).to_vec()).collect();
    assert!(!leaked("live ring", &String::from_utf8_lossy(&raw)).is_empty(), "the control finds the secret");

    backend().delete("anthropic_api_key").unwrap();
    std::env::remove_var("PM_SINK_GREP_SERVICE_TOKEN");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(found.is_empty(), "secrets leaked: {found:#?}");
}
