# Publish Destinations Execution Breakdown

## Purpose

This document breaks the publish destinations plan into narrow work packages
that can be implemented and validated incrementally.

Each package names the primary crates and files it must change, the contract it
must establish, what must not widen in the same slice, and the minimum
validation expected before the package can be considered done.

## Scope Lock

- operator-facing terminology remains `publish destinations`
- first-slice persistence names remain `publish_targets` and
  `build_publish_bindings`
- zero-destination projects remain valid throughout every package
- the first delivery window supports only `filesystem` and `itch`
- the first delivery window allows at most one consuming binding per build
  target
- YAML parity remains deferred until the desktop-managed contract is stable
- do not widen a package once implementation starts unless one missing backend
  contract makes the current slice impossible to validate

## Delivery Rules

- prefer one durable runtime-store contract before UI branching logic
- keep destination-wide config separate from binding policy
- introduce active artifact location before implementing consuming publish
  backends
- validate the narrowest affected slice first before moving to a broader
  package
- preserve shell bindings as thin DTO and command layers over runtime logic
- keep credentials and capability checks backend-specific instead of inventing
  a generic plugin system in the first slice

## Execution Order

1. scope guard and execution breakdown
2. runtime-store persistence contract
3. publish planning snapshots and ordering semantics
4. filesystem move backend
5. Itch.io backend contract
6. create-project destination authoring
7. edit-project destination authoring and destructive confirmations
8. diagnostics and inspection updates
9. docs and end-to-end validation

## Current Status

- Completed: planning baseline in `publish-destinations-plan.md`
- In progress: Work Package 1
- Remaining: Work Packages 2 through 8

## Work Package 0 - Scope Guard And Breakdown

### Objective

Freeze the publish-destination vocabulary, first-slice invariants, and package
order before implementation widens.

### Target Files

- `planning/publish-destinations-plan.md`
- `planning/publish-destinations-execution-breakdown.md`

### Tasks

- keep `Publish Destinations` as the operator-facing product term
- keep `publish_target` and `build_publish_binding` as internal persistence
  names in the first slice
- keep active artifact location as a mandatory prerequisite for consuming
  backends
- keep the one-consuming-binding-per-build-target rule explicit in planning

### Done When

- the planning documents define the same first-slice invariants
- the next work packages can execute without reopening naming or ownership
  questions

### Validation

- confirm the planning documents still match the intended publish-destination
  contract

## Work Package 1 - Runtime-Store Persistence Contract

### Objective

Extend the durable store so publish runs can carry execution snapshots and
artifacts can carry one canonical active location.

### Target Files

- `crates/runtime-store/migrations/*`
- `crates/runtime-store/src/models.rs`
- `crates/runtime-store/src/lib.rs`

### Tasks

- add one migration for publish-run execution snapshots
- add one migration for artifact active-location persistence
- extend `PublishRunRecord` with snapshot data
- extend `PublishExecutionPlan` so it can resolve source path from active
  location instead of only `artifact_root_path + path`
- extend `ArtifactRecord` and `ArtifactInspectionRecord` with active-location
  fields
- persist default active-location values when artifacts are registered or
  replaced
- persist a baseline execution contract snapshot when publish runs are created
- add focused tests for migration, snapshot persistence, and active-location
  resolution

### Out Of Scope

- consuming versus non-consuming planner validation
- Itch capability checks
- shell authoring flows

### Done When

- the database stores execution snapshot JSON for publish runs
- the database stores active artifact location fields
- runtime-store read models expose the new fields coherently
- publish execution plans can resolve source paths from the active location

### Validation

- focused Rust tests in `crates/runtime-store`

## Work Package 2 - Publish Planning Snapshots And Ordering

### Objective

Make publish planning create stable per-run execution contracts and enforce
first-slice ordering rules.

### Target Files

- `crates/runtime-store/src/lib.rs`
- `crates/runtime-store/src/models.rs`
- `crates/runtime-bin/src/main.rs` only if queue inspection output needs the
  new planner fields

### Tasks

- serialize destination config and binding policy into per-run snapshots during
  publish planning
- classify bindings as consuming or non-consuming
- enforce deterministic per-artifact publish order
- reject more than one enabled consuming binding per build target
- ensure queued runs are stable after later destination edits
- add tests for mixed upload-plus-move scenarios and configuration rejection

### Out Of Scope

- actual filesystem move implementation
- Itch transport execution
- UI validation flows

### Done When

- queued publish runs no longer depend on mutable live destination rows for
  behavioral correctness
- planner semantics reject physically incompatible consuming configurations

### Validation

- focused Rust tests for `plan_build_publish_runs` and publish-run inspection

## Work Package 3 - Filesystem Move Backend

### Objective

Replace the current destination-wide filesystem copy behavior with
binding-aware, consuming move behavior.

### Target Files

- `crates/runtime-publish/src/lib.rs`
- `crates/runtime-bin/src/main.rs`
- `crates/runtime-store/src/lib.rs`
- `crates/runtime-store/src/models.rs` only if completion inputs must expand

### Tasks

