# handy-unity-builder

handy-unity-builder is a self-hosted, local-first build orchestration service
for Unity repositories. It is not a Unity gameplay codebase. The server keeps
durable pipeline and execution state in SQLite, uses Redis only for transient
coordination, and launches GameCI-compatible Unity builds through the host
Docker daemon.

## Runtime Model

- `unity-build-api` exposes the HTTP management API, health endpoint, and runtime
	bootstrap.
- `unity-build-api` now also runs automatic repository polling and release planning
	coordination in the background for enabled repositories that have enabled
	build targets, serializes release backlogs per repository, and pauses polling
	for a repository while its current build backlog is still active.
- `unity-build-api` loads declarative pipeline manifests from `pipelines/` during
	startup and synchronizes them into the runtime state store before automation
	starts.
- `unity-build-worker` consumes queued build runs, prepares isolated workspaces,
	launches GameCI-compatible build containers, writes logs, and registers
	artifacts.
- `artifact-publish-worker` consumes queued publish runs and writes artifacts to the
	configured destination.
- `redis` provides transient queues, locks, idempotency keys, and worker
	signaling.
- The mounted data directory stores logs, artifacts, and repository workspaces.
- SQLite lives in a dedicated internal Docker volume and is intentionally kept
	out of the project tree.

## Local Development

Prepare the local environment file first:

```bash
cp .env.template .env
```

Then edit `.env` and set `HOST_DATA_DIR` to an absolute host path before
starting the stack.

Start the local stack:

```bash
docker compose up --build
```

Check that the server is alive:

```bash
curl "http://127.0.0.1:${APP_PORT:-8080}/healthz"
```

Inspect the effective runtime configuration from the development container:

```bash
docker compose exec unity-build-api go run ./cmd/hgb config
```

Run the operator-facing HTTP client from the project root:

```bash
go run ./cmd/hub help
go run ./cmd/hub dispatch revolutions v1.0.0
go run ./cmd/hub runtime automation
go run ./cmd/hub runtime pipelines
```

If you prefer a lightweight task runner at the repository root, the project
also ships a `package.json` with convenience wrappers such as `npm run
hub:help`, `npm run hub:runtime:automation`, `npm run hub:runtime:pipelines`,
and `npm run hub:dispatch -- revolutions v1.0.0`.

For a Go-native project surface, the repository also ships a `Makefile`.
Use `make help` to list the supported targets. The most common ones are
`make build`, `make test`, `make test-focused`, `make hub-runtime-automation`,
and `make hub-dispatch REPO=revolutions TAG=v1.0.0`.

Run the operator CLI from the development container:

```bash
docker compose exec unity-build-api go run ./cmd/hgb releases dispatch manual --repository-id 1 --git-tag v1.0.0
docker compose exec unity-build-api go run ./cmd/hgb releases plan --release-run-id 1
```

Declarative pipeline configuration lives under the repository-root
`pipelines/` directory. See [docs/pipeline-yaml-guide.md](docs/pipeline-yaml-guide.md)
for the manifest contract, a complete example, and the AI questionnaire flow.

In development, the Compose-backed `unity-build-api` service runs through
`air`, which watches `.yml` and `.yaml` changes under the workspace. Editing a
manifest file restarts the dev server so the updated pipeline set is loaded
again automatically.

With the main server running, enabled repositories that have enabled build
targets are polled automatically using their configured repository polling
interval. Queued releases are also planned automatically into queued build
runs, so the normal tag-driven path no longer requires a manual `cmd/poller`
or `hgb releases plan` step. When one polling pass sees more than one unseen
tag, the runtime queues all of them from the oldest tag to the newest tag, but
it only starts one release build process per repository at a time. A release
build process means all enabled build targets for that tag. Polling for that
repository stays paused until every build target of the current release reaches
a terminal status, even if some targets fail.

Use `hub runtime automation` to inspect that live queue state. It reports
whether each repository is `ready`, `scheduled`, `inactive`, or `paused`, and
when paused it includes the ordered release backlog that is blocking the next
poll.

The manual commands still exist for diagnostics, forced runs, and explicit
operator control.

The local Compose stack uses these service identities:

- `unity-build-api`
- `unity-build-worker`
- `artifact-publish-worker`
- `redis`

## Configuration

Configuration precedence is:

1. built-in defaults
2. JSON file referenced by `APP_CONFIG_PATH`
3. environment variables

Supported environment variables:

- `APP_CONFIG_PATH`
- `APP_ENV`
- `APP_PORT`
- `HTTP_ADDR`
- `DATA_DIR`
- `HOST_DATA_DIR`
- `PIPELINES_DIR`
- `APP_DB_PATH`
- `REDIS_ADDR`
- `REDIS_USERNAME`
- `REDIS_PASSWORD`
- `REDIS_DB`
- `LOG_LEVEL`

Example JSON config file:

```json
{
	"http_addr": ":8080",
	"data_dir": "/data",
	"host_data_dir": "/absolute/host/path/to/data",
	"pipelines_dir": "/workspace/pipelines",
	"database_path": "/var/lib/handy-unity-bulder/hub.db",
	"redis_addr": "redis:6379",
	"redis_db": 0,
	"log_level": "debug"
}
```

Important details:

- Docker Compose reads `.env` automatically. The repository ships
	[.env.template](.env.template) as the documented source template.
