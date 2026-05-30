# Project-Scoped Repository Auth Plan

## Purpose

This document defines the migration from the current global repository login
model to a project-scoped repository authentication flow.

The target behavior is:

- the operator enters only the repository URL during create or edit
- the operator declares whether the repository should be treated as public or
  private
- HGP detects the repository platform automatically from that URL
- public repositories continue without credentials
- private repositories expose one explicit connection surface inside that
  project's create or edit flow
- successful authentication binds only that project instead of silently binding
  every repository on the same provider
- app startup, repository polling, release planning, and build execution never
  open credential windows on their own

## Scope Lock

- this plan applies to managed repository projects in the desktop shell
- this plan applies to the create-project wizard, edit-project screen, auth
  provider inventory, Tauri command layer, runtime Git auth resolution, and
  repository inspection payloads
- this plan does not change publish target credentials
- this plan does not add SSH repository support in the first slice; the current
  HTTPS-only repository model remains in place
- this plan does not expand local workspace projects; repository projects remain
  the only active source mode in scope
- this plan does not replace the secret storage backend; it changes when and
  where authentication is requested

## Goals

- make repository authentication a project-owned decision instead of a global
  provider side effect
- detect provider family from the repository URL without requiring extra
  operator input
- request authentication only when the operator declares that the repository is
  private
- keep automated runtime paths non-interactive at all times
- preserve reusable provider accounts while making repository bindings explicit
- create a provider-agnostic contract so GitHub, GitLab, Bitbucket, and later
  hosts fit the same flow

## Current State

- the desktop shell exposes a global auth provider inventory centered on a
  GitHub browser login flow
- the create-project wizard treats any GitHub URL as requiring a connected
  GitHub login before the project can be created
- `persist_github_auth_login` currently binds the GitHub credential to every
  GitHub repository project
- `persist_repository_project` currently auto-binds a default GitHub credential
  when one is already known on the machine
- the edit-project screen only reports a static credential summary and does not
  own the authentication workflow
- provider detection is hardcoded to a GitHub host check
- repository visibility is not probed before the UI decides whether login is
  required
- runtime poll and build paths resolve repository credentials during automated
  work instead of restricting interactive auth to explicit operator actions

## Diagnosis

### 1. Authentication Ownership Is Global Instead Of Project-Scoped

The current shell treats GitHub login as a machine-wide provider state and then
applies that state broadly.

Consequences:

- connecting GitHub can silently affect multiple repository projects
- creating one GitHub project can inherit credentials without an explicit
  project-level decision
- the operator does not get one clear moment where repository access is
  confirmed for the specific project being created or edited

### 2. Provider Recognition Is GitHub-Specific And Too Narrow

The current repository URL handling only recognizes GitHub through a dedicated
host parser.

Consequences:

- Bitbucket, GitLab, and later hosts do not fit the same contract
- UI copy and validation logic are locked to one provider instead of one
  repository-access model
- the shell cannot decide capability or login strategy from URL alone for
  non-GitHub hosts

### 3. The UI Requests Authentication Before It Knows Whether It Is Needed

The create flow currently asks for GitHub login based on URL host family alone.
It does not first determine whether the repository is public.

Consequences:

- public repositories still trigger auth friction
- the operator must think about credentials before HGP has even proved that the
  repository needs them
- the product violates the desired rule that login should appear as rarely as
  possible

### 4. Automated Repository Work Can Still Drive Credential UX

Repository polling and build preparation resolve Git auth from runtime-side
automation paths. Even when credentials are stored, the runtime still owns the
moment where those credentials are translated into Git operations.

Consequences:

- startup and process execution can still be the moment where provider tooling
  tries to recover or refresh auth state
- auth recovery is not isolated to explicit create or edit actions
- private repository failures can repeat across polling cycles instead of
  surfacing one durable re-auth requirement on the project

### 5. The Edit Flow Does Not Provide A First-Class Re-Auth Surface

Project detail only reports the currently bound credential name.

Consequences:

- re-auth is displaced to the global providers screen instead of the owning
  project
- changing repository URL or provider does not naturally lead the operator
  through a repository-specific reassessment
- the project screen cannot explain whether the repository is public, private,
  authenticated, stale, or unsupported

## Design Invariants

