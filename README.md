# HGP

> "If your daily workstation can already build and publish your project
> manually, you should not need a second machine just to automate that same
> work. HGP exists to run that pipeline for you, on your own
> machine, with repeatability, logs, and operator control."

HGP is a local-first desktop orchestration product for game projects
release pipelines. The shipped product model is a Tauri desktop shell with a
bundled Rust runtime, SQLite-backed workflow state, filesystem-backed runtime
assets, and host-native execution.

Repository and development surfaces use `handy-games-publisher`.
Canonical Git remote: `git@github.com:indiegabo/handy-games-publisher.git`.
Public beta docs: <https://indiegabo.github.io/handy-games-publisher/>.

## Product Model

- `src-tauri` hosts the desktop shell and Tauri command surface.
- `src-react` hosts the React + TypeScript + Vite tray popup and
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

## Desktop Icons

The desktop shell now uses a single logo source for both the bundled app icons
and the runtime tray surface.

- `src-tauri/icons/hgp-logo.png` is the shared source image used
  to regenerate the packaged app icons declared in the Tauri bundle config and
  to load the runtime tray icon.

Regenerate the bundled app icon set with `npm run icons:generate`. Tauri dev
and build commands now run that step automatically before launching the shell.

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
npm run dev --prefix src-react
```

The browser preview is useful for component work but does not expose the Tauri
command bridge.

Run the desktop shell development loop from the repository root:

```bash
npm start
```

`npm start` resolves the local Tauri CLI from the root package and launches
the desktop shell from the repository root. The Tauri development loop owns the UI
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

Localization packs under `src-tauri/localizations/` use
`en.json` as the source of truth for the message catalog. When adding or
translating locale strings, update `en.json` first, run
`npm run localization:sync` to mirror its keys into every non-English pack,
translate the target locale values, and finish with
`npm run localization:check` so no localized file drifts from the English key
set.

When capturing ad hoc build or runtime output during troubleshooting, write
those logs under `tmp/diagnostics/` instead of the repository root. The
workspace already keeps Cargo artifacts under `tmp/` to avoid polluting the
top-level tree with temporary files.

Build the desktop shell without the Tauri development loop:

```bash
npm run build --prefix src-react
cargo run --manifest-path src-tauri/Cargo.toml
```

Validate the touched slices:

```bash
npm run build --prefix src-react
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
- `.github/workflows/semantic-release.yml` runs after successful `main` branch
  CI, calculates the next SemVer from Conventional Commits, updates the shared
  version files, creates the release commit and Git tag, publishes the GitHub
  release, and then invokes the desktop bundle publication workflow.
- `.github/workflows/release-bundle.yml` builds the Windows and Linux
  installers for a published release or a reusable workflow call, uploads
  installer artifacts plus checksums to the GitHub release, and publishes the
  installers to Itch channels through Butler.
- `scripts/prepare-tauri-sidecar.ps1` builds and stages the packaged
  `runtime-bin` sidecar expected by release bundles.
  sidecar used by Itch publish flows in development and release bundles.

Itch publish automation in `.github/workflows/release-bundle.yml` requires:

- repository secret `ITCH_BUTLER_API_KEY`
- repository variable `ITCH_PROJECT` in `owner/game` format
- repository variable `ITCH_CHANNEL_WINDOWS_EXE`
- repository variable `ITCH_CHANNEL_WINDOWS_MSI`
- repository variable `ITCH_CHANNEL_LINUX`

Storefront assets that support the Itch release surface should live under
`.github/assets/itch/`. The current project cover image is stored at
`.github/assets/itch/itch-cover.png`.

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
