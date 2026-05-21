# HGP

> "If your daily workstation can already build and publish your Unity project
> manually, you should not need a second machine just to automate that same
> work. HGP exists to run that pipeline for you, on your own
> machine, with repeatability, logs, and operator control."

HGP is a local-first desktop orchestration product for Unity
release pipelines. The shipped product model is a Tauri desktop shell with a
bundled Rust runtime, SQLite-backed workflow state, filesystem-backed runtime
assets, and host-native Unity execution.

Repository and development surfaces use `handy-games-publisher`.
Canonical Git remote: `git@github.com:indiegabo/handy-games-publisher.git`.

## Product Model

- `apps/desktop/src-tauri` hosts the desktop shell and Tauri command surface.
- `apps/desktop/ui` hosts the React + TypeScript + Vite tray popup and
  reusable UI component kit loaded by the shell.
- `crates/runtime-bin` hosts the bundled runtime entrypoint and developer
  command surface.
- `crates/runtime-store` owns durable workflow state and local coordination.
- `crates/runtime-manifests`, `crates/runtime-git`, `crates/runtime-runner`,
  and `crates/runtime-publish` keep pipeline sync, source access, build
  execution, and publication explicit.
- `pipelines/` stores declarative repository pipeline manifests.
- SQLite stores durable metadata, configuration, and workflow state.
- The filesystem stores logs, artifacts, workspaces, and runtime snapshots.
- Unity execution runs through locally installed editors on the operator host.

## Desktop Tray Lifecycle

The desktop shell currently behaves like a tray-resident popup instead of a
normal document window.

- startup launches the bundled runtime supervisor, creates one tray icon, and
  opens the main window pinned to the lower-right corner of the primary
  monitor
- the main window is always-on-top, hidden from the taskbar, and sized as a
  compact popup instead of a full dashboard
- closing the window hides it to the tray and keeps the runtime alive
- left-clicking the tray icon or using the `Open HGP` tray action restores the
  popup window
- the `Quit` tray action marks the shell as intentionally exiting and then runs
  the normal runtime shutdown path

The current visible UI is a compact React + TypeScript + Vite desktop shell
built from reusable dark-theme components. It now uses a dispatch-board main
feed plus focus-screen working views, managed overlays, and staged flows.

## Repository Layout

```text
apps/
  desktop/
    src-tauri/
    ui/

crates/
  runtime-bin/
  runtime-config/
  runtime-core/
  runtime-git/
  runtime-manifests/
  runtime-publish/
  runtime-runner/
  runtime-store/

docs/
pipelines/
planning/
```

## Development Loop

Use the pinned Rust toolchain from `rust-toolchain.toml`. The workspace is
currently pinned to Rust `1.94.0`, which is the validated baseline for the
desktop shell on Windows.

Bootstrap the bundled runtime state:

```bash
cargo run -p runtime-bin -- bootstrap
```

Synchronize manifests from `pipelines/` into the runtime store:

```bash
cargo run -p runtime-bin -- manifests sync
```

Inspect the local runtime status:

```bash
cargo run -p runtime-bin -- status
```

Install the JavaScript workspace dependencies and the local Tauri CLI once:

```bash
npm install
```

Preview the desktop UI in a browser while iterating on shared primitives and
focus-screen surfaces:

```bash
npm run dev --prefix apps/desktop/ui
```

The browser preview is useful for component work but does not expose the Tauri
command bridge.

Run the desktop shell development loop from the repository root:

```bash
npm start
```

`npm start` resolves the local Tauri CLI from the root package and launches
the desktop shell from `apps/desktop`. The Tauri development loop owns the UI
development server and version synchronization through `beforeDevCommand`, and
the desktop shell startup path then launches the bundled runtime supervisor.
On Windows the runner also attempts to enter the Visual Studio developer
environment before invoking Tauri, but it still requires the native Tauri
prerequisites, including the Visual Studio C++ workload for the MSVC
toolchain.

The desktop app targets Windows, Linux, and macOS. The current hands-on
validation loop is still Windows-based, so host-specific behavior should keep
explicit extension points for Linux and macOS instead of treating Windows as
the only supported route.

The desktop app version is centralized in `Cargo.toml` under
`[workspace.package].version`. Run `npm run version:sync` after changing that
value if you need to refresh the mirrored version fields without starting the
desktop development loop.

When capturing ad hoc build or runtime output during troubleshooting, write
those logs under `tmp/diagnostics/` instead of the repository root. The
workspace already keeps Cargo artifacts under `tmp/` to avoid polluting the
top-level tree with temporary files.

Build the desktop shell without the Tauri development loop:

```bash
npm run build --prefix apps/desktop/ui
cargo run --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Validate the touched slices:

```bash
npm run build --prefix apps/desktop/ui
cargo check -p desktop-shell -j 1 -q
cargo test -p runtime-store -q
```

Run the runtime interrupted-recovery smoke target without contending for the
workspace default Cargo binary output:

```bash
npm run smoke:runtime
```

The workspace default Cargo artifacts live under `tmp/cargo-targets/default/`.
The smoke entrypoint keeps its isolated Cargo artifacts under
`tmp/cargo-targets/runtime-smoke/` so ad hoc validation does not contend with
the default build cache or spray additional target directories across the
repository root.

## Documentation Map

- [docs/architecture.md](docs/architecture.md)
- [docs/focus-screen-development-guide.md](docs/focus-screen-development-guide.md)
- [docs/desktop-ui-testing-strategy.md](docs/desktop-ui-testing-strategy.md)
- [docs/tauri-runtime-dev-loop.md](docs/tauri-runtime-dev-loop.md)
- [docs/pipeline-yaml-guide.md](docs/pipeline-yaml-guide.md)
- [docs/unity-adapter-contract.md](docs/unity-adapter-contract.md)
- [planning/project-brief.md](planning/project-brief.md)
- [planning/desktop-product-strategy.md](planning/desktop-product-strategy.md)
- [planning/desktop-delivery-roadmap.md](planning/desktop-delivery-roadmap.md)
- [planning/semantic-release-plan.md](planning/semantic-release-plan.md)
- [planning/runtime-ui-event-delivery-plan.md](planning/runtime-ui-event-delivery-plan.md)

## Release Automation

- `.github/workflows/ci.yml` rejects deprecated build surfaces and validates
  the active Rust and Tauri slices.
- `.github/workflows/release-please.yml` maintains SemVer release PRs and tags
  from Conventional Commits on `main`.
- `.github/workflows/release-bundle.yml` builds the Windows desktop bundle for
  published releases and uploads installer artifacts plus checksums.
- `scripts/prepare-tauri-sidecar.ps1` builds and stages the packaged
  `runtime-bin` sidecar expected by release bundles.

## Operator Expectations

- Repository manifests define polling, build targets, publish targets, and
  build-to-publish bindings.
- The desktop shell is the primary operator surface and now ships as a
  dispatch-board main feed plus a focus-screen system with managed overlays and
  staged flows.
- New screens and refactors should follow
  [docs/focus-screen-development-guide.md](docs/focus-screen-development-guide.md)
  instead of inventing ad-hoc local UI patterns.
- Runtime crates stay focused and explicit. Shell bindings stay thin.
- Operator-facing recovery paths should be available in the app before the
  documentation asks for manual host intervention.
