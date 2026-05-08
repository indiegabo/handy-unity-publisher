# System Patterns

## Architectural Direction

The intended architecture is a modular Go application with separate concerns for application orchestration, repository monitoring, build execution, publishing, persistence, and worker coordination.

## Expected Core Flow

1. An operator registers a Unity repository and its access credentials.
2. The system polls the repository for new tags.
3. A new tag triggers release detection and Unity version discovery.
4. The application schedules a build in an ephemeral Docker container using GameCI.
5. Generated artifacts are published to one or more external targets.
6. Operational state is persisted locally in SQLite.

## Design Patterns Implied by the README

- Clear separation between command entrypoints (`cmd/`) and internal application logic (`internal/`)
- Encapsulation of infrastructure-specific logic in dedicated packages such as Docker, Git, database, and publishing modules
- Worker-oriented execution for asynchronous or queued build jobs
- Local-first persistence and deployment, with Docker used both for app hosting and build isolation

## Missing Details To Define Later

- Repository polling cadence and scheduling model
- Credential storage and security model
- Build queue lifecycle and retry behavior
- Artifact retention strategy
- Publishing target abstractions and provider interfaces
