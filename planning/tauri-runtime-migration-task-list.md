# Tauri Runtime Migration Task List

## Purpose

This checklist tracks the work required to migrate handy-unity-bulder from the
current Go and Docker architecture to a Tauri desktop product with a bundled
local runtime.

This list assumes the strategic decisions documented in
[tauri-runtime-migration-strategy.md](./tauri-runtime-migration-strategy.md).

## Phase 0 - Decisions And Scope Lock

- [ ] Confirm the final product is a Tauri desktop application.
- [ ] Confirm the final shipped runtime is Rust-based.
- [ ] Confirm Docker is removed from the target product model.
- [ ] Confirm Redis is removed from the target product model.
- [ ] Confirm UI is deferred until the runtime works with manifests and local
  execution.
- [ ] Confirm host-native Unity runners are the default execution strategy.
- [ ] Confirm the desktop shell may supervise a separate bundled runtime
  process.
- [ ] Define the initial supported host matrix for the migration order.
- [ ] Decide the first host to fully support in the new runtime.
- [ ] Freeze the current manifest contract unless a migration blocker requires a
  deliberate format change.

## Phase 1 - Repository Restructure

- [ ] Create a Cargo workspace at the repository root.
- [ ] Add a minimal Tauri shell scaffold.
- [ ] Add a minimal runtime binary crate.
- [ ] Add runtime core crates for domain, storage, manifests, git, runner, and
  publishing.
- [ ] Define the target repository layout for desktop, runtime, and shared
  crates.
- [ ] Decide how frontend assets will live before real UI work begins.
- [ ] Document how the runtime is started in development without relying on the
  current Go and Docker flow.
- [ ] Mark Dockerfile and docker-compose as legacy development artifacts rather
  than target architecture.

## Phase 2 - Runtime Bootstrap And Supervision

- [ ] Implement runtime startup and shutdown paths.
- [ ] Implement structured logging for the runtime.
- [ ] Implement application data directory resolution per platform.
- [ ] Implement runtime health reporting.
- [ ] Implement shell-to-runtime process supervision contract.
- [ ] Implement runtime restart policy for recoverable crashes.
- [ ] Define runtime version handshake between the desktop shell and runtime.
- [ ] Add a minimal local diagnostics view or command for runtime status.

## Phase 3 - Local Storage And State

- [ ] Port SQLite bootstrap and migration execution to Rust.
- [ ] Recreate the durable schema for credentials, repositories, build targets,
  trigger rules, publish targets, bindings, release runs, build runs,
  artifacts, and publish runs.
- [ ] Recreate path conventions for logs, artifacts, and workspaces under app
  data directories.
- [ ] Replace Redis queue semantics with a SQLite-backed or in-process local
  queue and lease model.
- [ ] Define runtime-safe concurrency limits for a single local host.
- [ ] Port status transition rules and invariants from the Go implementation.
- [ ] Add recovery behavior for runtime restarts during queued or running work.

## Phase 4 - Manifest Compatibility

- [ ] Port manifest loading and validation to Rust.
- [ ] Port credential source resolution semantics.
- [ ] Port manifest synchronization into durable runtime state.
- [ ] Preserve repository, build target, publish target, and binding semantics.
- [ ] Add compatibility fixtures using current manifest examples.
- [ ] Add manifest failure diagnostics with actionable error messages.
- [ ] Define whether manifest editing remains file-based during the no-UI stage.

## Phase 5 - Secrets And Configuration

- [ ] Define persistent non-secret configuration storage.
- [ ] Define OS-native secret storage integration strategy.
- [ ] Implement secret references for Git credentials.
- [ ] Implement secret references for Unity credentials when needed.
- [ ] Replace `.env` as the primary operator setup model.
- [ ] Define import or migration behavior for existing local `.env`-based setups.
- [ ] Add runtime diagnostics for missing or invalid secrets.

## Phase 6 - Host Capability Detection

- [ ] Implement host OS and architecture detection.
- [ ] Detect whether the runtime is inside WSL.
- [ ] Detect installed Unity editors.
- [ ] Detect Unity versions and executable paths.
- [ ] Detect the local license context for the runtime environment.
- [ ] Detect available Git tooling.
- [ ] Detect platform-specific build prerequisites where practical.
- [ ] Produce a capability profile that the runtime can inspect and surface.
- [ ] Implement a runner selection service based on capability profiles.

## Phase 7 - Host-Native Unity Execution

- [ ] Define the common runner trait or interface.
- [ ] Implement host Windows Unity runner.
- [ ] Implement host Linux Unity runner.
- [ ] Design host macOS Unity runner.
- [ ] Implement background Unity process launch without a visible editor window.
- [ ] Capture stdout, stderr, and Editor.log consistently.
- [ ] Preserve artifact path conventions across runners.
- [ ] Implement timeout, cancellation, and forced termination semantics.
- [ ] Implement failure classification for licensing, compile, package, and
  runtime errors.
