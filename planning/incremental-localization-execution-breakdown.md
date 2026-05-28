# Incremental Localization Execution Breakdown

## Purpose

This document breaks the desktop-shell localization redesign into narrow work
packages that can be implemented and validated incrementally.

The target architecture replaces monolithic locale JSON files with per-locale
source directories, keeps numbered increments immutable once a release is
frozen, treats `en/working.json` as the only mutable day-to-day authoring
surface between releases, compiles final runtime bundles before the main UI
becomes available, and keeps release-time translation work scoped to freezing
that English working file into the next increment for each official locale.

## Scope Lock

- official locale sources live under
  `src-tauri/localizations/<locale>/`
- every official locale directory must contain `origin.json`
- numbered official locale deltas use ordered immutable
  `increment-<n>.json` files
- `src-tauri/localizations/en/working.json` is the only mutable
  localization source edited during normal development
- non-English official locales do not require a mutable working file between
  releases
- release-time localization work freezes `en/working.json` into the next
  numbered increment and mirrors that increment into every other official
  locale directory
- the runtime consumes compiled final bundles, not raw increment files
- the desktop shell must compile or validate locale bundles before the main UI
  is available
- the primary locale selector may include community locales discovered in the
  persisted source tree
- the fallback locale selector must include official locales only
- English remains the canonical key catalog and the terminal fallback bundle

## Delivery Rules

- keep bundle compilation, cache invalidation, and official-locale detection in
  the desktop shell rather than the frontend
- treat numbered increments as migration-like inputs; never widen the runtime
  design to support editing historical increment files in place
- treat `en/working.json` as an explicit mutable pre-release layer, not as a
  retroactive edit to any numbered increment
- separate editable persisted locale sources from derived compiled bundle cache
- use per-locale bundle caches so one new increment does not invalidate every
  locale
- write bundle cache metadata and compiled bundles atomically to avoid partial
  startup state after crashes or interrupted writes
- preserve community locales stored under the persisted localization source
  tree even when official locale sources are resynced
- prefer focused Rust and UI validation at the end of each work package rather
  than one broad end-of-project pass

## Implementation Fronts

1. Source layout and release authoring contract
2. Shell bootstrap, incremental bundle compiler, and cache
3. Frontend consumption, selector rules, and release automation

## Execution Order

1. scope guard and source-layout contract
2. official locale discovery and persisted source sync
3. incremental bundle compiler and per-locale cache
4. shell bootstrap gate and localization settings contract
5. frontend compiled-bundle consumption and selector rules
6. release-time increment mirroring workflow
7. documentation and end-to-end validation

## Current Status

- Completed: architectural decision to move official locales to directory-based
  sources with frozen increments plus one English working file
- Completed: English locale source tree has started moving to
  `src-tauri/localizations/en/origin.json`
- In progress: execution breakdown and implementation planning
- Remaining: Work Packages 1 through 6

## Work Package 0 - Scope Guard And Baseline

### Objective

Freeze the incremental-localization vocabulary, invariants, and execution order
before runtime and UI implementation begins.

### Target Files

- `planning/incremental-localization-execution-breakdown.md`
- `planning/semantic-release-plan.md` only if the release workflow section must
  reference localization increments in the same slice

### Tasks

- lock the official locale directory layout and frozen-increment rule
- lock English as the canonical authoring source during development
- lock `en/working.json` as the only mutable day-to-day source file
- lock compiled bundle ownership in the shell rather than the frontend
- lock primary-versus-fallback selector behavior before UI work starts

### Done When

- the planning documents no longer depend on flat `*.json` locale pack files
- the implementation order can proceed without reopening ownership questions

### Validation

- confirm this breakdown matches the intended incremental-localization
  contract

## Work Package 1 - Source Layout And Official Locale Manifest

### Objective

Replace flat official locale files with locale directories and establish one
deterministic source manifest the shell can trust at build and runtime.

### Target Files

- `src-tauri/localizations/**`
- `src-tauri/build.rs`
- `scripts/localization-check.mjs` or the current localization validation
  script surface
- `scripts/localization-sync.mjs` or the current localization sync surface

### Tasks

- move every official locale from `<locale>.json` to
  `<locale>/origin.json`
- define the increment document contract for `increment-<n>.json`
- define the mutable working-file contract for `en/working.json`
- make build-time discovery enumerate official locale directories instead of a
  hardcoded locale list
- generate one manifest that records official locale codes and the ordered
  increment head each build ships with
- update localization validation tooling to understand directory-based locale
  sources and immutable increment numbering
- keep English as the canonical key catalog for validation and sync tooling

### Out Of Scope

- persisted source sync
- compiled bundle cache
- frontend selector behavior

### Done When

- the build no longer depends on `include_str!("../localizations/<locale>.json")`
- official locales are discovered from directory structure
- localization tooling can validate the new source layout

### Validation

- `npm run localization:check`
- focused shell build validation for the changed source layout

## Work Package 2 - Persisted Source Sync And Locale Discovery

### Objective

Sync official locale source directories into the operating-system persistence
tree and discover all runtime-available locales from persisted sources.

### Target Files

- `src-tauri/src/lib.rs`
- `src-tauri/build.rs` if manifest consumption requires generated
  code or embedded metadata

### Tasks

- replace `EMBEDDED_LOCALIZATION_FILES` with directory-driven official locale
  source sync
- persist official locale source directories under a dedicated persisted source
  root
- overwrite official persisted files only when the shipped source manifest says
  the on-disk official chain is behind or missing
- preserve community locale directories that exist only in persisted sources
- discover available locales from persisted source directories rather than the
  bundled source tree after sync completes
- expose `is_official` from the build-generated official manifest instead of a
  hardcoded locale-code array

