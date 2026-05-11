# Runtime UI Event Delivery Plan

## Purpose

This document defines how the supervised runtime will push operational events
into the desktop shell, how the shell will relay them to the UI, and when the
shell must escalate automatic build activity into operating system
notifications.

The target behavior is:

- A repository poll detects a new tag while the app is running in background.
- The runtime emits a domain event describing the detection and queued work.
- The desktop shell relays the event to the UI when the window is open.
- The shell emits operating system notifications for automatic build start and
  build completion.
- The UI uses pushed events for low-latency updates and keeps snapshot refresh
  as a reconciliation fallback.

## Goals

- Preserve SQLite as the operational source of truth for runtime state.
- Avoid using SQLite as the primary push transport for UI event delivery.
- Keep runtime-to-shell delivery durable across shell reloads and runtime
  restarts.
- Route automatic build lifecycle notifications to the operating system when
  the user did not explicitly request the work.
- Route event payloads into the open UI with low latency and without a full
  diagnostics refresh for every change.
- Keep the desktop shell alive in the background through a tray-based lifecycle.

## Current State

- The desktop shell currently supervises the runtime process and exposes
  pull-based Tauri commands.
- The frontend currently polls the shell every 15 seconds for a full
  diagnostics snapshot.
- The shell currently exits on app close instead of hiding to tray.
- No native notification plugin is configured yet.
- The runtime already persists health, supervision, runtime logs, build stage
  logs, build execution reports, and retained log archives.

## Proposed Architecture

### 1. Durable Runtime Event Stream

Add an append-only event stream under the runtime state directory.

Suggested path:

- `state/runtime-events.jsonl`

Suggested companion cursor path owned by the shell:

- `state/runtime-events.cursor.json`

The event stream is a delivery surface, not the canonical source of truth.
SQLite remains authoritative for releases, build runs, publish runs, and
artifacts.

### 2. Event Producer In The Runtime

The runtime emits domain events at the exact points where state transitions are
already committed or intentionally about to be surfaced.

Initial event topics:

- `automation.tag_detected`
- `automation.release_queued`
- `build.run_started`
- `build.run_finished`
- `publish.run_started`
- `publish.run_finished`
- `runtime.status_changed`

Initial emission points:

- Poll detection after a new tag is accepted for release dispatch.
- Build start immediately after `start_build_run` succeeds.
- Build finish on success, failure, or cancellation.
- Publish start and finish after their persisted status transitions.
- Runtime lifecycle transitions during bootstrap, shutdown, and supervision
  failure handling.

### 3. Desktop Shell Relay

The Tauri shell owns the bridge from runtime events into:

- Tauri window events for the UI.
- Native operating system notifications.

The shell runs a lightweight background watcher that:

- tails `runtime-events.jsonl`
- keeps a durable read cursor
- deduplicates by `event_id`
- emits `runtime:event` through Tauri event APIs
- decides whether to raise a native notification

### 4. UI Consumption Model

The frontend changes from polling-only to a hybrid model:

- Load a full diagnostics snapshot at startup.
- Subscribe to `runtime:event` for incremental updates.
- Trigger targeted refreshes for affected panels when needed.
- Keep periodic snapshot refresh as reconciliation and offline recovery.

### 5. Tray-Resident App Lifecycle

The shell must stop treating window close as process exit.

Required behavior:

- Closing the main window hides it to tray.
- The shell remains alive and keeps supervising the runtime.
- The tray exposes at least `Open`, `Pause notifications`, and `Quit`.
- `Quit` performs the current full shutdown path.

## Event Contract

Suggested schema:

```json
{
  "event_id": "evt_01J...",
  "occurred_at": "2026-05-11T20:00:00Z",
  "topic": "build.run_started",
  "severity": "info",
  "origin": "runtime-bin",
  "user_requested": false,
  "repository_id": 1,
  "release_run_id": 42,
  "build_run_id": 99,
  "publish_run_id": null,
  "summary": "Automatic build started for Revolutions v1.0.3",
  "payload": {
    "repository_name": "Revolutions",
    "git_tag": "v1.0.3",
    "target_name": "windows-player",
    "status": "running"
  }
}
```

Contract requirements:

- Every event has a durable `event_id`.
- Every event carries enough IDs to reload canonical state from SQLite.
- `user_requested` differentiates automatic work from explicit operator
  actions.
- `summary` is safe to surface directly in notifications.
- `payload` carries compact context, not large logs or artifacts.

## Notification Policy

### Automatic Build Start

When `build.run_started` has `user_requested = false`:

- Always emit `runtime:event` to the UI.
- Always raise a native operating system notification.
- If the window is visible, the UI may also render an in-app toast.

### Automatic Build Finish

When `build.run_finished` has `user_requested = false`:

- Always emit `runtime:event` to the UI.
- If the main window is visible, render an in-app toast.
- If the main window is hidden or closed-to-tray, raise an operating system
  notification.

### Explicit User-Initiated Build Activity

When `user_requested = true`:

