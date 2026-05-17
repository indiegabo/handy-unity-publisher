# Publish Destinations Plan

## Purpose

This document defines the implementation of project-owned publish destinations
for HGP.

The target behavior is:

- one project may define zero, one, or many publish destinations
- each destination declares one publish backend plus any shared configuration
- each destination may bind zero, one, or many build targets
- each binding declares what that destination should do with the artifact
  produced by that build target
- unbound build targets keep their artifacts in the runtime-managed output
- destructive target edits must explicitly warn when they also remove publish
  bindings
- filesystem publication may move an artifact and update its canonical stored
  location
- Itch.io publication may upload selected artifacts through a first-class
  destination type

## Scope Lock

- this plan applies to desktop create-project and edit-project flows
- this plan applies to repository inspection payloads, runtime persistence,
  publish planning, publish execution, and publish diagnostics
- this plan applies to two first-class destination kinds in the first delivery
  window: `filesystem` and `itch`
- this plan applies to publish credentials only when they belong to a publish
  destination backend such as Itch.io
- this plan does not redesign repository polling, release creation, or build
  execution outside the publish slice
- this plan does not introduce a generic third-party plugin framework in the
  first slice
- this plan does not require a broad SQL or Rust rename from
  `publish_target` to `publish_destination` in the first slice
- this plan focuses on desktop-managed project authoring first; declarative YAML
  parity can follow once the destination contract is stable

## Terminology

- publish destination: the operator-facing configuration entity currently backed
  by `publish_target`
- destination kind: the backend family selected for one destination, such as
  `filesystem` or `itch`
- destination binding: the relation between one build target and one publish
  destination, currently backed by `build_publish_binding`
- binding policy: the destination-kind-specific instructions attached to one
  destination binding
- active artifact location: the canonical location that later routines must use
  when they need the current artifact after publication side effects
- consuming publish step: a publish binding that mutates the local source file
  location, such as a filesystem move
- non-consuming publish step: a publish binding that reads the artifact without
  changing its local canonical location, such as an upload

## Goals

- make publish destinations a first-class project configuration surface
- allow each destination kind to expose binding policies per build target
- let projects bind only the build targets that matter for a given destination
- preserve zero-destination projects as valid and fully supported
- update the artifact's canonical location when publication physically moves it
- add Itch.io publication without collapsing the publish model into
  destination-specific hacks
- keep destructive edits explicit, reviewable, and safe
- keep the first slice compatible with the current durable publish model where
  practical

## Current Baseline

- the desktop shell already owns create-project and edit-project flows for
  repository settings and build targets
- the shell does not yet expose a first-class authoring surface for publish
  destinations or build-to-destination bindings
- the runtime store already persists `publish_targets` and
  `build_publish_bindings`
- the current publish runtime supports one backend, `filesystem`
- the current filesystem backend derives the destination path from one
  destination-wide `root_path` instead of per-build-target binding policy
- successful publish runs persist `destination_ref`, but the artifact's durable
  canonical location is not rewritten after a move or copy-like publication
- repository inspection already reports registered publish target counts, but
  it does not expose an editor contract for publish destination configuration
- queued publish work currently resolves publish metadata from live records
  instead of an explicit per-run snapshot

## Diagnosis

### 1. The Current Publish Model Is Too Coarse For Per-Target Behavior

The existing publish target model centers the destination row itself and leaves
the binding too weak.

Consequences:

- a filesystem destination cannot assign different folder paths per build target
- a remote destination cannot declare one channel, track, or upload rule per
  build target cleanly
- the backend contract is pushed toward destination-wide configuration even when
  the real behavior belongs to the binding

### 2. Artifact State Stops Being Canonical After Side-Effecting Publication

The current publish result stores `destination_ref`, but the artifact row still
describes the build output path rather than the latest usable source location.

Consequences:

- downstream routines cannot trust one canonical artifact location after a move
- later publish steps can read stale source paths
- diagnostics cannot clearly answer where the artifact actually lives now

### 3. Queued Publish Work Does Not Yet Own A Stable Execution Snapshot

Publish runs are durable work items, but their detailed destination behavior is
still resolved from mutable configuration.

Consequences:

- editing a destination while work is queued can change what a pending publish
  run will do
- queued work cannot be inspected as one fixed execution contract
- the system has no clean place to persist per-binding publish behavior

