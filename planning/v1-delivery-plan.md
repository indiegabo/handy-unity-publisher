# V1 Delivery Plan

## Purpose

This document defines the first delivery plan for handy-unity-bulder.

See [Architecture](../docs/architecture.md) for the runtime component model,
durable storage boundaries, and worker responsibilities that back this plan.

The goal of V1 is to ship a self-hosted, local-first release automation tool
that can:

- register Unity repositories as pipeline definitions
- detect new tags from configured repositories
- resolve the required Unity version for a tag
- launch isolated Docker-based Unity builds
- collect artifacts and store execution state
- publish artifacts through at least one supported publish flow

V1 should prioritize a complete end-to-end path over breadth.
The first version must be operable in Docker on a single local host and remain
simple enough to run in WSL-oriented development environments.
It must also establish a modular, worker-oriented architecture that can scale
without collapsing into a monolithic server process.

## Product Boundaries

### In Scope for V1

- Go application runtime
- SQLite persistence in a mounted host volume
- Redis service for transient coordination in local and deployed runtime
- Docker Compose local runtime
- Docker-based development flow with live reload
- Redis-backed queue, locking, and idempotency coordination
- Repository configuration CRUD
- Multiple trigger sources with explicit trigger rules
- Build target CRUD
- Publish target CRUD
- Build-to-publish bindings
- Polling for tags
- Manual release dispatch
- Release run creation
- Unity version detection from repository contents
- GameCI-compatible build execution in ephemeral containers
- Artifact tracking on the filesystem
- Publish run orchestration
- Worker-oriented background execution boundaries
- One usable publish path for the first release
- Minimal operator-facing API and CLI surfaces
- Focused unit tests for core logic and state transitions
- End-to-end validation for critical operator workflows

### Out of Scope for V1

- Fully elastic, multi-region worker fleets
- Full web frontend
- Rich authentication and authorization layers
- Multi-broker or cloud-managed queue infrastructure
- Large log storage inside SQLite
- Multiple external publish integrations shipped at once
- Broad cloud deployment support

## Operating Assumptions

- The application runs inside Docker.
- The runtime includes a Redis service alongside the application.
- The application can access the Docker socket or Docker API.
- SQLite lives on a mounted host path such as `/data/app.db`.
- SQLite remains the durable system of record for configuration and execution
  state.
- Redis is used for transient coordination concerns such as queues, locks,
  idempotency keys, and short-lived cache entries.
- Logs, artifacts, and workspaces live on mounted filesystem paths.
- A repository is a full pipeline definition, not a watch-only entry.
- Build containers are ephemeral and disposable.
- Background work should move through delegated workers or services instead of
  growing indefinitely inside the main server process.
- Thin entrypoints and explicit package boundaries are required to avoid a
  monolithic codebase.
- The initial release remains intentionally lightweight.

## Architectural Principles

- Keep HTTP handlers, CLI commands, schedulers, and workers thin.
- Model durable workflow state in SQLite and transient coordination in Redis.
- Prefer message-driven handoff for background work instead of deeply nested
  in-process orchestration chains.
- Introduce queue, lock, and idempotency interfaces at consumer boundaries.
- Split responsibilities into focused packages or services; avoid god objects
  and giant orchestration methods.
- Make worker job contracts explicit, serializable, and easy to validate.
- Treat a task as complete only after the related unit and end-to-end checks
  have been executed successfully for the affected workflow.

## Testing Strategy

- Prefer focused unit tests and table-driven tests for pure logic, validation,
  normalization, and state transitions.
- Add integration tests when behavior crosses boundaries such as SQLite,
  Redis, CLI commands, HTTP handlers, migrations, or release orchestration.
- Add end-to-end tests or smoke flows for critical operator journeys such as
  bootstrap, repository management, manual dispatch, polling, build execution,
  and publishing.
- Keep Docker-heavy end-to-end coverage concentrated on the smallest critical
  flows so most behavior remains testable without launching full runtime
  dependencies.
- A delivery slice is not done until the related unit and end-to-end checks
  pass.

## Delivery Strategy

### Phase 0 - Foundation Decisions

Objective: lock the basic operational direction before implementation spreads.

Deliverables:

- Confirm that the current repository name remains unchanged for now.
- Align documentation around Go, SQLite, Docker, and GameCI.
- Define the V1 acceptance criteria.

Exit criteria:

- Team agrees on the first usable scope.
- Documentation no longer describes the project as a gameplay codebase.

### Phase 1 - Project Bootstrap

Objective: create the runnable skeleton of the application.

Deliverables:

- Go module initialization
- `cmd/server` entrypoint
- `cmd/hgb` CLI entrypoint
- base `internal/` package layout
- configuration loading
- structured logging setup
- Dockerfile
- Docker Compose runtime for local development
- Redis service for local runtime
- Redis connection configuration
- live reload configuration for containerized development

Exit criteria:

- `docker compose up` starts the application and Redis services.
- The server and CLI binaries build successfully.
- The local runtime can establish a Redis connection.
- editing Go source in the mounted workspace triggers an automatic rebuild or
  restart in the development container.

### Phase 2 - Persistence and Migrations

Objective: establish durable local state.

Deliverables:

- migration runner
- initial schema for core entities
- SQLite bootstrap with WAL mode and busy timeout
- durable versus transient state boundaries between SQLite and Redis
- queue, lock, and idempotency interfaces at consumer boundaries
- repository layer for core reads and writes
- filesystem data directory conventions

Exit criteria:

- database initializes from scratch through migrations
- core tables exist with foreign keys and unique constraints
- durable execution state remains in SQLite rather than drifting into Redis
- restart preserves data on the mounted host volume

### Phase 3 - Repository Pipeline Management

Objective: make repository pipelines configurable.

Deliverables:

- repository CRUD
- credential registration and lookup
- build target CRUD
- publish target CRUD
- binding CRUD
- validation rules for invalid pipeline definitions
- minimal API and CLI commands for management

Exit criteria:

- an operator can define one full repository pipeline without manual DB edits
- invalid pipeline configuration is rejected with clear errors

### Phase 4 - Triggering and Release Detection

Objective: accept or detect work to perform from explicit trigger sources in a
deterministic way.

Deliverables:

- trigger rule schema for manual, polling, and future webhook sources
- manual dispatch surface for operators
- Git access abstraction
- repository clone or fetch workflow
- tag polling logic
- tag deduplication and idempotency rules
- release run creation with lifecycle status
- Redis-backed dispatch of downstream release work
- worker loop or scheduled polling path

Exit criteria:

- an operator can manually dispatch a release run for a configured repository
  and tag
- polling a repository with a new tag creates exactly one release run
- every release run records its trigger source and optional trigger rule
- downstream work is dispatched exactly once for the detected release
- repeated polling does not duplicate the same release run

### Phase 5 - Build Planning

Objective: translate a repository tag into executable build work.

Deliverables:

- Unity version detection from repository files
- GameCI image resolution rules
- build run creation for enabled targets
- serializable build job payloads for workers
- workspace preparation rules
- timeout and retry policy definition
- status transitions for queued and planned work

Exit criteria:

- a release run expands into the correct build runs
- the selected Unity image is traceable and deterministic
- planned build jobs are serialized and enqueued exactly once for repeated
  planning of the same queued release

### Phase 6 - Build Execution

Objective: run isolated Unity builds and collect outputs.

Deliverables:

- Docker client integration package
- worker execution path that claims build jobs from Redis
- lock or lease discipline for claimed jobs
- ephemeral build container execution
- mounted workspace and artifact paths
- log capture to filesystem
- artifact discovery and registration
- build status updates for success and failure

Exit criteria:

- one configured target can build successfully inside Docker after worker
  dispatch through Redis-backed coordination
- failed builds retain useful logs and terminal state
- produced artifacts are recorded in SQLite and present on disk

### Phase 7 - Publishing

Objective: move built artifacts to a destination.

Deliverables:

- publish orchestration flow driven by bindings
- publish run creation and state tracking
- publisher interface at the consumer boundary
- publish job dispatch for worker execution
- one practical publish implementation for V1

Recommendation:

- implement a filesystem publisher for deterministic local validation
- optionally add Itch.io as the first real remote publisher after the local
  flow is stable

Exit criteria:

- a successful build can trigger at least one publish run through the
  coordinated worker flow
- published output is traceable from release run to artifact to destination

### Phase 8 - Operational Hardening

Objective: make the first release usable and diagnosable.

Deliverables:

- health and readiness checks
- queue and worker diagnostics
- startup validation for critical configuration
- structured error handling
- operator documentation
- focused unit test coverage for critical domain logic
- smoke-test flow for local Docker environments
- end-to-end coverage for critical operator workflows

Exit criteria:

- operators can bootstrap, run, and troubleshoot the system locally
- failure modes are visible without attaching a debugger
- critical delivery slices have related unit and end-to-end coverage that runs
  green

## Task List

### Foundation

- [x] Finalize the V1 acceptance criteria in documentation
- [x] Keep the current repository naming until a dedicated rename decision
- [x] Create an architecture document linked to this plan

### Bootstrap

