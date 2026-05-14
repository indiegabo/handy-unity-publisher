//! Bridges the durable runtime event stream into Tauri window events with a
//! persisted read cursor owned by the desktop shell.

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::Path;
use std::thread;
use std::time::Duration;

use runtime_core::{read_runtime_event_batch, RuntimeEventRecord};
use runtime_store::StorageLayout;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_notification::{NotificationExt, PermissionState};

use crate::MAIN_WINDOW_LABEL;

const RUNTIME_EVENT_NAME: &str = "runtime:event";
const RUNTIME_EVENT_RELAY_POLL_INTERVAL: Duration = Duration::from_millis(150);
const RECENT_EVENT_ID_LIMIT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeNotificationPolicy {
    Always,
    WhenWindowHidden,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct RuntimeEventCursor {
    pub(crate) offset: u64,
    pub(crate) last_event_id: Option<String>,
}

pub(crate) fn start_runtime_event_bridge<R: Runtime>(
    app_handle: AppHandle<R>,
    storage: StorageLayout,
) {
    thread::spawn(move || {
        let mut cursor = match load_or_seed_runtime_event_cursor(
            &storage.runtime_events_path,
            &storage.runtime_events_cursor_path,
        ) {
            Ok(cursor) => cursor,
            Err(error) => {
                eprintln!("failed to initialize runtime event relay cursor: {error}");
                RuntimeEventCursor::default()
            }
        };
        let mut recent_event_ids = VecDeque::with_capacity(RECENT_EVENT_ID_LIMIT);

        loop {
            if let Err(error) = relay_pending_runtime_events(
                &app_handle,
                &storage.runtime_events_path,
                &storage.runtime_events_cursor_path,
                &mut cursor,
                &mut recent_event_ids,
            ) {
                eprintln!("failed to relay runtime events: {error}");
            }

            thread::sleep(RUNTIME_EVENT_RELAY_POLL_INTERVAL);
        }
    });
}

fn relay_pending_runtime_events<R: Runtime>(
    app_handle: &AppHandle<R>,
    events_path: &Path,
    cursor_path: &Path,
    cursor: &mut RuntimeEventCursor,
    recent_event_ids: &mut VecDeque<String>,
) -> io::Result<()> {
    let batch = read_runtime_event_batch(events_path, cursor.offset)?;
    if batch.next_offset != cursor.offset && batch.events.is_empty() {
        cursor.offset = batch.next_offset;
        persist_runtime_event_cursor(cursor_path, cursor)?;
        return Ok(());
    }

    let mut last_seen_event_id = None;
    for event in batch.events {
        last_seen_event_id = Some(event.event_id.clone());
        if !should_relay_runtime_event(cursor, recent_event_ids, &event) {
            continue;
        }

        app_handle
            .emit_to(MAIN_WINDOW_LABEL, RUNTIME_EVENT_NAME, event.clone())
            .map_err(|error| io::Error::other(error.to_string()))?;
        if let Err(error) = maybe_notify_native_runtime_event(app_handle, &event) {
            eprintln!("failed to show native notification for {}: {error}", event.event_id);
        }
        remember_runtime_event_id(recent_event_ids, &event.event_id);
    }

    if batch.next_offset != cursor.offset {
        cursor.offset = batch.next_offset;
        if last_seen_event_id.is_some() {
            cursor.last_event_id = last_seen_event_id;
        }
        persist_runtime_event_cursor(cursor_path, cursor)?;
    }

    Ok(())
}

fn should_relay_runtime_event(
    cursor: &RuntimeEventCursor,
    recent_event_ids: &VecDeque<String>,
    event: &RuntimeEventRecord,
) -> bool {
    if cursor.last_event_id.as_deref() == Some(event.event_id.as_str()) {
        return false;
    }

    !recent_event_ids
        .iter()
        .any(|event_id| event_id == &event.event_id)
}

fn remember_runtime_event_id(recent_event_ids: &mut VecDeque<String>, event_id: &str) {
    if recent_event_ids.len() == RECENT_EVENT_ID_LIMIT {
        recent_event_ids.pop_front();
    }
    recent_event_ids.push_back(event_id.to_owned());
}

fn maybe_notify_native_runtime_event<R: Runtime>(
    app_handle: &AppHandle<R>,
    event: &RuntimeEventRecord,
) -> io::Result<()> {
    let Some(policy) = native_notification_policy(event) else {
        return Ok(());
    };

    let main_window_visible = is_main_window_visible(app_handle);
    if !should_show_native_notification(policy, main_window_visible) {
        return Ok(());
    }

    if !ensure_notification_permission(app_handle)? {
        return Ok(());
    }

    let (title, body) = native_notification_content(event);
    app_handle
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|error| io::Error::other(error.to_string()))
}

