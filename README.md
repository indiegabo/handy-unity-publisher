# handy-unity-builder

> "If your daily workstation can already build and publish your Unity project
> manually, you should not need a second machine just to automate that same
> work. handy-unity-builder exists to run that pipeline for you, on your own
> machine, with repeatability, logs, and operator control."

handy-unity-builder is a local-first desktop orchestration product for Unity
release pipelines. The shipped product model is a Tauri desktop shell with a
bundled Rust runtime, SQLite-backed workflow state, filesystem-backed runtime
assets, and host-native Unity execution.

## Product Model

- `apps/desktop/src-tauri` hosts the desktop shell and Tauri command surface.
- `apps/desktop/ui` ships the operator dashboard loaded by the shell.
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

Launch the desktop shell:

```bash
cargo run --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Validate the touched slices:

```bash
cargo check -p desktop-shell -j 1 -q
cargo test -p runtime-store -q
node --check apps/desktop/ui/app.js
```

Run the runtime interrupted-recovery smoke target without contending for the
workspace default Cargo binary output:

```bash
bash scripts/runtime-smoke.sh
```

The workspace default Cargo artifacts live under `tmp/cargo-targets/default/`.
The smoke entrypoint keeps its isolated Cargo artifacts under
`tmp/cargo-targets/runtime-smoke/` so ad hoc validation does not contend with
the default build cache or spray additional target directories across the
repository root.

## Documentation Map

- [docs/architecture.md](docs/architecture.md)
- [docs/tauri-runtime-dev-loop.md](docs/tauri-runtime-dev-loop.md)
- [docs/pipeline-yaml-guide.md](docs/pipeline-yaml-guide.md)
- [docs/unity-build-methods.md](docs/unity-build-methods.md)
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
- The desktop shell is the primary operator surface for runtime health,
  repositories, releases, build history, artifact inspection, and credential
  management.
- Runtime crates stay focused and explicit. Shell bindings stay thin.
- Operator-facing recovery paths should be available in the app before the
  documentation asks for manual host intervention.
