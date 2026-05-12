# Project Brief

## Project Name

Product name: `HUP`

Repository name: `handy-unity-publisher`

Canonical Git remote: `git@github.com:indiegabo/handy-unity-publisher.git`

Operator-facing product references should use `HUP`. Development and
repository references should use `handy-unity-publisher`.

## Vision

Build a self-hosted, local-first desktop application that automates Unity
release pipelines from Git repositories.

The product is a Tauri desktop shell backed by a bundled Rust runtime. It
should let one operator manage repository pipelines, detect releasable tags,
run Unity builds through locally installed editors, and publish artifacts to
configured destinations through one local application with explicit state and
operator-facing diagnostics.

The system must stay lightweight enough for solo developers and small teams,
while preserving clear boundaries for future growth in publish backends,
runtime diagnostics, and richer desktop workflows.

## Problem To Solve

Unity release automation is usually fragmented across shell scripts, manual
editor invocations, ad-hoc spreadsheets, and tribal knowledge about which tag
needs which editor version.

Teams repeatedly waste time on the same operational tasks:

- checking repositories for new release tags
- discovering the Unity version required by a given tag
- preparing a clean local workspace for a reproducible build
- executing platform-specific builds through the correct local editor
- collecting artifacts and routing them to the right destinations
- understanding what failed after a restart or interrupted run

This project centralizes those responsibilities into one local application with
durable state, explicit workflow transitions, and operator-facing diagnostics.

## Product Goals

- provide a Tauri desktop app as the primary operator experience
- bundle a Rust runtime that owns orchestration, persistence, and recovery
- register Unity repositories as full pipeline definitions, not simple watch entries
- define Git access, trigger rules, build targets, publish targets, and bindings per repository
- detect new tags automatically and support explicit manual release dispatch
- resolve the required Unity version for each release from repository content
- execute Unity builds host-natively through locally installed editors
- persist workflow state durably in SQLite under the app data directory
- keep logs, artifacts, and workspaces on the filesystem under app-managed paths
- expose enough runtime health and diagnostics in the desktop shell to operate locally with confidence

## Non-Goals For The Initial Phase

- full cloud SaaS deployment
- distributed worker clusters
- additional infrastructure requirements beyond the desktop app and the local
  operator host
- speculative multi-tenant permission systems
- advanced reporting or analytics beyond operational diagnostics
- storing large build logs or artifact blobs inside SQLite
- a second primary product surface that competes with the desktop app

## Core Workflow

1. an operator defines one or more repository pipelines
2. each pipeline includes:
   - Git source access
   - trigger or polling rules
   - build targets
   - publish targets
   - build-to-publish bindings
3. the runtime validates pipeline manifests and synchronizes them into SQLite
4. polling or manual dispatch creates a durable `release_run`
5. release planning resolves the Unity version and creates queued `build_runs`
6. the runtime prepares a local workspace and executes each build through the
   appropriate host-native Unity editor
7. successful builds register artifacts on disk and expand queued `publish_runs`
8. publish execution delivers artifacts to the configured destinations
9. the runtime and desktop shell expose status, logs, artifacts, and recovery
   state for the full lifecycle

## Key Functional Concepts

### Repository Pipeline

A registered Unity project plus the automation settings required to build and
publish releases.

Each repository is a pipeline definition that describes:

- what to build
- how to detect releases
- how to access source and credentials
- where outputs should be published
- which build outputs map to which publication targets

### Build Target

A named Unity build configuration for one repository.

Examples include:

- Windows
- Linux
- macOS
- WebGL
- Android

### Publish Target

A named artifact destination for one repository.

Examples include:

- local filesystem export
- Itch.io
- Steam
- Google Drive

### Binding

A link that tells the system which build target produces artifacts for which
publish target.

### Release Run

One durable processing instance for a specific repository tag or manually
requested release.

### Build Run

One durable execution unit for a single build target within a release run.

### Publish Run

One durable execution unit for delivering a produced artifact or build output to
one publish target.

## Technical Direction

### Product Shape

- desktop shell: Tauri
- runtime and application logic: Rust
- durable local state: SQLite
- mutable runtime files: filesystem under the resolved app data directory
- Unity execution: host-native via locally installed editors

### Runtime Boundaries

The desktop shell should remain thin and operator-facing.

The bundled runtime owns:

- bootstrap and runtime directory initialization
- manifest synchronization
- polling and manual release intake
- release, build, artifact, and publish state transitions
- local recovery after restart
- host capability checks and Unity invocation planning
- operator-facing diagnostics consumed by the desktop shell

### Repository Layout Direction

The active Rust-first structure is:

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

The active repository layout is Rust-first and organized around the desktop
shell plus focused runtime crates.

## Persistence And Filesystem Strategy

SQLite is the durable source of truth for workflow state.

Core tables include:

- credentials
- repositories
- trigger_rules
- build_targets
- publish_targets
- build_publish_bindings
- release_runs
- build_runs
- artifacts
- publish_runs

The runtime should use explicit status transitions so release, build, and
publish work can recover cleanly after restart.

Filesystem storage is reserved for:

- logs
- artifacts
- workspaces
- runtime health and supervision snapshots

The conceptual local layout is:

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

The database stores metadata, configuration, state, and file references. It
must not become blob storage for large artifacts or full log payloads.

## Configuration Model

Repository pipelines are declared through YAML manifests under `pipelines/`.

Each manifest defines:

- Git access
- trigger behavior
- build targets
- publish targets
- build-to-publish bindings
- credential references where required

Manifest synchronization validates and materializes those definitions into the
runtime store before automation proceeds.

## Target Users

- indie developers shipping Unity projects
- small teams that want one local automation hub
- self-hosted release workflows that prefer local control over hosted services
- Windows-first operators who run Unity editors directly on the host machine

## Immediate Next Steps

1. finalize the product naming strategy without breaking active surfaces
2. complete the desktop shell operator flows around runtime status, repositories, and credentials
3. complete the active runtime roadmap and remove unused repository surfaces
4. harden manifest validation and repository pipeline synchronization
5. expand host capability diagnostics for Unity editors, paths, and credentials
6. complete the end-to-end release flow from tag detection through publish execution
7. add richer publish backends on top of the existing durable publish model
