# Tauri Runtime Development Loop

## Purpose

This document defines the current development loop for the Tauri desktop app
and bundled local runtime.

The current scope is:

- validate the root Cargo workspace
- exercise the bundled runtime against one local runtime root
- run the desktop shell against the same runtime contract
- keep validation narrow, repeatable, and operator-visible

## Repository Shape

The active Rust-first layout is:

```text
apps/
  desktop/
    src-tauri/
    ui/

crates/
  runtime-bin/
  runtime-config/
  runtime-core/
  runtime-git/
  runtime-manifests/
  runtime-publish/
  runtime-runner/
  runtime-store/
```

The desktop UI source lives in `src-react` as a Vite-driven React +
TypeScript frontend. Shared buttons, fields, icons, badges, and surface
primitives live under `src-react/src/components`. Production assets are
emitted to `src-react/dist` and loaded by the shell build.

The runtime crates divide responsibilities explicitly:

- `runtime-config` resolves runtime roots and host settings
- `runtime-core` owns shared contracts and supervision data
- `runtime-manifests` loads and validates YAML pipeline definitions
- `runtime-git` prepares repository access and tagged workspaces
- `runtime-runner` resolves Unity executables and build execution plans
- `runtime-publish` executes artifact publication flows
- `runtime-store` owns durable state, queueing, leases, and status transitions
- `runtime-bin` exposes the runtime command surface used by development and
  shell supervision

## Toolchain Prerequisites

The repository currently pins Rust `1.94.0` through `rust-toolchain.toml`.

Install Rust through `rustup` first, then open a fresh terminal session so
`cargo` is available on `PATH`.

For the desktop shell, install the native prerequisites required by Tauri for
your host platform.

Keep Node available because the desktop shell now builds its popup surface with
Vite, React, and TypeScript.

## Runtime Root Layout

The bundled runtime manages one data root with this structure:

```text
<runtime-root>/
  state/
    runtime.db
    health.json
    supervision.json
    supervisor-state.json
  logs/
    runtime.jsonl
  artifacts/
  runs/
```

`bootstrap` ensures those directories exist, initializes SQLite state, and
prepares the runtime health snapshot files used by the shell.

Development builds intentionally suffix the persistent product directory with
`_DEV` so the sandbox stays isolated from production data on every supported
platform:

- Windows: `%LOCALAPPDATA%/HandyGamesPublisher_DEV/runtime`
- macOS: `~/Library/Application Support/HandyGamesPublisher_DEV/runtime`
- Linux: `$XDG_DATA_HOME/HandyGamesPublisher_DEV/runtime` or
  `~/.local/share/HandyGamesPublisher_DEV/runtime`

Use `HANDY_GAMES_PUBLISHER_RUNTIME_ROOT` when you need an isolated development
sandbox. Normal operation still expects one runtime root owned by the
application, while per-run workspaces live under `runs/` or repository-specific
workspace overrides recorded in SQLite.

## Repository Auth Contract

Repository auth behavior in the desktop shell and bundled runtime now follows
these rules:

- the operator enters only the repository URL first
- the shell assesses provider family and anonymous repository visibility before
  requesting repository auth
- public repositories stay credential-free by default
- private repositories expose one explicit connect or reconnect action inside
  the owning project create or edit flow
- polling and build execution stay non-interactive, and stale credentials are
  surfaced as durable repository auth state instead of reopening credential UI

For focused validation of that contract during development, prefer these
checks before wider desktop testing:

```bash
cargo test --package desktop-shell persist_repository_project_creates_repository_inspection_entry -- --nocapture
cargo test --package desktop-shell persist_repository_project_persists_repository_auth_state_in_inspection -- --nocapture
cargo test --package runtime-bin run_repository_poll_cycle_stops_on_authentication_failure_and_emits_runtime_event -- --nocapture
cargo test --package runtime-bin build_run_next_command_marks_repository_reauth_required_on_auth_resolution_failure -- --nocapture
```

## Runtime Commands

### Bootstrap And Inspection

Inspect the resolved runtime directories and file layout:

```bash
cargo run -p runtime-bin -- status
```

Install the JavaScript workspace dependencies and the local Tauri CLI once:

```bash
npm install
```

The root package installs the React + TypeScript UI workspace dependencies and
the local Tauri CLI used by the desktop development loop.

Install the desktop UI dependencies directly only when you are intentionally
working outside the root workspace flow:

```bash
npm install --prefix src-react
```

