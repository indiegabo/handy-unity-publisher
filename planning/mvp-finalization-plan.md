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
      with official `en` and `pt-BR` packs plus community drop-in language packs
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
- no install-root localization directory contract, locale discovery flow, or
      operator language selection yet; the shell still behaves like one hardcoded
      UI language
- semantic-release groundwork and a Windows release bundle workflow already
  tracked in `planning/semantic-release-plan.md`
- operator and architecture docs for developers, but no GitHub Pages tutorial
  site for end users yet

## Diagnosis

### 1. The Main Feed Still Lacks One Runtime-Wide Automation Posture Control

The shell can show worker state and open worker inspection surfaces, but it
cannot yet express one deliberate global choice between active automation and
healthy idle.

Consequences:

- the operator cannot pause polling without stopping runtime supervision
- the product still lacks one clear "leave the system alive, but do not intake
  new work" control
- the main feed cannot yet communicate the difference between runtime health and
  automation posture

### 2. Local Workspace Projects Still Stop At The Source Adapter Boundary

The UI already exposes the local-project idea, but the actual source-mode flow
does not continue into a usable release pipeline.

Consequences:

- the app cannot yet serve projects that are local only and not backed by Git
- the MVP still depends on repository intake even though the product direction
  already calls for local workspace support
- documentation cannot honestly present local projects as a supported operator
  path

### 3. Packaging Automation Exists, But The Distribution Contract Is Not Yet A Product Surface

Release workflows already exist, but the operator-facing beta path is still
missing a fully defined contract.

Consequences:

- versioning, artifact naming, install expectations, and upgrade expectations
  are not yet closed as one release story
- the project can build release artifacts without yet proving the shipped beta
  is installable and understandable
- the beta would risk being technically generated but not operationally ready

### 4. Settings Navigation Exists Without A Delivered Settings Workflow

The shell can navigate to `settings`, but the settings surface is not yet a
real operator screen.

Consequences:

- there is no central home for persistent app configuration, runtime paths,
  release metadata, or documentation entry points
- settings-related behavior remains scattered across unrelated screens
- the product lacks a stable place to expose app-level decisions before beta

### 5. Localization Is Missing From The Beta Product Contract

The beta still lacks one file-based localization contract, one operator-facing
language selection flow, and one community extension path for new languages.

Consequences:

- the packaged app cannot yet ship a clear language story for `en` and `pt-BR`
- the community cannot add a new language by dropping one file into the
      installed app directory
- settings cannot yet own the operator choice for primary language, fallback
      language, and missing-string behavior

### 6. Itch Credential Capability Exists Mostly As Plumbing, Not As A First-Class Operator Flow

The backend already understands `itch-api-key` and publish execution already
depends on reusable credentials, but the product still needs one explicit place
to author and reuse those credentials.

Consequences:

- Itch support is harder to discover than it should be for a beta feature
- credential reuse remains too implicit for a first-time operator
- the beta risks documenting behavior that the UI still exposes only indirectly

### 7. Public End-User Documentation Is Missing From The Product Contract

The repository already contains technical project docs, but MVP users need
guided operator documentation instead of engineering context alone.

Consequences:

- onboarding still depends on repository reading or direct guidance
- screenshots, workflows, and troubleshooting paths are not yet published as
  part of the product
- packaging a beta without public operator docs would weaken the release

### 8. The MVP Still Needs One Explicit Exit Gate

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
      `<install-root>/localizations/en.json` and
      `<install-root>/localizations/pt-BR.json`
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

## Mission Exit Criteria

- [ ] the operator can toggle the system between active automation and healthy
      idle from the main screen
- [ ] repository projects still work while idle mode blocks automatic polling
      intake only
- [ ] local workspace projects can be created, released, and inspected from the
      app
- [ ] the settings route is replaced by a real focus screen with persisted
      non-secret settings and operator entry points
- [ ] the settings surface exposes primary-language and fallback-language
      selection backed by install-root localization file discovery
- [ ] the packaged beta ships official `en` and `pt-BR` locale files, and one
      additional pack dropped into `<install-root>/localizations` becomes
      selectable without a rebuild
- [ ] Itch credentials can be created from the app and reused by project
      publish flows
- [ ] one packaged Windows beta can be built, installed, and verified locally
- [ ] one GitHub Pages documentation site is published with tutorials and
      screenshots for the shipped beta
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

- [ ] define the beta contract for local-project release intake; prefer manual
      dispatch only unless a stronger trigger model is explicitly approved
- [ ] implement the local workspace source adapter in the create-project wizard
- [ ] implement edit support for local workspace source settings in project
      detail
- [ ] persist the local workspace path and any required source-mode metadata as
      durable project configuration
- [ ] define one explicit local release identity contract such as operator-
      supplied version label, release label, or snapshot label
- [ ] teach release planning and workspace preparation to use the local source
      path instead of repository checkout
- [ ] reuse the existing build-target and publish-destination model without
      creating a second-class local-only workflow
- [ ] make local-project validation explicit when paths, Unity settings, or
      outputs are invalid
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

- [ ] replace the `settings` placeholder in `App.tsx` with a real focus screen
      built on `ScreenScaffold`
- [ ] define the persistent non-secret settings contract needed for beta
- [ ] expose only actionable settings that matter before or during operation
- [ ] include app version, bundled runtime version, and release-channel or beta
      metadata where the operator can inspect them easily
- [ ] expose runtime directory and path entry points that help the operator act
      on logs, artifacts, workspaces, or overrides
