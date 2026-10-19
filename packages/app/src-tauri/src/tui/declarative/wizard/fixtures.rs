//! Fixture files the wizard writes (SPEC 5.2): the live outputs as the agent printed them, each with
//! an `.expect.toml`, plus a generated garbage-text fixture. Names and layout match what
//! `check::check_dir` reads and what the hand-written Kilo fixtures use.

use std::path::Path;

use crate::tui::declarative::schema::OutputFormat;
use crate::tui::declarative::DeclarativeProfile;
use crate::tui::TuiProfile;

use super::session::{Sample, Stage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureFile {
    pub name: String,
    pub text: String,
}

/// `jsonl` for an event stream, `json` for one object, `txt` for plain text.
pub fn extension(format: OutputFormat) -> &'static str {
    match format {
        OutputFormat::JsonLines => "jsonl",
        OutputFormat::JsonSingle => "json",
        OutputFormat::PlainText => "txt",
    }
}

fn quote(text: &str) -> String {
    toml::Value::String(text.to_string()).to_string()
}

/// What a parser must make of text that is not agent output at all.
pub const GARBAGE_TEXT: &str = "this is not agent output\nplain words, no braces, no events\n";

pub fn garbage_files() -> Vec<FixtureFile> {
    vec![
        FixtureFile { name: "headless_garbage.txt".into(), text: GARBAGE_TEXT.into() },
        FixtureFile {
            name: "headless_garbage.expect.toml".into(),
            text: "kind = \"headless\"\nresult = \"\"\nsession = \"\"\ndone = false\nerror_code = \"AGENT_OUTPUT_UNPARSEABLE\"\n"
                .into(),
        },
    ]
}

/// The capture and expectation files for passed live-test samples. Expectations record what the
/// draft's parser returned; the stage checks (`livetest::judge`) already proved those values are the
/// right ones (pong, pong2, or an error), so the fixtures pin them for the next time the agent changes.
pub fn sample_files(profile: &DeclarativeProfile, format: OutputFormat, samples: &[Sample]) -> Vec<FixtureFile> {
    let ext = extension(format);
    let has_session = profile
        .definition()
        .headless
        .as_ref()
        .and_then(|h| h.output.as_ref())
        .is_some_and(|o| o.session.is_some());
    let mut files = Vec::new();
    for sample in samples {
        let stem = sample.stage.fixture_stem();
        let exit = sample.exit.unwrap_or(0);
        let mut expect = String::new();
        match sample.stage {
            Stage::Pong | Stage::Sentence | Stage::Resume => {
                let result = profile.extract_result(&sample.stdout, exit).unwrap_or_default();
                expect.push_str(&format!("result = {}\n", quote(&result)));
                if has_session {
                    let session = profile.extract_provider_session(&sample.stdout).unwrap_or_default();
                    expect.push_str(&format!("session = {}\n", quote(&session)));
                }
                if let Some(model) = profile.extract_model(&sample.stdout) {
                    expect.push_str(&format!("model = {}\n", quote(&model)));
                }
                expect.push_str(&format!("done = {}\n", profile.headless_result_ready(&sample.stdout)));
            }
            Stage::BadModel => {
                expect.push_str("result = \"\"\n");
                if exit != 0 {
                    expect.push_str(&format!("exit = {exit}\n"));
                }
                if let Some((code, message)) = profile.classify_headless_failure(&sample.stdout, exit) {
                    expect.push_str(&format!("error_code = {}\nerror = {}\n", quote(code), quote(&message)));
                }
            }
        }
        files.push(FixtureFile { name: format!("{stem}.{ext}"), text: sample.stdout.clone() });
        files.push(FixtureFile { name: format!("{stem}.expect.toml"), text: expect });
    }
    files
}

/// Everything `finish` writes into the fixtures folder: the samples, then the garbage case.
pub fn final_files(profile: &DeclarativeProfile, format: OutputFormat, samples: &[Sample]) -> Vec<FixtureFile> {
    let mut files = sample_files(profile, format, samples);
    files.extend(garbage_files());
    files
}

/// Write `files` into `dir` (created if needed). Existing files are never replaced.
pub fn write_all(dir: &Path, files: &[FixtureFile]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    for file in files {
        let path = dir.join(&file.name);
        let mut handle = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        // the last net (SPEC 11.21): no email address reaches a fixture file, expectation files included, so the pair
        // stays consistent (`check_dir` reads `<email>` back as an example address)
        std::io::Write::write_all(&mut handle, super::exec::mask_private(&file.text).as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}