- Emit `runtime:event` to the UI.
- Do not raise an operating system notification in the first implementation.
- Consider making this behavior user-configurable later.

### Tag Detection

`automation.tag_detected` should update the UI, but it should not trigger a
native notification in the first implementation. The build start notification is
the operator-relevant signal.

## Delivery Semantics

- The shell must tolerate duplicate event reads through `event_id`
  deduplication.
- On shell startup, the watcher resumes from the persisted cursor.
- If the cursor is invalid or the event file rotated unexpectedly, the shell
  falls back to a full diagnostics refresh and resumes from the current end of
  file.
- The UI should treat pushed events as hints and reload canonical data through
  Tauri commands when deeper detail is required.

## Suggested Implementation Order

1. Make the shell tray-resident and background-safe.
2. Add the durable runtime event stream and append API.
3. Emit automatic build lifecycle events from the runtime.
4. Add the shell-side event bridge and native notification routing.
5. Add frontend listeners and in-app event presentation.
6. Expand event coverage to publish lifecycle and runtime supervision state.

## Task List

### Phase 0 - Scope Lock

- [ ] Confirm the shell remains the only process allowed to talk directly to
      the operating system notification APIs.
- [ ] Confirm the runtime event stream is append-only JSONL under the runtime
      state directory.
- [ ] Confirm `user_requested` is the policy switch for notification routing.
- [ ] Confirm the first implementation only notifies the operating system for
      automatic build start and finish.

### Phase 1 - Runtime Event Stream

- [ ] Add `runtime_events_path` to the runtime storage layout.
- [ ] Define `RuntimeEventRecord` and supporting topic enums.
- [ ] Add an append-only event writer with safe directory creation and atomic
      line appends.
- [ ] Add tests for event serialization and append behavior.
- [ ] Add tests for duplicate-safe cursor replay assumptions.

### Phase 2 - Runtime Event Emission

- [ ] Emit `automation.tag_detected` after the poller accepts a new tag.
- [ ] Emit `automation.release_queued` after a new automatic release is
      dispatched.
- [ ] Emit `build.run_started` immediately after `start_build_run` succeeds.
- [ ] Emit `build.run_finished` for successful build completion.
- [ ] Emit `build.run_finished` for failed build completion.
- [ ] Emit `build.run_finished` for canceled or timed-out builds.
- [ ] Include repository, release, build, tag, and target context in emitted
      payloads.
- [ ] Derive `user_requested` from persisted release trigger metadata.

### Phase 3 - Desktop Shell Event Relay

- [ ] Add a shell-managed background watcher for `runtime-events.jsonl`.
- [ ] Add a persisted cursor file for the shell event reader.
- [ ] Deduplicate replayed events by `event_id`.
- [ ] Emit `runtime:event` into the main Tauri window when the UI is available.
- [ ] Add a focused shell refresh path for event-driven diagnostics updates.
- [ ] Add tests for shell replay, cursor persistence, and duplicate handling.

### Phase 4 - Tray And Notification Delivery

- [ ] Add native notification support to the desktop shell dependencies.
- [ ] Register and configure the notification plugin.
- [ ] Add a tray menu with `Open`, `Pause notifications`, and `Quit`.
- [ ] Convert main window close into `hide-to-tray` instead of full app exit.
- [ ] Track whether the main window is visible for notification routing.
- [ ] Notify the operating system on automatic build start.
- [ ] Notify the operating system on automatic build finish when the UI is not
      visible.
- [ ] Add notification deduplication so restart replay does not resend old
      toasts.

### Phase 5 - UI Event Consumption

- [ ] Add frontend listeners for `runtime:event`.
- [ ] Replace polling-only status changes with event-driven incremental updates.
- [ ] Add in-app toast presentation for build lifecycle events.
- [ ] Add event-aware refresh targeting for build history, release status, and
      artifact views.
- [ ] Keep full diagnostics polling as a fallback reconciliation path.
- [ ] Add tests or deterministic fixtures for event-driven UI refresh logic.

### Phase 6 - Expanded Coverage

- [ ] Emit `publish.run_started` and `publish.run_finished` after build event
      delivery is stable.
- [ ] Emit runtime supervision and health events for crash recovery surfaces.
- [ ] Add notification preferences for optional explicit-build completion
      notifications.
- [ ] Evaluate whether retained build execution reports should surface as event
      payload references.

## Risks

- A polling-only UI masks event ordering bugs because it periodically heals the
  view anyway.
- A shell that still exits on window close makes background notifications
  impossible.
- Event replay after shell restart can duplicate notifications unless the shell
  persists an explicit read cursor and deduplication state.
- Overloading SQLite with notification transport semantics would tangle delivery
  concerns with operational state transitions.

## Exit Criteria

- The shell survives window close and keeps the runtime alive in background.
- A newly detected tag can produce an automatic build start notification.
- Automatic build completion reaches the visible UI immediately.
- Automatic build completion raises an operating system notification when the UI
  is hidden.
- Restarting the shell does not duplicate already delivered notifications.
- The UI can recover canonical state after missed events by reloading normal
  diagnostics snapshots.
