# HGP Refactor Master Plan

## Purpose

This document defines the full-project refactor plan that moves the product
from Handy Unity Publisher (HUP) to Handy Games Publisher (HGP).

It is intentionally both:

- the design plan for the refactor
- the live execution checklist that should be updated as work lands

The goal is not to bolt a generic label onto a Unity-only system. The goal is
to convert the current product into an engine-aware orchestration platform
whose first and only implemented engine worker is Unity.

This plan is explicitly forward-only:

- do not spend time preserving fallback behavior for unreleased HUP-era shapes
- do not design transitional compatibility paths for data or APIs that never
  shipped
- assume the application has not gone live and that the correct move is to
  delete legacy scaffolding once the contract-first path is proven

## Continuation Report

### Execution Snapshot

This report records the last validated implementation state so work can resume
from a precise local anchor on another machine.

Completed in code:

- `crates/runtime-manifests/src/lib.rs` now uses
  `handy.games.publisher/v1alpha1`.
- Manifests now require `spec.repository.engine`.
- Build targets now expose `buildKind` and engine-scoped `contract` payloads.
- Only `contract.unity` is accepted right now.
- Manifest validation now rejects any non-Unity engine.
- Manifest validation now rejects non-`player` Unity `buildKind` values.
- Manifest synchronization persists `engine_kind` on repositories.
- Manifest synchronization persists `build_kind` and `contract_json` on build
  targets.
- Manifest synchronization now writes contract-first build target rows end to
  end, and the removed-target disable path no longer reintroduces legacy
  `platform`, `build_method`, or `unity_version_override` storage.
- `crates/runtime-store/src/models.rs` now exposes repository-project create
  and update DTOs with `engine_kind`, `build_kind`, `contract_json`, and
  `runner_config_json`.
- `crates/runtime-store/src/lib.rs` now validates repository-project
  create/update input as engine-aware data and currently accepts only
  `engine_kind = unity` with `build_kind = player`.
- Repository-project create/update persistence now writes `engine_kind`,
  `build_kind`, and `contract_json` as the canonical build-target model and no
  longer persists `platform`, `build_method`, or
  `unity_version_override`.
- `crates/runtime-store/src/models.rs` and `crates/runtime-store/src/lib.rs`
  now expose `engine_kind` in repository polling/inspection records.
- `apps/desktop/src-tauri/src/lib.rs` now exposes engine-aware repository
  project command DTOs with `engine_kind` plus build-target
  `contract.unity.target_platform` and rejects non-Unity engine payloads at the
  shell boundary.
- Repository inspection returned by the desktop shell now includes
  `engine_kind` so the UI can render and edit the actual persisted engine.
- `apps/desktop/ui/src/services/projects.ts` now matches the engine-aware shell
  contract instead of the old Unity-shaped payload.
- `apps/desktop/ui/src/components/CreateProjectWizard.tsx` now collects
  `engine_kind`, emits `contract.unity`, and uses actual Unity targetPlatform
  values instead of the old shorthand platform strings.
- `apps/desktop/ui/src/components/RepositoryProjectDetail.tsx` now edits the
  same engine-aware contract and normalizes persisted Unity targetPlatform
  values back into the form state.
- `apps/desktop/ui/src/components/ProjectsFocusScreen.tsx` now surfaces the
  repository engine on the project cards.
- `crates/runtime-contracts/src/lib.rs` now defines shared `EngineKind` and
  `BuildKind` enums for runtime planning and execution contracts.
- `crates/runtime-store/src/models.rs` and `crates/runtime-store/src/lib.rs`
  now use typed `EngineKind` and `BuildKind` in the runtime-internal build
  execution and release-planning corridor instead of passing raw strings
  through those generic paths.
- `crates/runtime-bin/src/builds.rs` now branches build plan resolution
  through the shared engine adapter registry, rejects unimplemented engines
  before runner execution, and names the active execution path as the Unity
  build worker.
- `crates/runtime-runner/src/unity.rs` now owns the explicit
  `runtime_runner::unity` boundary and the full Unity-specific runner surface:
  public DTOs, processor/executor path, diagnostics/discovery, capability
  inspection internals, private Unity config / target mapping /
  execution error-classification helpers, and the Unity process/log machinery
  that used to live in the crate root.
- `crates/runtime-runner/src/engine.rs` now exposes the first explicit
  `EngineAdapterRegistry` surface for build execution adapter resolution.
- `crates/runtime-runner/src/lib.rs` now keeps the shared workspace/artifact
  helpers plus the engine registry surface and still avoids re-exporting the
  Unity-specific runner API from the crate root.
