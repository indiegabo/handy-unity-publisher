# Project-Scoped Repository Auth Execution Breakdown

## Purpose

This document breaks the project-scoped repository auth plan into narrow work
packages that can be implemented and validated incrementally.

Each package names the target files, the contract it must establish, what must
not change in the same slice, and the minimum validation expected before the
package can be considered done.

## Scope Lock

- publish target credentials remain out of scope
- SSH repository support remains out of scope
- local workspace projects remain out of scope
- runtime automation must stay non-interactive throughout every package
- do not widen a package once implementation starts unless a missing contract
  makes the current slice impossible to validate

## Delivery Rules

- prefer one durable backend contract before UI branching logic
- keep provider detection separate from provider login capability
- preserve reusable provider accounts while moving repository binding decisions
  into the owning project flow
- validate the narrowest affected slice first before running broader checks
- keep startup, polling, and build execution free of interactive auth prompts

## Execution Order

1. scope guard and baseline removal
2. provider detection and repository access assessment
3. durable repository auth state
4. project-scoped binding commands
5. create-project wizard integration
6. edit-project integration
7. auth provider inventory repositioning
8. runtime auth hardening
9. documentation and smoke validation

## Current Status

- Completed: Work Packages 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, and 10
- In progress: none
- Remaining: interactive provider connectors beyond GitHub

## Work Package 0 - Scope Guard And Baseline Cleanup

### Objective

Freeze the immediate behavioral rules before deeper auth work lands.

### Target Files

- `planning/project-scoped-repository-auth-plan.md`
- `planning/project-scoped-repository-auth-execution-breakdown.md`

### Tasks

- keep project-scoped auth as the only accepted ownership model
- keep public-repository access credential-free by default
- keep interactive auth restricted to create and edit project flows
- keep provider inventory secondary to project-owned auth decisions

### Done When

- the planning documents say clearly where interactive auth is allowed
- the next work packages can be implemented without reopening scope questions

### Validation

- confirm the planning documents still match the intended behavior

## Work Package 1 - Remove Global Auto-Binding

### Objective

Stop the shell from automatically binding GitHub credentials to repositories as
the first root-cause removal.

### Target Files

- `src-tauri/src/lib.rs`

### Tasks

- remove bind-all behavior from provider login completion
- remove default GitHub credential auto-binding during repository project
  creation
- preserve reusable provider credentials as standalone records
- keep existing provider inventory loading intact until the later UI packages

### Out Of Scope

- provider detection redesign
- explicit public versus private visibility selection UX
- create and edit UI changes beyond what is needed to keep tests aligned

### Done When

- logging into GitHub no longer mutates unrelated repository projects
- creating a GitHub repository project no longer silently inherits credentials
- existing tests are updated to prove the new project-scoped baseline

### Validation

- focused Rust tests for `persist_github_auth_login` and
  `persist_repository_project`

## Work Package 2 - Provider Registry And URL Parsing

### Objective

Introduce one provider-detection layer that understands repository URL host
families independently from login workflows.

### Target Files

- `src-tauri/src/lib.rs` or one extracted module under
  `src-tauri/src/`
- `crates/runtime-git/src/lib.rs` or one extracted module under
  `crates/runtime-git/src/`

### Tasks

- define provider IDs for GitHub, GitLab, Bitbucket, and unknown
- normalize HTTPS repository URLs into one canonical assessment input
- detect provider family and instance URL from the repository URL alone
- add focused tests for supported and unsupported URL shapes

### Out Of Scope

- credential binding
- visibility probing
- UI consumption of the new contract

### Done When

- one backend function can classify repository provider identity from a URL
- GitHub-only special casing is no longer the only host check in the auth path

### Validation

- focused Rust tests for provider parsing and normalization

## Work Package 3 - Repository Access Assessment Command

### Objective

Expose one backend command that determines provider identity from the URL and
supports a visibility-aware repository access assessment without auto-probing
public versus private state.

### Target Files

- `src-tauri/src/lib.rs`
- `crates/runtime-git/src/lib.rs`
- `src-react/src/services/projects.ts` or a new dedicated repository
  access service

### Tasks

- define the provider-detection and access-assessment payload shapes
- detect provider identity from URL heuristics and optional lightweight
  instance probing only when heuristics are inconclusive
- let create and edit flows supply repository visibility explicitly when
  deciding auth requirement
- include operator-facing status text in the response
- keep any non-interactive Git probe free from credential helper escalation

### Out Of Scope

- persistent repository auth state
- create and edit screen rendering
- provider-specific login commands

### Done When

- the shell can answer which provider owns a repository URL without requiring
  login first
- the backend can combine provider identity with operator-selected visibility
  to decide whether auth is required

### Validation

- focused Rust tests for assessment classification

## Work Package 4 - Durable Repository Auth State

### Objective

Persist repository provider, visibility, and binding state so the UI and
runtime can reason about access without recomputing everything on every read.

### Target Files

- `crates/runtime-store/migrations/*`
- `crates/runtime-store/src/models.rs`
- `src-tauri/src/lib.rs`

### Tasks

- add repository auth state columns through a migration
- extend store models and shell DTOs with the new auth fields
- ensure repository create and update flows can persist assessment output
- invalidate stale auth state when repository URL changes

### Out Of Scope

- login execution
- final UI presentation logic

