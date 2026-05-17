# AI Context

Conversation Reference: hgp-desktop-active-context
Project Repository: indiegabo/handy-games-publisher

## Purpose

This file gives AI agents and future contributors the current product truth for
the repository.

The active product is a Tauri desktop application with a bundled Rust runtime.
Any implementation or documentation work should start from that assumption.

## Product Summary

HGP is a self-hosted, local-first build orchestration product
for Unity repositories.

The operator workflow is:

1. declare repository pipelines under `pipelines/`
2. synchronize those manifests into the runtime store
3. poll repositories or dispatch releases manually
4. dispatch build runs on the local host
5. register artifacts and downstream publish runs
6. inspect and manage runtime state from the desktop shell

The product is not a Unity gameplay codebase.

## Active Architecture

- `apps/desktop/src-tauri` is the desktop shell crate.
- `apps/desktop/ui` contains the React + TypeScript + Vite tray popup surface.
- `apps/desktop/ui/src/components` contains the reusable React primitives used
  across the desktop shell.
- `crates/runtime-bin` exposes the bundled runtime commands and supervision
  loop.
- `crates/runtime-store` owns durable SQLite-backed workflow state.
- `crates/runtime-manifests` synchronizes declarative pipeline manifests.
- `crates/runtime-git` owns repository access and workspace preparation.
- `crates/runtime-runner` owns host capability checks and Unity execution
  planning.
- `crates/runtime-publish` owns downstream artifact publication behavior.

## Durable State And Filesystem Model

- SQLite is the durable source of truth.
- The SQLite database lives under the resolved app data directory.
- Logs, artifacts, workspaces, and runtime snapshots live on the filesystem.
- The runtime creates and manages those directories explicitly.
- The desktop shell reads runtime diagnostics through Tauri commands.

## Unity Execution Model

- Unity execution is host-native.
- The runtime uses locally installed Unity editors.
- Editor discovery, version selection, and capability checks stay explicit and
  deterministic.
- Build and publish execution should remain testable without shell-specific
  wiring.

## Configuration Model

- Repository configuration comes from YAML manifests under `pipelines/`.
- Each manifest describes Git access, trigger behavior, build targets, publish
  targets, and build-to-publish bindings.
- The runtime store mirrors validated manifest state into SQLite.
- Repository authentication is project-scoped: the shell assesses the
  repository URL first, keeps public repositories credential-free by default,
  and requests repository auth only when the detected access state requires it.
- Reusable provider sessions can exist globally, but binding one of those
  credentials to a repository is an explicit project-level action.
- After the operator supplies a PAT or completes a provider-backed sign-in,
  HGP is expected to perform repository polling, workspace synchronization, and
  build-related Git operations without any additional login prompts.
- PAT secret material is stored through shell-managed host keyring references,
  while SQLite stores only the durable credential metadata, repository auth
  state, and references.
- When runtime Git work later fails because credentials are stale, missing, or
  invalid, the repository enters a durable recovery state such as
  `reauth_required` instead of reopening provider UI from polling or builds.

## Active Operator Surfaces

The desktop shell currently ships as a tray-resident popup:

- startup shows a compact always-on-top window anchored to the lower-right
  corner of the primary monitor
- closing the window hides it to tray and keeps the runtime alive
- tray clicks and the `Open HGP` menu action reopen the popup
- the `Quit` tray action exits the shell and triggers runtime shutdown
- the visible UI is currently a compact dark showcase of reusable React
  components wired to the Tauri command bridge

The shell still exposes Tauri commands for:

- runtime health
- lifecycle rules
- release status
- repository inspection
- build history
- artifact inspection
- secret and credential management
- runtime directories
- Unity runner settings
- recent runtime logs

## Development Loop

Use the pinned Rust toolchain from `rust-toolchain.toml`.

Common validation and development commands:

```bash
cargo run -p runtime-bin -- bootstrap
cargo run -p runtime-bin -- manifests sync
cargo run -p runtime-bin -- status
npm install --prefix apps/desktop/ui
npm run dev --prefix apps/desktop/ui
npm run build --prefix apps/desktop/ui
cargo run --package desktop-shell
cargo check -p desktop-shell -j 1 -q
cargo test -p runtime-store -q
```

When changing the desktop UI, validate the touched React + TypeScript slice
with `npm run build --prefix apps/desktop/ui`, then rerun the narrow
desktop-shell compile check.

## Documentation Priorities

- Keep docs aligned with the active desktop product surfaces.
- Prefer one consistent product vocabulary across README, docs, and planning.
- Document operator-visible runtime behavior before polishing incidental tools.
- Keep architecture and planning docs in sync with shipped commands and data
  layout.

## Agent Guardrails

- Keep Rust runtime crates focused and explicit.
- Keep Tauri commands thin and delegate orchestration to runtime crates.
- Treat the desktop shell as the primary operator surface.
- Prefer the reusable desktop UI primitives under
  `apps/desktop/ui/src/components` before creating one-off controls.
- Keep desktop UI work compact, dark, and tool-oriented.
- Do not invent alternate onboarding or packaging models in documentation.

## Required Reads For Common Tasks

- Read `docs/architecture.md` before broad architectural edits.
- Read `docs/pipeline-yaml-guide.md` before editing manifests under
  `pipelines/`.
- Read `docs/unity-adapter-contract.md` before inventing or changing Unity
  build target contract values or Unity build methods.
- Read `planning/project-brief.md` and
  `planning/desktop-product-strategy.md` before broad product-direction work.
- Read `planning/desktop-delivery-roadmap.md` before roadmap or delivery work.