- the repository URL determines provider family, while repository visibility is
  an explicit operator choice in create and edit flows
- interactive login is allowed only from explicit create-project or edit-project
  actions
- automated runtime operations must be non-interactive even when credentials are
  missing or stale
- reusable provider accounts may stay global, but repository bindings must be
  explicit and per project
- public repositories must continue to work with `credentials_id = NULL`

## Target Behavior

### URL Entry Or Change

When the operator enters or edits a repository URL, the shell must:

- normalize and validate the URL
- identify provider family and instance URL from the host and path shape
- expose the operator-selected repository visibility
- expose a clear status message describing whether project authentication is
  required for that visibility on the detected provider

### Public Repositories

For public repositories:

- no login button is required to create or save the project when the operator
  marks the repository as public
- the project stores no repository credential binding by default
- if the operator changes a previously private repository to public, its stored
  project credential binding is cleared automatically on save
- polling, tag discovery, and checkout continue anonymously
- the UI should clearly say that no repository authentication is required

### Private Repositories

For private repositories:

- the create or edit flow shows the project-specific connection controls needed
  for the detected provider
- providers with shell-backed login support may expose an inline login action
  for the current project
- providers without supported login must say clearly that only public
  repositories are available on that platform for now
- successful login connects only the current repository project
- the binding may reuse a provider account credential record, but only through
  an explicit per-project association
- runtime automation uses the stored project binding without trying to open new
  login windows

### Invalid Or Unknown Repositories

For invalid or unresolved repositories:

- HGP must not ask for login as a first response
- the UI must show why classification failed and allow retry after URL changes
- project creation or save should stay blocked only when the access state is not
  trustworthy enough to continue

### Re-Authentication

When credentials become stale or the repository URL changes in a way that makes
the current binding invalid:

- the project enters a durable `reauth_required` or equivalent state
- the edit-project screen becomes the explicit recovery point
- the runtime reports a deterministic auth failure without launching provider
  UI

## Proposed Architecture

### 1. Repository Access Assessment Contract

Add one provider-detection surface and one visibility-aware repository-access
assessment surface that the shell can call from both create and edit flows.

Suggested response shape:

```json
{
  "provider_id": "github",
  "provider_label": "GitHub",
  "instance_url": "https://github.com",
  "normalized_url": "https://github.com/org/project.git",
  "visibility": "public",
  "auth_requirement": "none",
  "auth_status": "not_required",
  "supports_interactive_login": true,
  "message": "Public repository detected through anonymous remote access."
}
```

Responsibilities:

- provider detection should start from URL heuristics and only escalate to a
  lightweight unauthenticated instance probe when those heuristics are
  inconclusive
- visibility should be supplied explicitly by the operator instead of being
  auto-classified from anonymous access
- compact status text suitable for direct UI display
- enough detail for the shell to choose whether login, retry, or save should be
  available

### 2. Provider Registry And URL Parsing

Replace the current GitHub-only host check with a provider registry.

First-class provider IDs for the new contract should be:

- `github`
- `gitlab`
- `bitbucket`
- `unknown`

Requirements:

- support public hosted URLs first
- preserve the instance URL as part of the detected provider identity
- for self-hosted GitHub Enterprise and GitLab hosts, try host and path
  heuristics first and run a lightweight unauthenticated instance probe only
  when those heuristics do not identify the provider confidently
- separate provider detection from login capability so a provider can still be
  recognized before its login connector is finished

Recommended order:

1. normalize the HTTPS URL
2. try provider detection from host and URL shape heuristics
3. if the provider is still inconclusive, run a lightweight unauthenticated
  instance probe to identify the product family
4. persist the detected provider identity and instance URL for the access
  assessment result

### 3. Repository Visibility Is Operator-Declared

Repository visibility should not be auto-classified from anonymous access in
the first slice. The operator must choose whether the repository should be
treated as public or private.

Required rules:

1. normalize the HTTPS URL
2. detect the provider from URL heuristics and optional lightweight instance
  probing only when heuristics are inconclusive
3. trust the operator-selected visibility when deciding whether auth is
  required
4. if the operator selects `private` for an unsupported provider, block the
  save and say that only public repositories are available for that platform

### 4. Project-Scoped Credential Binding Model

