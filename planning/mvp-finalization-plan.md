# MVP Finalization Mission Plan

## Purpose

This document defines the mission required to close the HGP MVP and ship
`v1.0.0-beta.0` as a coherent operator-ready desktop product.

It exists to answer one practical question:

- what must land, and what must be proven, before this beta can be called a
  real MVP instead of an advanced internal build

## Release Target

The release target for this mission is:

- one Windows-first packaged desktop beta distributed as `v1.0.0-beta.0`
- one bundled runtime that remains healthy while the shell exposes the full
  operator path needed for repository projects, local workspace projects,
  credentials, publication, diagnostics, and documentation
- one GitHub Pages documentation site that teaches the delivered product rather
  than future ideas

## Mission Outcome

When this mission is complete, an operator should be able to:

- install the packaged desktop application and confirm its running version
- launch the shell, see a healthy runtime, and understand whether automation is
  working or intentionally idle
- switch the whole automation system between active polling and healthy idle
  without stopping the runtime
- choose one shell language and one fallback language from settings, with the
  packaged app discovering official and community language packs from the
  installed localization directory
- create and operate both repository-backed and local-workspace Unity projects
- trigger, inspect, and recover release execution from the app
- create reusable secret credentials, including Itch credentials, and bind them
  where publication requires them
- read one public documentation site with guided setup steps, workflow
  tutorials, screenshots, and troubleshooting guidance

## Scope Lock

In scope:

- one runtime-wide polling or automation toggle exposed from the main screen
- the local-workspace project release flow from project creation through build
  and publish inspection
- one operator-usable settings surface instead of the current placeholder shell
- one file-based localization system rooted at `<install-root>/localizations`
      with official `en`, `es`, `pt-BR`, and `zh-CN` packs plus community drop-in
      language packs
  discovered automatically
- reusable secret credential inventory with first-class Itch credential entry
- one Windows-first release packaging and distribution contract for the desktop
  app and bundled runtime
- one public documentation site published through GitHub Pages
- one MVP release-candidate gate that proves the shipped beta works end-to-end

Out of scope for this mission:

- a broad redesign of the main feed beyond the additions required by this plan
- new publish backends beyond the already accepted `filesystem` and `itch`
- distributed workers, cloud orchestration, or SaaS-style multi-host behavior
- speculative Linux and macOS packaging parity before the Windows-first beta is
  stable
- a remote translation service, in-app translation editor, or online language
  pack marketplace for the beta window
- configurable archive naming templates for packaged artifacts such as `{{name}}.{{version}}.{{platform}}.zip`, including operator-authored token mapping and per-project naming rules
- a second documentation stack that duplicates repository docs for contributors

## Current Baseline

The current baseline already includes:

- the shell route for `settings`, but it currently renders only a title inside
  `apps/desktop/ui/src/App.tsx`
- the create-project wizard option for a local workspace project, but the
  source adapter is still marked unsupported in
  `apps/desktop/ui/src/components/CreateProjectWizard.tsx`
- reusable secret credential loading and persistence through
  `secret_settings` and `save_secret_credential` in
  `apps/desktop/src-tauri/src/lib.rs`
- support for `itch-api-key` in both the Tauri secret settings contract and the
  frontend service layer
- Itch publication runtime behavior and tests under `crates/runtime-publish`
  and `crates/runtime-bin`
- one install-root localization directory contract, official `en`, `pt-BR`,
      `es`, and `zh-CN` locale packs, packaged resource bundling, and settings-owned primary or
  fallback selection now exist, but translated surface adoption and installed
  package verification still remain
- semantic-release groundwork and a Windows release bundle workflow already
  tracked in `planning/semantic-release-plan.md`
- operator and architecture docs for developers, but no GitHub Pages tutorial
  site for end users yet

## Diagnosis

### 1. The Main Feed Has The Automation Posture Control, But It Still Needs Beta-Proof Validation

The shell can now express one deliberate global choice between active
automation and healthy idle, but the beta still needs packaged validation to
prove that posture cleanly across the feed, runtime, and tray-adjacent flows.

Consequences:

- the core operator control exists, but the smoke matrix still needs to prove
  idle-mode intake behavior outside the dev loop
- tray posture remains the missing polish layer for a disciplined resident
  desktop tool
- packaged validation must confirm the feed continues to distinguish runtime
  health from automation posture after install

### 2. Local Workspace Projects Exist, But The Beta Story Still Needs Explicit Validation

