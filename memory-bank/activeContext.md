# Active Context

## Current Snapshot

The project is still being defined. The repository currently contains the README and now a set of AI context documents that capture the intended direction of the system.

## What Is Known

- The product is meant to be a self-hosted Unity build orchestrator
- Go, SQLite, Docker, and GameCI are the initial technical choices
- A CLI and a server entrypoint are expected
- The project should support multi-target artifact publishing

## Immediate Next Logical Steps

- Create the initial Go module and repository structure described in the README
- Define configuration, repository registration, and credential management boundaries
- Document the architecture and decision records under `docs/`
- Establish the first build, lint, and test workflow once the codebase exists

## Working Assumptions

- Documentation currently acts as the source of truth
- Future implementation work should stay aligned with the modular structure proposed in the README
- Operational persistence must preserve data outside ephemeral containers