- [ ] decide whether credentials live directly in settings or behind one clear
      settings-owned entry point, then keep that model consistent
- [ ] wire documentation entry points from settings to the public docs site and
      key local runtime locations
- [ ] add save, validation, and revert behavior where settings are editable
- [ ] close the slice with focused shell validation and the relevant native
      Tauri checks

Acceptance snapshot:

- navigating to settings opens a real working screen instead of a title stub
- the operator can inspect and change the app-level settings required for the
  beta
- settings acts as the stable home for documentation and configuration entry
  points

### 4. Localization

Mission:
Add one file-based localization system that ships with `en` and `pt-BR`, lets
the operator choose both primary and fallback languages from settings, and lets
the community add new languages by dropping files into the installed app.

Why this matters:
The beta needs one clear language contract that works offline, survives
packaging, and stays open to community translations without a rebuild.

Directory contract:

```text
<install-root>/localizations/en.json
<install-root>/localizations/pt-BR.json
<install-root>/localizations/es.json
```

Track checklist:

- [ ] define the install-root localization directory contract as
      `<install-root>/localizations`
- [ ] choose one durable community-authorable locale file format for beta;
      prefer JSON unless a stronger packaging reason appears
- [ ] ship official `en` and `pt-BR` locale files inside that directory as part
      of the packaged app
- [ ] discover selectable locales by scanning the localization directory at
      runtime and using the file stem as the locale code
- [ ] ignore invalid locale files without crashing the shell and surface clear
      diagnostics when a pack cannot be loaded
- [ ] add one settings select for the primary language and one settings select
      for the fallback language
- [ ] make the language selects list only locale files that are physically
      present in the localization directory at runtime
- [ ] resolve missing strings through the operator-selected fallback locale
      first and `en` last
- [ ] verify that dropping a valid file such as
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

- [ ] define the beta home for reusable secret credentials, preferably under
      the new settings surface or one settings-owned child flow
- [ ] expose the inventory returned by `secret_settings` as a readable,
      redacted operator list
- [ ] allow creating and updating reusable credentials through the shared
      credential composer flow
- [ ] ensure `itch-api-key` is explicitly available with clear labeling and
      validation guidance
- [ ] make Itch credentials reusable from both create-project and edit-project
      publish-destination flows
- [ ] surface compatibility errors clearly when an Itch destination lacks a
      valid bound credential
- [ ] avoid storing or displaying raw secret material after save; rely on the
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
- [ ] verify the packaged shell includes the expected bundled runtime and Itch
      sidecar behavior
- [ ] align release artifact names, checksums, and release notes around one
      version string
- [ ] document the install, upgrade, uninstall, rollback, and prerelease flow
      for the beta
- [ ] add or finish local dry-run commands for packaging validation
- [ ] run at least one packaged-install smoke path on Windows from a clean app
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

- [ ] choose one lightweight static documentation stack and repository layout
      for GitHub Pages publication
- [ ] publish the site through GitHub Actions with deterministic versioned
      deployment behavior
- [ ] write a first-run guide covering installation, first launch, and runtime
      health expectations
- [ ] write project-creation tutorials for repository projects and local
      workspace projects
- [ ] write one guide for the global active or idle toggle and the expected
      automation posture semantics
- [ ] write one guide for reusable credentials and one guide for Itch
      publication setup
- [ ] write one troubleshooting section covering Unity detection, repository
      auth, Itch prerequisites, runtime directories, and common recovery paths
- [ ] capture product screenshots from the real beta UI and redact all secrets
- [ ] link the public docs from `README.md` and the in-app settings surface

Acceptance snapshot:

- the public docs explain the actual beta workflows with screenshots
- first-time operators can reach installation and release execution guidance
  without repository archaeology
- docs terminology matches the shipped UI and runtime behavior

### 8. Release-Candidate Gate

Mission:
Close the MVP with one explicit pass or fail gate that proves the beta works as
one product.

Why this matters:
Without this gate, the project can drift into "almost MVP" indefinitely.

Track checklist:

- [ ] define one beta smoke matrix that covers the critical operator paths
- [ ] include at least one repository-to-filesystem flow in that matrix
- [ ] include at least one repository-to-Itch flow in that matrix
- [ ] include at least one local-workspace release flow in that matrix
- [ ] include active-to-idle and idle-to-active transition validation in that
      matrix
- [ ] verify that automated polling and build paths do not launch interactive
      auth prompts
- [ ] sweep empty states, blocked states, and recovery copy across the touched
      screens
- [ ] confirm packaged-build install behavior, restart behavior, and version
      inspection before calling the beta ready
- [ ] freeze MVP scope after the gate passes unless a regression forces a
      targeted fix

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
6. Tray notification posture finishing pass
   - after the global automation toggle lands, finish the tray-side
     notification posture controls that make the app behave like a disciplined
     resident desktop tool
   - keep this below restart recovery, auth diagnostics, and packaged-install
     validation in MVP priority

## Suggested Execution Order

1. land the global polling control so the main feed gains the missing runtime
   posture contract
2. finish the local workspace release flow so both MVP project modes are real
3. replace the settings placeholder and anchor app-level configuration there
4. land file-based localization and fallback selection on that settings surface
5. close Itch credential authoring and reuse from the settings-owned flow
6. finalize Windows-first packaging and beta distribution behavior
7. publish the operator documentation site with screenshots of the shipped beta
8. run the release-candidate gate and freeze scope for `v1.0.0-beta.0`