fn native_notification_policy(event: &RuntimeEventRecord) -> Option<NativeNotificationPolicy> {
    if event.user_requested {
        return None;
    }

    match event.topic.as_str() {
        "automation.poll_auth_failed" => Some(NativeNotificationPolicy::Always),
        "build.run_started" => Some(NativeNotificationPolicy::Always),
        "build.run_finished" => Some(NativeNotificationPolicy::WhenWindowHidden),
        _ => None,
    }
}

fn should_show_native_notification(
    policy: NativeNotificationPolicy,
    main_window_visible: bool,
) -> bool {
    match policy {
        NativeNotificationPolicy::Always => true,
        NativeNotificationPolicy::WhenWindowHidden => !main_window_visible,
    }
}

fn native_notification_content(event: &RuntimeEventRecord) -> (String, String) {
    let title = match event.topic.as_str() {
        "automation.poll_auth_failed" => String::from("Repository polling stopped"),
        "build.run_started" => String::from("Automatic build started"),
        "build.run_finished" => match build_status_from_event(event) {
            Some("failed") => String::from("Automatic build failed"),
            Some("canceled") | Some("cancelled") => String::from("Automatic build canceled"),
            _ => String::from("Automatic build finished"),
        },
        _ => String::from("HUP build update"),
    };

    (title, event.summary.clone())
}

fn build_status_from_event(event: &RuntimeEventRecord) -> Option<&str> {
    event.payload.get("status").and_then(serde_json::Value::as_str)
}

fn is_main_window_visible<R: Runtime>(app_handle: &AppHandle<R>) -> bool {
    app_handle
        .get_webview_window(MAIN_WINDOW_LABEL)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false)
}

fn ensure_notification_permission<R: Runtime>(app_handle: &AppHandle<R>) -> io::Result<bool> {
    let notification = app_handle.notification();
    let permission = notification
        .permission_state()
        .map_err(|error| io::Error::other(error.to_string()))?;

    match permission {
        PermissionState::Granted => Ok(true),
        PermissionState::Denied => Ok(false),
        PermissionState::Prompt | PermissionState::PromptWithRationale => notification
            .request_permission()
            .map(|state| state == PermissionState::Granted)
            .map_err(|error| io::Error::other(error.to_string())),
    }
}

fn load_or_seed_runtime_event_cursor(
    events_path: &Path,
    cursor_path: &Path,
) -> io::Result<RuntimeEventCursor> {
    match fs::read_to_string(cursor_path) {
        Ok(content) => match serde_json::from_str::<RuntimeEventCursor>(&content) {
            Ok(cursor) => Ok(cursor),
            Err(_) => seed_runtime_event_cursor(events_path, cursor_path),
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            seed_runtime_event_cursor(events_path, cursor_path)
        }
        Err(error) => Err(error),
    }
}

fn seed_runtime_event_cursor(
    events_path: &Path,
    cursor_path: &Path,
) -> io::Result<RuntimeEventCursor> {
    let cursor = RuntimeEventCursor {
        offset: current_event_stream_len(events_path)?,
        last_event_id: None,
    };
    persist_runtime_event_cursor(cursor_path, &cursor)?;
    Ok(cursor)
}

fn persist_runtime_event_cursor(
    cursor_path: &Path,
    cursor: &RuntimeEventCursor,
) -> io::Result<()> {
    if let Some(parent) = cursor_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_vec_pretty(cursor).map_err(io::Error::other)?;
    fs::write(cursor_path, content)
}