- `crates/runtime-store::BuildExecutionPlan` now loads `build_kind` and
  `contract_json` from `build_targets` and no longer carries legacy
  `platform` / `build_method` execution inputs in the runtime-only plan shape.
- `crates/runtime-store/src/lib.rs` release build planning now loads
  `build_kind` and `contract_json` for enabled build targets and prefers
  `contract.unity.editorVersion` when choosing the planned Unity version for
  each build target and now rejects planning rows that do not carry a usable
  `contract_json.unity` payload.
- `crates/runtime-bin` now consumes the explicit Unity runner API instead of
  the old generic execution type names.
- `crates/runtime-bin/src/builds.rs` now resolves Unity execution plans from
  the typed `BuildKind` plus `contract_json` fields and now rejects build rows
  that do not carry a usable Unity contract payload instead of falling back to
  legacy `platform` / `build_method` inputs.
- `crates/runtime-bin/src/main.rs` now derives the build-event platform from
  `contract_json` instead of depending on a legacy `platform` field in the
  stored build execution plan.
- `apps/desktop/src-tauri/src/lib.rs` now consumes Unity host diagnostics and
  discovery helpers through `runtime_runner::unity` instead of importing the
  Unity adapter surface from the crate root.
- `crates/runtime-bin/src/builds.rs` test fixtures now use
  `runtime_runner::unity::*` for Unity-only capability DTOs instead of
  reaching through the crate root compatibility re-exports.
- `crates/runtime-bin` test fixtures and interrupted-cleanup E2E fixtures now
  seed `build_kind` and `contract_json` explicitly so the runtime build path
  exercises the same contract-first data shape as the real runtime.
- The `manifest_sync_command_outputs_report_and_persists_pipeline_state` test
  fixture now uses the current `handy.games.publisher/v1alpha1` manifest schema
  instead of the stale HUP-era layout.
- `crates/runtime-manifests/src/lib.rs` now serializes Unity `contract_json`
  without empty compatibility-only fields and persists build-target contract
  data without legacy column writes.
- `crates/runtime-store/src/lib.rs` operator-facing build target runtime
  settings, build history, and artifact inspection read models now derive
  `platform` / `build_method` from the engine contract instead of reading
  those legacy columns semantically, and those read models now reject rows
  without a usable contract payload.
- `crates/runtime-store/migrations/0002_pipeline_definitions.sql` now creates
  `build_targets` directly in the contract-first shape, so fresh databases no
  longer materialize `platform`, `build_method`, or
  `unity_version_override`.
- `crates/runtime-store/src/lib.rs` now applies schema-aware migration SQL for
  `0009` and `0010`: fresh databases no-op the old runner-model cleanup and
  treat `0010` as an `engine_kind` addition only, while legacy databases still
  use `0010` to rebuild old `build_targets` rows into
  `build_kind + contract_json`.
- `crates/runtime-store/migrations/0003_execution_runs.sql` now creates
  `release_runs.engine_version` and `build_runs.engine_version` directly in the
  fresh-database baseline instead of persisting runtime execution state under
  Unity-specific column names.
- `crates/runtime-store/src/lib.rs` now registers schema-aware migration
  handling for `0011_runtime_engine_version.sql`, so fresh databases no-op the
  runtime execution rename while upgraded databases rename legacy
  `release_runs.unity_version` / `build_runs.unity_version` columns to
  `engine_version`.
- Runtime execution read models, dispatch payloads, shell DTOs, and desktop UI
  process-feed/release-status types now use `engine_version` for generic
  runtime execution facts while Unity contract fields remain explicitly
  Unity-scoped under `contract.unity.editorVersion`.
- Core product identity surfaces now use HGP end to end: Tauri product/binary
  metadata, bundled runtime binary names, runtime configuration env vars,
  keyring service naming, tray/notification/UI copy, package metadata, and
  shell/runtime development scripts now use `HGP`, `hgp-runtime`, and
  `handy-games-publisher` semantics instead of the HUP-era names.
- The repository now includes `npm run identity:check`, backed by
  `scripts/validate-hgp-identity.mjs`, to fail fast when non-historical HUP-era
  product identity reappears outside allowlisted historical files.
- `docs/pipeline-yaml-guide.md` and `docs/unity-adapter-contract.md` now teach
  the real HGP manifest contract with `buildKind`, `contract.unity`,
  `targetPlatform`, `buildMethod`, and optional `editorVersion` instead of the
  removed `platform` / `buildMethod` / `runner.unityVersion` manifest shape.