### 4. Destructive Project Edits Ignore Downstream Publish Coupling

Build targets are upstream entities for destination bindings, but the current
project editing model does not surface that coupling explicitly.

Consequences:

- removing a build target can silently destroy publish intent
- the operator does not get one clear warning about which destinations are
  affected
- edit flows are vulnerable to accidental data loss even when the database keeps
  referential integrity

### 5. The Current Publish Backend Contract Cannot Express Itch.io Cleanly

Itch.io publication needs backend-specific configuration, backend-specific
binding policy, capability checks, and explicit credentials.

Consequences:

- stretching the current filesystem contract would create backend-specific
  special cases in the wrong places
- the shell has no consistent shape for destination-specific validation
- the runtime has no explicit way to classify consuming versus non-consuming
  publish behavior

### 6. Multi-Destination Publication Needs Deterministic Artifact-Local Order

Once one artifact may flow through multiple destinations, publish ordering is
part of correctness.

Consequences:

- a move can invalidate the source path for later uploads if execution order is
  uncontrolled
- two consuming bindings on the same artifact are physically incompatible in the
  first slice
- publish worker concurrency must respect artifact-local sequencing even when
  repository-wide queues continue to exist

## Design Invariants

- a project may define zero or many publish destinations
- destination kinds own shared backend configuration and optional credentials
- destination bindings own per-build-target behavior
- unbound build targets must remain valid and publish nothing
- queued publish runs must execute from a snapshotted contract captured during
  release planning
- publish execution must resolve the current source from the artifact's active
  location rather than assuming the original build output path forever
- non-consuming destination bindings may coexist freely for the same build
  target
- at most one consuming destination binding may be enabled per build target in
  the first slice
- consuming destination bindings must execute after all non-consuming bindings
  for the same artifact
- removing a build target or publish destination that still owns bindings must
  require explicit confirmation in the shell
- the first slice should preserve existing SQL table names and Rust type names
  where that reduces migration risk; operator-facing copy may still say
  "publish destination"

## Target Behavior

### Create Project Wizard

The create-project wizard should gain a new `Publish Destinations` step after
`Build Targets`.

That step must allow the operator to:

- add zero or more destinations
- choose the destination kind for each row
- name the destination
- bind credentials when the destination kind requires them
- configure any destination-wide settings
- define bindings from selected build targets to the destination
- configure one binding policy per selected build target

The review step must summarize:

- how many destinations are configured
- which build targets are bound to each destination
- which build targets remain unbound
- which destinations require credentials or host capability checks

Saving a project with zero destinations must remain valid.

### Edit Project Flow

The repository detail screen should gain one dedicated `Publish Destinations`
section.

That section must let the operator:

- inspect existing destinations
- add new destinations
- edit destination-wide configuration
- edit per-target bindings and binding policies
- remove destinations explicitly
- understand whether a destination is disabled, misconfigured, unauthenticated,
  or blocked by missing host capability

Removing a build target must warn before the change is accepted whenever that
target is currently bound to one or more destinations.

The warning must list at least:

- the build target being removed
- the destination names that will lose bindings
- whether the removal will delete persisted binding configuration

Removing a destination must also warn when it owns one or more bindings.

### Filesystem Destination

The first filesystem-backed destination should be presented to the operator as
`Move To Folder`.

First-slice behavior:

- the destination itself owns only shared metadata such as name, enabled state,
  and optional default behavior flags
- each binding policy must declare an absolute destination directory path
- the binding policy must use a consuming `move` operation in the first slice
- successful execution moves the artifact into the configured folder and
  preserves the filename by default
- successful execution updates both the publish run result and the artifact's
  active location
- if a build target is not bound to that destination, its artifact remains in
  the runtime-managed output location

### Itch.io Destination

The first Itch.io-backed destination should be presented as `Itch.io Upload`.

First-slice behavior:

- the destination owns shared Itch project identity such as account name and
  game slug
- the destination binds one publish credential suitable for Itch authentication
- each binding policy must declare the Itch channel that receives that build
  target's artifact
- successful execution uploads the artifact as the new version for that channel
- successful execution records one remote destination reference for diagnostics
- successful execution does not rewrite the artifact's active local location

### Publish Planning And Execution

When a build target succeeds and produces an artifact:

- the release planner must inspect enabled destination bindings for that build
  target
