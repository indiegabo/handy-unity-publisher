# Focus Screen UI Hierarchy Execution Breakdown

## Purpose

This document breaks the focus-screen hierarchy plan into small execution
packages that are ready to implement in the desktop UI.

Each package is scoped to a narrow surface, names the expected file targets,
states what must not change, and defines a minimal validation step.

## Scope Lock

- the `main` screen is out of scope and must remain visually unchanged
- runtime contracts, Tauri command shapes, and repository workflow logic are
  out of scope unless a UI slice cannot be completed without a thin shell
  adjustment
- this breakdown is for focused screens only

## Delivery Rules

- prefer extending shared primitives before adding one-off wrappers
- keep the compact dark operator-facing style
- do not widen the work package once implementation has started
- validate each package with `npm run build --prefix apps/desktop/ui`

## Execution Order

1. shared foundations
2. project list
3. edit project
4. create project
5. auth providers and project workers
6. consistency sweep

## Work Package 0 - Baseline Guardrails

### Objective

Freeze the current scope boundaries before visual refactor work begins.

### Target Files

- `planning/focus-screen-ui-hierarchy-plan.md`
- `planning/focus-screen-ui-hierarchy-execution-breakdown.md`

### Tasks

- keep the `main` screen explicitly out of scope in planning documents
- keep focus-screen work ordered by dependency rather than by convenience
- require UI build validation for every package

### Done When

- implementation documents explicitly exclude the `main` screen
- the next work packages can be executed independently

### Validation

- confirm planning text still matches the intended execution scope

## Work Package 1 - Shared Foundations

### Objective

Add the shared primitives and stylesheet structure required for page, section,
and item hierarchy without changing the `main` screen.

### Target Files

- `apps/desktop/ui/src/components/Surface.tsx`
- `apps/desktop/ui/src/components/VerticalAccordion.tsx`
- `apps/desktop/ui/src/styles.css`
- `apps/desktop/ui/src/App.tsx` only if a focus-screen wrapper needs a thin
  integration change that does not affect the `main` branch

### Tasks

- add a reusable focus-page frame primitive or surface variant for page-level
  headers and body spacing
- split primary section surface treatment from inset child surface treatment
- add one reusable metadata-row or summary-row pattern
- strengthen the shared accordion body separation model for section use
- define a stronger page, section, and item type ladder in shared CSS

### Out Of Scope

- rewriting individual focus screens beyond the minimum integration needed to
  prove the shared primitives
- any change to the `main` screen layout, spacing, or action bars

### Done When

- the component kit can express page frame, section panel, inset item surface,
  and summary row without one-off markup
- shared CSS contains distinct hierarchy rules rather than one generic panel
  treatment reused everywhere

### Validation

- `npm run build --prefix apps/desktop/ui`

## Work Package 2 - Project List Frame

### Objective

Turn the project list into the first fully upgraded focus screen using the new
hierarchy model.

### Target Files

- `apps/desktop/ui/src/components/ProjectsFocusScreen.tsx`
- `apps/desktop/ui/src/components/Surface.tsx` only if the shared primitives
  still need a small extension
- `apps/desktop/ui/src/styles.css`

### Tasks

- add a page-level header with title, description, and refresh action
- place the list inside one dominant section surface
- make the repository name the first scan anchor in each entry
- demote secondary facts into a quieter metadata row
- keep new-project highlighting visible without making the row louder than the
  rest of the list

### Out Of Scope

- changing repository inspection data shape
- adding filters, search, or new business actions

### Done When

- the page reads as one coherent screen rather than a panel dropped into space
- each project entry has a clear reading order: name, URL, primary state,
  secondary summary
- badge count is reduced to decision-relevant status only

### Validation

- `npm run build --prefix apps/desktop/ui`

## Work Package 3 - Project List Item Density Pass

### Objective

Refine the project entry treatment after the new page frame lands so the list
stays dense but easier to scan.

### Target Files

- `apps/desktop/ui/src/components/ProjectsFocusScreen.tsx`
- `apps/desktop/ui/src/styles.css`

### Tasks

- simplify card or row internals so title, URL, and status no longer compete
- reduce redundant badge use and replace secondary pills with muted text where
  possible
- ensure hover, focus, and highlighted states remain distinct and accessible

### Out Of Scope

- changing navigation behavior or selection rules

### Done When

- entries can be scanned vertically without parsing every badge
- interaction states remain clear and visually subordinate to content

### Validation

- `npm run build --prefix apps/desktop/ui`

## Work Package 4 - Edit Project Page Header

### Objective

Give project detail a stable page-level frame for identity and save actions
before touching the deeper section internals.

### Target Files

- `apps/desktop/ui/src/components/RepositoryProjectDetail.tsx`
- `apps/desktop/ui/src/styles.css`

### Tasks

- add a page header that establishes project identity and page purpose
- move save and reload actions into a stable page-level or section-level action
  cluster with clear ownership