Start the Vite development server when iterating on shared components in a
browser preview:

```bash
npm run dev --prefix src-react
```

The browser preview is useful for component work but does not expose live
Tauri command invocations.

Run the desktop shell development loop from the repository root:

```bash
npm start
```

The root `npm start` command resolves the local Tauri CLI and runs `tauri dev`
from the repository root. The Tauri development loop owns the UI development server
and version synchronization through `beforeDevCommand`, and the desktop shell
startup path then launches the bundled runtime supervisor. Before Tauri starts,
the root launcher also ensures the Butler sidecar exists under
`src-tauri/bin` so Itch destinations can publish without asking
operators for a local Butler path. On Windows the runner also attempts to enter
the Visual Studio developer environment before invoking Tauri, but it still
requires the native Tauri prerequisites, including the Visual Studio C++
workload for the MSVC toolchain.

The desktop app targets Windows, Linux, and macOS. The current hands-on
validation loop is still Windows-based, so host-specific behavior should keep
explicit extension points for Linux and macOS instead of treating Windows as
the only supported route.

The desktop app version is centralized in the workspace Cargo manifest at
`Cargo.toml` under `[workspace.package].version`. Run `npm run version:sync`
after changing that value if you want to refresh the mirrored JSON manifests
without starting the desktop development loop.

When a troubleshooting session needs captured output, write temporary log files
under `tmp/diagnostics/` rather than the repository root. The workspace already
routes Cargo artifacts through `tmp/` for the same reason.

Build the desktop UI assets consumed by plain Cargo runs:

```bash
npm run build --prefix src-react
```

Bootstrap runtime metadata, health reports, and SQLite state:

```bash
cargo run -p runtime-bin -- bootstrap
```

Read the persisted runtime health report:

```bash
cargo run -p runtime-bin -- health
```

Read the current shell-to-runtime supervision contract:

```bash
cargo run -p runtime-bin -- contract
```

`bootstrap` also reconciles interrupted local work by releasing inherited queue
leases and moving interrupted `build_runs` and `publish_runs` back to `queued`.
The JSON output includes a `recovery_report` so the current development loop
can verify what was reconciled at startup.

### Manifest Synchronization

Synchronize filesystem-backed pipeline manifests into the local SQLite state:

```bash
cargo run -p runtime-bin -- manifests sync
```

Override the manifest directory when the current working directory is not the
repository root:

```bash
cargo run -p runtime-bin -- manifests sync --dir C:/path/to/pipelines
```

The runtime reads `./pipelines` by default. Manifest synchronization validates
pipeline definitions and persists repository, build target, publish target, and
binding state before automation proceeds.

### Release Intake And Automation

Inspect the current automation snapshot, including queue message counts,
coordination leases, and per-repository release, build, and publish backlog:

```bash
cargo run -p runtime-bin -- automation inspect
```

Poll all eligible repositories once:

```bash
cargo run -p runtime-bin -- automation poll-once
```

Dispatch one manual release:

```bash
cargo run -p runtime-bin -- releases dispatch manual --repository-id 1 --git-tag v1.2.3
```

Dispatch one manual release and clear derived state for a rebuild:

```bash
cargo run -p runtime-bin -- releases dispatch manual --repository-id 1 --git-tag v1.2.3 --rebuild
```

Resolve release planning and persist `unity_version` when it is still missing:

```bash
cargo run -p runtime-bin -- releases plan --release-run-id 1
```

The long-lived `serve` loop also runs repository polling and release planning,
so queued manual releases can advance without requiring an extra command once
the runtime is already active.

### Build And Publish Execution

### Smoke Validation

Run the interrupted-recovery end-to-end smoke target through the repository
entrypoint instead of mixing it into ad hoc unit-test commands:

```bash
npm run smoke:runtime
```

The workspace default Cargo artifacts live under
`tmp/cargo-targets/default/`.
The smoke entrypoint pins `CARGO_TARGET_DIR` to
`tmp/cargo-targets/runtime-smoke/` so local runtime processes holding the
default runtime binary do not block the validation binary rebuild.
The isolated artifacts stay under `tmp/` so the repository root does not
accumulate one-off Cargo target directories.

Stage the next queued build run through Git auth resolution, workspace
planning, and workspace materialization:

```bash
cargo run -p runtime-bin -- builds stage-next
```

Execute the next queued host-native build run:

```bash
cargo run -p runtime-bin -- builds run-next
```