- if no bindings exist, no publish runs are created for that artifact
- if bindings exist, the planner creates one publish run per enabled binding
- each publish run stores a snapshot of the resolved destination kind,
  destination config, binding policy, and the artifact source contract known at
  planning time
- non-consuming publish runs for the artifact must be ordered before the single
  consuming publish run, if one exists
- later edits to destination configuration must affect only newly planned runs,
  not already queued runs

## Proposed Architecture

### 1. Persist Publish Destinations As Project-Owned Configuration

The first slice should preserve the existing durable entities and reinterpret
them through better operator-facing terminology.

Recommended mapping:

- `publish_targets` remains the durable destination table
- `build_publish_bindings` remains the durable destination-binding table
- `publish_runs` remains one execution record per artifact and destination
  binding

Recommended contract changes:

- use `publish_targets.config_json` for destination-wide configuration only
- use `build_publish_bindings.options_json` for binding policy only
- add one publish-run snapshot field such as `execution_contract_json` so queued
  work does not depend on live mutable destination rows for correctness
- add explicit artifact active-location fields so later routines can resolve the
  current local source deterministically

Suggested artifact extension:

```json
{
  "active_location_kind": "filesystem_absolute",
  "active_location_ref": "D:/Releases/Windows/game.zip"
}
```

Suggested publish-run snapshot shape:

```json
{
  "destination": {
    "id": 12,
    "name": "steamlike-filesystem",
    "kind": "filesystem",
    "config": {}
  },
  "binding": {
    "id": 77,
    "build_target_id": 4,
    "policy": {
      "operation": "move",
      "directory_path": "D:/Releases/Windows"
    }
  },
  "artifact": {
    "source_kind": "runtime_artifact",
    "source_ref": "artifacts/windows/game.zip"
  }
}
```

### 2. Separate Destination-Wide Config From Binding Policy

The core modeling rule should be:

- destination row: backend identity, shared configuration, credentials, enabled
  state
- binding row: build-target selection plus binding-specific publish behavior

Suggested first-slice binding policy shapes:

Filesystem:

```json
{
  "operation": "move",
  "directory_path": "D:/Releases/Windows"
}
```

Itch:

```json
{
  "channel": "windows-stable",
  "userversion_template": "{{git_tag}}"
}
```

Suggested destination config shape for Itch:

```json
{
  "account_name": "indiegabo",
  "game_slug": "revolutions"
}
```

This split keeps backend-specific behavior on the correct side of the
build-target relationship.

### 3. Track The Active Artifact Location Explicitly

HGP needs one canonical answer to the question: where should the next routine
read this artifact from?

The build output location and the active current location are not always the
same once consuming publish steps exist.

Recommended contract:

- keep the original build output metadata for diagnostics and provenance
- add active-location fields that point to the latest usable local source
- if no consuming publication has happened, the active location resolves to the
  original runtime artifact location
- if a consuming publication succeeds, the active location is rewritten to the
  new absolute filesystem path
- if a non-consuming publication succeeds, the active location remains unchanged

The runtime should expose helper logic so later publish runs and post-publish
automation never need to guess between original and moved paths.

### 4. Classify Consuming Versus Non-Consuming Publish Behavior

The planner and executor must understand whether a destination binding mutates
the artifact's local source.

First-slice classification:

- filesystem move: consuming
- Itch upload: non-consuming

Required planner behavior:

- reject configurations with more than one enabled consuming binding for the
  same build target
- create stable per-artifact execution order
- run all non-consuming bindings before the single consuming binding

This avoids impossible first-slice states such as two different destinations
both trying to own the final moved file.

### 5. Execute Publish Runs From Snapshotted Contracts

Publish runs already represent durable work. They should also own the exact
behavior they will execute.

Required changes:

- release planning must serialize the resolved destination and binding contract
  into the publish run snapshot
- runtime execution should read the snapshot instead of reassembling behavior
  from currently editable destination rows
- diagnostics should surface both the stored destination metadata and the
  actual per-run execution snapshot when helpful

This keeps queued work stable across later project edits.

### 6. Deliver Filesystem Move As A Binding-Aware Backend

The current filesystem backend copies into a derived release path from one
destination-wide root.

The new filesystem backend should instead:

- parse binding policy from the publish-run snapshot
- validate that `directory_path` is present and absolute
- resolve the current active artifact source path
- move the artifact into the configured directory
- update the artifact's active location after successful completion
- persist the final absolute path into `publish_runs.destination_ref`

Compatibility note:

- legacy rows that still carry destination-wide `root_path` may be supported as
  a temporary fallback while shell-created destination bindings migrate to the
  new policy shape

### 7. Deliver Itch.io Through A Host-Native Publisher Surface

The Itch.io destination should be implemented through one explicit runtime
publisher path instead of shell-side upload logic.

Recommended first-slice shape:

- add `itch` to the publish backend enum
- use a dedicated credential kind such as `itch-api-key`
- execute uploads through a host-native Itch transport, preferably `butler`
- keep shell commands thin and route real execution through `runtime-publish`
- surface missing executable, missing credential, and upload failure as durable
  publish-run errors

Recommended Itch result surface:

- `destination_ref` stores one stable remote reference such as
  `itch://indiegabo/revolutions/windows-stable?v=v1.2.3`
- logs and error output must be sanitized so credentials never appear in shell
  diagnostics

### 8. Add First-Class Desktop Authoring Surfaces

The shell should treat publish destinations as one dedicated project editing
slice rather than a hidden extension of build target editing.

Recommended UI structure:

- create-project wizard step: `Publish Destinations`
- edit-project section: `Publish Destinations`
- destination cards or accordions for destination-wide settings
- nested binding editors per destination or per build target
- review copy that highlights which build targets will publish and where

Validation rules must include at least:

- destination names are unique per project
- filesystem binding directory paths are absolute and non-empty
- Itch destination config includes account and game slug
- Itch bindings include a channel
- only one consuming binding may be enabled per build target
- removing a build target or destination with bindings requires explicit
  confirmation

### 9. Surface Destination State In Diagnostics And Inspection

Repository and process inspection must explain publish destination state without
forcing the operator to decode database internals.

Required surfaces:

- repository summaries should report publish destination count instead of
  legacy publish target wording in the UI
- repository detail should show each destination's kind, enabled state,
  credential state, and bound targets
- process detail should show which destination executed for each publish run
- artifact inspection should show the active artifact location and any retained
  destination references

## Implementation Handoff Report

Last updated: 2026-05-17.

This section is the authoritative resume snapshot for the current delivery
slice. Use it to continue work on another device without replaying the full
investigation. The task list below remains the original backlog checklist.

### Completed Work

- Naming and scope decisions are effectively locked in practice:
  - operator-facing copy uses `Publish Destinations`
  - internal persistence still uses `publish_targets` and
    `build_publish_bindings`
  - zero-destination projects remain valid
  - the first slice still allows only one enabled consuming binding per build
    target
- Planning artifacts already exist:
  - `planning/publish-destinations-plan.md`
  - `planning/publish-destinations-execution-breakdown.md`
- Persistence and execution contract work landed:
  - publish runs persist `execution_contract_json`
  - artifacts persist `active_location_kind` and `active_location_ref`
  - publish execution resolves the source from the artifact active location
  - publish target credential binding is snapshotted by
    `publish_target_credentials_id`
- Publish planner semantics landed:
  - bindings are classified as `consuming` or `non_consuming`
  - filesystem move is consuming
  - Itch upload is non-consuming
  - non-consuming bindings execute before the single consuming binding
  - planning rejects more than one enabled consuming binding for the same
    build target
  - artifact-local claim gating preserves the planned execution order
- Filesystem destination backend landed:
  - filesystem publication is binding-aware
  - move semantics update the artifact active location after success
  - the moved absolute path is persisted into
    `publish_runs.destination_ref`
- Itch destination backend landed:
  - `runtime-publish` supports the `itch` backend
  - credential kind `itch-api-key` is implemented
  - Itch target config uses `account_name`, `game_slug`, and optional
    `butler_path`
  - Itch binding options use `channel` and `userversion_template`
  - the resolved userversion defaults to `git_tag` when the template is empty
  - successful remote refs use the shape
    `itch://<account>/<game>:<channel>@<resolved_userversion>`
  - `runtime-bin` resolves the snapshotted publish-target credential binding
    before calling `runtime-publish`