Keep credentials reusable, but stop treating them as automatically bound to all
repositories on the same provider.

Required changes:

- remove the global bind-all behavior from provider login commands
- remove default credential auto-binding during repository project creation
- add explicit connect, reconnect, and disconnect actions at the project level
- clear or invalidate incompatible bindings when repository URL or provider
  changes
- preserve `repositories.credentials_id` as the binding edge, but make the edge
  operator-driven instead of provider-driven

### 5. Durable Repository Auth State

The repository model needs durable auth assessment fields so the UI and runtime
can reason about access state without re-deriving it from scratch every time.

Prefer explicit repository fields over opaque JSON.

Suggested durable state:

- `source_provider_id`
- `source_instance_url`
- `visibility_status`
- `auth_requirement_status`
- `auth_binding_status`
- `auth_status_message`
- `auth_last_verified_at`

Suggested auth binding states:

- `not_required`
- `required_unbound`
- `bound_ready`
- `reauth_required`
- `unsupported`
- `unknown`

This state should flow into repository inspection payloads and any future
runtime event surfaces.

### 6. Non-Interactive Runtime Auth Contract

Runtime automation must never be allowed to escalate into an interactive login
moment.

Required rules:

- if `credentials_id` is empty, Git operations run anonymously
- if credentials exist, helper resolution still runs in non-interactive mode
- auth failures become deterministic runtime errors and repository state updates
- polling and build preparation must not open browser flows, host login windows,
  or credential prompts
- repeated auth failures for one project should collapse into a durable
  repository-level `reauth_required` signal instead of repeating the same
  failure forever

### 7. UI Flow Changes

#### Create Project Wizard

Replace the current GitHub-specific auth callout with a repository-access card
owned by the access step.

The card should show:

- detected provider
- an explicit visibility selector
- whether login is required
- current binding status
- inline actions such as `Check access`, `Log in`, `Connect`, `Disconnect`,
  or `New credential`

Behavior:

- URL changes trigger debounced provider detection
- public repositories show a success state and no login CTA when the operator
  selects `Public`
- private repositories show project-specific connection controls only when the
  detected provider supports private login
- unsupported private providers show a clear public-only message instead of a
  fake credential flow
- unsupported providers show a clear capability message instead of pretending
  the flow is complete

#### Edit Project Screen

Project detail must gain the same repository-access ownership as create.

The edit flow should:

- show provider, visibility, and binding state as first-class project metadata
- keep repository visibility operator-editable from the project screen
- clear the stored project credential binding automatically when the operator
  changes the repository to public and saves
- support re-check, reconnect, and disconnect directly from the project screen
- mark auth state stale when the repository URL changes
- prevent silent reuse of an incompatible credential after provider changes

#### Auth Providers Screen

The global providers screen remains useful, but its role changes.

It should become:

- an inventory of reusable connected accounts
- a diagnostic screen for provider availability
- a secondary place to establish a reusable account session

It should stop implying:

- that connecting one provider automatically configures all matching projects
- that the global screen is the primary moment where repository auth decisions
  are made

## Primary Change Surfaces

### Desktop UI

- `src-react/src/components/CreateProjectWizard.tsx`
- `src-react/src/components/RepositoryProjectDetail.tsx`
- `src-react/src/components/AuthProvidersFocusScreen.tsx`
- `src-react/src/services/auth.ts`
- `src-react/src/services/projects.ts`
- one new repository-access service module if the existing services become too
  crowded

### Desktop Shell

- `src-tauri/src/lib.rs`

### Runtime Git And Automation

- `crates/runtime-git/src/lib.rs`
- a new access-assessment module under `crates/runtime-git/src/` if the Git
  crate needs cleaner separation between probing and checkout logic
- `crates/runtime-bin/src/workers.rs`
- `crates/runtime-bin/src/builds.rs`

### Store And Contracts

- `crates/runtime-store/src/models.rs`
- one new migration under `crates/runtime-store/migrations/`
- `crates/runtime-contracts/src/lib.rs` only if the access-assessment payload is
  promoted into a shared contract

### Documentation

- `docs/architecture.md`
- `docs/pipeline-yaml-guide.md` only if repository auth guidance there must be
  aligned with the new project-scoped model

