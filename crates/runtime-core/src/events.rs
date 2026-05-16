//! Defines the durable runtime event stream shared by the runtime, shell,
//! and desktop UI bridge.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use runtime_store::StorageLayout;
use serde::{Deserialize, Serialize};

static RUNTIME_EVENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Stores one durable runtime event emitted into the append-only JSONL stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeEventRecord {
    pub event_id: String,
    pub occurred_at_unix_millis: u64,
    pub topic: String,
    pub severity: String,
    pub origin: String,
    pub user_requested: bool,
    pub repository_id: Option<i64>,
    pub release_run_id: Option<i64>,
    pub build_run_id: Option<i64>,
    pub publish_run_id: Option<i64>,
    pub summary: String,
    pub payload: serde_json::Value,
}

/// Describes one new runtime event before the durable identifiers are assigned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEventInput {
    pub topic: String,
    pub severity: String,
    pub origin: String,
    pub user_requested: bool,
    pub repository_id: Option<i64>,
    pub release_run_id: Option<i64>,
    pub build_run_id: Option<i64>,
    pub publish_run_id: Option<i64>,
    pub summary: String,
    pub payload: serde_json::Value,
}

/// Returns the parsed runtime events available after the provided byte offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEventBatch {
    pub next_offset: u64,
    pub events: Vec<RuntimeEventRecord>,
}

/// Appends one durable runtime event to the JSONL event stream.
pub fn emit_runtime_event(
    storage: &StorageLayout,
    input: RuntimeEventInput,
) -> io::Result<RuntimeEventRecord> {
    let event = RuntimeEventRecord {
        event_id: next_runtime_event_id()?,
        occurred_at_unix_millis: unix_timestamp_millis()?,
        topic: input.topic,
        severity: input.severity,
        origin: input.origin,
        user_requested: input.user_requested,
        repository_id: input.repository_id,
        release_run_id: input.release_run_id,
        build_run_id: input.build_run_id,
        publish_run_id: input.publish_run_id,
        summary: input.summary,
        payload: input.payload,
    };
    append_runtime_event(&storage.runtime_events_path, &event)?;
    Ok(event)
}

/// Appends a pre-built runtime event record to the JSONL stream.
pub fn append_runtime_event(path: &Path, event: &RuntimeEventRecord) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let line = serde_json::to_string(event).map_err(json_error)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

/// Reads the complete runtime events available after the provided byte offset.
pub fn read_runtime_event_batch(path: &Path, offset: u64) -> io::Result<RuntimeEventBatch> {
    let mut file = match OpenOptions::new().read(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(RuntimeEventBatch {
                next_offset: 0,
                events: Vec::new(),
            });
        }
        Err(error) => return Err(error),
    };

    let file_length = file.metadata()?.len();
    let normalized_offset = offset.min(file_length);
    file.seek(SeekFrom::Start(normalized_offset))?;

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    let complete_length = buffer
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .unwrap_or(0);

    let next_offset = normalized_offset + complete_length as u64;
    let mut events = Vec::new();
    for line in buffer[..complete_length]
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let event = serde_json::from_slice::<RuntimeEventRecord>(line).map_err(json_error)?;
        events.push(event);
    }

    Ok(RuntimeEventBatch {
        next_offset,
        events,
    })
}

fn next_runtime_event_id() -> io::Result<String> {
    let sequence = RUNTIME_EVENT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    Ok(format!(
        "evt_{}_{}_{}",
        unix_timestamp_millis()?,
        process::id(),
        sequence,
    ))
}

fn unix_timestamp_millis() -> io::Result<u64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    let millis = duration.as_millis();
    Ok(millis.min(u64::MAX as u128) as u64)
}

fn json_error(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use runtime_config::RuntimeDirectories;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn runtime_event_stream_appends_and_reads_complete_batches() {
        let root = test_root("append-and-read");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);

        let first = emit_runtime_event(
            &storage,
            RuntimeEventInput {
                topic: String::from("build.run_started"),
                severity: String::from("info"),
                origin: String::from("runtime-bin"),
                user_requested: false,
                repository_id: Some(7),
                release_run_id: Some(11),
                build_run_id: Some(13),
                publish_run_id: None,
                summary: String::from("Automatic build started"),
                payload: serde_json::json!({ "status": "running" }),
            },
        )
        .expect("first runtime event should append");
        let second = emit_runtime_event(
            &storage,
            RuntimeEventInput {
                topic: String::from("build.run_finished"),
                severity: String::from("info"),
                origin: String::from("runtime-bin"),
                user_requested: false,
                repository_id: Some(7),
                release_run_id: Some(11),
                build_run_id: Some(13),
                publish_run_id: None,
                summary: String::from("Automatic build finished"),
                payload: serde_json::json!({ "status": "succeeded" }),
            },
        )
        .expect("second runtime event should append");

        let batch = read_runtime_event_batch(&storage.runtime_events_path, 0)
            .expect("runtime event batch should read");

        assert_eq!(batch.events, vec![first, second]);
        assert!(batch.next_offset > 0);
    }

    #[test]
    fn runtime_event_stream_ignores_incomplete_trailing_lines() {
        let root = test_root("ignore-incomplete-tail");
        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);

        emit_runtime_event(
            &storage,
            RuntimeEventInput {
                topic: String::from("automation.release_queued"),
                severity: String::from("info"),
                origin: String::from("runtime-bin"),
                user_requested: true,
                repository_id: Some(3),
                release_run_id: Some(5),
                build_run_id: None,
                publish_run_id: None,
                summary: String::from("Release queued"),
                payload: serde_json::json!({ "status": "queued" }),
            },
        )
        .expect("runtime event should append");

        let mut file = OpenOptions::new()
            .append(true)
            .open(&storage.runtime_events_path)
            .expect("event stream should be appendable");
        write!(file, "{{\"event_id\":\"partial")
            .expect("partial line should be written");

        let batch = read_runtime_event_batch(&storage.runtime_events_path, 0)
            .expect("runtime event batch should read complete lines");

        assert_eq!(batch.events.len(), 1);

        let file_length = fs::metadata(&storage.runtime_events_path)
            .expect("event stream metadata should load")
            .len();
        assert!(batch.next_offset < file_length);
    }

    fn test_root(suffix: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "hgp-runtime-core-events-{}-{}",
            suffix,
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("test root should be created");
        path
    }
}