- `APP_PORT` is the simplest way to move the HTTP listener and the published
	Docker port together. `HTTP_ADDR` still exists for full bind-address
	overrides and wins over `APP_PORT` when both are set.
- `APP_DB_PATH` overrides `database_path`. In the local Compose stack it should
	point at the dedicated internal database volume path,
	`/var/lib/handy-unity-bulder/hub.db`.
- `PIPELINES_DIR` points at the declarative manifest directory. In the local
	Compose stack it should stay at `/workspace/pipelines`, which maps to the
	repository-root `pipelines/` directory.
- `HOST_DATA_DIR` must be an absolute host-visible path when workers launch
	build containers through `/var/run/docker.sock`. In Compose, this variable
	drives both the bind mount source and the host paths handed to the Docker
	daemon.
- SQLite export and import are explicit operator actions through `hub db
	export` and `hub db import`. Import is disruptive and restarts the app
	runtime containers after the snapshot upload completes.
- If `HOST_DATA_DIR` is left empty, the runtime falls back to `DATA_DIR`. That
	only works when the process and the Docker daemon resolve the same absolute
	filesystem path.

## Data Layout

The default data layout under `DATA_DIR` is:

- `logs/`
- `artifacts/`
- `workspaces/`

Operator-visible build outputs are grouped by release under:

- `artifacts/<repository-name>.<git-tag>/`

Inside that directory, each build target now lands at the canonical path:

- `<repository-name>.<git-tag>.<build-target><ext>`

Archive targets always end with `.zip`. Logs and temporary workspaces still
use `build-run-<id>` names because those paths are worker-internal execution
details, not release-facing artifact names.

The `repository-name` component is a normalized slug derived from the durable
repository name: lowercase only, spaces become `-`, and accents or other
special characters are stripped. For example, `Meu Repositório` becomes
`meu-repositorio`.

The repository-root `pipelines/` directory holds declarative repository
configuration. Runtime manifest files are intentionally git-ignored so each
operator or local environment can keep its own pipeline set without polluting
the repository history. SQLite stores durable runtime state. Redis does not
hold durable business state. Logs, build outputs, and checked-out repositories
stay on disk.

## Management Surfaces

### CLI

The primary operator CLI is exposed through `cmd/hub` and consumes the running
app over HTTP.

The lower-level in-runtime CLI remains exposed through `cmd/hgb`.

The `hub` CLI currently supports:

- `dispatch [--git-commit <sha>] [--rebuild] <repository> <git-tag>`
- `runtime automation`
- `runtime pipelines`
- `db export --path <target>`
- `db import --path <source>`
- `install` for user-local global installation

Important installation detail: `hub install` copies the currently running
binary. To refresh the globally installed CLI with the latest repository code,
run `go run ./cmd/hub install` from the project root.

`hub db import` restarts `unity-build-api`, `unity-build-worker`, and
`artifact-publish-worker` after a successful snapshot upload so every long-lived
process reopens the imported SQLite file. If your stack uses a non-default
Compose project name, set `HUB_COMPOSE_PROJECT` before running the import.

See the operator reference in [docs/hub-guide.md](docs/hub-guide.md).

The `hgb` CLI currently supports:

- runtime config inspection
- manual release dispatch
- release planning

See the internal CLI reference in [docs/cli-guide.md](docs/cli-guide.md).

Declarative repository setup is documented in
[docs/pipeline-yaml-guide.md](docs/pipeline-yaml-guide.md).

For the Unity-side `build_method` contract, runtime expectations, and
copy-paste-ready Editor scripts for multiple platforms, see
[docs/unity-build-methods.md](docs/unity-build-methods.md).

### HTTP API

Stable operational endpoints:

- `GET /`
- `GET /healthz`
- `GET /api/v1/runtime/automation`
- `GET /api/v1/runtime/pipelines`
- `GET /api/v1/runtime/database/export`
- `POST /api/v1/runtime/database/import`

## Declarative Configuration

Repository configuration no longer comes from CLI CRUD flows. Instead, the
server reads one YAML manifest per repository pipeline from `pipelines/`.

See [docs/pipeline-yaml-guide.md](docs/pipeline-yaml-guide.md) for:

- the directory contract
- the supported schema
- a full example
- a copy-ready template
- an AI questionnaire for collecting the required values safely

## Validation Status

The current repository has focused test coverage for configuration loading,
credentials management, Git authentication propagation, release dispatch,
release planning, build execution orchestration, publishing, and Redis-backed
coordination.

End-to-end coverage currently includes:

- trigger-rule CLI management
- manual release dispatch and queueing
- poll-driven release queueing
- batch poll sweep execution
- release planning into queued build runs
- server bootstrap, health, management CRUD, and persistence across restart

The main remaining proof gap for strict V1 is a full local GameCI smoke path
that exercises the Docker-backed build worker and the build-to-publish flow
under the complete container stack.

## Troubleshooting

- If build containers cannot see checked-out sources or artifact directories,
	verify that `HOST_DATA_DIR` points to the host path that matches the mounted
	`DATA_DIR` volume.
- If the server fails at startup, run `docker compose logs unity-build-api` and
	confirm that SQLite and Redis bootstrap both succeed.
- If polling or build planning must access a private repository, confirm that
	the repository references a valid credentials record and that the credential
	kind matches the remote Git server.
- If queue consumers appear idle, confirm that `redis` is healthy and that the
	corresponding worker service is running.