fn current_event_stream_len(events_path: &Path) -> io::Result<u64> {
    match fs::metadata(events_path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use runtime_config::RuntimeDirectories;
    use runtime_core::{emit_runtime_event, RuntimeEventInput};

    fn test_storage_layout(suffix: &str) -> StorageLayout {
        let root = std::env::temp_dir().join(format!("desktop-shell-runtime-events-{suffix}"));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }
        let directories = RuntimeDirectories::from_root(&root);
        StorageLayout::from_directories(&directories)
    }

    #[test]
    fn load_or_seed_runtime_event_cursor_starts_at_end_of_stream() {
        let storage = test_storage_layout("seed-cursor");
        emit_runtime_event(
            &storage,
            RuntimeEventInput {
                topic: String::from("build.run_started"),
                severity: String::from("info"),
                origin: String::from("runtime-bin"),
                user_requested: false,
                repository_id: Some(1),
                release_run_id: Some(2),
                build_run_id: Some(3),
                publish_run_id: None,
                summary: String::from("Build started"),
                payload: serde_json::json!({ "status": "running" }),
            },
        )
        .expect("runtime event should append");

        let cursor = load_or_seed_runtime_event_cursor(
            &storage.runtime_events_path,
            &storage.runtime_events_cursor_path,
        )
        .expect("cursor should seed from the runtime event stream");

        assert_eq!(
            cursor.offset,
            fs::metadata(&storage.runtime_events_path)
                .expect("event stream metadata should load")
                .len(),
        );
        assert!(storage.runtime_events_cursor_path.exists());
    }

    #[test]
    fn should_relay_runtime_event_skips_recent_duplicates() {
        let cursor = RuntimeEventCursor {
            offset: 0,
            last_event_id: Some(String::from("evt_last")),
        };
        let mut recent_event_ids = VecDeque::new();
        recent_event_ids.push_back(String::from("evt_recent"));

        assert!(!should_relay_runtime_event(
            &cursor,
            &recent_event_ids,
            &RuntimeEventRecord {
                event_id: String::from("evt_last"),
                occurred_at_unix_millis: 1,
                topic: String::from("build.run_started"),
                severity: String::from("info"),
                origin: String::from("runtime-bin"),
                user_requested: false,
                repository_id: Some(1),
                release_run_id: Some(2),
                build_run_id: Some(3),
                publish_run_id: None,
                summary: String::from("duplicate"),
                payload: serde_json::json!({}),
            },
        ));
        assert!(!should_relay_runtime_event(
            &cursor,
            &recent_event_ids,
            &RuntimeEventRecord {
                event_id: String::from("evt_recent"),
                occurred_at_unix_millis: 1,
                topic: String::from("build.run_started"),
                severity: String::from("info"),
                origin: String::from("runtime-bin"),
                user_requested: false,
                repository_id: Some(1),
                release_run_id: Some(2),
                build_run_id: Some(3),
                publish_run_id: None,
                summary: String::from("duplicate"),
                payload: serde_json::json!({}),
            },
        ));
        assert!(should_relay_runtime_event(
            &cursor,
            &recent_event_ids,
            &RuntimeEventRecord {
                event_id: String::from("evt_new"),
                occurred_at_unix_millis: 1,
                topic: String::from("build.run_started"),
                severity: String::from("info"),
                origin: String::from("runtime-bin"),
                user_requested: false,
                repository_id: Some(1),
                release_run_id: Some(2),
                build_run_id: Some(3),
                publish_run_id: None,
                summary: String::from("new"),
                payload: serde_json::json!({}),
            },
        ));
    }

    #[test]
    fn native_notification_policy_targets_automatic_builds_and_poll_auth_failures() {
        assert_eq!(
            native_notification_policy(&test_event("automation.poll_auth_failed", false, None)),
            Some(NativeNotificationPolicy::Always)
        );
        assert_eq!(
            native_notification_policy(&test_event("build.run_started", false, None)),
            Some(NativeNotificationPolicy::Always)
        );
        assert_eq!(
            native_notification_policy(&test_event("build.run_finished", false, Some("failed"))),
            Some(NativeNotificationPolicy::WhenWindowHidden)
        );
        assert_eq!(
            native_notification_policy(&test_event("build.run_started", true, None)),
            None
        );
        assert_eq!(
            native_notification_policy(&test_event("publish.run_finished", false, None)),
            None
        );
    }

    #[test]
    fn notification_visibility_policy_matches_build_event_rules() {
        assert!(should_show_native_notification(
            NativeNotificationPolicy::Always,
            true,
        ));
        assert!(should_show_native_notification(
            NativeNotificationPolicy::Always,
            false,
        ));
        assert!(!should_show_native_notification(
            NativeNotificationPolicy::WhenWindowHidden,
            true,
        ));
        assert!(should_show_native_notification(
            NativeNotificationPolicy::WhenWindowHidden,
            false,
        ));
    }

    #[test]
    fn native_notification_content_uses_build_status_for_finished_events() {
        assert_eq!(
            native_notification_content(&test_event("automation.poll_auth_failed", false, None)).0,
            "Repository polling stopped"
        );
        assert_eq!(
            native_notification_content(&test_event("build.run_started", false, None)).0,
            "Automatic build started"
        );
        assert_eq!(
            native_notification_content(&test_event("build.run_finished", false, Some("failed"))).0,
            "Automatic build failed"
        );
        assert_eq!(
            native_notification_content(&test_event("build.run_finished", false, Some("canceled"))).0,
            "Automatic build canceled"
        );
        assert_eq!(
            native_notification_content(&test_event("build.run_finished", false, Some("succeeded"))).0,
            "Automatic build finished"
        );
    }

    fn test_event(topic: &str, user_requested: bool, status: Option<&str>) -> RuntimeEventRecord {
        RuntimeEventRecord {
            event_id: String::from("evt_test"),
            occurred_at_unix_millis: 1,
            topic: topic.to_owned(),
            severity: String::from("info"),
            origin: String::from("runtime-bin"),
            user_requested,
            repository_id: Some(1),
            release_run_id: Some(2),
            build_run_id: Some(3),
            publish_run_id: None,
            summary: String::from(
                "Automatic polling stopped for Revolutions after an authentication failure",
            ),
            payload: serde_json::json!({ "status": status }),
        }
    }
}