UI, shell, and runtime behavior now support local workspace projects as
first-class pipeline definitions, but the beta still lacks explicit end-to-end
proof and operator documentation for the local-only path.

Consequences:

- the app can now serve projects that are local only and not backed by Git,
  but the release story still needs packaged validation
- documentation still needs one honest operator walkthrough for local-project
  setup and release
- the remaining risk sits in end-to-end proof, not in the core source-mode
  contract

### 3. Packaging Automation Exists, But The Distribution Contract Is Not Yet A Product Surface

Release workflows already exist, but the operator-facing beta path is still
missing a fully defined contract.

Consequences:

- versioning, artifact naming, install expectations, and upgrade expectations
  are not yet closed as one release story
- the project can build release artifacts without yet proving the shipped beta
  is installable and understandable
- the beta would risk being technically generated but not operationally ready

### 4. Settings Workflow Now Covers Docs Routing And Packaged Validation

The shell now exposes a real settings screen with version metadata, runtime
health, storage entry points, reusable credential actions, and persisted
non-secret localization preferences. It now also links to the public beta docs
and has packaged-install proof from a real Windows NSIS install plus launch
smoke path.

Consequences:

- settings now anchors the app-level decisions already shipped for beta
- public docs now have one explicit operator entry path from settings
- packaged validation now proves the installed shell launches with bundled
  resources, sidecars, and install-root locale packs present
- the remaining documentation gap is publication hardening and screenshot
  capture, not missing entry points from the product

### 5. Localization Contract Exists, But Translated Surface Adoption Still Needs Closure

The beta now has one file-based localization contract, one operator-facing
primary or fallback selection flow, packaged official locale packs, and a
community drop-in path for additional languages. The shell chrome now consumes
that contract through a UI-side provider and live-refreshes after settings
updates, but broader translated surface adoption still needs closure.

Consequences:

- the shell now owns locale discovery and preference persistence for `en`,
  `pt-BR`, and drop-in community packs
- the main shell chrome now resolves UI strings through
  `primary -> fallback -> en` using the install-root locale packs
- the remaining localization gap is no longer the shell contract or the app
  chrome; it is focus-screen and component adoption across the rest of the
  UI
- settings now owns both the operator language choice and the live UI refresh
  path for updated localization preferences

### 6. Itch Credential Authoring Is Live, But Cross-Flow Proof Still Needs Coverage

The app can now author reusable Itch credentials from settings and reuse them
in publish flows, but the beta still needs explicit coverage proving the
cross-flow handoff end to end.

Consequences:

- Itch support is now visible to operators instead of hidden in plumbing
- the remaining risk is validation depth, not basic UI discoverability
- documentation should describe settings as the beta entry point for reusable
  Itch credentials

### 7. Public End-User Documentation Is Missing From The Product Contract

The repository already contains technical project docs, but MVP users need
guided operator documentation instead of engineering context alone.

Consequences:

- onboarding still depends on repository reading or direct guidance
- screenshots, workflows, and troubleshooting paths are not yet published as
  part of the product
- packaging a beta without public operator docs would weaken the release

### 8. Workflow-Critical Code Is Still Too Monolithic For Human-Scale Maintenance

The MVP has accumulated too much decision-making inside a few oversized files,
including `crates/runtime-store/src/lib.rs` at roughly 18k lines,
`apps/desktop/src-tauri/src/lib.rs` above 8k lines, and
`crates/runtime-bin/src/main.rs` above 6k lines.

Consequences:

- critical behavior still requires archaeology through giant files instead of
  following clear module ownership
- reviews, bug fixing, and regression analysis stay slower and riskier than
  they should be for a public beta
- the product may be functionally complete while still failing the human
  comprehension standard needed for routine maintenance

### 9. The MVP Still Needs One Explicit Exit Gate

The current work is close to the MVP, but the beta still needs one closing
acceptance gate that tests the full operator path.

Consequences:

- features may land without proving they cooperate under one packaged build
- the team can drift into continued feature work without a freeze line
- the beta label would reflect hope rather than validated behavior

## MVP Invariants

- `idle` means the runtime and workers remain alive and healthy, but automatic
  polling intake is suspended
- in-flight work must not be killed merely because the operator toggled idle
- shell freshness must remain event-driven; the toggle must not introduce UI
  timer polling as a substitute for runtime events