- Inspection and operator UI exposure landed partially:
  - `runtime-store` exposes publish destination config and binding inspection
    records
  - desktop shell inspection includes destination config, credential state,
    and binding consumption behavior
  - repository detail renders a read-only `Publish Destinations` section that
    explains non-consuming versus consuming bindings and the ordering rule
  - project list summary already uses `publish destination` wording

### Validated Work

- Focused `runtime-store` tests for snapshot persistence passed.
- Focused `runtime-store` tests for active artifact location resolution passed.
- Focused `runtime-store` tests for consuming versus non-consuming binding
  inspection passed.
- Focused `runtime-publish` tests for filesystem move behavior passed.
- Focused `runtime-publish` tests for Itch publish behavior passed.
- Focused `runtime-bin` worker test for snapshotted Itch credential execution
  passed.
- Focused `desktop-shell` inspection test passed.
- `npm run build --prefix apps/desktop/ui` passed.
- `cargo check -p desktop-shell -j 1 -q` passed.

### Current Code Anchors

- `crates/runtime-store/src/models.rs`
- `crates/runtime-store/src/lib.rs`
- `crates/runtime-publish/src/lib.rs`
- `crates/runtime-bin/src/main.rs`
- `apps/desktop/src-tauri/src/lib.rs`
- `apps/desktop/ui/src/services/projects.ts`
- `apps/desktop/ui/src/components/RepositoryProjectDetail.tsx`
- `apps/desktop/ui/src/components/ProjectsFocusScreen.tsx`

### Remaining Work

- Create-project authoring is still missing:
  - no `Publish Destinations` wizard step exists yet
  - no destination create/edit/remove flow exists in the wizard
  - no review-step summary exists for bound versus unbound build targets
- Edit-project flow is only partially done:
  - the inspection section exists
  - destination editing is not implemented yet
  - destructive confirmation for removing a bound build target or destination
    is not implemented yet
  - destination kind mutation rules are not implemented yet
- Diagnostics are still incomplete:
  - active artifact location is not yet surfaced in all publish and artifact
    views
  - process detail views still need destination kind, destination name, and
    result reference exposure
  - explicit Itch capability diagnostics are still limited
- Final validation is still incomplete:
  - focused UI tests for destination authoring do not exist yet
  - the smoke scenarios from Phase 8 have not been run yet

### Immediate Resume Plan

1. Start from Phase 5. Add the `Publish Destinations` wizard step and reuse
   the destination and binding shapes already exposed by inspection.
2. Continue with Phase 6 by turning the repository detail section from
   inspection-only into a full editor for destination-wide config and binding
   policy.
3. Add destructive confirmation flows before allowing build target or
   destination removal when bindings exist.
4. Finish Phase 7 by surfacing active artifact location and final destination
   refs in operator diagnostics.
5. Run focused UI tests and then the smoke scenarios described in Phase 8.

### Resume Warnings

- Do not reopen Phase 1 through Phase 4 unless a focused regression test
  fails. Those slices are already implemented and validated.
- Treat the current publish destination inspection contract as the reference
  shape for the next UI authoring work.
- The Itch backend depends on the snapshotted
  `publish_target_credentials_id`; do not fall back to live publish target
  credential lookups during execution.

## Task List

### Phase 0 - Scope Lock And Naming

- [x] Confirm `Publish Destinations` as the operator-facing label for the new
      project step and section.
- [x] Confirm the first slice keeps `publish_target` and
      `build_publish_binding` as internal persistence names.
- [x] Confirm zero-destination projects remain valid.
- [x] Confirm the first slice allows at most one consuming binding per build
      target.
- [x] Confirm YAML parity is deferred until the desktop destination contract is
      stable.

### Phase 1 - Persistence And Execution Contract

- [x] Define destination-wide config schemas for `filesystem` and `itch`.
- [x] Define binding-policy schemas for `filesystem` and `itch`.
- [x] Add publish-run snapshot storage such as `execution_contract_json`.
- [x] Add artifact active-location fields and migration.
- [x] Extend runtime-store models and query helpers for the new fields.
- [x] Add tests for snapshot serialization and artifact active-location
      persistence.

### Phase 2 - Publish Planning Semantics

- [x] Extend release planning to create publish runs from destination bindings.
- [x] Snapshot destination and binding behavior at publish-run creation time.
- [x] Classify destination bindings as consuming or non-consuming.
- [x] Reject projects that configure more than one consuming binding for the
      same build target.