- The desktop shell now exposes the engine selector consistently in both
  create and edit flows while still rejecting non-Unity repository engines in
  the backend create/update path; targeted `desktop-shell` tests prove that
  non-Unity selections remain visible but impossible to persist.
- Clean-boot validation now covers both automation and a real runtime start:
  the focused `runtime-core` bootstrap test was updated to assert the current
  11-migration HGP schema, the focused `runtime-store` initialization test
  passed on a fresh database, and `hgp-runtime bootstrap` against a clean
  temporary root persisted only HGP runtime names, paths, and schema metadata.
- Worker inspection now surfaces engine identity explicitly in the desktop
  process feed: `runtime-store` includes `repository_engine_kind` in the feed
  payload, the process feed UI renders an explicit engine badge, and focused
  process-feed tests prove the backend carries that field.
- `apps/desktop/src-tauri/src/lib.rs` now includes a focused
  `load_process_feed` shell test so engine identity is proven at the Tauri
  shell boundary, not only in `runtime-store` and the React render layer.
- `crates/runtime-bin/src/builds.rs` now resolves a generic
  `BuildExecutionDispatchPlan` from `engine_kind`, records generic
  dispatch-stage messages, and only then delegates into the explicit
  `UnityHostNative` worker path instead of treating Unity as the implicit
  top-level build worker.
- `crates/runtime-runner/src/unity.rs` now owns the Unity-specific runtime
  archive packaging policy, including `_DoNotShip` filtering and the
  platform-specific `.pdb`, `.dSYM`, and `.symbols.json` exclusion rules,
  while `crates/runtime-bin/src/builds.rs` now delegates build-output
  packaging through the explicit adapter boundary.
- `crates/runtime-runner/src/unity.rs` now also owns the persisted Unity build
  stage identity for runtime tracking, including the `unity-build` step key,
  `Execute Unity Build` label, and platform-specific log stem, while
  `crates/runtime-bin/src/builds.rs` now consumes that adapter-provided stage
  identity through a generic internal execute-build slot.
- `crates/runtime-store/src/lib.rs` release planning now loads
  `repositories.engine_kind` into `ReleaseBuildPlanningState`, resolves
  release `engine_version` through an engine-aware dispatcher, and only then
  delegates Unity-specific repository detection and contract version selection
  into internal Unity helpers instead of calling Unity detection directly from
  the generic planning path.
- `crates/runtime-bin/src/main.rs` now includes a clean-root smoke test that
  creates a repository project through
  `LocalCoordinator::create_repository_project`, dispatches a manual release,
  runs the release planner cycle, and proves the path reaches
  `build run-next` successfully with a Unity target.
- `apps/desktop/ui/src/components/RepositoryEngineField.tsx` now centralizes
  the repository engine selector used by both create and edit flows, so the UI
  keeps one authoritative list of visible-but-disabled future engines.
- `apps/desktop/ui/src/components/RepositoryEngineField.test.tsx` plus the new
  Vitest setup now prove that Unity remains selectable while `Unreal`,
  `Godot`, `GameMaker`, `Defold`, and `Cocos Creator` stay visible but
  disabled in the shared selector.
- `scripts/revolutions-managed-repository.sql` and the matching
  `runtime-store` seed test now persist only contract-first build-target rows.
- The `queue_lease_renewer_keeps_claimed_message_leased_until_acknowledged`
  test now uses a wider lease window on Windows so the renewer assertion is
  stable instead of scheduler-dependent.

### Validation Already Run

These validations were executed and passed after the last implementation round:

- `cargo test -p runtime-store --lib`
- `cargo test -p runtime-manifests --lib`
- `cargo test -p runtime-bin`
- `cargo test -p runtime-runner`
- `cargo test -p desktop-shell --lib`
- `C:/Users/gabao/.cargo/bin/cargo.exe test -p runtime-bin build_execution_dispatch_plan_with_profile -- --nocapture`
- `C:/Users/gabao/.cargo/bin/cargo.exe test -p runtime-bin package_build_output_excludes -- --nocapture`
- `C:/Users/gabao/.cargo/bin/cargo.exe test -p runtime-bin build_run_next_command_numbers_platform_logs_across_sequential_builds -- --nocapture`
- `C:/Users/gabao/.cargo/bin/cargo.exe test -p runtime-bin build_run_next_command_numbers_logs_by_execution_order_without_packaging -- --nocapture`
- `C:/Users/gabao/.cargo/bin/cargo.exe test -p runtime-bin project_creation_smoke_reaches_unity_build_dispatch -- --nocapture`
- `C:/Users/gabao/.cargo/bin/cargo.exe test -p runtime-manifests --lib -- --nocapture`
- `C:/Users/gabao/.cargo/bin/cargo.exe test -p runtime-store plan_release_builds_ -- --nocapture`
- `C:/Users/gabao/.cargo/bin/cargo.exe test -p runtime-store get_build_execution_plan_loads_joined_metadata -- --nocapture`
- `C:/Users/gabao/.cargo/bin/cargo.exe test -p runtime-runner resolve_build_execution_adapter -- --nocapture`
- `C:/Users/gabao/.cargo/bin/cargo.exe test -p runtime-bin resolve_build_execution_dispatch_plan_with_profile -- --nocapture`
- `C:/Users/gabao/.cargo/bin/cargo.exe test -p desktop-shell load_process_feed_reports_repository_engine_identity -- --nocapture`
- `npm run identity:check`
- `npm run test --prefix apps/desktop/ui`
- `npm run build --prefix apps/desktop/ui`
- VS Code task `build desktop shell`, which completed both
  `npm run build --prefix apps/desktop/ui` and
  `cargo build --package desktop-shell`

