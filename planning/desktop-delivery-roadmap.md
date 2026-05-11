# Desktop Delivery Roadmap

## Purpose

This roadmap tracks the active delivery work for the handy-unity-bulder desktop
product.

It describes what is already established, what is currently in flight, and
what still needs to land before the operator experience feels complete.

## Current Baseline

The current baseline already includes:

- [x] Cargo workspace and Tauri desktop shell scaffold
- [x] bundled runtime bootstrap, health reporting, and supervision snapshots
- [x] SQLite bootstrap and durable workflow schema
- [x] YAML manifest loading and synchronization
- [x] repository polling and manual release dispatch
- [x] release planning and local queue dispatch
- [x] host-aware build and publish claim selection
- [x] first host-native Unity runner implementation
- [x] filesystem publication path
- [x] shell diagnostics for runtime status, logs, directories, and runner
      settings

## Delivery Tracks

### 1. Runtime Platform

- [x] initialize runtime roots and app-data layout
- [x] persist health, supervision, and runtime logs
- [x] reconcile interrupted work on startup
- [x] expose runtime status and supervision contract commands
- [ ] harden restart behavior under repeated failure conditions
- [ ] add deeper runtime self-diagnostics for operator troubleshooting

### 2. Configuration And Secrets

- [x] synchronize repository pipeline definitions from YAML manifests
- [x] persist repository, build target, publish target, and binding records
- [ ] define persistent non-secret app settings storage
- [ ] integrate OS-native secret storage for operator-managed credentials
- [ ] surface missing-secret diagnostics directly in the shell
- [ ] finish repository source configuration for managed and local workspace
      modes

### 3. Host Capability And Execution Coverage

- [x] detect host OS, architecture, and WSL state
- [x] detect installed Unity editors and executable paths
- [x] classify build failures and timeout outcomes
- [x] verify the first supported host-native build path
- [ ] add Linux runner support
- [ ] design and implement macOS runner support
- [ ] finish background Unity process handling and richer log capture

### 4. Release And Workspace Orchestration

- [x] prepare isolated workspaces for release execution
- [x] resolve Unity versions from repository content when needed
- [x] dispatch queued build and publish work through durable local queues
- [x] register artifacts and downstream publish runs after successful builds
- [ ] complete source-mode support for managed repositories and local
      workspaces
- [ ] finish app-level defaults and per-repository overrides for workspaces,
      logs, and artifacts
- [ ] harden repository-local sequencing and backlog visibility in the shell

### 5. Desktop Shell Experience

- [x] start or reconnect to the runtime from the desktop shell
- [x] present runtime health, directories, logs, and runner settings
- [x] expose settings for credential entry and binding state
- [ ] add tray-resident lifecycle behavior
- [ ] add native notifications for automatic build activity
- [ ] move from snapshot-only refresh toward pushed runtime events
- [ ] complete first-class repository and release management flows in the UI

### 6. Publishing And Distribution

- [x] publish artifacts to the filesystem target
- [x] inspect persisted publish outputs from runtime commands
- [ ] add additional publish backends on top of the durable publish model
- [ ] normalize version reporting across shell and runtime surfaces
- [ ] automate SemVer tagging, release notes, and desktop artifact publication
- [ ] publish checksums and installer bundles for supported hosts

## Immediate Next Window

The next practical delivery window should focus on:

1. persistent settings and secret storage
2. repository source-mode completion
3. tray and notification behavior in the shell
4. release packaging and version normalization