- [ ] Verify that builds can run headlessly on the first supported host.

## Phase 8 - Workspace And Git Flow

- [ ] Port workspace preparation logic to Rust.
- [ ] Port Git authentication handling to Rust.
- [ ] Port repository clone, fetch, checkout, and clean behavior.
- [ ] Preserve tag-based workspace materialization semantics.
- [ ] Improve clone and fetch diagnostics so exit codes always include command
  stderr in persisted failures.
- [ ] Add tests for private repository access and credential failures.

## Phase 9 - Release Orchestration

- [ ] Port manual dispatch behavior.
- [ ] Port polling logic.
- [ ] Port release planning behavior.
- [ ] Port build run claiming and execution flow.
- [ ] Port artifact discovery and recording.
- [ ] Port publish run planning after successful builds.
- [ ] Preserve repository-local sequencing rules for queued releases.
- [ ] Preserve automation inspection surfaces for debugging.

## Phase 10 - Publishing

- [ ] Port the filesystem publisher as the first publish path.
- [ ] Port publish execution planning.
- [ ] Port publish status transitions and error reporting.
- [ ] Preserve artifact-to-publish binding semantics.
- [ ] Add local diagnostics for published outputs.

## Phase 11 - Runtime-First Developer Experience

- [ ] Add a runtime CLI or local developer commands for health inspection.
- [ ] Add a runtime command for manifest sync.
- [ ] Add a runtime command for manual release dispatch.
- [ ] Add a runtime command for automation inspection.
- [ ] Add a runtime command for log and artifact path inspection.
- [ ] Document the no-UI development loop for Windows-first development.

## Phase 12 - Tauri Shell Integration

- [ ] Start the runtime automatically from the Tauri shell.
- [ ] Surface runtime health in the shell.
- [ ] Surface runtime logs in the shell.
- [ ] Add settings for runtime directories.
- [ ] Add settings for Unity discovery and runner diagnostics.
- [ ] Add settings for secret entry or secret binding.
- [ ] Define app lifecycle rules for quit, restart, and runtime crash recovery.

## Phase 13 - Initial UI After Runtime Stability

- [ ] Add repository inspection UI.
- [ ] Add build history UI.
- [ ] Add release status UI.
- [ ] Add artifact inspection UI.
- [ ] Add runtime diagnostics UI.
- [ ] Add secret and credential management UI.

## Phase 14 - Migration And Cleanup

- [ ] Decide the final cutover point where the Go runtime stops being shipped.
- [ ] Remove legacy Go runtime binaries from the product packaging path.
- [ ] Remove Docker and Redis from onboarding docs.
- [ ] Archive or remove obsolete Go runtime entrypoints.
- [ ] Archive or remove obsolete Docker integration code.
- [ ] Rewrite README and architecture docs around the desktop product model.
- [ ] Add packaging and installation docs for Windows, Linux, and macOS.
- [ ] Define upgrade or data migration behavior for existing local SQLite data.

## Cross-Cutting Testing Tasks

- [ ] Build a fixture suite that compares Rust runtime behavior against current
  Go behavior for core domain flows.
- [ ] Add focused unit tests for manifest parsing, storage, status transitions,
  and runner selection.
- [ ] Add integration tests for SQLite migrations and local runtime bootstrap.
- [ ] Add smoke tests for one full dispatch-to-artifact flow on the first
  supported host.
- [ ] Add regression coverage for Unity licensing and no-window execution.
- [ ] Add regression coverage for Git clone and credential failures.

## Cross-Cutting Documentation Tasks

- [ ] Maintain a living translation ledger from Go modules to Rust runtime
  modules.
- [ ] Adapt `.github/copilot-instructions.md` to the Tauri desktop and bundled
  runtime development model so future agents stop assuming Go, Docker, Redis,
  and GameCI as the target architecture.
- [ ] Document the host capability model.
- [ ] Document the runner model.
- [ ] Document the secret storage model.
- [ ] Document the runtime supervision contract between the Tauri shell and the
  bundled runtime.

## Ready-For-UI Runtime Exit Criteria

- [ ] The runtime starts locally without Docker.
- [ ] The runtime persists state in SQLite.
- [ ] The runtime loads current manifests successfully.
- [ ] The runtime can detect host capabilities.
- [ ] The runtime can select a compatible runner.
- [ ] The runtime can execute at least one headless Unity build on a supported
  host.
- [ ] The runtime can record logs, artifacts, and failures locally.
- [ ] The runtime can complete a manual release dispatch without depending on
  the old Go and Docker stack.