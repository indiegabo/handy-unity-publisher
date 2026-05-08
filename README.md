# Handy Unity Bulder

A self-hosted build orchestrator for Unity projects with repository polling, automated GameCI builds, and multi-target publishing.

## Status

Early design / bootstrap phase.

## Goals

- Register Unity repositories with access credentials
- Poll repositories for new tags
- Detect Unity version from each tag
- Run builds through Docker and GameCI
- Publish artifacts to targets like Itch.io, Steam, and Google Drive
- Persist state locally with SQLite stored on a Docker-mounted volume
- Provide a CLI for administration and operations

## Initial architecture decisions

- Language: Go
- Database: SQLite initially
- Runtime: app runs in Docker on local WSL
- Build execution: ephemeral Docker containers controlled by the app
- Persistence: SQLite database file must live on a mounted host volume

## Proposed project layout

```text
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
```
