# Focus screen UI hierarchy plan

## Purpose

This document defines the visual hierarchy system for the HGP focus-screen
initiative.

It explains how the shell should communicate structure at a glance so an
operator can tell where a page starts, where a section starts, what belongs to
what, and which action has ownership.

This is the visual companion to the behavioral model in
`planning/ui-focus-mental-model.md` and the delivery program in
`planning/focus-page-tasklists.md`.

## Scope lock

This plan applies to:

- project list
- create project wizard
- repository editor
- project workers
- auth providers
- process detail
- publish destinations editor

The main feed remains an accepted visual baseline.
However, shell-level overlay governance and back-handling behavior in
`App.tsx` are still in scope when they affect focus-screen navigation.

## Current baseline

The desktop UI already has:

- a compact dark visual language
- reusable shared primitives such as `SurfacePanel`, `Badge`, `Button`, and
  `VerticalAccordion`
- focus screens mounted through the shell-level focus-screen shell
- working create, list, edit, and inspection flows

The problem is not missing functionality.
The problem is that visual hierarchy is too weak to support the density of the
current workflows.

## Diagnosis

### 1. Page and section boundaries collapse together

Many focus screens drop directly into panels without a strong page-level frame.

Result:

- the operator sees containers before they see the page
- actions can feel attached to a panel instead of the screen
- the back action exists, but the working context is visually underdefined

### 2. Parent and child surfaces use nearly the same grammar

Sections, callouts, nested editors, and inner cards often share similar border,
background, and spacing treatment.

Result:

- parent ownership is unclear
- nested editors compete with their parent sections
- support content can look as important as primary task content

### 3. Typography does not create enough rank

Titles, summaries, helper copy, and metadata cluster too closely in size and
weight.

Result:

- operators must read more to understand structure
- scan cost stays high even when the data model is correct

### 4. Badges are compensating for missing hierarchy

Badges communicate real status, but they are also doing structural work that
layout should handle first.

Result:

- cards and section headers feel noisy
- the loudest visual element is often the least important one

## Refactor goals

- establish a clear page, section, and item hierarchy
- preserve the compact dark operator-facing style
- reduce scanning cost without reducing information density into empty space
- keep existing flow and runtime wiring intact unless a slice explicitly
  changes behavior
- make support content clearly secondary without hiding it
- ensure the main feed baseline remains visually stable even while shell-level
  overlay behavior improves

## Hierarchy model

### 1. Page level

Every focus screen should establish a page frame that contains:

- page title
- concise page description
- primary action or primary action cluster
- optional summary facts when they help navigation or save decisions

The page frame answers:

- where am I
- what is this screen for
- what can I do from here

### 2. Section level

Each major functional slice should live inside a section surface with stronger
ownership than the items inside it.

Examples:

- repository identity and settings
- build targets
- publish destinations
- runtime status
- retained outputs
- authentication state

Sections should expose:

- title
- short description when needed
- optional summary row
- local actions when the section owns them

### 3. Item level

Interactive rows, target editors, nested cards, and inventory entries should be
visually subordinate to the containing section.

Items should communicate:

- item title
- one line of primary supporting data
- compact status metadata
- optional secondary actions

### 4. Support surfaces

Warnings, explanatory copy, contextual help, and auth notes are support
surfaces.

Rules:

- support surfaces may be visible and important
- support surfaces must not compete with the primary task region
- support surfaces should use calmer contrast and quieter typography than the
  section they support

## Surface system

The UI should define four distinct surface roles and use them consistently.

### Surface A: page frame

Use for the screen-level shell.

Rules:

- no heavy card treatment by default
- strong title rhythm and stable action alignment
- optional summary strip sits directly below the title region

### Surface B: primary section panel

Use for the dominant functional regions on a page.

Rules:

- clearly owned background
- visible but restrained border treatment
- more padding than nested item surfaces
- stronger separation between header and body

### Surface C: inset item surface

Use for rows, target editors, summary blocks, and nested cards.

Rules:

- lower contrast than the parent section panel
- lower visual rank than the section header
- no ambiguity about parent ownership

### Surface D: support callout

Use for warnings, instructions, and contextual notes.

Rules:

- calmer than primary action surfaces
- never indistinguishable from section panels
- content should be short and directionally useful

## Typography rules

### Page titles

- visibly larger than section titles
- reserved for page identity only

### Section titles

- strong enough to anchor accordion headers and panel headers
- clearly above item titles in rank

### Item titles

- readable in dense inventories
- subordinate to section titles

### Metadata and helper copy

- quieter than titles
- grouped with the content they annotate
- never allowed to overpower the main label

## Action placement rules

- page-level actions belong in the page frame
- section-level actions belong in the owning section header
- item-level actions stay attached to the item they affect
- destructive actions should not hide inside generic action clusters when the
  operator needs to evaluate impact first

## Badge and metadata rules

Badges should annotate structure, not replace it.

Rules:

- keep at most one or two decision-relevant badges in the first visual row
- push secondary facts into muted metadata rows where possible
- reserve strong badge tones for real status or risk
- avoid badge clouds that turn a card into a wall of pills

## Screen-specific hierarchy targets

### Main shell and process feed

- preserve the accepted visual baseline
- centralize overlay governance without visually restyling the feed into a
  focus page
- ensure global quick-action surfaces still feel subordinate to the main board

### Project list

- page header with title, description, and refresh action
- one dominant list section
- project entry reading order: name, URL, primary state, secondary facts
- quick actions should not break the reading rhythm

### Repository editor

- page header with project identity and save cluster
- clear section ownership for repository settings, build targets, destinations,
  and automation
- collapsed summaries remain readable
- nested editors read as children of their section

### Create project wizard

- stable wizard-level frame
- stepper as navigation support, not the dominant page header
- form region, support region, and review region remain distinct

### Project workers

- runtime-wide summary has one clear hierarchy level
- per-project and per-worker entries sit below that level
- bulk or destructive controls do not visually disappear into metadata noise

### Process detail

- outcome summary remains compact and readable inline
- heavy logs and artifacts move into dedicated viewer flows
- retained outputs and actions stay close, but clearly subordinate to the page
  outcome context

### Auth providers

- provider identity is the anchor, not the badge cluster
- bound and unbound state are clear without overusing chip color
- bind and rebind actions read like guided subflows, not incidental buttons

### Publish destinations editor

- destination identity, binding rules, and credentials are distinct surfaces
- large target inventories move to dedicated selection flows
- save and validation ownership remain obvious

## Shared component implications

The hierarchy plan should be expressed primarily through shared primitives.

Required shared capabilities:

- page frame or `ScreenScaffold`
- primary section surface
- inset item surface
- summary or metadata row
- stronger accordion header and body separation
- support callout treatment

## CSS decisions that must be standardized

- spacing tokens for page, section, item, and support rhythms
- title sizes for page, section, and item hierarchy
- border and background contrast levels for primary vs inset surfaces
- divider treatment for section header and body boundaries
- summary row spacing and wrapping behavior
- badge tone rules for neutral, strong, warning, success, and muted states

## Validation criteria

The hierarchy work is successful when:

- an operator can distinguish page, section, item, and support surfaces without
  reading every label
- focused screens feel related through one shared structural grammar
- information density remains high, but scan cost drops
- the main feed remains visually stable
- the desktop UI still passes build validation

## Execution companion

Implementation-ready work packages for this hierarchy plan live in
`planning/focus-screen-ui-hierarchy-execution-breakdown.md`.
