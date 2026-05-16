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
- Manifest synchronization still projects Unity contract fields into the legacy
  persisted columns `platform`, `build_method`, and
  `unity_version_override` so the existing runtime path still works.

### Validation Already Run

These validations were executed and passed after the last implementation round:

- `cargo test -p runtime-manifests`
- `cargo test -p runtime-store initialize_database`

More specifically, the `runtime-store` validation confirmed that the migration
stack still boots and upgrades successfully after adding the engine-contract
schema slice.

### Current Persistence State

The new migration currently present is:

- `crates/runtime-store/migrations/0010_engine_contract_model.sql`

Its current role is narrow:

- add `repositories.engine_kind`
- add `build_targets.build_kind`
- add `build_targets.contract_json`

Important: this means the plan item about rewriting baseline migrations for the
clean-database assumption is still not done. The current code chose the
smallest safe incremental migration so the manifest and store slices could land
without widening scope.

### What Is Still Intentionally Incomplete

The following areas remain Unity-centric and are the next real continuation
surface:

- `crates/runtime-store/src/models.rs`
- `crates/runtime-store/src/lib.rs`

The create/update repository project DTOs and normalization paths still speak in
old Unity-shaped fields such as `platform`, `build_method`, and
`unity_version_override`.

The runtime worker path is also still old-model internally:

- build dispatch does not yet branch on `engine_kind`
- the current build worker is not yet explicitly extracted as the Unity worker
- `runtime-runner` still contains Unity-specific behavior under generic names

### Recommended Next Slice

Resume from the repository project create/update path, not from docs or broad
rename work.

The next concrete slice should be:

1. add `engine_kind`, `build_kind`, and contract-aware payloads to repository
   project DTOs in `runtime-store`
2. normalize and persist those inputs through the create/update project flows
3. validate with the narrowest tests that cover repository project creation and
   update

Reason: this is the closest unresolved abstraction boundary after the manifest
slice. It moves engine-awareness into operator-authored project data before the
worker extraction begins.

### Caution For Resume

Do not mark Phase 2 as complete yet.

Only the manifest boundary and initial persistence slice have landed. The
runtime domain, DTOs, worker dispatch, shell payloads, and UI are still behind
the plan and should be treated as the active frontier.

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
- This refactor assumes a clean SQLite database during verification. No
  persistence compatibility path is required for legacy HUP state.

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

### Phase 0: Naming And Vocabulary Lock

- [ ] Confirm final long and short names: `Handy Games Publisher`, `HGP`,
      `handy-games-publisher`, `hgp-runtime`.
- [ ] Confirm final API version: `handy.games.publisher/v1alpha1`.
- [ ] Confirm final environment variable prefix: `HANDY_GAMES_PUBLISHER_`.
- [ ] Confirm final app-data directory and runtime directory names.
- [ ] Confirm canonical terminology: `engine`, `build kind`, `contract`,
      `adapter`, `runner family`, `Unity worker`.

### Phase 1: Product Identity Rename

- [ ] Rename operator-facing product references from HUP to HGP.
- [ ] Rename development-facing references from
      `handy-unity-publisher` to `handy-games-publisher`.
- [ ] Update package metadata in `Cargo.toml`, `package.json`, lockfiles, and
      Tauri bundle metadata.
- [ ] Rename binary names, process labels, and task labels that still carry HUP
      identity.
- [ ] Rename runtime configuration constants for product directory, runtime
      name, and env vars.
- [ ] Update scripts, examples, fixture names, and generated artifact labels.
- [ ] Update `.github/copilot-instructions.md` and AI context docs to HGP
      terminology.
- [ ] Update planning, architecture, strategy, README, and operational docs.
- [ ] Perform a repository-wide sweep for `HUP`,
      `handy-unity-publisher`, and `handy.unity.publisher` leftovers.

### Phase 2: Engine-Aware Core Model

- [ ] Add `engine_kind` to repository domain objects, persistence models, DTOs,
      and shell payloads.
- [ ] Introduce generic `build_kind` to build-target domain objects with
      `player` as the initial default.
- [ ] Remove or re-scope fake-generic Unity fields from the core build-target
      shape.
- [ ] Replace public `buildMethod` semantics with engine-scoped contract data.
- [ ] Replace public `platform` semantics with engine-scoped contract data.
- [ ] Replace `unity_version` persistence and DTO fields with generic
      `engine_version` semantics where they are runtime-level facts.
- [ ] Move Unity-only version override behavior into the Unity contract or a
      generic engine-version override concept.
- [ ] Split build-target persistence shape into `contract_json` and
      `runner_config_json`.