- expose compact project summary facts in a consistent row

### Out Of Scope

- refactoring build-target internals in the same slice
- changing persistence, validation, or save behavior

### Done When

- the edit screen announces itself before the first accordion section begins
- save actions no longer feel buried inside the first content block

### Validation

- `npm run build --prefix apps/desktop/ui`

## Work Package 5 - Edit Project Section Separation

### Objective

Make section boundaries and nested target ownership obvious inside project
detail.

### Target Files

- `apps/desktop/ui/src/components/RepositoryProjectDetail.tsx`
- `apps/desktop/ui/src/components/VerticalAccordion.tsx` only if a shared
  section-header refinement is still needed
- `apps/desktop/ui/src/styles.css`

### Tasks

- strengthen accordion header and body separation
- surface summary facts in collapsed section headers where useful
- render nested build target editors as inset child surfaces
- ensure runtime-status tiles read as section content rather than unrelated
  cards

### Out Of Scope

- adding new project settings fields
- altering build-target data contracts

### Done When

- accordion sections remain understandable while collapsed
- expanded bodies feel visually owned by the section header above them
- build targets clearly read as children of the `Build Targets` section

### Validation

- `npm run build --prefix apps/desktop/ui`

## Work Package 6 - Create Project Wizard Frame

### Objective

Separate wizard-level structure from step-level content.

### Target Files

- `apps/desktop/ui/src/components/CreateProjectWizard.tsx`
- `apps/desktop/ui/src/styles.css`

### Tasks

- add a persistent page frame for the full wizard
- keep the stepper as progress support rather than the dominant page header
- give each step a stronger contextual header or summary region
- keep form input regions visually distinct from support content

### Out Of Scope

- changing step order
- changing repository creation contract or validation rules

### Done When

- the wizard reads as one page with step transitions, not a sequence of loosely
  related panels
- step context is understandable before reading the fields themselves

### Validation

- `npm run build --prefix apps/desktop/ui`

## Work Package 7 - Create Project Support And Review Surfaces

### Objective

Demote auxiliary callouts and strengthen the final review step.

### Target Files

- `apps/desktop/ui/src/components/CreateProjectWizard.tsx`
- `apps/desktop/ui/src/styles.css`

### Tasks

- reclassify auth and explanatory callouts as support content
- prevent support cards from competing with the active form region
- make the review step the strongest confirmation surface in the wizard
- reduce badge noise in the review summary

### Out Of Scope

- changing authentication flow behavior
- adding new wizard steps

### Done When

- callouts help orientation without acting like peer sections
- the review step reads as a final checkpoint with clear summary hierarchy

### Validation

- `npm run build --prefix apps/desktop/ui`

## Work Package 8 - Auth Providers Alignment

### Objective

Bring auth providers into the same page, section, and item hierarchy model.

### Target Files

- `apps/desktop/ui/src/components/AuthProvidersFocusScreen.tsx`
- `apps/desktop/ui/src/styles.css`

### Tasks

- wrap the screen in the shared page-frame pattern
- keep provider cards subordinate to the page's primary section
- standardize metadata rows and badge usage with the rest of the focus screens

### Out Of Scope

- authentication provider logic changes

### Done When

- auth providers feel visually related to the list, wizard, and edit screens

### Validation

- `npm run build --prefix apps/desktop/ui`

## Work Package 9 - Project Workers Alignment

### Objective

Bring project workers into the same hierarchy model without flattening the
operational density of the screen.

### Target Files

- `apps/desktop/ui/src/components/ProjectWorkersFocusScreen.tsx`
- `apps/desktop/ui/src/styles.css`

### Tasks

- add a shared page-frame header
- clarify the distinction between runtime-wide status and per-project worker
  items
- normalize status chips and metadata rows to the shared conventions

### Out Of Scope

- runtime control behavior changes
- worker inspection payload changes

### Done When

- the runtime summary and per-project worker entries stop competing for the
  same visual rank

### Validation

- `npm run build --prefix apps/desktop/ui`

## Work Package 10 - Consistency Sweep

### Objective

Remove leftover inconsistencies after the screen-specific slices land.

### Target Files

- `apps/desktop/ui/src/styles.css`
- any touched focus-screen component that still diverges from the shared model

### Tasks

- normalize title spacing and copy spacing
- normalize badge tones and metadata placement
- remove local overrides that duplicate the shared hierarchy model
- confirm no focus-screen-specific styling leaked into the `main` screen

### Done When

- focused screens share one visual grammar
- the `main` screen remains unchanged
- redundant local CSS has been reduced

### Validation

- `npm run build --prefix apps/desktop/ui`

## Recommended Execution Rhythm

- keep each work package small enough to review in one pass
- run validation immediately after each package lands
- avoid mixing foundation work with multiple screen rewrites in one change
- prefer landing shared primitives before styling around their absence