- read filesystem policy from the publish-run snapshot
- resolve the source from artifact active location
- move the artifact into the binding-specific absolute directory
- update publish-run `destination_ref` on success
- update artifact active location when the move succeeds
- keep filesystem destinations on the binding-only `directory_path` contract;
  update stale tests instead of preserving a compatibility path

### Out Of Scope

- Itch uploads
- create or edit shell UX

### Done When

- the filesystem backend consumes the artifact through the new binding-aware
  contract
- downstream publish steps and diagnostics see the moved artifact as the active
  source

### Validation

- focused Rust tests in `runtime-publish`, `runtime-store`, and `runtime-bin`

## Work Package 4 - Itch.io Backend Contract

### Objective

Add one explicit Itch.io publish backend with backend-specific credentials,
config parsing, and host capability checks.

### Target Files

- `crates/runtime-publish/src/lib.rs`
- `crates/runtime-bin/src/main.rs`
- `crates/runtime-store/src/models.rs` only if shared DTOs need extension
- `apps/desktop/src-tauri/src/lib.rs` only if capability diagnostics must be
  exposed in the same slice

### Tasks

- add `itch` backend resolution
- define one credential kind for Itch authentication
- detect or resolve the chosen upload transport
- build deterministic upload commands from destination config and binding
  policy
- persist stable remote destination references
- sanitize logs and failures so credentials never leak

### Out Of Scope

- shell authoring UI beyond capability display if strictly required
- broader credential inventory redesign

### Done When

- one Itch destination can upload one bound artifact through the runtime
- failures surface as durable publish-run errors without leaking secrets

### Validation

- focused Rust tests for config parsing, capability classification, and command
  construction

## Work Package 5 - Create-Project Destination Authoring

### Objective

Add one dedicated `Publish Destinations` step to the create-project wizard.

### Target Files

- `apps/desktop/ui/src/components/CreateProjectWizard.tsx`
- `apps/desktop/ui/src/services/projects.ts`
- `apps/desktop/src-tauri/src/lib.rs`

### Tasks

- add the new wizard step after `Build Targets`
- add destination editors per destination kind
- add per-build-target binding editors
- add client-side validation for config, policy, and one-consuming-binding rule
- include destination summaries in the review step
- warn when removing a build target also removes destination bindings

### Out Of Scope

- edit-project maintenance workflows
- process diagnostics redesign

### Done When

- a new project can be authored with zero or many publish destinations
- destination and binding validation happens before save

### Validation

- focused UI tests for the wizard step and destructive confirmation states

## Work Package 6 - Edit-Project Destination Authoring

### Objective

Add one dedicated destination-management section to repository detail.

### Target Files

- `apps/desktop/ui/src/components/RepositoryProjectDetail.tsx`
- `apps/desktop/ui/src/services/projects.ts`
- `apps/desktop/src-tauri/src/lib.rs`

### Tasks

- render persisted destinations and bound-target summaries
- allow destination create, edit, and removal
- allow per-target binding and policy editing
- warn when removing a destination also removes bindings
- warn when removing a build target also removes destination bindings
- surface credential and capability state where relevant

### Out Of Scope

- new backend kinds beyond `filesystem` and `itch`
- broader project-detail layout refactors unrelated to destination workflows

### Done When

- persisted project destinations can be maintained in-place from edit-project
- destructive changes are explicit and contextual

### Validation

- focused UI tests for edit-project destination authoring and confirmation flows

## Work Package 7 - Diagnostics And Inspection Updates

### Objective

Expose publish-destination state and active artifact location clearly in shell
and runtime diagnostics.

### Target Files

- `crates/runtime-store/src/lib.rs`
- `crates/runtime-store/src/models.rs`
- `crates/runtime-bin/src/main.rs`
- `apps/desktop/src-tauri/src/lib.rs`
- `apps/desktop/ui/src/components/ProjectsFocusScreen.tsx`
- `apps/desktop/ui/src/components/ProcessDetailFocusScreen.tsx`

### Tasks

- change shell copy from `publish target` to `publish destination`
- expose destination summaries in repository inspection payloads
- expose active artifact location in artifact inspection surfaces
- expose destination name, kind, and result reference in process detail
- surface Itch capability and credential state where operator decisions depend
  on it

### Out Of Scope

- new notification or event-stream behavior

### Done When

- operators can tell what published, where it went, and which artifact location
  is now canonical

### Validation

- focused Rust tests for DTO loading plus focused UI tests for copy and display

## Work Package 8 - Documentation And End-To-End Validation

### Objective

Document the final operator contract and validate the new publish-destination
flow through focused smoke scenarios.

### Target Files

- `docs/*` as needed
- `planning/publish-destinations-plan.md`
- `planning/publish-destinations-execution-breakdown.md`
- targeted tests under the affected crates and UI packages

### Tasks

- document filesystem move behavior and active artifact location semantics
- document Itch prerequisites and credential requirements
- update any stale `publish target` product copy that survived prior packages
- run focused runtime-store, runtime-publish, runtime-bin, and UI validations
- run one smoke scenario with zero destinations, one filesystem destination,
  and one mixed Itch plus filesystem project

### Done When

- docs match the implemented publish-destination contract
- targeted validations cover the main operator paths and backend behaviors

### Validation

- focused test runs for affected crates and UI slices plus one smoke workflow