- [ ] Rewrite baseline migrations directly for the clean-database assumption.
- [ ] Rewrite test seed helpers to create HGP-shaped repositories and build
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
- [ ] Rewrite YAML guide examples around HGP and `contract.unity`.
- [x] Ensure backend validation rejects any manifest whose engine is not Unity.

### Phase 4: Runtime Adapter Architecture

- [ ] Define `EngineKind` and `BuildKind` in shared runtime contracts.
- [ ] Add an engine adapter registry to the runtime.
- [ ] Extract engine-agnostic execution helpers from Unity-specific code in
      `runtime-runner`.
- [ ] Create explicit Unity adapter modules under the runtime runner layer.
- [ ] Keep host process spawning, workspace layout, and generic artifact
      persistence outside the Unity adapter.
- [ ] Move Unity editor discovery, license checks, build-target mapping,
      command assembly, and Unity-specific artifact rules into the Unity
      adapter.
- [ ] Ensure diagnostics types are engine-aware without leaking future adapter
      details into generic orchestration.
- [ ] Defer any new crate split until the adapter boundary stabilizes or a
      second engine becomes real.

### Phase 5: Unity Worker Extraction

- [ ] Rename the current build worker path so it is explicitly the Unity build
      worker.
- [ ] Add a generic build dispatch worker that claims the next build run,
      reads `engine_kind`, and delegates to the correct adapter.
- [ ] Keep release planning and publish execution generic.
- [ ] Ensure every build-run claim path consults `engine_kind` before choosing
      behavior.
- [ ] Keep Unity-specific log stage naming and artifact heuristics inside the
      Unity adapter/worker path.
- [ ] Rename runtime diagnostics and messages so they distinguish generic build
      dispatch from the Unity worker.

### Phase 6: Desktop Shell And UI

- [ ] Add engine selection to project creation and project editing flows.
- [ ] List at least `Unity`, `Unreal`, `Godot`, `GameMaker`, `Defold`, and
      `Cocos Creator` in the selector.
- [ ] Keep every non-Unity engine option disabled in the UI.
- [ ] Add clear copy that only Unity is currently supported.
- [ ] Send `engine_kind` through Tauri commands and services.
- [ ] Reject hacked non-Unity payloads in backend command handlers.
- [ ] Make build-target labels engine-aware, for example `Unity Build Method`
      and `Unity Target Platform`.
- [ ] Keep `buildKind` internal or defaulted in the UI until a second contract
      requires a user choice.
- [ ] Add engine badges or fields to repository lists, detail views, review
      screens, and worker inspection screens.
- [ ] Rename shell DTOs that currently imply Unity-only meaning, such as
      `UnityRunnerSettings` and `UnityBuildTargetRunnerSettings`.

### Phase 7: Documentation Rewrite

- [ ] Rewrite project brief and product strategy around HGP.
- [ ] Rewrite architecture docs around engine-aware orchestration and Unity as
      the first adapter.
- [ ] Move Unity operational guidance into explicitly Unity-scoped docs.
- [ ] Rename or relocate `unity-build-methods.md` so it is clearly the Unity
      adapter contract guide.
- [ ] Document the adapter architecture and the internal `invoke kind` rule.
- [ ] Document the supported-engine matrix and mark non-Unity engines as
      planned only.
- [ ] Rewrite environment variable, runtime path, and binary name references.

### Phase 8: Tests And Validation

- [ ] Rewrite unit tests to seed repositories with `engine_kind = unity`.
- [ ] Rewrite manifest tests for HGP API version and `contract.unity`.
- [ ] Add tests for generic build dispatch selecting the Unity adapter.
- [ ] Add tests that backend validation rejects non-Unity engine selections.
- [ ] Add UI tests for disabled future-engine options.
- [ ] Add diagnostics tests proving worker inspection surfaces display engine
      explicitly.
- [ ] Add repository-wide validation sweeps for HUP/HUP-prefixed leftovers.
- [ ] Run focused Rust validation for `runtime-config`, `runtime-manifests`,
      `runtime-runner`, `runtime-store`, `runtime-bin`, and desktop shell
      surfaces touched by the refactor.
- [ ] Run desktop UI build validation for the HGP shell after form changes.
- [ ] Run an end-to-end smoke path on a clean database from project creation to
      Unity build dispatch.

### Phase 9: Completion Gates

- [ ] No public field remains generic in name while still encoding Unity-only
      meaning.
- [ ] No build worker path treats Unity as an implicit default.
- [ ] No operator-facing HUP identity remains except intentionally historical
      references.
- [ ] The application boots against a clean database and persists only HGP
      naming and schema.
- [ ] The shell lets operators choose an engine, but only Unity is accepted.
- [ ] The current runtime build flow is clearly the Unity adapter and Unity
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
