---
name: hup-incremental-localization-workflow
description: "Work with HGP incremental localization sources, compiled locale bundles, and release-time locale mirroring. Use when changing src-tauri/localizations, shell localization bootstrap/cache logic, or the release workflow that freezes the current English working file into the next increment for every official locale."
argument-hint: "[localization task, locale workflow, or release mirroring task]"
user-invocable: true
---

# HGP Incremental Localization Workflow

Use this skill when working on the desktop-shell localization system under
`src-tauri/localizations`, the shell-side bundle compiler and
cache, or the release-time workflow that advances official locale chains.

The mutable pre-release working file is
`src-tauri/localizations/en/working.json`.

## When To Use

- Refactor flat locale packs into per-locale source directories
- Build or change shell bootstrap logic for compiled locale bundles
- Implement per-locale bundle cache metadata or incremental replay
- Update localization validation or sync scripts for the incremental layout
- Add the release-time command that freezes the current English working file
  into the next increment for every official locale
- Review locale workflow questions such as `working file`, `increment`,
  `official locale`, `community locale`, `bundle cache`, or `fallback locale`

## Critical Rules

1. Canonical source: English is the canonical authoring source during normal
   development.
2. Mutable work surface: immediately after each release, create
   `src-tauri/localizations/en/working.json` for the next
   localization cycle; only that file may be edited during day-to-day
   development.
3. Frozen history: numbered `increment-<n>.json` files are release artifacts
   and must remain immutable once created.
4. Release freeze: the release workflow freezes the current English working
   file into the next numbered increment and mirrors that increment into every
   other official locale, including `en`.
5. Runtime ownership: the desktop shell consumes compiled locale bundles, not
   raw increment sources.
6. Cache ownership: bundle cache validity is keyed to the frozen increment
   chain plus the current working-file state for the locale being compiled.
7. Selector contract: primary locale selection may include community locales;
   fallback locale selection must include official locales only.
8. Terminal fallback: English remains the terminal fallback bundle when a key
   is absent from other bundles.

## Procedure

1. Start from the smallest localization surface that still controls the
   behavior: source layout, shell bootstrap/cache logic, or release tooling.
2. Keep official locale source discovery directory-driven; do not reintroduce
   hardcoded locale file names.
3. Treat `en/working.json` as the only mutable localization input between
   releases; never patch a historical numbered increment to add new copy.
4. Separate persisted editable locale sources from derived compiled bundle
   cache.
5. Preserve community locales in persisted storage when syncing official
   locales.
6. When implementing caching, prefer per-locale incremental replay from the
   last frozen increment plus one mutable working overlay rather than replaying
   the full chain on every boot.
7. When implementing release tooling, freeze only the current working file;
   never rewrite historical numbered increments.
8. After a release freeze, recreate `en/working.json` for the next cycle
   instead of leaving the mutable file absent.
9. Validate the narrowest affected surface first, then run the relevant build
   or test command for the touched shell or UI slice.

## Validation

- `npm run localization:check`
- focused Rust tests for shell localization bootstrap, sync, and cache logic
- `cargo build --package desktop-shell` when shell localization contracts
  change
- `npm run build --prefix src-react` when the frontend localization
  consumer changes