More specifically, the `runtime-store` validation confirmed that the migration
stack still boots and upgrades successfully after moving fresh databases to the
contract-first `build_targets` schema while keeping the legacy `0010` bridge
available only for upgrade paths.

The latest focused validations also proved that the top of `builds run-next`
is now engine-aware before entering the Unity worker path, that Unity stage
identity now comes from the adapter boundary instead of generic worker code,
and that release planning now branches on `engine_kind` before delegating to
Unity-specific version detection while a clean runtime root can still move from
project creation to queued release planning and a successful Unity build
dispatch without SQL fixture seeding.

### Current Persistence State

The migration stack now splits cleanly between fresh-database bootstrap and the
legacy upgrade bridge:

- `crates/runtime-store/migrations/0002_pipeline_definitions.sql` creates
  `build_targets` directly with `build_kind`, `runner_type`, `output_kind`,
  `output_path_template`, `timeout_seconds`, `enabled`, `contract_json`, and
  `config_json`.
- `crates/runtime-store/migrations/0009_build_target_runner_model_cleanup.sql`
  is now a fresh-database no-op because the baseline schema already matches the
  final build-target shape.
- `crates/runtime-store/migrations/0010_engine_contract_model.sql` remains only
  as a legacy upgrade bridge; `runtime-store` selects schema-aware SQL so fresh
  databases only add `repositories.engine_kind`, while older databases still
  rebuild `build_targets` rows into `build_kind + contract_json` during
  upgrade.
- `crates/runtime-store/migrations/0003_execution_runs.sql` now creates runtime
  execution rows with `engine_version`, and
  `crates/runtime-store/migrations/0011_runtime_engine_version.sql` exists
  only to rename legacy runtime execution columns during upgrade.

This means the plan item about rewriting baseline migrations for the
clean-database assumption is now done, while the upgrade bridge remains
intentionally confined to migration-only code.

### What Is Still Intentionally Incomplete

The runtime binary, desktop shell, and lower-level runner crate now resolve
Unity through an explicit `runtime_runner::unity` boundary, and the crate root
no longer re-exports Unity-specific runner APIs. The main structural cleanup in
`runtime-runner` is complete.

The contract-first persistence slice that used to be the active frontier is now
also complete:

- manifest synchronization, repository-project persistence, direct SQL seeds,
  runtime planning, runtime execution, and operator-facing read models now work
  from `build_kind + contract_json` without writing legacy build-target
  columns
- fresh databases persist only the contract-first build-target shape; the only
  remaining references to `platform`, `build_method`, and
  `unity_version_override` live inside the intentional legacy upgrade bridge in
  `0010_engine_contract_model.sql` and the schema-aware migration selector in
  `runtime-store`

The shell/UI surface is now engine-aware in shape, but it is still Unity-only
in capability:

- non-Unity engines remain visible but disabled in the UI
- focused Vitest coverage now proves that selector contract explicitly keeps
  future engines visible but disabled
- backend validation rejects any hacked non-Unity repository payload
- build target editing still assumes `contract.unity` and a host-native Unity
  executable path

The broader refactor is still incomplete for the remaining adapter/worker
boundary cleanup and the last contract/documentation pass.

### Recommended Next Slice

Resume from the remaining adapter-boundary cleanup and contract sweep, not
from the now-closed shared runtime contract/registry slice, Phase 5
planning/logging cleanup, UI test debt, product identity, or build-target
model cleanup.

The next concrete slice should be:

1. extract the next engine-agnostic execution helpers from
   `runtime_runner::unity` so the registry-backed runner boundary owns only the
   Unity-specific pieces that truly belong there