### Done When

- repository inspection can report provider, visibility, auth requirement, and
  binding state directly
- repository URL changes can reset incompatible auth state deterministically

### Validation

- focused migration and repository inspection tests

## Work Package 5 - Project-Scoped Binding Commands

### Objective

Add explicit commands to connect, reconnect, and clear repository auth for one
project.

### Target Files

- `src-tauri/src/lib.rs`
- `crates/runtime-store/src/models.rs` only if new command payloads need shared
  structs

### Tasks

- add one connect command scoped to a repository project
- add one reconnect command for stale or changed bindings
- add one disconnect command that clears repository credential binding
- reuse provider account records without reintroducing global repository
  mutations

### Out Of Scope

- provider inventory redesign
- runtime failure handling beyond command-level updates

### Done When

- one repository can be authenticated without mutating any other repository
- the backend can clear or replace repository credentials intentionally

### Validation

- focused Rust tests for per-repository connect, reconnect, and disconnect

## Work Package 6 - Create Project Access Card

### Objective

Replace the GitHub-specific create-step auth callout with one repository-access
card driven by the assessment contract.

### Target Files

- `src-react/src/components/CreateProjectWizard.tsx`
- `src-react/src/services/projects.ts` or the dedicated repository access
  service
- `src-react/src/styles.css` if styling changes are needed

### Tasks

- debounce repository provider detection during editing
- render provider, explicit visibility, auth requirement, and binding status
  inline
- allow repositories marked public to proceed without login
- show project-specific connection controls only for private repositories on
  providers with supported login
- show a clear public-only message for unsupported private providers

### Out Of Scope

- edit-project screen changes
- provider inventory screen changes

### Done When

- the create wizard no longer blocks every GitHub URL on provider login state
- the access step exposes only the connection controls required by the
  repository assessment, including inline credential creation when needed

### Validation

- `npm run build --prefix src-react`

## Work Package 7 - Edit Project Access Ownership

### Objective

Give the edit-project screen the same repository-access ownership as the create
wizard.

### Target Files

- `src-react/src/components/RepositoryProjectDetail.tsx`
- `src-react/src/services/projects.ts` or the dedicated repository access
  service
- `src-react/src/styles.css` if styling changes are needed

### Tasks

- surface provider, visibility, and binding state in project detail
- add re-check, reconnect, and disconnect actions
- invalidate auth state after repository URL changes
- keep the save flow aligned with project-scoped repository ownership

### Out Of Scope

- provider inventory redesign
- broader detail-page visual refactors unrelated to auth ownership

### Done When

- edit-project becomes the first-class recovery surface for repository auth
- the page no longer reduces repository auth to a static credential label

### Validation

- `npm run build --prefix src-react`

## Work Package 8 - Auth Provider Inventory Repositioning

### Objective

Demote the global providers screen from primary auth entry point to reusable
account inventory and diagnostics surface.

### Target Files

- `src-react/src/components/AuthProvidersFocusScreen.tsx`
- `src-react/src/services/auth.ts`
- `src-tauri/src/lib.rs`

### Tasks

- update copy so providers are presented as reusable account sessions
- remove language that claims repositories will use a provider by default
- keep provider availability and reconnect actions visible
- keep the screen useful for diagnostics without letting it own repository
  binding decisions

### Out Of Scope

- create and edit access flows
- runtime auth failure logic

### Done When

- the providers screen no longer advertises global repository binding behavior
- the screen reads as inventory and diagnostics, not as the main auth workflow

### Validation

- `npm run build --prefix src-react`

## Work Package 9 - Runtime Non-Interactive Auth Hardening

### Objective

Guarantee that polling and build execution stay non-interactive even when
credentials are stale, missing, or invalid.

### Target Files

- `crates/runtime-git/src/lib.rs`
- `crates/runtime-bin/src/workers.rs`
- `crates/runtime-bin/src/builds.rs`
- `src-tauri/src/lib.rs` if repository auth status updates are
  surfaced there

### Tasks

- ensure every automated Git path disables terminal and provider interaction
- classify auth failures into durable repository auth states when possible
- stop repeated auth thrash during polling for repositories that need re-auth
- preserve anonymous access for public repositories without credentials

### Out Of Scope

- adding new provider login connectors
- wider runtime scheduling refactors

### Done When

- startup, polling, and build execution do not open browser or credential
  windows
- private repository auth problems surface as deterministic runtime state, not
  interactive recovery

### Validation

- focused Rust tests for runtime auth resolution and failure classification

## Work Package 10 - Documentation And Smoke Validation

### Objective

Align architecture and operator documentation after the functional slices land,
then validate the full public/private flow end to end.

### Target Files

- `docs/architecture.md`
- `docs/pipeline-yaml-guide.md` if repository auth guidance there becomes stale
- any relevant planning status documents that need progress updates

### Tasks

- document project-scoped repository auth behavior
- document that public repositories do not require login by default
- document that automated runtime paths remain non-interactive
- run one smoke path covering public repository create, private repository auth,
  app restart, and runtime execution

### Done When

- docs match the implemented auth ownership model
- the end-to-end path proves the app no longer reopens credentials windows as a
  default habit

### Validation

- `npm run build --prefix src-react`
- focused Rust tests for touched auth slices
- one manual or scripted smoke run across the full repository-auth flow