- the packaged beta must ship official locale files at
      `<install-root>/localizations/en.json`,
      `<install-root>/localizations/pt-BR.json`, and
      `<install-root>/localizations/zh-CN.json`
- locale discovery must remain file-based: a valid pack dropped into the
  install-root localization directory becomes selectable without code changes
- the operator may choose one fallback locale, but unresolved strings must
  still end at `en` as the final fallback
- local workspace projects must remain first-class pipeline definitions, not a
  hidden bypass path outside the normal release model
- the beta local-project flow should prefer explicit manual release dispatch
  over speculative file watching unless a stronger contract is deliberately
  chosen
- Itch credentials must be reusable secret records, not one-off publish-target
  inline blobs
- the settings surface should show only actionable operator decisions, not
  duplicate status-only panels that belong elsewhere
- the Windows distribution path is required for `v1.0.0-beta.0`; Linux and
  macOS may remain documented future tracks
- public docs must teach only the workflows that truly ship in the beta
- workflow-critical store, runtime, and shell behavior must be traceable
  through bounded modules with explicit ownership instead of continuing growth
  in single-file control centers

## Mission Exit Criteria

- [x] the operator can toggle the system between active automation and healthy
      idle from the main screen
- [x] repository projects still work while idle mode blocks automatic polling
      intake only
- [x] local workspace projects can be created, released, and inspected from the
      app
- [x] the settings route is replaced by a real focus screen with operator entry
      points
- [x] one persisted non-secret settings contract exists for the app-level
      decisions that must ship in beta
- [x] the settings surface exposes primary-language and fallback-language
      selection backed by install-root localization file discovery
- [x] the packaged beta ships official `en`, `es`, `pt-BR`, and `zh-CN` locale
      files, and one additional pack dropped into `<install-root>/localizations` becomes
      selectable without a rebuild
- [x] Itch credentials can be created from the app and reused by project
      publish flows
- [x] one packaged Windows beta can be built, installed, and verified locally
- [ ] one GitHub Pages documentation site is published with tutorials and
      screenshots for the shipped beta
- [ ] the largest workflow-critical monoliths are refactored into smaller,
      ownership-driven modules so store, shell, and runtime behavior remains
      understandable to human maintainers
- [ ] the MVP smoke matrix and targeted validation checks pass before the beta
      is declared ready

## Task List

### 1. Global Polling Control

Mission:
Add one simple main-feed control that switches the runtime between active
automation and healthy idle without collapsing runtime supervision.

Why this matters:
This is the missing posture control that lets the operator keep HGP alive while
deliberately stopping automatic release intake.

Track checklist:

- [ ] define one runtime-wide automation mode contract with at least `active`
      and `idle`
- [ ] persist that mode outside ephemeral process memory so restart behavior is
      deterministic
- [ ] keep runtime health and worker liveness separate from automation mode
- [ ] ensure idle mode blocks repository polling intake without interrupting
      running builds or publishes
- [ ] decide and document whether manual release or instant-check actions stay
      available while idle; the preferred beta posture is yes
- [ ] add the main-feed toggle next to the worker status control with clear
      active and idle copy
- [ ] surface the current automation posture in worker inspection surfaces so
      the shell explains why no new polling work is arriving
- [ ] add runtime validation proving idle blocks polling rather than all worker
      behavior
- [ ] add shell coverage for toggle rendering, transitions, and refresh or
      restart persistence

Acceptance snapshot:

- the operator can switch between active and idle from the main screen
- worker and runtime health remain visible while idle is active
- automatic polling stops while manual operator actions remain available

### 2. Local Workspace Release Flow

Mission:
Close the full local-project path so HGP can manage Unity projects that live on
the local filesystem and are not backed by a remote repository.

Why this matters:
Without this slice, the product is still missing one of the most important MVP
operator modes already implied by the product direction.

Track checklist:

- [x] define the beta contract for local-project release intake; prefer manual
      dispatch only unless a stronger trigger model is explicitly approved
- [x] implement the local workspace source adapter in the create-project wizard
- [x] implement edit support for local workspace source settings in project
      detail
- [x] persist the local workspace path and any required source-mode metadata as
      durable project configuration
- [ ] define one explicit local release identity contract such as operator-
      supplied version label, release label, or snapshot label
- [x] teach release planning and workspace preparation to use the local source
      path instead of repository checkout
- [x] reuse the existing build-target and publish-destination model without
      creating a second-class local-only workflow