2. finish the remaining contract sweep at manifest and read-model edges so
   string slugs remain only at external IO boundaries rather than in generic
   runtime decisions
3. sync the last documentation pass only after those boundaries match the code
   exactly

Reason: the contract-first path, the runtime-level `engine_version`
transition, the engine selector contract, the adapter-owned build stage
identity, the adapter-owned artifact heuristics, the genericized release
planning branch, the shared `EngineKind` / `BuildKind` runtime contracts, and
the first `EngineAdapterRegistry` surface are now proven through write, read,
planning, execution, seeds, migrations, focused runtime tests, shell tests, UI
tests, the desktop build, and the automated identity sweep. The leverage now
is shrinking the remaining Unity-specific residue inside the runner/orchestrator
boundary instead of reopening already-settled Phase 5 cleanup.

### Caution For Resume

Do not mark Phase 2 as complete yet.

The manifest boundary, persistence slice, shell/UI repository project contract
slice, runtime build dispatch slice, explicit Unity runner boundary cleanup,
contract-first runtime execution plan loading, contract-first Unity version
selection during release planning, contract-first manifest write path,
contract-required runtime planning/execution, and the runtime-level
`engine_version` rename have landed. Operator-facing read models now also
project from `contract_json` first and reject missing contracts. The baseline
migration rewrite, schema-aware upgrade bridge, runtime execution rename
migration, fixture cleanup, and HGP identity sweep also landed. The remaining
active frontier is no longer physical legacy-column retirement, generic runtime
version naming, or product identity; it is the last stale contract docs and any
remaining adapter/worker boundary cleanup.

Do not reopen fallback work on resume. If a slice appears to require a legacy
compatibility path, the slice should be redesigned to stay forward-only.

## Hard Decisions

- Product identity moves from `HUP` / `handy-unity-publisher` to `HGP` /
  `handy-games-publisher`.
- Engine becomes a first-class repository property.
- The public strategy is `engine + buildKind` when `buildKind` is materially
  relevant.
- `invoke kind` remains internal to the runtime adapter layer and must not be
  exposed as user-authored pipeline configuration.
- The current build flow is re-scoped as the Unity build contract rather than a
  fake-generic build contract.
- Only Unity is supported at the end of this refactor.
- The desktop UI may list future engines, but every non-Unity option stays
  disabled and backend validation must reject non-Unity requests.
- This refactor assumes a clean SQLite database and a not-yet-launched product. No persistence compatibility path is required for legacy HUP state, and new work should delete compatibility scaffolding rather than harden it.

## Target End State

At the end of this refactor:

- the product, runtime, docs, UI, package metadata, and environment variables
  all speak in HGP terms
- repository records explicitly declare which engine they use
- build workers always resolve engine before selecting execution logic
- the current build worker is explicitly the Unity worker
- the current Unity-only build contract is clearly marked as Unity-specific
- engine-independent orchestration stays generic
- `invoke kind` is selected internally by the Unity adapter
- the UI exposes engine selection but only allows Unity to be chosen
- the system can grow future engine adapters without reopening the domain model

## Scope Boundaries

This refactor includes:

- product rename from HUP to HGP
- repository, runtime, and shell model updates for engine awareness
- adapter architecture inside the runtime
- extraction of the current Unity build flow into a Unity-specific worker path
- UI changes to expose engine selection and Unity-only support messaging
- documentation and test rewrites

This refactor does not include:

- implementation of Unreal, Godot, GameMaker, Defold, or Cocos workers
- backward-compatibility shims for old SQLite state
- fallback migration flows for pre-existing HUP pipelines
- public exposure of `invoke kind`
- speculative cloud or distributed execution changes

## Refactor Principles

- Do not keep fake-generic fields that are actually Unity-only.
- Do not hide Unity assumptions inside names like `runner`, `platform`, or
  `build_method` once engine becomes first-class.
- Keep orchestration generic and engine contracts explicit.
- Keep adapter boundaries narrow, typed, and testable.
- Prefer replacing old HUP naming directly over supporting dual naming during
  clean-database validation.
- Do not add fallback paths for unreleased legacy state; if old scaffolding is
  in the way, remove it.
- Do not expose future engine support in a way that implies it already works.

## Primary Impact Surfaces

The refactor is expected to touch at least these areas:

- planning, architecture, AI context, and operator documentation
- repository and package metadata such as `Cargo.toml`, `package.json`, and
  Tauri bundle metadata
- runtime configuration constants, product directory names, binary names, and
  environment variable prefixes
