# Tauri Runtime Migration Strategy

## Purpose

This document defines the migration strategy from the current Go and
Docker-oriented architecture to a Tauri desktop product with a bundled local
runtime.

The goal is not to add a desktop shell on top of the existing deployment model.
The goal is to change the product shape itself:

- one installable desktop application per platform
- one bundled local runtime supervised by that application
- host-native build execution by default
- Docker removed from the core product model
- UI postponed until the runtime is functionally correct

This plan treats the current Go codebase as a domain reference and migration
source, not as the final runtime architecture.

## Decision Summary

- The product becomes a Tauri application.
- The distributed product must feel like one application to the operator.
- The packaged application may still contain multiple internal processes, but
  they must ship, version, start, and update together.
- Docker, Docker Compose, Redis, and GameCI are removed from the primary
  product model.
- The runtime becomes local-first and host-native.
- Unity builds must run without opening a visible editor window.
- Early development focuses on the runtime, manifest compatibility, and build
  execution, not on desktop windows and UI polish.

## Product Intent After Migration

The migrated product should automate what a developer can already do manually
on the same machine.

That means the runtime must prefer the execution context where Unity is already
installed, licensed, and known to work.

Examples:

- On Windows, use the installed Unity Editor on Windows.
- On Linux, use the installed Unity Editor on Linux.
- On macOS, use the installed Unity Editor on macOS.

The product should not require a second machine and should not require Docker
to reproduce work that already succeeds on the developer's workstation.

## Target Product Shape

The final delivered product should look like one desktop application.

Internally, it should contain two layers:

1. Desktop shell
2. Bundled runtime

### Desktop shell

The Tauri shell is responsible for:

- application bootstrap
- configuration entry and editing
- runtime health inspection
- log viewing
- workflow supervision
- future UI surfaces for repositories, builds, artifacts, and publishing
- starting, stopping, and restarting the bundled runtime
- presenting errors when the runtime is unhealthy or incompatible

### Bundled runtime

The runtime is responsible for:

- manifest loading and synchronization
- durable state management
- scheduling and queueing
- repository polling
- manual dispatch
- workspace preparation
- Unity version resolution
- runner selection
- build execution
- artifact discovery
- publish execution
- local diagnostics and structured logs

The runtime must be packaged with the Tauri application and versioned with it.

## One Product, Not Two User-Facing Apps

The operator must install and launch one product.

This does not require a single process.
It requires a single product experience.

The expected internal model is:

- a Tauri desktop process
- a bundled runtime process launched and supervised by the desktop process

This allows the runtime to remain operationally robust without forcing the UI
to carry background scheduling, build orchestration, and process supervision
inside the window process.

## Why Tauri Fits This Direction

Tauri matches the new product direction because:

- it supports native desktop packaging
- it keeps the application backend in Rust
- it is well suited to shipping a desktop shell plus bundled binaries
- it avoids making Electron the mandatory runtime foundation
- it keeps the product lighter than an Electron-first architecture

The strategic value is not only the frontend shell.
The key value is that Tauri gives the project a desktop distribution model while
the runtime remains local, bundled, and supervised.

## Runtime-First Delivery Strategy

UI is intentionally delayed.

The first Tauri-oriented milestone should not focus on windows, forms, or
frontend flows.
It should focus on making the bundled runtime work correctly on a real desktop
host.

During the early migration stages, the runtime should still be operable through
manifest files, local commands, and runtime inspection surfaces, so the
architecture can be proven before frontend work expands.

## Architectural Principles For The New Runtime

- Prefer host-native execution over containerized execution.
- Keep the runtime local-first and self-contained.
- Preserve SQLite as the durable system of record.
- Prefer filesystem logs, artifacts, and workspaces.
- Replace external transient infrastructure with local runtime services when
  the product no longer needs them.
- Separate orchestration logic from OS-specific execution adapters.
- Detect host capabilities explicitly instead of assuming a fixed environment.
- Treat Unity execution as a runner capability, not as a universal Docker job.
- Keep build execution invisible to the operator unless the runtime surfaces
  logs or errors.

## What Leaves The Product Model

The following items leave the core product model:

- Docker as a mandatory runtime dependency
- Docker Compose as the normal operator workflow
- Redis as mandatory transient coordination infrastructure
- GameCI as the default build strategy for every environment
- environment-file driven operator setup as the primary product experience

These tools may remain useful during migration or for specific development
scenarios, but they are no longer the target product shape.

## What Remains Valuable From The Current Codebase

The existing implementation is still strategically valuable.
It contains domain logic, status models, storage assumptions, manifest flow,
and operational lessons that should be translated into the new runtime.

The migration should preserve:

- repository pipeline definition semantics
- trigger and release lifecycle semantics
- build and publish state models
- artifact naming rules
- manifest compatibility when practical
- SQLite schema intent, constraints, and status transitions
- improved failure reporting and diagnostics expectations

## Recommended Technical Direction

## Runtime language

The final bundled runtime should be Rust-based so the product is materially a
Tauri application rather than a Tauri shell supervising a permanent Go backend.

However, the migration should be staged.
The Go codebase should be treated as the reference implementation during the
translation period.