- [ ] make local-project validation explicit when paths, Unity settings, or
      outputs are invalid across the remaining packaged beta checks
- [ ] add end-to-end validation for create local project -> dispatch local
      release -> build -> inspect outputs -> publish to at least one supported
      destination

Acceptance snapshot:

- the operator can register one local Unity workspace and release it from the
  app
- local projects use the same inspection and publish surfaces as repository
  projects where practical
- local-project behavior is documented honestly as a shipped beta capability

### 3. Settings Surface

Mission:
Replace the current settings placeholder with one real operator-focused screen
that owns app-level configuration and entry points.

Why this matters:
The beta needs one obvious place for app-wide decisions, version visibility,
paths, and documentation access.

Track checklist:

- [x] replace the `settings` placeholder in `App.tsx` with a real focus screen
      built on `ScreenScaffold`
- [x] define the persistent non-secret settings contract needed for beta
- [x] expose only actionable settings that matter before or during operation
- [x] include app version, bundled runtime version, and release-channel or beta
      metadata where the operator can inspect them easily
- [x] expose runtime directory and path entry points that help the operator act
      on logs, artifacts, workspaces, or overrides
- [x] decide whether credentials live directly in settings or behind one clear
      settings-owned entry point, then keep that model consistent
- [x] wire documentation entry points from settings to the public docs site and
      any packaged local operator references that actually ship
- [x] add save and validation behavior where settings-owned credential actions
      are editable
- [x] close the slice with focused shell validation and the relevant native
      Tauri checks

Acceptance snapshot:

- navigating to settings opens a real working screen instead of a title stub
- the operator can inspect shell metadata, runtime paths, and reusable
  credential inventory from one stable screen
- settings acts as the stable home for configuration and entry points that are
  already shipped, including localization controls and a public docs route

### 4. Localization

Mission:
Add one file-based localization system that ships with `en`, `es`, `pt-BR`, and `zh-CN`, lets
the operator choose both primary and fallback languages from settings, and lets
the community add new languages by dropping files into the installed app.

Why this matters:
The beta needs one clear language contract that works offline, survives
packaging, and stays open to community translations without a rebuild.

Directory contract:

```text
<install-root>/localizations/en.json
<install-root>/localizations/pt-BR.json
<install-root>/localizations/zh-CN.json
<install-root>/localizations/es.json
```

Track checklist:

- [x] define the install-root localization directory contract as
      `<install-root>/localizations`
- [x] choose one durable community-authorable locale file format for beta;
      prefer JSON unless a stronger packaging reason appears
- [x] ship official `en`, `es`, `pt-BR`, and `zh-CN` locale files inside that
      directory as part of the packaged app
- [x] discover selectable locales by scanning the localization directory at
      runtime and using the file stem as the locale code
- [x] ignore invalid locale files without crashing the shell and surface clear
      diagnostics when a pack cannot be loaded
- [x] add one settings select for the primary language and one settings select
      for the fallback language
- [x] make the language selects list only locale files that are physically
      present in the localization directory at runtime
- [x] adopt one UI-side localization provider so the main shell chrome,
      auth providers focus screen, main-feed process items, projects focus
      screen, project quick view, process detail focus screen, worker quick
      view, project workers focus screen, and settings language panel consume
      the locale contract without a rebuild
- [ ] resolve missing strings through the operator-selected fallback locale
      first and `en` last
- [x] verify that dropping a valid file such as
      `<install-root>/localizations/es.json` makes `es` selectable without a
      rebuild
- [ ] add shell coverage for locale discovery, settings persistence, and
      fallback resolution behavior

Acceptance snapshot:

- the packaged beta ships with selectable `en` and `pt-BR`
- the operator can choose both primary and fallback language from settings
- adding a valid pack such as `es.json` to the install-root localization
  directory makes `es` available without code changes
- unresolved strings fall back to the selected fallback locale and finally to
  `en`

### 5. Credential Inventory And Itch Reuse

Mission:
Turn reusable secret credentials into one first-class operator flow and ensure
Itch credential authoring is available directly from the app.

Why this matters:
The runtime already supports Itch publication, so the product now needs the UI
surface that makes this support discoverable and reusable.

Track checklist:

- [x] define the beta home for reusable secret credentials, preferably under
      the new settings surface or one settings-owned child flow
- [x] expose the inventory returned by `secret_settings` as a readable,
      redacted operator list
