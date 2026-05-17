# Focus Screen UI Hierarchy Plan

## Purpose

This document defines a focused UI hierarchy refactor for the dense desktop
shell screens that operators use to inspect, create, and edit repository
projects.

The goal is to make screen structure legible at a glance so operators can tell
where a page starts, where a section starts, and where an actionable item
starts without reading every label.

## Scope Lock

This plan applies to focused screens such as:

- project list
- create project
- edit project
- project workers
- auth providers

This plan explicitly does not change the `main` screen. The current `main`
screen layout, composition, and visual hierarchy are treated as accepted
baseline and must remain intact while this plan is executed.

## Current Baseline

The current desktop UI already has:

- a compact dark visual language
- reusable shared primitives such as `SurfacePanel`, `Badge`, `Button`, and
  `VerticalAccordion`
- focused screens mounted through the shell-level `focus-screen-shell`
- working create, list, and edit flows for repository projects

The current problem is not missing functionality. The problem is weak visual
separation between hierarchy levels on information-dense screens.

## Diagnosis

The current focus surfaces suffer from four concrete issues.

### 1. Page And Section Boundaries Collapse Together

Focused screens usually render directly inside `focus-screen-shell` and then
drop into one or more panels. The page itself does not establish a strong
header region that explains context, intent, and primary action.

Result:

- the screen feels like a stack of containers rather than one coherent page
- the back action exists, but the page context is visually underdefined

### 2. Parent And Child Surfaces Use Nearly The Same Grammar

Large sections, nested callouts, inner cards, and accordion items frequently
share the same border weight, fill treatment, spacing rhythm, and corner
language.

Result:

- a section container does not dominate its children
- inner editable items compete with their parent section for attention

### 3. Typography Does Not Create Enough Rank

Panel titles, item titles, helper copy, summary copy, and metadata all live in
narrow size and weight bands.

Result:

- the operator must read more to understand structure
- scanning costs stay high even when the content model is correct

### 4. Status Chips Carry Too Much Structural Work

Badges currently communicate useful status, but they are also compensating for
missing layout hierarchy.

Result:

- cards and section headers feel busy
- status metadata becomes louder than the content it is supposed to annotate

## Refactor Goals

- establish a clear three-level visual hierarchy: page, section, item
- preserve the compact dark operator-facing style
- reduce scanning cost without reducing information density into empty space
- keep the existing functional flow and Tauri command wiring intact
- improve readability through layout rules before adding new ornamental UI
- keep the `main` screen unchanged

## Hierarchy Model

### 1. Page Level

Every focused screen should establish a page-level frame that contains:

- a page title
- a concise page description
- the page's primary action or primary action cluster
- optional page summary facts when they help routing decisions

The page frame is responsible for answering:

- where am I
- what is this screen for
- what is the most important thing I can do here

### 2. Section Level

Each major functional slice should live inside a section surface with stronger
ownership than the items inside it.

Examples:

- project identity and repository settings
- build targets
- runtime status
- authentication state

Sections should expose:

- section title
- short description
- optional section summary row
- local actions

### 3. Item Level

Interactive rows, target cards, and list entries should be visually subordinate
to their containing section.

Items should communicate:

- item title
- one line of primary supporting data
- compact status metadata
- optional secondary actions

## Surface Rules

The UI should define three distinct surface treatments and use them
consistently.

### Surface A: Page Frame

Use for the screen-level layout shell.

Rules:

- no heavy card treatment by default
- strong title and spacing rhythm
- primary action alignment remains stable across screens
- optional summary strip can sit directly under the page header

### Surface B: Primary Section Panel

Use for the dominant functional regions on a page.

Rules:

- visible border and clearly owned background
- more padding than nested item surfaces
- header separated from body with stronger spacing or a divider

### Surface C: Inset Item Surface

Use for rows, target editors, summary blocks, and inner cards.

Rules:

- lighter contrast than the parent section panel
- lower elevation than the parent section panel
- no visual ambiguity about whether it is a child of the section

## Typography Rules

The refactor should create a stronger type ladder.

### Page Titles

- visibly larger than section titles
- reserved for page identity only

### Section Titles

- stronger than field labels and item titles
- stable enough to anchor accordion headers and panel headers

### Item Titles

- readable in dense lists
- subordinate to section titles

### Metadata And Helper Copy

- quieter than titles
- visually grouped with the thing they annotate
- never allowed to overpower the main label

## Badge And Metadata Rules

Badges should annotate structure, not replace it.

Rules:

- keep at most one or two decision-relevant badges in the first visual row
- push secondary metadata into quieter text rows where possible
- reserve strong badge tones for true status, not generic facts
- avoid badge clouds that turn a card into a wall of pills

## Layout Strategy By Screen

### 1. Project List

Current problem:

- each project card carries title, URL, direction affordance, several badges,
  and summary copy with nearly equal visual weight

Target structure:

- page header with title, description, and refresh action
- one primary list section
- each project entry reduced to a clear scan order:
  title, repository URL, primary status facts, secondary summary