- [x] Initialize the Go module
- [x] Create `cmd/server/main.go`
- [x] Create `cmd/hgb/main.go`
- [x] Create the initial `internal/` package tree
- [x] Implement configuration loading from environment and files
- [x] Add structured logging
- [x] Create the application Dockerfile
- [x] Update Docker Compose for local runtime and mounted data paths
- [x] Add Redis service to Docker Compose local runtime
- [x] Define Redis connection configuration
- [x] Assign explicit Docker service identities without numeric suffixes
- [x] Add containerized live reload for Go development
- [x] Add a dedicated `unity-build-worker` runtime service for queued build execution

### Persistence

- [x] Add a migration runner
- [x] Create the first migration for core tables
- [x] Enable WAL mode and busy timeout in SQLite bootstrap
- [x] Define durable versus transient state boundaries between SQLite and Redis
- [x] Add queue, lock, and idempotency interfaces at consumer boundaries
- [x] Define repository-layer interfaces through consumer needs
- [x] Establish `/data`, `/data/logs`, `/data/artifacts`, and
      `/data/workspaces` conventions

### Pipeline Configuration

- [x] Implement repository CRUD
- [x] Implement credentials CRUD or secure registration flow
- [x] Implement build target CRUD
- [x] Implement publish target CRUD
- [x] Implement build-publish binding CRUD
- [x] Add validation for duplicate names, invalid bindings, and missing refs
- [x] Add CLI commands for core management operations
- [x] Add minimal HTTP endpoints for management operations

Status note:

- The HTTP server now exposes minimal CRUD-style management endpoints for
  credentials, repositories, build targets, trigger rules, publish targets,
  and build-publish bindings, backed directly by the same SQLite stores used
  by the CLI.
- Repository definitions can now reference stored Git credentials, and both
  the CLI and HTTP API validate the supported credential kinds before those
  records can be used downstream.
- Focused handler tests cover the new routes, and the restart/persistence e2e
  flow exercises the live server against real SQLite state across a full
  process restart.

### Polling and Release Runs

- [x] Formalize multiple trigger sources and trigger rule schema
- [x] Expose trigger rule CRUD for repository trigger declarations
- [x] Implement manual dispatch CLI for release runs
- [x] Implement Git authentication loading
- [x] Implement clone and fetch workflow
- [x] Implement tag discovery
- [x] Persist last observed tag or equivalent polling state
- [x] Create release runs for unseen tags
- [x] Enqueue downstream release work in Redis
- [x] Add Redis-backed idempotency and locking around queue dispatch
- [x] Guarantee idempotency across repeated polls
- [x] Add a worker or scheduler entrypoint

### Build Planning and Execution

- [x] Detect Unity version from repository contents
- [x] Resolve the correct GameCI image
- [x] Expand one release run into build runs
- [x] Define serializable build job payloads
- [x] Add a dedicated worker process or service boundary for build execution
- [x] Prepare isolated workspaces for each build run
- [x] Consume build jobs from Redis-backed workers
- [x] Wire a durable execution processor to a Docker-backed GameCI executor
- [x] Add a dedicated `cmd/build-worker` entrypoint and Compose runtime wiring
- [x] Validate the worker container startup path and required `docker`/`git` binaries
- [x] Execute build containers through Docker
- [x] Capture build logs to disk
- [x] Discover and register produced artifacts
- [x] Persist build status transitions and failure reasons

Status note:

- Docker-backed build execution, workspace preparation, log writing, durable
  execution plans, and the dedicated worker runtime are implemented and
  validated through focused tests plus container startup checks.
- Artifact discovery now walks the prepared artifact root, records relative
  files in SQLite, and fails the build when execution produces no regular
  files under the mounted artifact directory.
- The remaining build-execution gap is proof, not missing implementation: a
  real local GameCI smoke path still needs to exercise those pieces together
  under the full container stack.

### Publishing

- [x] Add durable publish target and build-publish binding management surfaces
- [x] Expand build results into publish runs using bindings
- [x] Define a publisher contract
- [x] Queue publish jobs for Redis-backed workers
- [x] Implement a local filesystem publisher
- [x] Record publish status transitions and output metadata
- [x] Decide whether Itch.io is part of strict V1 or immediate V1.1

Status note:

- Successful build workers now expand registered artifacts through enabled
  build-publish bindings, persist queued `publish_runs`, and dispatch those
  publish jobs into Redis with lock/idempotency coordination.
- A dedicated `publish-worker` runtime now claims queued `publish_runs`,
  resolves the joined execution plan, copies filesystem targets into the
  configured destination root, and records `destination_ref` plus
  running/succeeded/failed lifecycle transitions durably in SQLite.
