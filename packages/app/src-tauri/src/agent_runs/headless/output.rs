//! Reading a headless child's stdout/stderr into the run transcript (UTF-8 safe across read
//! boundaries) and reading a stream back.

use crate::agent_runs::transcript;
use crate::operations;
use std::io::Read;
use std::thread;

/// Everything the child wrote to one stream so far.
pub(super) fn stream_text(project: &str, operation_id: &str, stream: &str) -> String {
    transcript::read(project, operation_id, 0)
        .map(|page| page.chunks)
        .unwrap_or_default()
        .iter()
        .filter(|chunk| chunk.stream == stream)
        .map(|chunk| chunk.text.as_str())
        .collect()
}

pub(super) fn spawn_reader<R: Read + Send + 'static>(
    project: String,
    operation_id: String,
    stream: &'static str,
    mut reader: R,
) -> Option<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name(format!("puppet-agent-{stream}"))
        .spawn(move || {
            let mut bytes = [0u8; 8192];
            let mut pending = Vec::new();
            let mut first_output = stream == "stdout";
            while let Ok(read) = reader.read(&mut bytes) {
                if read == 0 {
                    break;
                }
                if first_output {
                    first_output = false;
                    operations::mark_operation_stage(&project, &operation_id, "first_output");
                }
                pending.extend_from_slice(&bytes[..read]);
                append_valid_utf8(&project, &operation_id, stream, &mut pending, false);
            }
            append_valid_utf8(&project, &operation_id, stream, &mut pending, true);
        })
        .ok()
}

pub(in crate::agent_runs) fn append_valid_utf8(
    project: &str,
    operation_id: &str,
    stream: &str,
    pending: &mut Vec<u8>,
    flush_incomplete: bool,
) {
    loop {
        match std::str::from_utf8(pending) {
            Ok(text) => {
                if !text.is_empty() {
                    transcript::append(project, operation_id, stream, text);
                }
                pending.clear();
                return;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if valid > 0 {
                    let text = std::str::from_utf8(&pending[..valid]).unwrap_or_default();
                    transcript::append(project, operation_id, stream, text);
                    pending.drain(..valid);
                }
                match error.error_len() {
                    Some(length) => {
                        transcript::append(project, operation_id, stream, "\u{fffd}");
                        pending.drain(..length.min(pending.len()));
                    }
                    None if flush_incomplete => {
                        if !pending.is_empty() {
                            transcript::append(project, operation_id, stream, "\u{fffd}");
                            pending.clear();
                        }
                        return;
                    }
                    None => return,
                }
            }
        }
    }
}
