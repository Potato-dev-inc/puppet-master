use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::OnceLock;
use uuid::Uuid;

const MAX_TRANSCRIPT_BYTES: usize = 256 * 1024;
const MAX_RETURN_CHUNKS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptChunk {
    pub sequence: u64,
    pub stream: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptPage {
    pub chunks: Vec<TranscriptChunk>,
    pub next_cursor: u64,
    pub truncated: bool,
}

#[derive(Default)]
struct Buffer {
    next: u64,
    bytes: usize,
    chunks: Vec<TranscriptChunk>,
}

static BUFFERS: OnceLock<Mutex<HashMap<String, Buffer>>> = OnceLock::new();

fn buffers() -> &'static Mutex<HashMap<String, Buffer>> {
    BUFFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn transcript_path(project: &str, operation_id: &str) -> PathBuf {
    PathBuf::from(project)
        .join(".puppet-master")
        .join("operations")
        .join(format!("{operation_id}.transcript.jsonl"))
}

pub fn append(project: &str, operation_id: &str, stream: &str, text: &str) {
    if text.is_empty() {
        return;
    }
    let key = format!("{project}\n{operation_id}");
    let path = transcript_path(project, operation_id);
    let _ = buffers();
    let mut all = buffers().lock();
    let buffer = all.entry(key).or_insert_with(|| Buffer {
        next: last_sequence(&path),
        ..Buffer::default()
    });
    let chunk = TranscriptChunk {
        sequence: buffer.next.saturating_add(1),
        stream: stream.to_string(),
        text: text.to_string(),
    };
    buffer.next = chunk.sequence;
    buffer.bytes = buffer.bytes.saturating_add(chunk.text.len());
    buffer.chunks.push(chunk.clone());
    while buffer.bytes > MAX_TRANSCRIPT_BYTES && buffer.chunks.len() > 1 {
        if let Some(removed) = buffer.chunks.first() {
            buffer.bytes = buffer.bytes.saturating_sub(removed.text.len());
        }
        buffer.chunks.remove(0);
    }
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    if serde_json::to_writer(&mut file, &chunk).is_ok() {
        let _ = file.write_all(b"\n");
    }
    let _ = compact_if_needed(&path);
}

fn compact_if_needed(path: &PathBuf) -> std::io::Result<()> {
    let metadata = fs::metadata(path)?;
    if metadata.len() <= (MAX_TRANSCRIPT_BYTES * 2) as u64 {
        return Ok(());
    }
    // Keep the on-disk file bounded. In-memory readers retain sequence and truncation state.
    let file = fs::File::open(path)?;
    let mut records = BufReader::new(file)
        .lines()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    let mut bytes = records.iter().map(String::len).sum::<usize>();
    while records.len() > MAX_RETURN_CHUNKS || bytes > MAX_TRANSCRIPT_BYTES {
        if let Some(record) = records.first() {
            bytes = bytes.saturating_sub(record.len());
        }
        records.remove(0);
    }
    let temp = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    {
        let mut output = fs::File::create(&temp)?;
        for record in records {
            output.write_all(record.as_bytes())?;
            output.write_all(b"\n")?;
        }
        output.flush()?;
    }
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temp, path)
}

fn last_sequence(path: &PathBuf) -> u64 {
    fs::File::open(path)
        .ok()
        .map(BufReader::new)
        .into_iter()
        .flat_map(|reader| reader.lines().filter_map(Result::ok))
        .filter_map(|line| serde_json::from_str::<TranscriptChunk>(&line).ok())
        .map(|chunk| chunk.sequence)
        .max()
        .unwrap_or(0)
}

pub fn read(project: &str, operation_id: &str, after: u64) -> std::io::Result<TranscriptPage> {
    let path = transcript_path(project, operation_id);
    if !path.exists() {
        return Ok(TranscriptPage {
            chunks: Vec::new(),
            next_cursor: after,
            truncated: false,
        });
    }
    let file = fs::File::open(&path)?;
    let mut chunks = BufReader::new(file)
        .lines()
        .filter_map(Result::ok)
        .filter_map(|line| serde_json::from_str::<TranscriptChunk>(&line).ok())
        .collect::<Vec<_>>();
    let next_cursor = chunks
        .last()
        .map_or(after, |chunk| chunk.sequence.max(after));
    let mut truncated = chunks
        .first()
        .is_some_and(|chunk| chunk.sequence > after.saturating_add(1));
    chunks.retain(|chunk| chunk.sequence > after);
    let mut bytes = chunks.iter().map(|chunk| chunk.text.len()).sum::<usize>();
    while chunks.len() > MAX_RETURN_CHUNKS || bytes > MAX_TRANSCRIPT_BYTES {
        if let Some(chunk) = chunks.first() {
            bytes = bytes.saturating_sub(chunk.text.len());
        }
        chunks.remove(0);
        truncated = true;
    }
    Ok(TranscriptPage {
        chunks,
        next_cursor,
        truncated,
    })
}

pub fn read_turns(
    project: &str,
    operation_ids: &[String],
    after: u64,
) -> std::io::Result<TranscriptPage> {
    let mut chunks = Vec::new();
    let mut sequence = 0_u64;
    for operation_id in operation_ids {
        let page = read(project, operation_id, 0)?;
        for chunk in page.chunks {
            sequence = sequence.saturating_add(1);
            chunks.push(TranscriptChunk {
                sequence,
                stream: chunk.stream,
                text: chunk.text,
            });
        }
    }
    let next_cursor = sequence.max(after);
    let mut truncated = chunks
        .first()
        .is_some_and(|chunk| chunk.sequence > after.saturating_add(1));
    chunks.retain(|chunk| chunk.sequence > after);
    let mut bytes = chunks.iter().map(|chunk| chunk.text.len()).sum::<usize>();
    while chunks.len() > MAX_RETURN_CHUNKS || bytes > MAX_TRANSCRIPT_BYTES {
        if let Some(chunk) = chunks.first() {
            bytes = bytes.saturating_sub(chunk.text.len());
        }
        chunks.remove(0);
        truncated = true;
    }
    Ok(TranscriptPage {
        chunks,
        next_cursor,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_pages_advance_cursor_and_report_gaps() {
        let dir = std::env::temp_dir().join(format!("pm-transcript-{}", Uuid::new_v4()));
        for n in 0..(MAX_RETURN_CHUNKS + 3) {
            append(dir.to_str().unwrap(), "op-1", "stdout", &format!("{n}\n"));
        }
        let page = read(dir.to_str().unwrap(), "op-1", 0).unwrap();
        assert_eq!(page.chunks.len(), MAX_RETURN_CHUNKS);
        assert!(page.truncated);
        assert_eq!(page.next_cursor, (MAX_RETURN_CHUNKS + 3) as u64);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn worker_transcript_merges_turns_in_order() {
        let dir = std::env::temp_dir().join(format!("pm-transcript-{}", Uuid::new_v4()));
        let project = dir.to_str().unwrap();
        append(project, "op-a", "user_task", "first");
        append(project, "op-b", "stdout", "second");
        let page = read_turns(
            project,
            &["op-a".into(), "op-b".into(), "op-missing".into()],
            0,
        )
        .unwrap();
        assert_eq!(
            page.chunks
                .iter()
                .map(|chunk| chunk.text.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        assert_eq!(page.next_cursor, 2);
        let later = read_turns(project, &["op-a".into(), "op-b".into()], 1).unwrap();
        assert_eq!(later.chunks.len(), 1);
        assert_eq!(later.chunks[0].text, "second");
        let _ = fs::remove_dir_all(dir);
    }
}