- runtime manifest schema, API version, and stored build-target model
- runtime store models and migrations
- runtime runner abstractions and Unity discovery/execution code
- runtime build worker dispatch and test fixtures
- Tauri shell DTOs, diagnostics payloads, and repository inspection surfaces
- React UI project creation flows, project detail screens, and worker screens
- scripts, test helpers, and fixture naming

## Architectural Direction

### Core Model

The generic core should own only concepts that truly survive engine changes:

- repository source and credentials
- release intake and release planning
- build target identity
- build kind
- runner family
- output expectations
- artifact registration
- publish targets and bindings
- runtime health, diagnostics, and supervision

Everything else that depends on engine semantics belongs to an engine-specific
build contract or engine adapter.

### Engine-Aware Domain Model

The repository should become the top-level selector for engine behavior.

Conceptual model:

```text
Repository
  - engine_kind

BuildTarget
  - name
  - build_kind
  - runner_family
  - output_kind
  - output_path_template
  - timeout_seconds
  - contract_json
  - runner_config_json
```

Key consequences:

- `platform` is not generic enough to stay as a first-class cross-engine field
  because it currently encodes Unity `BuildTarget` semantics.
- `build_method` is not generic enough to stay public because it currently means
  Unity `-executeMethod`.
- `unity_version_override` must become a generic engine-version concept or move
  into the Unity contract.
- the existing `config_json` field should be split conceptually into
  `contract_json` and `runner_config_json` so user-authored engine contract does
  not get mixed with runtime launch overrides.

### Public Build Contract Strategy

The public strategy is:

- repository chooses `engine`
- build target chooses `buildKind` when relevant
- engine-specific contract fields live under engine-owned contract data
- runtime selects `invoke kind` internally

For this refactor, Unity remains the only accepted engine contract.

Conceptual shape:

```yaml
apiVersion: handy.games.publisher/v1alpha1
kind: Pipeline

metadata:
  name: revolutions

spec:
  repository:
    engine: unity
    url: https://example.com/org/revolutions.git
    defaultBranch: main
    enabled: true
    pollingIntervalSeconds: 300

  build:
    targets:
      - name: windows-player
        buildKind: player
        runner:
          type: host-native
          timeoutSeconds: 5400
        output:
          kind: directory
          path: Builds/Windows
        contract:
          unity:
            targetPlatform: StandaloneWindows64
            buildMethod: Builder.PerformWindows
            editorVersion: 2022.3.14f1
```

This shape makes the truth explicit:

- the repository is a Unity repository
- the build target is a player build
- the Unity adapter owns `targetPlatform`, `buildMethod`, and `editorVersion`
- no user chooses `invoke kind`

### Adapter Architecture

The runtime should use an adapter registry behind a generic orchestration core.

Conceptual interfaces:

```rust
enum EngineKind {
    Unity,
    Unreal,
    Godot,
    GameMaker,
    Defold,
    CocosCreator,
}

enum BuildKind {
    Player,
    Server,
    Content,
    Pack,
    Patch,
}

trait EngineAdapter {
    fn kind(&self) -> EngineKind;
    fn validate_contract(&self, request: &BuildContractValidationRequest)
        -> Result<(), AdapterError>;
    fn resolve_engine_version(&self, request: &EngineVersionRequest)
        -> Result<Option<String>, AdapterError>;
    fn inspect_host_capabilities(&self, request: &HostCapabilityRequest)
        -> EngineCapabilityReport;
    fn build_execution_plan(&self, request: &BuildExecutionPlanRequest)
        -> Result<AdapterExecutionPlan, AdapterError>;
    fn execute(&self, request: &AdapterExecuteRequest)
        -> Result<AdapterExecutionResult, AdapterError>;
    fn collect_artifacts(&self, request: &ArtifactCollectionRequest)
        -> Result<Vec<DiscoveredArtifact>, AdapterError>;
}
```

The orchestration core should own:

- queue claims
- release/build/publish state transitions
- workspaces
- process execution primitives
- filesystem registration
- artifact persistence
- publish routing

The adapter should own:

- contract validation
- engine version resolution
- host capability semantics for that engine
- command assembly
- engine-specific logs and error classification
- artifact interpretation rules for that engine

### Module And Crate Direction

To avoid architectural mud without over-fragmenting too early:

- keep the current crate graph initially
- extract engine-agnostic runtime execution helpers inside `runtime-runner`
- create explicit adapter modules such as `runtime-runner::adapters::unity`
- add a small registry layer such as `runtime-runner::engine`
- defer a dedicated `runtime-engine-unity` crate split until a second adapter is
  real or the Unity adapter surface stabilizes enough to justify the boundary