## Success Criteria

- marking a repository as public never forces a login step
- marking a repository as private asks for login only inside that project's
  create or edit flow when the provider supports private login
- connecting one provider account does not silently bind every matching
  repository project
- opening the app with existing repository projects does not launch credential
  UI
- starting polling, release planning, or build execution does not launch
  credential UI
- a stale private repository binding becomes a durable project re-auth state
  instead of a surprise prompt
- repository inspection shows provider, visibility, and auth state clearly

## Suggested Implementation Order

1. define the repository-access assessment contract and provider registry
2. add durable repository auth state and inspection payload fields
3. remove auto-binding behavior from provider login and project creation
4. wire the create-project wizard to URL-based access assessment
5. wire the edit-project screen to re-check and reconnect flows
6. harden runtime automation so auth recovery is always non-interactive
7. expand login connectors beyond GitHub while keeping provider detection
   provider-agnostic from the start

## Task List

### Phase 0 - Scope Lock

- [x] confirm that interactive auth is allowed only from create or edit project
      flows
- [x] confirm that provider inventory remains secondary to project-owned auth
      state
- [x] confirm that public repositories stay credential-free by default
- [x] confirm that HTTPS repository URLs remain the only source mode in the
      first slice

### Phase 1 - Repository Access Assessment

- [x] add a provider registry that detects GitHub, GitLab, Bitbucket, and
      unknown providers from repository URL alone
- [x] add one shell command that returns provider identity and operator-facing
  metadata for a repository URL
- [x] make repository visibility an explicit operator choice in create and edit
  flows instead of auto-classifying it from anonymous access
- [x] classify private-provider support from provider detection instead of
  probing visibility first
- [x] add focused tests for URL parsing and probe classification

### Phase 2 - Durable Repository Auth State

- [x] add repository auth assessment fields through a migration
- [x] extend runtime-store models with provider, visibility, and auth binding
      state
- [x] expose the new fields through repository inspection payloads
- [x] ensure repository updates can invalidate stale auth state after URL
      changes
- [x] add tests for migration, create, and update behavior

### Phase 3 - Project-Scoped Binding Commands

- [x] remove bind-all behavior from provider login commands
- [x] remove default GitHub credential auto-binding during project creation
- [x] add explicit connect, reconnect, and disconnect commands for one
      repository project
- [x] preserve reusable credential records while binding them only through the
      target repository
- [x] add tests that prove login no longer mutates unrelated repositories

### Phase 4 - Create And Edit UI Flows

- [x] replace the GitHub-specific access-step callout with a repository-access
      card in the create wizard
- [x] debounce provider detection while the operator edits the repository URL
- [x] show provider, explicit visibility, auth requirement, and project
  binding status inline
- [x] allow create without login for repositories marked public
- [x] require explicit login only for repositories marked private on supported
  providers
- [x] add the same repository-access section to the edit-project screen
- [x] support re-check, reconnect, and disconnect in edit flow
- [x] update provider inventory copy so it no longer promises default binding

### Phase 5 - Runtime Automation Hardening

- [x] ensure all automated Git auth paths remain non-interactive
- [x] classify stale or missing credentials as project auth failures rather than
      interactive recovery moments
- [x] teach polling and build preparation to surface durable re-auth state for
      private repositories
- [x] avoid repeated auth thrash when one repository is already marked
  `required_unbound` or `reauth_required`
- [x] add focused tests around runtime auth resolution and failure
      classification

### Phase 6 - Provider Expansion

- [x] keep GitHub as the first end-to-end project-scoped connector because that
      host-backed login path already exists
- [ ] add GitLab and Bitbucket login connectors behind the same project-scoped
      contract
- [x] keep unsupported providers recognizable even before their login connector
      is implemented
- [ ] confirm that public repository handling already works across providers
      before private login connectors are finished

### Phase 7 - Validation And Documentation

- [x] validate the desktop UI with `npm run build --prefix src-react`
- [x] validate the desktop shell and Rust auth changes with focused tests under
      the desktop shell and runtime Git crates
- [x] update architecture and operator documentation once the implementation
      contract stabilizes
- [x] run one end-to-end smoke path covering public repository creation,
      private repository binding, app restart, and non-interactive runtime
      execution