- [x] allow creating reusable credentials through the shared credential
      composer flow
- [x] allow updating reusable credentials through the shared credential
      composer flow
- [x] ensure `itch-api-key` is explicitly available with clear labeling and
      validation guidance
- [x] make Itch credentials reusable from both create-project and edit-project
      publish-destination flows
- [ ] surface compatibility errors clearly when an Itch destination lacks a
      valid bound credential
- [x] avoid storing or displaying raw secret material after save; rely on the
      existing redacted summary model
- [ ] add UI coverage proving a credential created from the credential screen
      can be reused by a publish destination without re-entry

Acceptance snapshot:

- the operator can create one reusable Itch credential from the app
- publish destinations can select and reuse that credential cleanly
- the beta UI makes Itch support visible without leaking secret material

### 6. Packaging And Distribution

Mission:
Define and prove one real beta distribution path instead of leaving release
automation as an internal-only capability.

Why this matters:
The MVP is only real when one operator can install and verify the packaged app
without reading the source tree.

Track checklist:

- [ ] decide the Windows-first beta artifact set, such as installer, portable
      bundle, or both, and keep the first slice minimal
- [ ] normalize shell and runtime version reporting around the shared workspace
      version source
- [x] verify the packaged shell includes the expected bundled runtime and Itch
      sidecar behavior
- [ ] align release artifact names, checksums, and release notes around one
      version string
- [ ] document the install, upgrade, uninstall, rollback, and prerelease flow
      for the beta
- [ ] add or finish local dry-run commands for packaging validation
- [x] run at least one packaged-install smoke path on Windows from a clean app
      state
- [ ] decide how GitHub release publication and GitHub Pages publication fit
      together in the beta release procedure

Acceptance snapshot:

- one packaged Windows beta can be installed and launched successfully
- the running shell and bundled runtime report the same version
- the release contract is clear enough to repeat for the next beta cut

### 7. Operator Documentation Site

Mission:
Publish one GitHub Pages site that teaches end users how to install, configure,
and operate the shipped beta.

Why this matters:
An MVP without operator documentation still behaves like an internal tool.

Track checklist:

- [x] choose one lightweight static documentation stack and repository layout
      for GitHub Pages publication
- [x] wire the site through GitHub Actions with deterministic deployment
      behavior
- [x] write a first-run guide covering installation, first launch, and runtime
      health expectations
- [x] write project-creation tutorials for repository projects and local
      workspace projects
- [x] write one guide for the global active or idle toggle and the expected
      automation posture semantics
- [x] write one guide for reusable credentials and one guide for Itch
      publication setup
- [x] write one troubleshooting section covering Unity detection, repository
      auth, Itch prerequisites, runtime directories, and common recovery paths
- [ ] capture product screenshots from the real beta UI and redact all secrets
- [x] link the public docs from `README.md` and the in-app settings surface

Acceptance snapshot:

- the public docs explain the actual beta workflows with screenshots
- first-time operators can reach installation and release execution guidance
  without repository archaeology
- docs terminology matches the shipped UI and runtime behavior

### 8. Human-Scale Architecture Refactor

Mission:
Refactor the current workflow-critical monoliths into smaller bounded modules
so the MVP is not only feature-complete, but also understandable and
maintainable by humans.

Why this matters:
Shipping the beta with critical logic still concentrated in files such as
`crates/runtime-store/src/lib.rs`, `apps/desktop/src-tauri/src/lib.rs`, and
`crates/runtime-bin/src/main.rs` would preserve delivery speed in the short
term while making the product harder to review, debug, and evolve safely.

Track checklist:

- [ ] identify the current monolithic hotspots and define target ownership
      boundaries before more feature work expands them further
- [ ] split `runtime-store` workflow-critical logic such as release dispatch,
      planning, and repository coordination into focused modules with narrow
      interfaces
- [ ] split `desktop-shell` command handlers, normalization helpers, auth or
      secret flows, and runtime supervision glue out of the main Tauri
      `src/lib.rs`
- [ ] split `runtime-bin` CLI entry, event emission, worker orchestration, and
      release or build coordination into clearer units
- [ ] preserve the accepted external contracts while moving implementation
      detail behind smaller files and better named modules
- [ ] add or update nearby technical documentation when new boundaries or
      invariants are not obvious from local code
- [ ] validate each refactor slice with focused tests so the work improves
      comprehension without introducing behavioral drift

Concrete work packages:

#### 8.1 `crates/runtime-store/src/lib.rs`

Primary boundary:
Keep `LocalCoordinator` as the store-facing facade, but move workflow logic out
of the giant root file into modules that own one responsibility each.

Concrete subtasks:

- [ ] extract release dispatch and rebuild flows into one dedicated module that
      owns on-demand dispatch, rebuild-by-id, and queue insertion rules
- [ ] extract release source normalization into one module that owns
      `source_identity`, `source_metadata_json`, version detection, and related
      source-mode helpers
- [ ] extract build and release planning into one module that owns execution
      plan materialization, build-target selection, and plan-level invariants
- [ ] extract repository and project coordination queries into one module so
      lookup-heavy store behavior stops living next to release mutation logic
- [ ] keep `src/lib.rs` focused on exports, coordinator wiring, and thin entry
      points instead of mixed query, mutation, planning, and helper code

Definition of done:

- critical release planning behavior is traceable through named modules instead
  of one 18k-line file
- tests for dispatch, rebuild, and planning sit near the owning slice whenever
  practical

#### 8.2 `apps/desktop/src-tauri/src/lib.rs`

Primary boundary:
Keep Tauri setup and command registration in the root shell file, but move
domain behavior, normalization, and workflow orchestration behind dedicated
shell modules.

Concrete subtasks:

- [ ] extract runtime lifecycle and supervision commands into one shell module
      that owns start, stop, restart, health, and automation posture actions
- [ ] extract project and release process commands into one module that owns
      repository detail, local-workspace dispatch, rerun, and process-history
      interactions
- [ ] extract credentials and auth flows into one module that owns provider
      status, GitHub auth actions, secret persistence, and binding updates
- [ ] extract normalization and validation helpers out of `src/lib.rs` so
      command inputs are validated close to their owning domain
- [ ] leave `src/lib.rs` as a thin composition root that wires commands,
      startup behavior, and shared shell state

Definition of done:

- shell-facing behavior is organized by operator domain rather than by one
  giant file of commands and helpers
- future Tauri commands can be added to bounded modules without enlarging the
  composition root again

#### 8.3 `crates/runtime-bin/src/main.rs`

Primary boundary:
Keep the runtime binary entry focused on bootstrap and command routing, while
worker orchestration, event formatting, and execution coordination move behind
clear runtime modules.

Concrete subtasks:

- [ ] extract CLI parsing and command routing helpers so entrypoint code stops
      sharing space with long-running runtime workflow logic
- [ ] extract release and worker orchestration into one module that owns queue
      processing, worker decisions, and lifecycle transitions
- [ ] extract runtime event context and emission helpers into one module that
      owns summaries, payload shaping, and trigger-source labeling
- [ ] extract build and publish execution coordination into focused modules so
      orchestration no longer depends on one mixed main file
- [ ] leave `main.rs` responsible for startup wiring, top-level routing, and
      process exit behavior only

Definition of done:

- runtime flow can be followed from `main.rs` into named modules without
  archaeology
- event emission, worker behavior, and execution coordination each have clear
  ownership boundaries

#### 8.4 Secondary UI Monolith Watchlist

Primary boundary:
Treat the large route-level UI files as secondary refactor candidates during
the same MVP window if they continue absorbing unrelated responsibilities.

Concrete subtasks:

- [ ] split `apps/desktop/ui/src/components/RepositoryProjectDetail.tsx` by
      operator surface, separating project summary, build target editing,
      publish target editing, and release-history interaction zones where the
      current file keeps growing
- [ ] split `apps/desktop/ui/src/components/CreateProjectWizard.tsx` by flow
      step or source-mode concern so repository and local-workspace creation do
      not keep accreting in one component
- [ ] split `apps/desktop/ui/src/components/PublishDestinationsEditor.tsx`
      further if credential binding, destination editing, and validation copy
      continue to expand in the same file

Definition of done:

- route-level UI files stop accumulating unrelated concerns that make operator
  flows hard to reason about during implementation and review

Refactor delivery rules:

- [ ] do not grow the identified monoliths further when a new module can own
      the next slice cleanly
- [ ] preserve accepted public contracts while moving implementation behind new
      boundaries
- [ ] treat reduced cognitive load and explicit ownership as the goal; lower
      line count is a signal, not the only success metric

Acceptance snapshot:

- critical operator flows can be traced through smaller modules with obvious
  ownership boundaries