This keeps the first refactor reversible while still forcing correct layering.

### Internal Invoke Mapping

`invoke kind` is an adapter implementation detail.

Internal mapping examples:

| Engine        | Build Kind | Internal Invoke Kind | Publicly Exposed? |
| ------------- | ---------- | -------------------- | ----------------- |
| Unity         | player     | `execute-method`     | no                |
| Unreal        | player     | `build-cook-run`     | no                |
| Godot         | player     | `export-preset`      | no                |
| Defold        | player     | `bob-bundle`         | no                |
| Cocos Creator | player     | `build-config`       | no                |

Only the Unity row is implemented in this refactor, but the architecture should
be shaped around this reality now.

## Recommended Execution Order

1. lock naming and model decisions
2. rename the product and repository-facing identity
3. land engine-aware domain changes with a clean schema
4. extract the current Unity flow into a Unity adapter and Unity worker
5. wire engine selection through shell and UI
6. rewrite docs and tests
7. run full verification on a clean database

This order reduces the risk of renaming generic Unity assumptions after they
have already spread into a fake multi-engine model.

## Main Risks And Containment

### Risk: False generic fields survive the rename

Containment:

- remove or re-scope `platform`, `build_method`, and `unity_version_override`
  instead of merely renaming them

### Risk: Engine contract and runner overrides get mixed together

Containment:

- split contract data from runner override data conceptually and in persisted
  shape

### Risk: UI implies unsupported engines work

Containment:

- disable all non-Unity choices in UI and enforce rejection server-side

### Risk: Unity remains implicit in worker dispatch

Containment:

- require every build dispatch path to branch on repository `engine_kind`

### Risk: Rename sweep misses operator-facing or runtime-facing identity

Containment:

- finish with explicit string and env-var sweeps across docs, scripts, tests,
  package metadata, and runtime constants

## Execution Checklist

### Phase 1: Product Identity Rename

- [x] Rename operator-facing product references from HUP to HGP.
- [x] Rename development-facing references from
      `handy-unity-publisher` to `handy-games-publisher`.
- [x] Update package metadata in `Cargo.toml`, `package.json`, lockfiles, and
      Tauri bundle metadata.
- [x] Rename binary names, process labels, and task labels that still carry HUP
      identity.
- [x] Rename runtime configuration constants for product directory, runtime
      name, and env vars.
- [x] Update scripts, examples, fixture names, and generated artifact labels.
- [x] Update `.github/copilot-instructions.md` and AI context docs to HGP
      terminology.
- [x] Update planning, architecture, strategy, README, and operational docs.
- [x] Perform a repository-wide sweep for `HUP`,
      `handy-unity-publisher`, and `handy.unity.publisher` leftovers.

### Phase 2: Engine-Aware Core Model

- [x] Add `engine_kind` to repository domain objects, persistence models, DTOs,
      and shell payloads.
- [x] Introduce generic `build_kind` to build-target domain objects with
      `player` as the initial default.
- [x] Remove or re-scope fake-generic Unity fields from the core build-target
      shape.
- [x] Replace public `buildMethod` semantics with engine-scoped contract data.
- [x] Replace public `platform` semantics with engine-scoped contract data.
- [x] Replace `unity_version` persistence and DTO fields with generic
      `engine_version` semantics where they are runtime-level facts.
- [x] Move Unity-only version override behavior into the Unity contract or a
      generic engine-version override concept.
- [x] Split build-target persistence shape into `contract_json` and
      `runner_config_json`.
- [x] Rewrite baseline migrations directly for the clean-database assumption.
- [x] Rewrite test seed helpers to create HGP-shaped repositories and build
      targets only.

### Phase 3: Manifest And Configuration Contract Rewrite

- [x] Rename manifest API version to `handy.games.publisher/v1alpha1`.
- [x] Add repository-level `engine` to the manifest schema.
- [x] Add build-target `buildKind` to the manifest schema with default `player`.
- [x] Introduce engine-scoped `contract` payloads and support only
      `contract.unity` for now.
- [x] Keep `invoke kind` internal and ensure it is absent from the manifest.
- [x] Update validation errors so they talk about engine contracts instead of
      generic build methods.
- [x] Rewrite YAML guide examples around HGP and `contract.unity`.
- [x] Ensure backend validation rejects any manifest whose engine is not Unity.

### Phase 4: Runtime Adapter Architecture

- [x] Define `EngineKind` and `BuildKind` in shared runtime contracts.
- [x] Add an engine adapter registry to the runtime.
- [x] Extract engine-agnostic execution helpers from Unity-specific code in
      `runtime-runner`.