### Out Of Scope

- final bundle compilation
- UI loading logic
- release-time mirroring

### Done When

- the shell can boot without fixed official locale filenames
- official source directories are synced into persisted storage
- community locales survive official sync untouched

### Validation

- focused Rust tests for persisted source sync and locale discovery

## Work Package 3 - Incremental Bundle Compiler And Cache

### Objective

Compile per-locale final bundles from `origin`, frozen increments, and the
current working layer when present, then cache the materialized result so
startup only applies missing frozen increments plus the current working file.

### Target Files

- `src-tauri/src/lib.rs`

### Tasks

- define one compiled bundle document format that preserves
  `display_name`, `native_name`, and merged `messages`
- define one per-locale cache metadata format with locale code,
  schema version, last applied increment number, and working-file cache state
- compile `origin.json` first and then apply ordered increments sequentially
- apply `working.json` only as the last overlay layer after frozen increments
- when cache metadata is current, reuse the compiled bundle without replaying
  the full chain
- when new increments exist, apply only the missing increments on top of the
  cached compiled bundle
- when `working.json` changes, refresh only the working-layer overlay instead
  of rebuilding the frozen increment chain
- rebuild from `origin` when the cache is missing, corrupted, gap-ridden, or
  schema-incompatible
- write compiled bundles and metadata atomically into a derived cache root
- emit warnings for malformed locale documents without crashing unrelated
  locale compilation when recovery is still possible

### Out Of Scope

- selector rules in the settings UI
- release-time translation automation

### Done When

- compiled bundles exist for every valid discovered locale before the UI uses
  them
- locale startup work scales with new increments instead of replaying every
  historical increment on every boot

### Validation

- focused Rust tests for cache hit, incremental apply, full rebuild, and
  corrupted-cache recovery

## Work Package 4 - Shell Bootstrap Gate And Settings Contract

### Objective

Make localization bundle readiness part of shell bootstrap and update the shell
settings contract so primary and fallback locales obey the new rules.

### Target Files

- `src-tauri/src/lib.rs`

### Tasks

- run localization source sync and bundle compilation before the main shell UI
  becomes available
- make `localization_settings` resolve against compiled bundle roots rather
  than raw source files
- keep host-locale auto-selection limited to official locale matches
- validate saved fallback preferences against official locales only
- normalize invalid saved fallback selections to English
- keep primary locale selection open to any compiled locale discovered in the
  persisted source tree
- ensure English remains the terminal fallback bundle when official fallback
  keys are missing

### Out Of Scope

- frontend visual changes beyond what the new contract strictly requires
- release authoring tooling

### Done When

- shell localization commands expose compiled-bundle-backed settings
- the shell refuses non-official fallback locale selections
- the main screen does not appear before bundle readiness is established

### Validation

- focused Rust tests for bootstrap gating and preference normalization
- `cargo build --package desktop-shell`

## Work Package 5 - Frontend Compiled-Bundle Consumption And Selector Rules

### Objective

Move the frontend to compiled bundles and enforce the split between primary and
fallback locale inventories.

### Target Files

- `src-react/src/LocalizationProvider.tsx`
- `src-react/src/components/SettingsFocusScreen.tsx`
- `src-react/src/services/runtime.ts`
- related localization tests under `src-react/src/**/*.test.tsx`

### Tasks

- load compiled bundles from the shell-provided bundle root instead of raw
  locale source files
- keep the merge order `primary -> fallback -> en` over compiled bundles
- split settings options so the primary selector lists every available locale
  and the fallback selector lists only official locales
- surface warnings or disabled states cleanly if a previously selected locale
  disappears from the compiled bundle inventory
- update tests that currently assume flat `<locale>.json` files or identical
  primary and fallback option inventories

### Out Of Scope

- release-time locale increment generation

### Done When

- the frontend no longer reads raw increment source files
- selector behavior matches the new primary-versus-fallback contract

### Validation

- focused UI unit tests for `LocalizationProvider` and `SettingsFocusScreen`
- `npm --prefix src-react run test`
- `npm run build --prefix src-react`

## Work Package 6 - Release-Time Increment Mirroring Workflow

### Objective

Add one disciplined release workflow that freezes the current English working
file into the next increment and mirrors that increment into every other
official locale directory.

### Target Files

- `scripts/**` for the release-time localization command surface
- `src-tauri/localizations/**`
- `docs/site/**` or operator-facing docs only if the release workflow is
  documented in the same slice

### Tasks

- define one release-time command that resolves the next increment number from
  the English locale directory
- freeze `en/working.json` into the next English release increment without
  mutating historical increments
- generate the matching increment file for every other official locale
- enforce increment-number parity across official locales
- validate that translated increment files preserve the English key set for the
  new release increment
- fail fast when one official locale is missing an increment file required by
  the current release head
- recreate `en/working.json` for the next development cycle after the release
  increment is frozen
- document the operational rule that day-to-day authoring happens only in
  `en/working.json` and other official locales are advanced at release time

### Out Of Scope

- community locale automation
- machine-translation quality policy beyond key-set parity and file generation

### Done When

- one release workflow can advance every official locale chain in lockstep
- historical increments remain immutable after release freeze

### Validation

- dry-run or focused tests for the release-time localization command
- `npm run localization:check`

## Final Validation Sweep

The mission is only done when the following checks are green for the final
slice set:

- focused Rust tests covering source sync, bundle compilation, cache reuse, and
  bootstrap gating
- focused UI tests covering compiled-bundle loading and selector behavior
- `cargo build --package desktop-shell`
- `npm run build --prefix src-react`
- manual verification that the shell boots with compiled bundles, recognizes a
  community locale from persisted storage, and limits fallback selection to
  official locales
