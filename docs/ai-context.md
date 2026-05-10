# AI Context

Conversation Reference: hgb-conversation-2026-05-08-01
Project Repository: indiegabo/handy-unity-bulder

Purpose of this file

This file is intended to give any AI agent working in VS Code or GitHub Copilot Chat the full working context of the project as defined in the initial design conversations.

The goal is that a new agent can read this file and continue implementation without having to recover the original chat.

Project summary

handy-unity-bulder is a self-hosted build orchestration tool for Unity projects.

It is intended to run locally in Docker, initially on the developer's WSL environment, and automate the following workflow:

1. register Unity repositories
2. monitor those repositories for new tags
3. when a new tag is found:
   - fetch repository contents
   - detect the Unity version used by that tag
   - choose the correct build environment
   - run Unity builds in Docker using GameCI or compatible images
   - collect artifacts
   - publish artifacts to one or more configured publication targets

The application should support multiple repositories, each with independent
configuration.

Configuration source of truth

- Repository configuration now comes from YAML manifests stored under the
  repository-root `pipelines/` directory.
- One YAML file defines one repository pipeline.
- `hub` and `hgb` are no longer the supported path for repository CRUD.
- The runtime loads and validates all `.yml` and `.yaml` files from
  `pipelines/` during server startup.
- Valid manifests are synchronized into the runtime state store before the
  automation coordinator begins polling and planning work.
- Invalid manifests are logged and skipped without blocking valid pipelines.

Core product idea

Each declared repository is not just a Git repository entry.

Each repository manifest acts as a release pipeline definition.

That means a repository must define:

- how to access the source repository
- how often to poll/check for new tags
- which Unity build targets should be generated
- which publication targets should receive artifacts
- which build targets map to which publication targets

Example:

- build targets:
  - Windows
  - Linux
  - WebGL

 -> Itch.io
  - Linux -> Itch.io
  - WebGL -> Itch.io
  - WebGL -> Google Drive

This mapping is important because not every build goes to every publication target.

Main technical decisions already made

Language
- The application should be written in Go.

Runtime coordination
- Redis should be available alongside the application runtime.
- Redis should handle transient coordination concerns such as queues, locks,
  idempotency keys, and short-lived cache entries.
- SQLite remains the durable store for workflow state.

Initial state store
- The runtime continues to use SQLite.
- The intention is to keep the system lightweight and easy to run locally.
- SQLite is considered sufficient for the initial single-node / local Docker
  setup.
- The recent declarative redesign did not replace SQLite because export/import,
  durable release history, build runs, and crash recovery still need a compact
  local durable store.

SQLite persistence requirement
- The SQLite database file now lives in a dedicated internal Docker volume.
- Operators can still export and import the database through `hub db export`
  and `hub db import`.
- The database must not live only inside ephemeral container storage.

Example internal path:

/data/app.db

With Docker volume mapping such as:

./data:/data

This allows:
- persistence across container restarts
- backups
- local inspection
- exporting/copying the .db file from the host

Storage strategy
SQLite stores:
- synchronized runtime metadata derived from YAML manifests
- release/build/publish state
- metadata
- references to files

The declarative `pipelines/` directory stores:
- repository definitions
- credentials references
- build target definitions
- publish target definitions
- build/publish bindings

The filesystem should store:
- build logs
- artifacts
- temporary workspaces

Suggested structure:

/data/
  app.db
  logs/
  artifacts/
  workspaces/

Operator-visible artifact naming:
- artifacts are grouped as `artifacts/<repository-name>.<git-tag>/`
- each target is normalized to `<repository-name>.<git-tag>.<build-target><ext>`
- `repository-name` is slugged to lowercase ASCII with spaces as `-` and no
  accents or special characters
- archive targets end in `.zip`
- workspaces and logs may still use `build-run-<id>` because those are
  execution-internal paths

Runtime
- The app itself should run inside Docker.
- The local Docker runtime should also include a Redis service for background
  coordination.

Build execution strategy
- The main app container should be able to orchestrate build containers.
- The main app should access the host Docker daemon via mounted Docker socket.
- Build jobs should run in ephemeral containers.
- Background workers should claim build and publish work through Redis-backed
  coordination rather than embedding all long-running execution inside the main
  HTTP process.

Example capability:
- app container calls Docker API/socket
- launches GameCI-based Unity build containers
- waits for completion
- collects artifacts
- updates execution status in SQLite

Unity build containers
The initial build execution strategy is based on:
- Docker
- GameCI images, or compatible Unity build images

The system must be able to choose the appropriate image/version based on the Unity version detected in the repository/tag.

Important product behavior

Repository manifests must include pipeline configuration

A declared repository needs more than Git access.

Each repository must carry configuration for:

1. Git source access
2. build targets
3. publication targets
4. bindings between build targets and publication targets

Build targets
Examples:
- WebGL
- Windows
- Linux
- macOS
- Android
- iOS
- PlayStation
- Nintendo

Suggested target fields:
- name
- platform
- runner type
- build method
- output kind
- output path template hint
- optional Unity version override
- optional image override
- timeout
- config JSON / structured config

The configured output path template no longer defines the final operator-
visible filename on disk. It acts as a hint for output style or extension,
while the runtime canonicalizes the stored artifact name by repository, tag,
and target.

AI agents that create build targets must read `docs/unity-build-methods.md`
before inventing or changing `build method` values.