- [x] Create explicit Unity adapter modules under the runtime runner layer.
- [x] Keep host process spawning, workspace layout, and generic artifact
      persistence outside the Unity adapter.
- [x] Move Unity editor discovery, license checks, build-target mapping,
      command assembly, and Unity-specific artifact rules into the Unity
      adapter.
- [x] Ensure diagnostics types are engine-aware without leaking future adapter
      details into generic orchestration.
- [x] Defer any new crate split until the adapter boundary stabilizes or a
      second engine becomes real.

### Phase 5: Unity Worker Extraction

- [x] Rename the current build worker path so it is explicitly the Unity build
      worker.
- [x] Add a generic build dispatch worker that claims the next build run,
      reads `engine_kind`, and delegates to the correct adapter.
- [x] Keep release planning and publish execution generic.
- [x] Ensure every build-run claim path consults `engine_kind` before choosing
      behavior.
- [x] Keep Unity-specific log stage naming inside the Unity adapter/worker
      path.
- [x] Keep Unity-specific artifact heuristics inside the Unity adapter/worker
      path.
- [x] Rename runtime diagnostics and messages so they distinguish generic build
      dispatch from the Unity worker.

### Phase 6: Desktop Shell And UI

- [x] Add engine selection to project creation and project editing flows.
- [x] List at least `Unity`, `Unreal`, `Godot`, `GameMaker`, `Defold`, and
      `Cocos Creator` in the selector.
- [x] Keep every non-Unity engine option disabled in the UI.
- [x] Add clear copy that only Unity is currently supported.
- [x] Send `engine_kind` through Tauri commands and services.
- [x] Reject hacked non-Unity payloads in backend command handlers.
- [x] Make build-target labels engine-aware, for example `Unity Build Method`
      and `Unity Target Platform`.
- [x] Keep `buildKind` internal or defaulted in the UI until a second contract
      requires a user choice.
- [x] Surface engine identity in worker inspection screens as explicitly as it
      already appears in repository lists, detail views, and review screens.
- [x] Rename shell DTOs that currently imply Unity-only meaning, such as
      `UnityRunnerSettings` and `UnityBuildTargetRunnerSettings`.

### Phase 7: Documentation Rewrite

- [x] Rewrite project brief and product strategy around HGP.
- [x] Rewrite architecture docs around engine-aware orchestration and Unity as
      the first adapter.
- [x] Move Unity operational guidance into explicitly Unity-scoped docs.
- [x] Rename or relocate the Unity adapter guide so it is clearly the Unity
      adapter contract guide.
- [x] Document the adapter architecture and the internal `invoke kind` rule.
- [x] Document the supported-engine matrix and mark non-Unity engines as
      planned only.
- [x] Rewrite environment variable, runtime path, and binary name references.

### Phase 8: Tests And Validation

- [x] Rewrite unit tests to seed repositories with `engine_kind = unity`.
- [x] Rewrite manifest tests for HGP API version and `contract.unity`.
- [x] Add tests for generic build dispatch selecting the Unity adapter.
- [x] Add tests that backend validation rejects non-Unity engine selections.
- [x] Add UI tests for disabled future-engine options.
- [x] Add diagnostics tests proving worker inspection surfaces display engine
      explicitly.
- [x] Add repository-wide validation sweeps for HUP/HUP-prefixed leftovers.
- [x] Run focused Rust validation for `runtime-config`, `runtime-manifests`,
      `runtime-runner`, `runtime-store`, `runtime-bin`, and desktop shell
      surfaces touched by the refactor.
- [x] Run desktop UI build validation for the HGP shell after form changes.
- [x] Run an end-to-end smoke path on a clean database from project creation to
      Unity build dispatch.

### Phase 9: Completion Gates

- [x] No public field remains generic in name while still encoding Unity-only
      meaning.
- [x] No build worker path treats Unity as an implicit default.
- [x] No operator-facing HUP identity remains except intentionally historical
      references.
- [x] The application boots against a clean database and persists only HGP
      naming and schema.
- [x] The shell lets operators choose an engine, but only Unity is accepted.
- [x] The current runtime build flow is clearly the Unity adapter and Unity
      worker.

## Refactor Is Not Done Until

The refactor must not be considered complete until all of these are true:

- product identity is HGP end-to-end
- engine is first-class in repository and build dispatch paths
- Unity is explicit rather than implicit
- non-Unity selections are visible but impossible to execute
- docs describe a real HGP architecture instead of a renamed HUP
- the codebase can add a second engine adapter without reopening the domain
  model again