- reviews and bug fixes no longer depend on spelunking one giant file per
  subsystem
- the beta architecture is legible enough for routine human maintenance

### 9. Release-Candidate Gate

Mission:
Close the MVP with one explicit pass or fail gate that proves the beta works as
one product.

Why this matters:
Without this gate, the project can drift into "almost MVP" indefinitely.

Track checklist:

- [x] define one beta smoke matrix that covers the critical operator paths
- [x] include at least one repository-to-filesystem flow in that matrix
- [x] include at least one repository-to-Itch flow in that matrix
- [x] include at least one local-workspace release flow in that matrix
- [x] include active-to-idle and idle-to-active transition validation in that
      matrix
- [ ] verify that automated polling and build paths do not launch interactive
      auth prompts
- [ ] sweep empty states, blocked states, and recovery copy across the touched
      screens
- [ ] confirm packaged-build install behavior, restart behavior, and version
      inspection before calling the beta ready
- [ ] freeze MVP scope after the gate passes unless a regression forces a
      targeted fix

### Beta Smoke Matrix (Gate `v1.0.0-beta.0`)

Execution rules:

- run all scenarios on one packaged Windows install build
- capture evidence per scenario as: shell screenshot + relevant runtime log
  excerpt + explicit pass/fail note
- treat every `Required` scenario as release-blocking

| ID    | Scenario                                                               | Required | Preconditions                                                                                       | Steps                                                                                                                                    | Pass Criteria                                                                                                                    |
| ----- | ---------------------------------------------------------------------- | -------- | --------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| SM-01 | Packaged install boot and version posture                              | Yes      | Fresh install path available; package generated                                                     | Install the package; launch shell; open settings; verify version and runtime status surfaces                                             | Shell launches without bootstrap failure; version is visible and coherent; runtime reaches healthy or explicit recoverable state |
| SM-02 | Automation posture transitions (`active -> idle -> active`)            | Yes      | At least one configured project with polling enabled                                                | From main screen toggle to idle; wait one polling interval; verify no new automatic intake; toggle back to active; verify intake resumes | Idle suspends automatic polling only; runtime remains healthy; returning to active restores polling intake                       |
| SM-03 | Repository-backed release to filesystem destination                    | Yes      | Repository project configured with at least one build target and one filesystem publish destination | Trigger release from shell; follow process detail to completion                                                                          | Build and publish complete; artifacts land in configured folder; process detail timeline is coherent                             |
| SM-04 | Repository-backed release to Itch destination with reusable credential | Yes      | Reusable Itch credential exists in settings; repository project bound to Itch destination           | Trigger release; verify credential binding use; observe publish completion                                                               | Publish succeeds without inline credential creation; credential inventory reuse works across flows                               |
| SM-05 | Local workspace release flow                                           | Yes      | Local workspace project configured with valid Unity path and build target                           | Create or open local project; trigger release; inspect results                                                                           | Local project path completes build and publish inspection flow end-to-end                                                        |
| SM-06 | No interactive auth prompts during automated polling/build paths       | Yes      | Polling active; auth bindings valid before run                                                      | Let polling produce at least one automated run; inspect shell/runtime behavior                                                           | No modal or interactive auth prompt interrupts automation path; failures are surfaced as actionable states                       |
| SM-07 | Empty, blocked, and recovery states copy sweep                         | Yes      | Test data that triggers empty and blocked conditions                                                | Visit main, workers, projects, project detail, process detail, settings; trigger representative error/recovery flows                     | Copy remains actionable and operator-directed; no dead-end status panels for critical paths                                      |
| SM-08 | Restart and relaunch continuity on packaged build                      | Yes      | Completed one successful run in current install                                                     | Close app and relaunch; re-check runtime status, settings, and project surfaces                                                          | App relaunches cleanly; persisted settings and project state remain coherent                                                     |

Focused validation commands executed during gate:

- `cargo test -p runtime-bin`
- `RUST_TEST_THREADS=1 cargo test -p runtime-store`
- `npm --prefix apps/desktop/ui run test`
- `npm --prefix apps/desktop/ui run test:e2e`
- `npm run smoke:runtime`
- `cargo build --package desktop-shell`

Latest gate evidence snapshot (`2026-05-23 15:40:04 -03:00`):

