# Technical Context

## Chosen Foundations

- Language: Go
- Database: SQLite
- Runtime: Docker running on local WSL
- Build execution model: ephemeral Docker containers controlled by the application
- Persistence model: SQLite database file stored on a Docker-mounted host volume

## Proposed Code Organization

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

## External Systems and Tooling

- Git repositories used as the source of Unity project releases
- GameCI for Unity build execution
- Docker for isolation and runtime orchestration
- Distribution targets such as Itch.io, Steam, and Google Drive

## Implementation Readiness

The repository has not yet defined a build, lint, or test toolchain. The current technical context comes from the README and should be expanded once the first Go application skeleton is added.
