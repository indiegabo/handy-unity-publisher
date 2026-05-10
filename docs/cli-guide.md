# CLI Guide

This document explains the internal `hgb` command-line interface.

If you want the operator-facing CLI that consumes the running app through its
HTTP API, see [docs/hub-guide.md](./hub-guide.md).

## Running the hgb CLI

From the repository root:

```bash
go run ./cmd/hgb help
```

From the local Docker runtime:

```bash
docker compose exec unity-build-api go run ./cmd/hgb help
```

`hgb` reads runtime configuration the same way as the server. The most common
variables are:

- `DATA_DIR`
- `HOST_DATA_DIR`
- `PIPELINES_DIR`
- `APP_DB_PATH`
- `APP_CONFIG_PATH`
- `REDIS_ADDR`

## Command Overview

Top-level help:

```bash
go run ./cmd/hgb help
```

Useful top-level commands:

```bash
go run ./cmd/hgb version
go run ./cmd/hgb config
```

Repository configuration no longer happens through `hgb`. The runtime now reads
declarative manifests from `pipelines/` at server startup.

See [docs/pipeline-yaml-guide.md](./pipeline-yaml-guide.md) for the manifest
schema and authoring rules.

## Release Commands

If you want the operator-facing HTTP equivalent from outside the runtime,
prefer:

```bash
go run ./cmd/hub dispatch revolutions v1.2.3
```

Use `hgb` when you need the lower-level in-runtime surface directly.

Dispatch one manual release:

```bash
go run ./cmd/hgb releases dispatch manual --repository-id 1 --git-tag v1.0.0
```

Dispatch one manual release with an explicit commit:

```bash
go run ./cmd/hgb releases dispatch manual --repository-id 1 --git-tag v1.0.0 --git-commit abcdef123456
```

Expand one queued release into build runs:

```bash
go run ./cmd/hgb releases plan --release-run-id 1
```

Important runtime note:

- when `unity-build-api` is running, the normal repository path polls enabled
  repositories with enabled build targets automatically using the repository
  polling interval from the synchronized YAML manifests
- newly queued releases are planned automatically into queued build runs by the
  runtime coordinator
- if one polling pass finds more than one unseen tag, the runtime queues all
  of them from the oldest unseen tag to the newest unseen tag, but only one
  repository-local release build process starts at a time
- polling for that repository stays paused until every target of the current
  release reaches a terminal status
- use `go run ./cmd/hub runtime automation` from outside the runtime when you
  need to inspect whether polling is ready, scheduled, inactive, or paused by
  release backlog
- `releases plan` remains available as a narrow diagnostic and recovery tool

## Unity build_method Contract

For the full `build_method` contract, runtime artifact rules, and complete
Unity Editor examples for Linux, Windows, macOS, WebGL, and Android, see
[docs/unity-build-methods.md](./unity-build-methods.md).