Execute the next queued publish run:

```bash
cargo run -p runtime-bin -- publishes run-next
```

Inspect persisted publish outputs for one build run:

```bash
cargo run -p runtime-bin -- publishes inspect --build-run-id 1
```

Inspect persisted publish outputs for one publish run:

```bash
cargo run -p runtime-bin -- publishes inspect --publish-run-id 1
```

The current runtime store persists the full workflow slice required for local
release execution:

- `credentials`
- `repositories`
- `trigger_rules`
- `build_targets`
- `publish_targets`
- `build_publish_bindings`
- `release_runs`
- `build_runs`
- `artifacts`
- `publish_runs`
- `worker_queue_messages`
- `worker_coordination_leases`
- `worker_idempotency_keys`

Queue claim selection enforces host-local ceilings during `serve` and focused
execution commands:

- build claims stop when host build capacity is full
- build claims skip work that would violate the active release lane per
  repository
- publish claims stop when host publish capacity is full
- stale queue messages are deleted when the referenced run no longer exists or
  is no longer `queued`

### Long-Lived Runtime Processes

Run the long-lived runtime loop directly:

```bash
cargo run -p runtime-bin -- serve
```

Run the runtime under the built-in supervisor and restart policy:

```bash
cargo run -p runtime-bin -- supervise
```

Mark the runtime state as stopped for lifecycle smoke tests:

```bash
cargo run -p runtime-bin -- shutdown
```

Useful loop overrides for supervision tests:

- `HANDY_GAMES_PUBLISHER_RUNTIME_WORKER_LOOP_INTERVAL_MILLIS`
- `HANDY_GAMES_PUBLISHER_RUNTIME_HEARTBEAT_INTERVAL_MILLIS`
- `HANDY_GAMES_PUBLISHER_RUNTIME_MAX_HEARTBEATS`
- `HANDY_GAMES_PUBLISHER_RUNTIME_CRASH_AFTER_HEARTBEATS`
- `HANDY_GAMES_PUBLISHER_RUNTIME_CRASH_ATTEMPTS`
- `HANDY_GAMES_PUBLISHER_RUNTIME_MAX_RESTARTS`
- `HANDY_GAMES_PUBLISHER_RUNTIME_RESTART_BACKOFF_MILLIS`

Useful concurrency overrides for host-local execution:

- `HANDY_GAMES_PUBLISHER_RUNTIME_MAX_CONCURRENT_BUILD_RUNS`
- `HANDY_GAMES_PUBLISHER_RUNTIME_MAX_CONCURRENT_PUBLISH_RUNS`
- `HANDY_GAMES_PUBLISHER_RUNTIME_MAX_ACTIVE_RELEASES_PER_REPOSITORY`

The current defaults keep the host conservative:

- one concurrent build run claim per host
- one concurrent publish run claim per host
- one active release lane per repository

## Desktop Shell Loop

Run the desktop shell directly from the repository root:

```bash
cargo run --package desktop-shell
```

The shell starts or reconnects to the bundled runtime, reads the persisted
health and supervision snapshots, and exposes thin Tauri commands for operator
diagnostics and management.

The current tray lifecycle works like this:

- startup launches the runtime supervisor, initializes the tray icon, and shows
  the popup window
- the popup is always-on-top, skipped from the taskbar, and pinned to the
  lower-right corner of the primary monitor work area
- closing the popup hides it instead of terminating the app
- tray left-click and the `Open HGP` tray action restore the popup window
- the `Quit` tray action marks the shell as intentionally exiting and allows
  the normal runtime shutdown path to run

The current visible UI inside the popup is intentionally minimal and only shows
the `HGP` wordmark while the tray shell behavior settles.

## Focused Validation

Use the narrowest validation that can falsify the touched slice:

```bash
cargo check --workspace
```

If only the runtime slice changed:

```bash
cargo check -p runtime-bin
```

If only the shell slice changed:

```bash
cargo check -p desktop-shell -j 1 -q
```

If durable workflow behavior changed:

```bash
cargo test -p runtime-store -q
```

If the React + TypeScript desktop UI or reusable component kit changed:

```bash
npm run build --prefix src-react
```

## Typical Local Flow

1. run `bootstrap`
2. synchronize manifests with `manifests sync`
3. inspect `status` or `health`
4. dispatch a manual release or run `automation poll-once`
5. execute focused build or publish commands, or start `serve`
6. launch the desktop shell for operator inspection and diagnostics
