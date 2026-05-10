# Architecture

See the companion delivery checklist in [V1 Delivery Plan](../planning/v1-delivery-plan.md).

## Overview

handy-unity-bulder is a self-hosted Unity release orchestration system built
for a single local or small-team host. The architecture keeps the HTTP server,
CLI entrypoints, build execution, and publish execution separated so that each
runtime stays narrow and operationally predictable.

The system is intentionally local-first:

- SQLite is the durable system of record.
- Redis is used only for transient coordination.
- Logs, artifacts, and workspaces stay on the filesystem.
- Docker is used to launch isolated Unity build containers.

## Runtime Components

### `unity-build-api`

The server exposes the operator-facing HTTP API and handles bootstrap for:

- configuration loading
- SQLite migrations and connection setup
- Redis connectivity checks
- health and root endpoints
- runtime inspection endpoints for declarative manifest sync and live
  automation state
- CRUD endpoints for credentials, repositories, build targets, trigger rules,
  publish targets, and build-publish bindings

The server should remain thin. It is responsible for validating and persisting
state, not for running long-lived Unity builds inline.

### `unity-build-worker`

The build worker consumes queued build jobs from Redis and is responsible for:

- claiming durable queued `build_runs`
- preparing repository workspaces for the requested Git tag
- resolving Git credentials for workspace sync when needed
- launching GameCI-compatible containers through the Docker CLI
- writing captured build logs to disk
- discovering and recording produced artifacts in SQLite
- planning downstream publish runs after a successful build

### `artifact-publish-worker`

The publish worker consumes queued publish jobs from Redis and is responsible
for:

- claiming durable queued `publish_runs`
- loading the execution plan that joins repository, release, target, and
  artifact metadata
- copying published artifacts through the selected publisher implementation
- persisting terminal publish status and destination references

### `redis`

Redis exists only for ephemeral coordination:

- job queues
- locks
- idempotency keys
- worker signaling

Business state must not drift into Redis. If Redis is lost, the durable truth
must still be reconstructible from SQLite and the filesystem.

## Durable Data Model

The initial SQLite schema centers on these entities:

- `credentials`
- `repositories`
- `build_targets`
- `trigger_rules`
- `publish_targets`
- `build_publish_bindings`
- `release_runs`
- `build_runs`
- `artifacts`
- `publish_runs`

These tables model one repository as a full release pipeline definition rather
than a simple watched Git remote.

## Filesystem Layout

The runtime expects a mounted data directory with this shape:

```text
/data/
  app.db
  logs/
  artifacts/
  workspaces/
```

Artifacts are grouped by release under `artifacts/<repository-name>.<git-tag>/`
and each target is normalized to `<repository-name>.<git-tag>.<build-target><ext>`.
Logs and workspaces keep execution-oriented `build-run-<id>` names because they
are worker-internal scratch and trace paths.

The `repository-name` portion is a slugged form of the durable repository name:
lowercase, spaces mapped to `-`, and accents or other special characters
removed.

`HOST_DATA_DIR` must point to the host-visible version of that path when the
application talks to the host Docker daemon through the mounted Docker socket.
This keeps bind mounts for GameCI containers consistent between the worker
container and the host daemon.

## Control Surfaces

### HTTP API

The server exposes JSON endpoints under `/api/v1/...` for operator management
of pipeline definitions. These endpoints use the same store contracts as the
CLI and do not embed orchestration logic that belongs in workers.

### CLI

`cmd/hgb` mirrors the operator actions required for:

- configuration inspection
- credentials CRUD
- repository CRUD
- build target CRUD
- trigger rule CRUD
- publish target CRUD
- build-publish binding CRUD
- manual release dispatch
- release polling
- release planning

The CLI is a thin transport over the same durable services used elsewhere.

## Release Lifecycle

The intended V1 flow is:

1. an operator registers credentials, a repository, build targets, publish
   targets, and bindings
2. a release is created by manual dispatch or polling
3. release planning resolves the Unity version and creates queued `build_runs`
   for exactly one repository-local release at a time
4. the build worker executes the build run and records artifacts
5. successful build results expand into queued `publish_runs`
6. the publish worker copies artifacts to the configured destination and
   records the destination reference

Each stage keeps an explicit durable status transition in SQLite.

### Polling and Repository-Local Sequencing

Repository polling now treats one repository as a serialized release lane.

- one polling pass can discover multiple unseen tags
- every unseen tag is queued in ascending order from the durable
  `last_seen_tag` baseline
- the runtime only starts one release build process per repository at a time
- one release build process means all enabled build targets for one Git tag
- polling for that repository stays paused while the current release still has
  queued or running build work
- the next queued tag only starts after every target of the current release
  reaches a terminal state, regardless of success or failure

This prevents the runtime from wasting tag-polling work while it already knows
that a repository still has release work waiting in front of the next poll.

Operators can inspect that state through `GET /api/v1/runtime/automation` or
`hub runtime automation`.

## Git and Credentials

Repositories can reference a credentials record. V1 currently supports:

- `git-http-basic`
- `git-http-bearer`

Those credentials are applied consistently across:

- tag polling
- Unity version detection during release planning
- workspace synchronization before build execution

## Build Execution Boundary

The build worker delegates container execution to the Docker integration
package. That package is responsible for translating one execution plan into a
GameCI-compatible `docker run` invocation with deterministic bind mounts and
environment variables.

This boundary matters because most release logic should stay testable without
running Docker. The worker and store logic can be exercised with stubbed
executors, while the Docker package carries the host-specific container launch
concerns.

## Publish Execution Boundary

The publish worker resolves one publish execution plan and delegates delivery
to a publisher implementation. V1 ships a filesystem publisher as the baseline
deterministic destination.

Remote publishers are intentionally deferred until the local-first path is
proven with stable credential handling, error reporting, and operator
diagnostics.

## V1 Scope Decision

Strict V1 includes the filesystem publisher only. Itch.io is deferred to V1.1.

The reason is architectural discipline:

- the filesystem publisher proves artifact selection and handoff locally
- Itch.io adds remote credential handling, API-specific retries, and external
  failure modes
- those concerns should be layered on top of a proven local publish path
  rather than bundled into the first completeness milestone