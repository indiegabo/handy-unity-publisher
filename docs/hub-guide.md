# hub Guide

`hub` is the operator-facing CLI for a running handy-unity-bulder app.

Unlike `hgb`, which runs inside the runtime environment, `hub` consumes the
server over HTTP and is intended for operator-facing actions against an already
running stack.

By default, `hub` connects to:

```text
http://127.0.0.1:8080
```

Override the target app with:

```bash
export HUB_BASE_URL=http://127.0.0.1:8080
```

If your Docker Compose project name is not the default repository-derived one,
set the project name that `hub db import` should restart after uploading a new
SQLite snapshot:

```bash
export HUB_COMPOSE_PROJECT=handy-unity-builder
```

If the app is not reachable, `hub` stops and tells you to start it with:

```bash
docker compose up -d
```

## Runtime Commands

Show help:

```bash
go run ./cmd/hub help
```

Show the last declarative pipeline synchronization report:

```bash
go run ./cmd/hub runtime pipelines
```

That report comes from `GET /api/v1/runtime/pipelines` and tells you which
files from `pipelines/` were applied or skipped during the last startup sync.

Show the live automation state for repository polling and queued release work:

```bash
go run ./cmd/hub runtime automation
```

That report comes from `GET /api/v1/runtime/automation` and answers the
operator questions that matter during polling:

- whether a repository is `ready`, `scheduled`, `inactive`, or `paused`
- why polling is paused or inactive
- how many enabled build targets the repository currently exposes
- how many queued release processes are still pending for that repository
- which tag is currently blocking the next poll and which tags are waiting

Important automation semantics:

- a polling pass queues every unseen tag from the durable `last_seen_tag`
  baseline in ascending tag order
- the runtime only starts one release build process per repository at a time
- one release build process means all enabled build targets for one Git tag
- polling for that repository remains paused while the current release still
  has queued or running build work, and the next queued tag only starts after
  every target of the current release reaches a terminal state

Typical report shape:

```json
{
  "generated_at": "2026-05-09T22:15:00Z",
  "repositories": [
    {
      "repository_id": 1,
      "repository_name": "revolutions",
      "enabled": true,
      "enabled_build_target_count": 3,
      "polling_interval_seconds": 30,
      "last_seen_tag": "v1.2.0",
      "poll_state": "paused",
      "reason": "active_release_backlog",
      "pending_release_count": 2,
      "release_queue": [
        {
          "release_run_id": 17,
          "git_tag": "v1.1.0",
          "planned": true,
          "build_process_active": true,
          "queued_build_runs": 2,
          "running_build_runs": 1,
          "terminal_build_runs": 0,
          "total_build_runs": 3
        },
        {
          "release_run_id": 18,
          "git_tag": "v1.2.0",
          "planned": false,
          "build_process_active": false,
          "queued_build_runs": 0,
          "running_build_runs": 0,
          "terminal_build_runs": 0,
          "total_build_runs": 0
        }
      ]
    }
  ]
}
```

## Release Dispatch

Request one manual release run by repository name and tag:

```bash
go run ./cmd/hub dispatch revolutions v1.2.3
```

Request one manual release run with an explicit commit override:

```bash
go run ./cmd/hub dispatch revolutions v1.2.3 --git-commit abcdef123456
```

Request a rebuild for an existing repository tag, forcing the runtime to clear
derived build and publish state before requeueing the release:

```bash
go run ./cmd/hub dispatch revolutions v1.2.3 --rebuild
```

Important dispatch notes:

- the repository must already exist in the synchronized runtime state coming
  from the declarative `pipelines/` manifests
- `hub dispatch` resolves the repository by its durable runtime name, then
  posts a manual release request over `POST /api/v1/releases/dispatch/manual`
- once queued, the normal runtime coordinator plans the release into build
  runs automatically
- `--rebuild` reuses the existing `release_run` for the same repository and tag
  when present, clears prior `build_runs`, `artifacts`, and `publish_runs`, and
  then requeues the release so canonical artifact and publish destinations are
  overwritten by the fresh execution

## Database Commands

Export the internal runtime SQLite file to a host path:

```bash
go run ./cmd/hub db export --path ./hub-backup.db
```

Import an existing SQLite snapshot into the internal runtime volume:

```bash
go run ./cmd/hub db import --path ./hub-backup.db
```

Important runtime notes:

- `hub db export` is non-disruptive and streams a snapshot created with SQLite
  snapshot semantics
- `hub db import` validates the uploaded file first, then replaces the runtime
  SQLite file
- after a successful import, `hub` restarts `unity-build-api`,
  `unity-build-worker`, and `artifact-publish-worker` so all long-lived processes reopen the imported
  database
- if your stack uses a non-default Compose project name, set
  `HUB_COMPOSE_PROJECT` before running `hub db import`

## What hub No Longer Does

`hub` is no longer used to create or mutate repository configuration records.

Repository configuration now comes from YAML manifests under:

```text
pipelines/
```

See [docs/pipeline-yaml-guide.md](./pipeline-yaml-guide.md) for the declarative
schema and authoring guide.

## Global Installation

To install `hub` into your local user bin directory:

```bash
go run ./cmd/hub install
```

Important installation note:

- `hub install` copies the binary that is currently running
- to install the latest code from this repository, run `go run ./cmd/hub install`
- running an already-installed older `hub install` just reinstalls that same
  older binary to the target path

By default this writes the binary to:

```text
the first matching user bin directory already present on PATH
```

Typical defaults are:

- `~/.local/bin/hub`
- `~/.local/go/bin/hub`

Install to a custom path:

```bash
go run ./cmd/hub install --path "$HOME/bin/hub"
```

After installation, make sure the target directory is on `PATH`.