- Strict V1 remains intentionally filesystem-only for publishing. Itch.io is
  deferred to V1.1 so the first release can prove local artifact handoff,
  credentials discipline, and operator diagnostics without adding remote API
  behavior to the acceptance bar.

### Operations and Quality

- [x] Add unit tests for config parsing and validation
- [x] Add unit tests for tag deduplication and status transitions
- [x] Add unit tests for Unity version and image resolution
- [x] Add unit tests for release planning, dispatch validation, and state transitions
- [x] Add integration tests for migrations
- [x] Add tests for Redis-backed coordination contracts
- [x] Add integration tests for repository pipeline CRUD
- [x] Add end-to-end coverage for CLI trigger-rule management
- [x] Add end-to-end coverage for manual dispatch and release queueing flows
- [x] Add end-to-end coverage for polling and release queueing flows
- [x] Add end-to-end coverage for the batch poll scheduler entrypoint
- [x] Add end-to-end coverage for release planning into queued build runs
- [x] Add an end-to-end smoke test for one local release flow
- [x] Add end-to-end coverage for bootstrap, health, and persistence validation
- [x] Document `HOST_DATA_DIR` and Docker socket path mapping for worker-driven builds
- [x] Document local bootstrap and troubleshooting steps

Status note:

- The server-management end-to-end flow now boots the real HTTP server process,
  validates `/healthz`, exercises management CRUD across credentials,
  repositories, build targets, trigger rules, publish targets, and bindings,
  then restarts the process and verifies durable persistence from SQLite.
- The internal app test suite now includes a full pipeline CRUD integration
  flow against a real SQLite database through the HTTP handler surface,
  covering credentials, repositories, build targets, trigger rules, publish
  targets, and build-publish bindings together.
- The end-to-end suite now includes a local release smoke test that drives the
  full `dispatch -> plan -> build worker -> publish worker` path with real
  SQLite state, real Redis queue semantics, artifact registration, and
  filesystem publication.
- The README now documents configuration precedence, `APP_CONFIG_PATH`,
  `HOST_DATA_DIR`, the Docker socket assumptions, and the local operator
  bootstrap path.

## Recommended Build Order

The implementation sequence should optimize for early end-to-end proof:

1. Bootstrap runtime and configuration
2. Persistence and migrations
3. Redis runtime and coordination interfaces
4. Repository pipeline CRUD
5. Trigger rules, manual dispatch, and release run creation
6. Worker dispatch and build planning
7. Docker build execution and artifact tracking
8. Filesystem publisher and publish worker flow
9. CLI and HTTP refinement
10. Hardening, tests, and operator documentation

## V1 Acceptance Criteria

V1 is considered complete when the system can do the following on a single
local Docker host:

1. Start through Docker Compose with persistent mounted data paths and a Redis
  service.
2. Support a development workflow where code changes trigger automatic rebuild
  or restart inside the app container.
3. Register one repository with credentials, build targets, publish targets,
   and bindings.
4. Manually dispatch one release run for a configured repository and tag.
5. Poll the repository and detect a new tag without creating duplicates.
6. Dispatch background work through Redis-backed coordination without
  duplicating execution.
7. Resolve the Unity version and select a compatible build image.
8. Execute at least one build target in an ephemeral container.
9. Persist release, build, artifact, and publish state in SQLite.
10. Write logs and artifacts to the filesystem.
11. Publish the resulting artifact through at least one supported path.
12. Expose enough CLI or HTTP surface to operate the workflow manually.
13. Maintain focused unit coverage for critical logic and end-to-end coverage
  for critical operator workflows.
14. Survive restart without losing database state or produced artifacts.

## Risks and Watchpoints

- The project name likely contains a typo, but renaming too early can create
  churn across module names and Docker surfaces.
- Unity version detection can fail on unusual repository layouts and should be
  isolated behind a testable component.
- Docker socket access is powerful and should be kept explicit and narrow.
- Redis must remain a transient coordination layer rather than becoming the
  durable source of truth for business state.
- Poor package boundaries can recreate a monolith even if multiple services
  exist on paper.
- Publishing integrations can expand scope aggressively; V1 should keep a hard
  limit on the number of supported destinations.
- SQLite is valid for V1, but long write transactions and blob misuse will
  create avoidable pain.

## Suggested Immediate Next Tasks

The next tasks to attack, in order, are:

1. Add a real local GameCI smoke test that exercises the worker, Docker executor, log capture, artifact registration, and durable build status transitions together.
2. Extend that smoke path to prove the same path under the full container stack rather than only the in-process test harness.
3. Add broader integration coverage for publish-management error paths and multi-binding publish plans.
4. Reassess the next remote publisher only after the GameCI-backed local smoke path is stable.