# Product Context

## Problem Space

Unity projects often require repeatable builds, version-aware automation, and multi-platform publishing, but lightweight self-hosted tooling for that workflow is limited. This project aims to centralize those tasks in one local service.

## Target Outcome

The product should let an operator register game repositories once and then rely on the system to detect new releases, run the correct Unity build pipeline, and publish deliverables to the configured targets.

## Primary User Value

- Reduce manual release handling for Unity projects
- Standardize build execution through GameCI and Docker
- Keep control of credentials, state, and execution in a self-hosted environment
- Provide simple operational control through a CLI

## Key Capabilities

- Repository registration and credential handling
- Tag polling and release detection
- Unity version discovery per release
- Containerized build orchestration
- Artifact publishing to external platforms
- Local operational persistence and administration

## Current Constraints

- The product definition is still high level
- No implementation has been committed yet
- The current repository only captures goals and architectural direction
