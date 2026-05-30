# Desktop Product Strategy

## Purpose

This document captures the active product strategy for HGP as a
local-first desktop application.

It defines the architectural commitments that should guide implementation,
documentation, release packaging, and operator-facing behavior.

## Product Commitments

The product commits to these foundations:

- one installable desktop application per supported host platform
- one bundled runtime supervised by the desktop shell
- one local runtime root owned by the application
- SQLite as the durable system of record
- filesystem-backed logs, artifacts, runs, and health snapshots
- host-native Unity execution through locally installed editors
- declarative repository pipelines synchronized from YAML manifests
- operator-visible diagnostics for runtime health, build activity, and publish
  state

## Product Shape

The desktop product is composed of two layers:

1. a Tauri desktop shell
2. a bundled Rust runtime

The shell exists to present state, accept operator actions, and supervise the
runtime lifecycle.

The runtime exists to own orchestration, persistence, recovery, and execution.

## Desktop Shell Responsibilities

The shell is responsible for:

- application bootstrap and window lifecycle
- runtime startup, reconnection, and supervision visibility
- runtime health and status presentation
- repository, release, build, artifact, and publish inspection
- credential entry and redacted diagnostics
- settings and runtime directory inspection
- operator actions that map to thin Tauri command surfaces

The shell should not absorb workflow rules that belong in the runtime.

## Bundled Runtime Responsibilities

The runtime is responsible for:

- runtime root initialization
- SQLite bootstrap and migrations
- manifest loading and synchronization
- polling and manual release intake
- release planning and queue dispatch
- workspace preparation and Git operations
- Unity capability checks and executable selection
- build execution and artifact registration
- publish execution and delivery state persistence
- health snapshots, supervision state, and runtime logs

## Repository Pipeline Model

One registered repository is a complete release pipeline definition.

Each pipeline describes:

- Git source access
- trigger behavior
- build targets
- publish targets
- build-to-publish bindings
- credential references and runner expectations

The runtime should treat repository records as durable automation definitions,
not as temporary watch entries.

## State Ownership

The runtime owns one local data root with the current structure:

```text
<runtime-root>/
  state/
    runtime.db
    health.json
    supervision.json
    supervisor-state.json
  logs/
    runtime.jsonl
  artifacts/
  runs/
```

SQLite stores durable metadata, workflow state, and file references.

The filesystem stores execution logs, build outputs, run workspaces, and shell-
readable runtime snapshots.

## Host Model

The current product scope is one local host supervised by one runtime
instance.

That host model requires:

- explicit Unity editor discovery
- host capability inspection before work claims
- conservative local concurrency ceilings
- deterministic workspace preparation
- durable restart recovery for queued or interrupted work

## Product Direction Rules

When there is ambiguity, prefer these decisions:

- keep the shell thin and operator-facing
- keep runtime crates focused and explicit
- prefer durable runtime state over implicit process memory
- prefer host capability checks over hidden assumptions
- document operator-visible behavior before adding ornamental tooling
- keep release, build, and publish flows inspectable from the desktop shell

## Near-Term Strategic Priorities

The current product priorities are:

- harden runtime reliability and restart recovery
- complete operator-visible shell flows for repositories, credentials, and
  release inspection
- expand host execution coverage beyond the first supported runner
- grow publish capabilities on top of the durable publish model
- establish one coherent release and packaging workflow for the desktop product