| Evidence Item                                     | Result | Notes                                                                                            |
| ------------------------------------------------- | ------ | ------------------------------------------------------------------------------------------------ |
| `cargo test -p runtime-bin`                       | PASS   | Unit + e2e runtime-bin coverage green (`68 + 4 + 3` tests)                                       |
| `RUST_TEST_THREADS=1 cargo test -p runtime-store` | PASS   | Runtime-store suite green (`76` tests); serialized mode avoids transient Windows lock contention |
| `npm --prefix apps/desktop/ui run test`           | PASS   | UI unit/integration suite green (`31` files, `172` tests)                                        |
| `npm --prefix apps/desktop/ui run test:e2e`       | PASS   | Playwright shell flow suite green (`4` tests) after locale-resilient selector updates            |
| `npm run smoke:runtime`                           | PASS   | Runtime smoke green (interrupted cleanup + publish destination e2e paths)                        |
| `cargo build --package desktop-shell`             | PASS   | Desktop shell build green with dependent desktop UI production build                             |

Scenario evidence status map:

| ID    | Status         | Evidence Type                                                                        |
| ----- | -------------- | ------------------------------------------------------------------------------------ |
| SM-01 | Pending manual | Packaged install boot + settings version posture still requires packaged-run capture |
| SM-02 | Pending manual | Active/idle polling transition still requires packaged operator validation           |
| SM-03 | Covered        | Runtime smoke + runtime-bin e2e publish flow coverage available                      |
| SM-04 | Covered        | Runtime smoke + publish destination e2e proves Itch credential publish binding       |
| SM-05 | Pending manual | Local workspace release flow requires packaged shell execution capture               |
| SM-06 | Pending manual | Needs explicit automated polling run proof without interactive auth prompts          |
| SM-07 | Pending manual | Copy/actionability sweep remains a manual operator validation activity               |
| SM-08 | Pending manual | Restart/relaunch continuity still requires packaged install evidence                 |

Gate decision rule:

- MVP gate passes only when every `Required` smoke scenario is green and the
  focused validation commands above finish successfully
- any failure keeps `v1.0.0-beta.0` blocked until a targeted fix is merged and
  the failed scenario is re-run with recorded evidence

Acceptance snapshot:

- the team can point to one concrete beta pass list instead of a general sense
  of completeness
- `v1.0.0-beta.0` is declared ready only after the smoke matrix and focused
  validation checks succeed

### Recorded MVP Hardening Recommendations

These recommendations are intentionally recorded as high-value MVP hardening
work. They are not automatically mandatory exit criteria, but they should be
pulled into the beta window whenever schedule allows or when validation shows
they are required for a credible public beta.

1. Durable credential and re-auth diagnostics
   - surface missing, stale, or incompatible credentials as explicit operator
     action states in the shell instead of generic runtime failure copy
   - prefer durable repository states such as `reauth_required` over repeated
     transient polling failures
2. Restart recovery and runtime self-diagnostics
   - harden restart behavior under repeated failure conditions before the beta
     is declared stable
   - expose deeper runtime self-diagnostics that help the operator recover
     locally without repository spelunking
3. Packaged-install smoke validation
   - treat one real Windows install-and-launch smoke path as release-critical
     even when build workflows already produce artifacts successfully
   - verify bundled sidecars, first launch, version reporting, and runtime
     bootstrap from the packaged build instead of the development loop
4. Event-driven freshness on critical shell surfaces
   - keep runtime, worker, and process visibility driven by emitted events on
     the critical operator paths
   - avoid falling back to UI polling for status coherence in the beta shell
5. Missing-secret and prerequisite action states
   - surface blocked operator actions clearly when Unity detection, Butler,
     repository auth, or publish credentials are missing
   - phrase these states as actionable next steps rather than status-only
     warnings
6. Tray notification posture finishing pass - after the packaged validation gap narrows, finish the tray-side
   notification posture controls that make the app behave like a disciplined
   resident desktop tool
   - keep this below restart recovery, auth diagnostics, and packaged-install
     validation in MVP priority

## Suggested Execution Order

1. land file-based localization and fallback selection on the delivered
   settings surface
2. close the remaining validation gaps for automation posture, local workspace
   releases, and settings-authored credential reuse
3. finalize Windows-first packaging and beta distribution behavior
4. publish the operator documentation site with screenshots of the shipped beta
5. refactor the workflow-critical monoliths into smaller human-scale modules
   before calling the product structurally ready for beta
6. run the release-candidate gate and freeze scope for `v1.0.0-beta.0`
