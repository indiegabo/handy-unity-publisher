# Architecture

See the delivery roadmap in
[Desktop Delivery Roadmap](../planning/desktop-delivery-roadmap.md).

## Overview

HGP is a self-hosted, local-first desktop orchestration system with an
engine-aware runtime model whose first and only shipped adapter is Unity.

The active product architecture is:

- a Tauri desktop shell for operator interaction
- a bundled Rust runtime for supervision, orchestration, and persistence
- SQLite as the durable source of truth
- filesystem-backed logs, artifacts, workspaces, and runtime snapshots
- host-native Unity execution through locally installed editors

The current delivery scope targets one local host and one supervised runtime
instance.

## System Shape

### Desktop Shell

`apps/desktop/src-tauri` hosts the Tauri desktop shell.

The shell is responsible for:

- launching or reconnecting to the bundled runtime process
- creating the tray icon and compact popup window lifecycle
- exposing thin Tauri commands for operator-facing diagnostics and management
- loading the Vite-built popup UI from `apps/desktop/ui/dist`
- composing that popup UI from reusable React primitives under
  `apps/desktop/ui/src/components`
- anchoring the popup window to the lower-right corner of the primary monitor
- hiding the window to tray on close while keeping the runtime alive
- executing the full shutdown path only when the tray `Quit` action is chosen

The shell remains intentionally thin. Orchestration rules belong in runtime
crates, not in window bindings.

The current visible UI is a compact React + TypeScript + Vite showcase built
from reusable dark-theme primitives under `apps/desktop/ui/src/components`.
The Tauri command surface for diagnostics and management remains in place
behind that shell while the broader operator views are rebuilt.

### Bundled Runtime

`crates/runtime-bin` hosts the bundled runtime entrypoint.

The runtime is responsible for:

- bootstrapping runtime directories and SQLite state
- synchronizing declarative manifests from `pipelines/`
- supervising one local orchestration loop
- persisting durable release, build, artifact, and publish state
- exposing diagnostic outputs consumed by the shell

The runtime may expose operator and development commands, but those commands are
supporting surfaces around the desktop product rather than a competing product
experience.

### Focused Runtime Crates

The runtime is intentionally decomposed into focused crates.

- `runtime-config` resolves paths, host settings, and app data layout.
- `runtime-core` owns shared runtime contracts and supervision snapshots.
- `runtime-store` owns SQLite-backed durable workflow state and local
  coordination.
- `runtime-manifests` validates and synchronizes declarative pipeline manifests.
- `runtime-git` owns repository access and workspace preparation.
- `runtime-runner` owns shared workspace/artifact helpers plus the explicit
  `runtime_runner::unity` adapter surface for host capability checks and Unity
  execution planning.
- `runtime-publish` owns artifact publication flows and destination execution.

## Engine And Adapter Model

The persisted repository model is engine-aware even though only Unity is
currently executable.

- repositories declare `engine_kind`
- build targets declare `buildKind`
- engine-specific execution inputs live under engine-scoped contracts such as
  `contract.unity`
- `invoke kind` remains internal adapter state chosen by runtime code, not a
  public manifest field

The current supported-engine matrix is intentionally narrow:

- Unity: supported end to end
- Unreal: planned only, visible but rejected by backend validation
- Godot: planned only, visible but rejected by backend validation
- GameMaker: planned only, visible but rejected by backend validation
- Defold: planned only, visible but rejected by backend validation
- Cocos Creator: planned only, visible but rejected by backend validation

## Durable Data Model

SQLite is the durable source of truth for workflow state.

Core entities include:

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

These tables model one repository as a full release pipeline definition rather
than a single release event or isolated build invocation.

The runtime uses explicit status transitions so release, build, and publish
flows remain recoverable after restart.

## Filesystem Layout

The runtime resolves an app-managed data directory and stores mutable runtime
files there.

The expected layout is conceptually:

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

Filesystem responsibilities are split deliberately:

- SQLite stores durable metadata, state, and file references.
- logs stay on disk as operator-facing execution traces.
- artifacts stay on disk as produced outputs.
- runs stay on disk as execution-local scratch state.
- runtime snapshots stay on disk as shell-readable health and supervision data.

The runtime should never treat the database as blob storage for large build
logs or artifact payloads.

## Configuration Model

Repository pipeline configuration comes from YAML manifests under `pipelines/`.