## Storage

- SQLite remains the durable source of truth.
- Filesystem storage remains the place for logs, artifacts, and workspaces.
- OS-native secret storage should replace plaintext `.env` as the long-term
  operator model.

## Local coordination

Redis should be removed from the final desktop product.

Local scheduling, queueing, leasing, idempotency, and heartbeat concerns should
be handled by the bundled runtime through SQLite-backed state transitions and
in-process scheduling services.

## Build execution model

Builds should execute through host-native runners.

The runtime must support a capability-driven runner model rather than assuming a
single execution strategy.

Minimum planned runner families:

- host-windows-unity
- host-linux-unity
- host-macos-unity

Any optional future container runner should be additive, not foundational.

## Seamless Unity Build Requirement

The runtime must launch Unity without opening a visible editor window for the
operator.

That means the runner layer must:

- run Unity in batch mode
- capture logs to files and runtime streams
- detach from the desktop shell window model
- keep the UI responsive while the build runs in the background
- report progress and failure through runtime state rather than interactive
  editor UI

On supported hosts, the runtime should use the appropriate process creation
flags and batchmode arguments so the build remains headless from the operator's
point of view.

## Capability Detection Instead of OS Guessing

The runtime should not only ask "what OS am I on?"

It should build a capability profile for the host.

That profile should include at least:

- operating system
- architecture
- application packaging mode
- whether the runtime itself is inside WSL or not
- whether Unity is installed
- which Unity versions are installed
- where Unity executables live
- whether a valid local Unity license is available in the same execution
  context
- which target platforms are supported by the installed editors
- whether Git is available
- whether platform-specific publish dependencies are available

Runner selection should be based on this capability profile, not on ad hoc OS
conditionals spread across the codebase.

## Proposed Repository Shape After Migration

One reasonable target layout is:

```text
apps/
  desktop/

src-tauri/
  Cargo.toml
  tauri.conf.json
  src/

crates/
  runtime-bin/
  runtime-core/
  runtime-config/
  runtime-store/
  runtime-manifests/
  runtime-git/
  runtime-runner/
  runtime-unity/
  runtime-publish/
  runtime-host/
  runtime-cli/

planning/
docs/
```

This shape keeps the Tauri shell separate from the runtime crates while still
shipping a single product.

## Runtime Supervision Model

The Tauri shell should supervise the runtime process.

Expected startup flow:

1. desktop shell starts
2. shell resolves application data directories
3. shell verifies runtime binary availability and version match
4. shell starts the runtime if not already running
5. shell waits for a local health signal
6. shell surfaces runtime status to the operator

Expected failure handling:

- restart runtime on recoverable crashes
- expose runtime logs
- surface migration and schema errors clearly
- keep failure reporting operator-friendly

## Development Workflow Before UI

The early migration should support a runtime-only development loop.

That means developers should be able to:

- run the runtime directly from the command line
- load and apply manifests
- inspect health and automation state
- dispatch builds manually
- watch logs and artifacts

The Tauri shell can remain minimal during this stage.

## Manifest Strategy

The runtime should preserve the current manifest-driven workflow during the
early migration.

This keeps the operator model stable while the execution architecture changes.

Manifest compatibility goals:

- preserve repository definition semantics
- preserve build target and publish target semantics
- preserve binding semantics
- preserve credential reference behavior where reasonable

Manifest changes should be delayed until after the runtime is stable unless a
format change is required by the host-native runner model.

## Secrets Strategy

The final product should stop relying on plaintext `.env` files as the primary
operator experience.

The preferred direction is:

- SQLite stores non-secret configuration and references
- OS-native secure storage holds secrets
- manifests and configuration reference named secrets instead of embedding them

Examples:

- Windows Credential Manager
- macOS Keychain
- Secret Service or equivalent on Linux

## Migration Style

This migration should follow a staged strangler approach, not a blind rewrite.

The current Go code remains useful as:

- behavior reference
- data model reference
- acceptance criteria source
- test oracle for translated logic

The final product should no longer require the Go runtime, but the migration
should not discard the current system knowledge.

## Translation Map From Current Go Modules

| Current area | Migration target |
| --- | --- |
| `internal/config` | Rust runtime config and app-dir resolution |
| `internal/db` | Rust SQLite store, migrations, and schema management |
| `internal/pipelines` | Rust manifest parser and synchronizer |
| `internal/credentials` | Rust credentials model plus OS secret bridge |
| `internal/repository` | Rust repository store and services |
| `internal/trigger` | Rust trigger rule services |
| `internal/release` | Rust release lifecycle services |
| `internal/build` | Rust build planning, run state, artifact logic, and workspace preparation |
| `internal/publish` | Rust publish planning and publisher implementations |
| `internal/git` | Rust Git integration and workspace sync |
| `internal/docker` | Removed from core product; replace with host-native runners |
| `internal/automation` | Rust scheduler and orchestration services |
| `cmd/server` | Runtime service entrypoint in Rust |
| `cmd/hgb` and `cmd/hub` | Runtime developer CLI and local inspection tools |
| worker binaries | Internal runtime background services or supervised tasks |