AI agents that create or edit repository pipeline manifests must read
`docs/pipeline-yaml-guide.md` before writing files under `pipelines/`.

Publication targets
Examples:
- Itch.io
- Steam
- Google Drive

Suggested fields:
- name
- type
- credentials reference
- enabled flag
- target-specific config

Bindings
A binding connects:
- one build target
to
- one publication target

This is needed so the system knows which artifact goes where.

Bindings may later contain target-specific options such as:
- artifact selector
- upload rename template
- Steam depot/channel info
- Itch channel
- Google Drive folder
- compression behavior

Release processing flow

When a new tag is detected for a repository:

1. create a release run
2. load enabled build targets for that repository
3. create one build run per build target
4. execute each build
5. collect resulting artifacts
6. inspect bindings for that build target
7. create publish runs for each linked publication target
8. publish artifacts
9. consolidate final release status

Runtime automation

- The main server now performs background polling for enabled repositories that
  have enabled build targets.
- The polling cadence comes from `repositories.polling_interval_seconds`.
- The runtime also plans queued releases into queued build runs automatically.
- AI agents should treat `cmd/poller` and `hgb releases plan` as diagnostic or
  manual override surfaces, not as mandatory steps for the normal runtime path.

Conceptual hierarchy

Repository
  -> ReleaseRun (tag v1.2.0)
      -> BuildRun (windows)
      -> BuildRun (linux)
      -> BuildRun (webgl)
          -> PublishRun (itch)
          -> PublishRun (gdrive)

Database direction

The durable runtime state store remains SQLite. Configuration now starts from
YAML, not from operator CRUD commands.

Tables considered core
- credentials
- repositories
- build_targets
- publish_targets
- build_publish_bindings
- release_runs
- build_runs
- artifacts
- publish_runs

SQLite notes
Use SQLite with:
- WAL mode
- short transactions
- limited write concurrency
- logs/artifacts outside the DB

Redis notes
Use Redis for:
- transient queues
- locks and leases for workers
- idempotency keys and short-lived coordination state
- optional short-lived caches

Do not use Redis as the canonical source of release, build, artifact, or
publish state.

Suggested SQLite practices:
- PRAGMA journal_mode=WAL;
- PRAGMA busy_timeout = 5000;

Naming concern

There was inconsistency during discussion:

- original idea mentioned handy-game-builder
- repository created as handy-unity-bulder

Important:
- bulder appears to be a typo for builder

The project should decide its official naming soon to avoid inconsistency in:
- module name
- binary name
- Docker image names
- docs
- CLI name

Until explicitly changed, the existing repository name is:

indiegabo/handy-unity-bulder

Suggested project layout

cmd/
  server/
  hgb/

internal/
  app/
  build/
  cli/
  config/
  credentials/
  db/
  docker/
  git/
  publish/
  release/
  repository/
  worker/

docs/
  architecture.md
  decisions/

Directory intent

- cmd/server/
  - main application entrypoint
- cmd/hgb/
  - CLI entrypoint
- internal/config/
  - configuration loading
- internal/db/
  - SQLite setup, migrations, repositories
- internal/git/
  - repository polling, cloning, tag inspection
- internal/build/
  - build target execution logic
- internal/docker/
  - Docker integration
- internal/publish/
  - publishers (itch, steam, gdrive, etc.)
- internal/release/
  - release orchestration
- internal/worker/
  - job loops / schedulers / workers
- internal/credentials/
  - secret handling, encryption, validation

Expected system characteristics

The tool is intended to be:

- self-hosted
- local-first
- simple to operate
- containerized
- extensible
- scalable through delegated services and workers
- modular and non-monolithic
- build-target aware
- publish-target aware

It is not intended initially to be:
- a large SaaS
- multi-tenant cloud service
- highly distributed system
- heavily concurrent write-heavy platform

First implementation priorities

Suggested order of implementation:

1. define the SQLite schema
2. define the Go project structure
3. build configuration loading
4. initialize SQLite and migrations
5. implement repository registration CRUD
6. implement build target / publish target / binding CRUD
7. implement repository polling
8. implement tag detection
9. implement Unity version detection
10. implement Docker build execution
11. implement artifact tracking
12. implement publication targets
13. implement CLI/admin commands

Constraints for future agents

Any future AI agent should preserve these assumptions unless explicitly changed by the user:

1. implementation language is Go
2. initial database is SQLite
3. SQLite file must be persisted on a Docker-mounted host volume
4. the app runs in Docker
5. Redis is part of the runtime for transient coordination
6. the app orchestrates build containers via Docker socket
7. build pipelines are configured per repository
8. repositories define build targets, publish targets, and bindings
9. logs and artifacts should live outside the SQLite DB
10. SQLite remains the durable source of truth while Redis handles queues,
  locks, and idempotency concerns
11. worker and service responsibilities should stay delegated rather than
  collapsing into a monolithic process
12. the system should remain suitable for local/self-hosted use first

What an AI agent should do next

If starting implementation from this point, the next best tasks are:

1. create planning/project-brief.md if missing
2. create docs/architecture.md
3. define initial SQLite schema
4. add Redis to the local Docker runtime
5. define queue, lock, and idempotency abstractions
6. create initial Go module and project skeleton
7. add configuration/bootstrap code
8. add Docker-oriented local dev setup
9. add migration tooling and first migration