Each manifest defines:

- Git source access
- trigger behavior
- build targets
- publish targets
- build-to-publish bindings
- credential references where required

Manifest synchronization materializes validated pipeline state into SQLite
before automation proceeds.

## Operator Surfaces

### Desktop Tray Shell

The desktop shell is the primary operator surface.

Its current operator-visible behavior is:

- startup opens a compact popup anchored to the lower-right corner of the
  primary monitor
- the popup stays always-on-top and out of the taskbar
- window close requests hide the popup to tray instead of terminating the app
- tray clicks and the `Open HGP` action reopen the popup
- the `Quit` tray action triggers intentional shell exit and runtime shutdown

The runtime diagnostics, repository inspection, and credential management
commands still exist behind the shell, and the current popup UI now renders a
compact dark showcase of the reusable component system that future operator
views will extend.

### Runtime Commands

The bundled runtime also exposes command surfaces for development,
verification, and narrow operator diagnostics.

Those commands remain thin wrappers around runtime crate behavior and support
local development, tests, and focused inspection.

## Release Lifecycle

The intended single-host flow is:

1. an operator declares one or more repository pipelines under `pipelines/`
2. manifest sync validates and persists those pipelines into SQLite
3. polling or manual dispatch creates durable `release_runs`
4. release planning creates queued `build_runs`
5. the local build execution path claims and completes build work
6. successful build results register artifacts and expand queued
   `publish_runs`
7. publish execution claims and completes downstream delivery work

Every stage records explicit durable transitions in SQLite so the runtime can
recover after process restart.

## Local Coordination Model

The current product uses local coordination backed by SQLite and runtime-owned
state files.

That means:

- queued work is durable
- release sequencing is repository-local and explicit
- restart recovery can be driven from the store and runtime snapshots
- runtime ownership of work claims is explicit and inspectable

The runtime should prefer short transactions, WAL mode, and explicit ownership
of work claims.

## Host Capability Model

Unity execution is host-native.

The runtime must make these concerns explicit:

- which host platform is active
- which Unity editors are installed or discoverable
- whether each configured build target has a valid local execution path
- whether required files, paths, and credentials exist before claiming work

Capability checks belong in runtime crates and should be visible in desktop
diagnostics.

## Build Execution Boundary

Build execution should resolve one durable build plan into one host-local Unity
invocation.

The current boundary is explicit:

- `runtime-bin` owns the build intake path that loads one stored build plan,
  checks `engine_kind`, and refuses unsupported engines before entering any
  Unity-specific execution path
- `runtime_runner::unity` owns the Unity-specific execution plan, host
  capability inspection, editor discovery, command assembly, execution
  processor, and failure classification
- the `runtime-runner` crate root keeps shared workspace preparation and
  artifact discovery outside the Unity adapter

That boundary is responsible for:

- workspace preparation
- Git synchronization for the requested tag or commit
- Unity executable resolution
- argument construction for the configured build method
- captured logs and terminal status persistence
- artifact discovery and registration

The orchestration layer should remain testable without launching real Unity
processes. Host-specific execution concerns belong behind explicit
`runtime-runner` boundaries.

## Publish Execution Boundary

Publish execution should resolve one durable publish plan into one destination
operation.

The first supported path is filesystem publication. Additional remote
publishers should layer on top of the same durable publish model rather than
introducing parallel control flows.

## Secret And Credential Management

Credentials are currently stored in SQLite configuration JSON and managed from
the desktop shell.

Repository project creation currently treats PAT input as the only first-class
operator authentication path. Once the operator provides that PAT, repository
polling and workspace synchronization must remain seamless and non-interactive
for runtime-owned Git operations.

The shell must never echo stored secret values back into operator diagnostics.
Instead, it should expose:

- credential kind
- key-shape validation status
- binding references
- storage warnings

PAT secret values should live in the host keyring or another redacted secret
backend, while SQLite persists only the credential metadata and secret
references required to resolve them at execution time.

Future wizard work will add an explicit operator choice between PAT input and a
provider-specific interactive sign-in flow when that flow is supported by the
repository host and current platform. When that capability arrives, any login
window belongs to project creation or credential refresh only, and the runtime
must continue to execute Git operations non-interactively after the required
token has been stored.

Future secret backends must preserve the same redaction discipline and durable
binding model.