## Migration Phases

### Phase 0 - Architecture Lock

Objective: confirm the product direction before code translation begins.

Deliverables:

- migration strategy document
- task list document
- confirmed Tauri-first decision
- confirmed host-native runner decision
- confirmed Docker removal from the target product
- confirmed runtime-first, UI-later sequencing

Exit criteria:

- team agrees that the final product is a Tauri desktop app with bundled local
  runtime
- team agrees that the final runtime is not Docker-dependent

### Phase 1 - Repository And Workspace Restructure

Objective: reshape the repository around Tauri and runtime crates.

Deliverables:

- Cargo workspace
- Tauri shell scaffold
- runtime binary scaffold
- runtime crate boundaries
- removal plan for Docker-only development assumptions

Exit criteria:

- the repository can build a minimal Tauri shell and a minimal runtime binary

### Phase 2 - Runtime Skeleton

Objective: create the new local runtime with health and supervision hooks.

Deliverables:

- runtime startup path
- health endpoint or local health channel
- structured logging
- application data directory resolution
- shell-to-runtime supervision contract

Exit criteria:

- the shell can start and monitor the runtime locally

### Phase 3 - Storage And Local State

Objective: re-establish durable local state without Docker-era dependencies.

Deliverables:

- SQLite bootstrap and migrations in Rust
- runtime directory layout
- artifact, log, and workspace path conventions
- local queue and lease strategy without Redis

Exit criteria:

- runtime persists state and recovers on restart without Redis

### Phase 4 - Manifest Compatibility Layer

Objective: make the new runtime understand current pipeline definitions.

Deliverables:

- manifest parser and validator in Rust
- manifest synchronization into SQLite
- credential reference translation strategy
- compatibility tests against current manifest behavior

Exit criteria:

- current manifests can be loaded into the new runtime with equivalent domain
  meaning

### Phase 5 - Capability Detection And Runner Resolution

Objective: teach the runtime how to understand the host it is running on.

Deliverables:

- host capability probe service
- Unity installation discovery
- Unity version discovery
- local license context inspection
- runner selection service

Exit criteria:

- the runtime can explain which runner it would use and why

### Phase 6 - Host-Native Unity Runners

Objective: execute Unity builds in the host context without visible editor UI.

Deliverables:

- Windows runner
- Linux runner
- macOS runner design or implementation depending on delivery order
- hidden process execution model
- log capture and lifecycle monitoring

Exit criteria:

- one supported host can run a background Unity build successfully through the
  new runtime

### Phase 7 - Release Lifecycle Translation

Objective: restore the end-to-end release orchestration behavior.

Deliverables:

- manual dispatch
- polling
- release planning
- build run state transitions
- artifact recording
- publish run planning

Exit criteria:

- one release can move from dispatch to terminal build state in the new runtime

### Phase 8 - Publishing And Diagnostics

Objective: restore artifact handoff and operational visibility.

Deliverables:

- filesystem publisher baseline
- runtime diagnostics views
- log browsing contract for the desktop shell
- failure categorization and actionable errors

Exit criteria:

- successful builds produce discoverable artifacts and publish actions

### Phase 9 - UI Foundation

Objective: add the first real desktop UX after the runtime is stable.

Deliverables:

- runtime health surface
- repository and manifest inspection
- build history surface
- artifact inspection
- basic settings and secret entry flows

Exit criteria:

- an operator can manage the local runtime through the desktop shell instead of
  manifests and CLI alone

### Phase 10 - Cutover And Legacy Removal

Objective: remove the old deployment assumptions from the product.

Deliverables:

- remove Docker-first docs from the target architecture
- remove Go runtime binaries from the shipped product
- archive or delete obsolete infrastructure code
- update onboarding and packaging docs

Exit criteria:

- the product ships as a Tauri app with bundled runtime and no mandatory Docker
  dependency

## Risks

### Risk: Big-bang rewrite loses domain behavior

Mitigation:

- translate in slices
- preserve current manifests as compatibility fixtures
- use the current Go behavior as the migration oracle

### Risk: Tauri shell becomes a thin wrapper over a still-Go product

Mitigation:

- define Rust runtime as the target from the start
- allow temporary coexistence only as a migration step
- set an explicit cutover phase that removes Go from the shipped product

### Risk: Host-native runners become too OS-specific too early

Mitigation:

- define a stable runner trait first
- isolate OS-specific process launch and Unity discovery behind adapters

### Risk: No-UI-first work stalls product momentum

Mitigation:

- keep runtime inspection tooling available during migration
- require clear runtime milestones before UI expansion

## Migration Acceptance Criteria

The migration should be considered successful when all of the following are
true:

- the product is distributed as a Tauri desktop application
- the product bundles a local runtime and starts it automatically
- the runtime no longer requires Docker or Redis for normal operation
- manifests can still define repository pipelines during the early desktop
  phase
- the runtime can detect host capabilities and choose the correct runner
- at least one host-native Unity runner performs a full release build without a
  visible editor window
- logs, artifacts, and failures are inspectable locally
- the shipped product no longer depends on the legacy Go runtime