- [x] Enforce deterministic per-artifact publish ordering.
- [x] Add tests for mixed non-consuming and consuming bindings.
- [ ] Add tests proving queued publish runs keep their original snapshot after
      later project edits.

### Phase 3 - Filesystem Move Backend

- [x] Replace the copy-oriented filesystem behavior with binding-aware move
      behavior for the new destination contract.
- [x] Resolve the publish source from the artifact's active location.
- [x] Move artifacts into the binding-specific absolute directory.
- [x] Update the artifact's active location on successful move.
- [x] Persist the moved absolute path into `publish_runs.destination_ref`.
- [ ] Add compatibility fallback for legacy destination-wide `root_path` rows if
      required.
- [ ] Add tests for selective binding where one build target moves and another
      remains in runtime output.

### Phase 4 - Itch.io Backend

- [x] Add `itch` backend support to `runtime-publish`.
- [x] Define one credential kind for Itch authentication.
- [x] Add host capability detection or executable resolution for the chosen
      upload transport.
- [x] Build a deterministic upload command from destination config and binding
      policy.
- [x] Persist a stable remote `destination_ref` for successful uploads.
- [x] Sanitize upload logs and failures so secrets never leak.
- [x] Add tests for config parsing, command construction, and failure
      classification.

### Phase 5 - Desktop Create-Project Flow

- [ ] Add the `Publish Destinations` wizard step after `Build Targets`.
- [ ] Add destination creation, removal, and editing UI.
- [ ] Add per-destination binding editors for build targets.
- [ ] Add client-side validation for destination config and binding policy.
- [ ] Add review-step summaries for destinations and unbound build targets.
- [ ] Add confirmation when removing a build target that owns destination
      bindings.
- [ ] Add focused UI tests for destination authoring and destructive warnings.

### Phase 6 - Desktop Edit-Project Flow

- [x] Add a dedicated `Publish Destinations` section to repository detail.
- [x] Render existing destinations with bound-target summaries.
- [ ] Allow editing destination-wide config and binding policies.
- [ ] Add confirmation when removing a destination that still owns bindings.
- [ ] Add confirmation when changing destination kind invalidates existing
      bindings.
- [ ] Add focused UI tests for edit-project destination workflows.

### Phase 7 - Diagnostics And Operator Reporting

- [x] Update repository summaries to use `publish destination` wording in the
      shell.
- [x] Extend repository inspection payloads with destination and binding
      summaries.
- [ ] Surface active artifact location in publish and artifact diagnostics.
- [ ] Show destination kind, destination name, and result reference in process
      detail views.
- [ ] Surface capability and credential state for Itch destinations.

### Phase 8 - Documentation And Validation

- [ ] Document the operator contract for publish destinations in the desktop
      product docs.
- [ ] Document filesystem move behavior and active artifact location semantics.
- [ ] Document Itch destination prerequisites and credential requirements.
- [x] Run focused Rust tests for runtime-store and runtime-publish.
- [ ] Run focused desktop UI tests for create and edit flows.
- [ ] Run one end-to-end smoke scenario with no destinations, one filesystem
      destination, and one mixed filesystem-plus-Itch scenario.

## Acceptance Criteria

- [ ] a project can be created and edited with zero publish destinations
- [ ] one project can define multiple publish destinations of different kinds
- [ ] one destination can bind only a selected subset of build targets
- [ ] an unbound build target keeps its artifact in runtime-managed output
- [ ] a filesystem destination can move one build target artifact into a
      binding-specific absolute folder
- [ ] after a filesystem move, the artifact's active location resolves to the
      moved absolute path
- [ ] an Itch destination can upload only the build targets that are explicitly
      bound to it
- [ ] removing a build target with existing destination bindings requires an
      explicit confirmation in the shell
- [ ] queued publish runs keep the snapshotted behavior they were created with,
      even if the project is edited later
- [ ] project validation rejects more than one enabled consuming binding for the
      same build target in the first slice
- [ ] repository and process diagnostics expose publish destination state and
      final result references clearly

## Suggested Execution Order

1. lock the naming and scope decisions
2. add persistence support for snapshots and active artifact location
3. implement planner ordering and validation semantics
4. land the filesystem move backend
5. add desktop authoring for destinations and destructive confirmations
6. land the Itch backend and capability diagnostics
7. update operator reporting and finish the validation pass
