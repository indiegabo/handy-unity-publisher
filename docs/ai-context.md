# AI Context

## Product Context

- The project is intended to centralize Unity release automation in a self-hosted service
- Its value is reducing manual build and publishing work for Unity projects
- The expected workflow is repository registration, tag polling, release detection, build execution, and artifact publishing
- The first publishing targets mentioned so far are Itch.io, Steam, and Google Drive

## Technical Context

- Go is the planned implementation language
- SQLite is the planned local persistence layer
- The app is expected to run in Docker on local WSL
- Build jobs should run in ephemeral Docker containers
- GameCI is the planned build toolchain for Unity builds
- Persistent SQLite data must live on a host-mounted Docker volume

## Architectural Direction

- Separate command entrypoints are expected for the server and CLI
- Internal packages are expected for app, build, cli, config, credentials, db, docker, git, publish, release, repository, and worker concerns
- The architecture should keep infrastructure responsibilities isolated in dedicated modules
- Build execution likely needs a worker-oriented flow because release detection and publishing are asynchronous operations

## Current Repository State

- The repository is still mostly documentation
- The README is the main source of truth for goals and initial architecture decisions
- There is not yet an established build, lint, or test toolchain in the repository

## Suggested Near-Term Focus

- Create the initial Go module and project skeleton
- Define repository registration and credential boundaries
- Document architecture and decisions under `docs/`
- Introduce the first build, lint, and test commands once implementation starts