Implementation direction:

- prefer list-row or austere card treatment over dense multi-block cards
- make the repository name the obvious anchor
- demote non-critical facts such as polling cadence and target counts into a
  quieter metadata row

### 2. Create Project

Current problem:

- the wizard stepper informs progress, but the active step body lacks a strong
  persistent page frame
- callouts and form blocks often feel like equal siblings instead of ordered
  support material

Target structure:

- persistent page header for the full wizard
- step context region that explains the current step and what decisions belong
  there
- form region for the active inputs
- summary or support region for contextual warnings, auth state, or review

Implementation direction:

- keep the stepper, but make it navigation support rather than the main header
- create a stable distinction between input fields and contextual callouts
- make review feel like a final confirmation surface, not just another panel

### 3. Edit Project

Current problem:

- the accordion structure is directionally correct, but section headers and
  section bodies do not separate strongly enough
- editable content often begins too close to the section summary

Target structure:

- page header with project identity and save cluster
- accordion sections with clear header/body separation
- summary facts visible before expansion
- inner build target editors rendered as subordinate inset items

Implementation direction:

- keep accordions as the section model
- strengthen header-to-body separation with spacing, divider, or body inset
- ensure nested target editors read as children of the `Build Targets` section

## Proposed Component Work

The refactor should prefer extending shared primitives instead of scattering
one-off wrappers.

### 1. Add A Focus Page Frame Primitive

Suggested responsibility:

- page title
- description
- primary actions
- optional summary row
- stable page body spacing

Suggested target surfaces:

- project list
- create project
- edit project
- project workers
- auth providers

### 2. Split Section Panels From Inset Panels

Extend the current surface primitives so the UI can express:

- primary section panel
- inset child panel
- summary strip or stat strip

This avoids reusing one generic panel style for every hierarchy level.

### 3. Strengthen Accordion Section Headers

Refine the shared accordion presentation so section headers can present:

- title and description
- compact summary facts
- actions
- clearer body separation when opened

### 4. Define A Reusable Metadata Row

Create one reusable pattern for compact status facts that can appear in:

- project list entries
- section summaries
- build target headers
- auth provider cards

This keeps metadata dense without turning every screen into badge noise.

## CSS Rules To Standardize

The implementation should standardize the following decisions in the shared
stylesheet.

- spacing tokens for page, section, and item rhythms
- title sizes for page, section, and item hierarchy
- border and background contrast levels for primary vs inset surfaces
- divider treatment for section header/body boundaries
- summary row spacing and wrapping behavior
- badge tone usage rules for neutral, strong, warning, and muted states

## Suggested Implementation Order

1. Add shared page-frame and multi-surface primitives in the UI component kit.
2. Apply the new hierarchy to the project list first.
3. Apply the new hierarchy to project detail second.
4. Rework the create-project wizard frame and contextual support third.
5. Align auth providers and project workers to the same page/section/item
   model.
6. Run a final consistency pass on typography, badges, and spacing.

## Task List

### Phase 0 - Scope Lock

- [x] Confirm the `main` screen remains visually unchanged.
- [x] Confirm the refactor only targets focused screens.
- [x] Confirm information density remains compact rather than spacious for its
      own sake.

### Phase 1 - Shared Foundations

- [x] Add a shared focus page frame primitive.
- [x] Add distinct styles for primary section surfaces and inset item surfaces.
- [x] Add shared summary-row and metadata-row patterns.
- [x] Define the page/section/item type ladder in the shared stylesheet.

### Phase 2 - Project List

- [x] Refactor the project list into a stronger page header plus one dominant
      list section.
- [x] Reduce project card noise and make the repository name the main anchor.
- [x] Move secondary facts into a quieter metadata row.
- [x] Keep new-project highlighting visible without overpowering the row.

### Phase 3 - Edit Project

- [x] Add a stable page header for project identity and save actions.
- [x] Strengthen accordion header/body separation.
- [x] Introduce section summary facts that remain readable while collapsed.
- [x] Demote nested build target editors into clear inset child surfaces.

### Phase 4 - Create Project

- [x] Add a persistent wizard page frame separate from the step content.
- [x] Reclassify callouts as support content rather than peer sections.
- [x] Make review the strongest confirmation surface in the wizard.
- [x] Keep step navigation legible without making it compete with the page
      title.

### Phase 5 - Consistency Pass

- [x] Align auth providers to the same hierarchy model.
- [x] Align project workers to the same hierarchy model.
- [x] Remove redundant badge usage that does not affect operator decisions.
- [x] Normalize spacing and typography across all focused screens.

## Validation Criteria

The refactor is successful when:

- an operator can distinguish page, section, and item boundaries without
  reading every label
- focused screens feel related to one another through one shared hierarchy
  model
- information density remains high, but scanning cost drops
- the `main` screen remains unchanged
- UI validation still passes through `npm run build --prefix apps/desktop/ui`

## Execution Companion

Implementation-ready work packages for this plan live in
`planning/focus-screen-ui-hierarchy-execution-breakdown.md`.