# Focus screen mental model and overlay contracts

## Purpose

This document defines the intended interaction model behind the HGP focus-screen
initiative.

It is not the tasklist.
It is the behavioral specification that explains how the shell should feel,
what each UI surface is responsible for, and which invariants must remain true
as the implementation evolves.

## Experience thesis

The HGP desktop shell should behave like a dense local operations console.

That means:

- the main feed acts as a dispatch board
- a focus screen acts as a working room for one operational concern
- an overlay acts as a temporary blocking subtask with a clear result
- a bottom sheet or light popover acts as a fast decision surface, not a new
  workflow branch

The operator should always know:

- where they are
- what the current screen is for
- which action owns the current task
- what will happen when they press Back, Escape, close, cancel, or confirm

## Operator modes

The UI needs to support five operator modes without making them feel like five
different products.

### 1. Browse and triage

The operator is scanning projects, workers, or process outcomes to decide where
to go next.

Requirements:

- strong page identity
- fast scan anchors
- quiet metadata rows
- no unnecessary modal interruption

### 2. Inspect and compare

The operator is reading details, histories, outputs, or status surfaces.

Requirements:

- section ownership is obvious
- long content can be opened in purpose-built viewers
- secondary facts do not drown the main narrative

### 3. Edit and configure

The operator is changing repository, build, publish, or credential settings.

Requirements:

- draft state is legible
- save ownership is explicit
- support copy does not compete with editable content
- long pickers and credential flows can leave the page temporarily and return
  a typed result

### 4. Confirm and commit

The operator is about to trigger, persist, bind, or destroy something.

Requirements:

- the action is local and unmistakable
- destructive work requires confirmation
- confirmation copy states impact, not just button labels

### 5. Monitor and recover

The operator is watching runtime behavior, inspecting failures, or recovering
from a broken flow.

Requirements:

- stale, failed, and partial states are explicit
- logs and artifacts remain readable under pressure
- dismissing an error surface never silently discards the underlying task

## Core surface taxonomy

### Dispatch board

The main feed is the operator's routing layer.

Responsibilities:

- expose the current system state at a glance
- route into focus screens
- host global overlay governance

Non-goal:

- becoming another dense focus screen with the same hierarchy treatment as the
  editors and inspectors

### Focus screen

A focus screen is a route-level working context.

Responsibilities:

- establish identity, intent, and primary actions
- hold local draft or inspection state
- group content into clear sections
- coordinate overlays for subordinate tasks

### Full-screen overlay

A full-screen overlay is a blocking subtask.

Use it for:

- pickers
- long lists
- credential composition
- long logs
- artifact inspection
- destructive confirmation with real context

A full-screen overlay should feel like a temporary tool that returns a result,
not like a disconnected page.

### Bottom sheet or lightweight overlay

Use for quick actions and short-lived decisions when context should remain
visible.

Do not use for:

- long forms
- multi-step authentication
- large textual inspection
- any flow where cancellation and confirmation semantics are complex

### StepFlow

StepFlow is a transactional sequence that can power a wizard or a multi-step
overlay.

Use it when:

- the operator must complete ordered decisions
- later steps depend on earlier results
- the flow needs explicit transition guards

Do not use it when a single focused form can express the task more directly.

## Navigation model

### Screen stack

- The main feed is the base screen.
- Focus screens are pushed onto the shell navigation stack.
- Focus screens are popped only after active overlays have been dismissed or
  resolved.

### Overlay precedence

The shell must resolve dismissal in this order:

1. top-most overlay
2. current focus screen
3. root-level exit confirmation if the shell defines one

This rule must apply consistently to:

- Back
- Escape
- explicit close buttons
- shell-controlled close requests

### Unsaved-change handling

- A screen that owns a mutable draft must decide whether navigation may proceed.
- The shell must not pop the screen before that decision resolves.
- An overlay opened from a dirty screen should return to the same draft context
  after dismissal.

## State ownership model

### Shell state

The shell owns:

- navigation stack
- overlay stack
- root-level lifecycle and dismissal precedence

### Screen state

A focus screen owns:

- the draft or inspection context for its current task
- data-fetch orchestration for the task it displays
- save, reload, retry, or refresh behavior local to that task

### Overlay state

An overlay owns only the temporary state required to complete its subtask.

It must return:

- a typed result when the operator confirms
- `null` when the operator cancels or dismisses the flow

It must not:

- silently mutate global state behind the parent screen
- create hidden persistence semantics that bypass the parent screen's save
  contract

## Screen anatomy

Every focus screen should answer three questions before the operator reads the
first field.

### 1. Where am I

Answered by:

- page title
- short description
- optional summary strip or contextual facts

### 2. What is this screen for

Answered by:

- the composition of the page header
- section naming
- the placement of primary actions

### 3. What can I safely do next

Answered by:

- stable action clusters
- obvious section ownership
- consistent confirmation and cancellation patterns

## Visual grammar expectations

This document does not define the final CSS, but it does define the expected
reading order.

### Page level

The page frame establishes identity, intent, and primary action.

### Section level

A section owns one major functional slice such as repository settings, build
targets, publish destinations, runtime state, or retained outputs.

### Item level

Rows, cards, nested editors, or list items are subordinate units inside a
section.

### Support content

Warnings, hints, auth notes, and callouts are support surfaces.
They may be important, but they should not compete visually with the task the
operator is actively performing.

## Interaction contracts by use case

### Pickers and large selectors

- Small desktop-native picker affordances can remain the fast path when they
  are reliable.
- A full-screen fallback must exist for embedded or constrained cases.
- The parent screen should await a result and update local draft state in one
  place.

### Credential composition

- Credential entry is a subtask, not a hidden field expansion.
- The credential composer should return a typed payload to the caller.
- Sensitive values must not leak into incidental UI state or logs.

### Confirmations

- Confirmation surfaces must explain what changes, what is destroyed, and what
  remains unaffected.
- The default focus target must be safe.
- Cancellation must be explicit and cheap.

### Long logs and artifacts

- Heavy inspection content should move into dedicated viewers.
- The base screen should keep a compact summary and route to the viewer when
  depth is required.
- Copy, select, and download must be available where the content is dense or
  long-lived.

### Wizards and staged flows

- Each step should have a local goal and a local validation contract.
- Support content should remain clearly secondary.
- Review should read like a final checkpoint, not another data-entry step.

## Feedback-state model

Every touched screen should be able to represent the following states clearly.

### Loading

Initial load before the operator can act.

### Refreshing

The current data remains visible, but a background refresh is underway.

### Empty

No items or no result exist yet, and the screen needs a useful next action.

### Error

The task failed and the operator needs a retry or recovery route.

### Stale or partial

Some data is visible, but the view cannot be treated as fully current or fully
complete.

### Pending action

A local action is running and the UI must communicate ownership without
freezing the whole screen.

## Accessibility and input model

- When an overlay opens, focus moves to the first meaningful interactive
  element.
- Focus is trapped while the overlay is active.
- Focus returns to the invoking control on close whenever possible.
- `aria-modal`, title labeling, and action labeling must be present.
- Keyboard users must be able to cancel, confirm, and traverse dense forms
  without ambiguity.

## Motion guidance

Motion should explain structure, not advertise itself.

Recommended timings:

- page push or pop: 200ms to 260ms
- full-screen overlay slide: 180ms to 220ms
- backdrop fade: 120ms to 160ms

The important rule is consistency.
The operator should not need to relearn timing expectations from one screen to
the next.

## API sketches in TypeScript

```ts
export type OverlayResult<T> = T | null;

export type OpenOverlay = <TResult, TProps extends object>(
  component: React.ComponentType<TProps & OverlayBindings<TResult>>,
  props: TProps,
) => Promise<OverlayResult<TResult>>;

export interface OverlayBindings<TResult> {
  closeWithResult: (result: TResult) => void;
  closeWithCancel: () => void;
}

export interface NavStackController {
  push: (screenId: string, params?: Record<string, unknown>) => void;
  pop: () => void;
  replace: (screenId: string, params?: Record<string, unknown>) => void;
  openOverlay: OpenOverlay;
}

export interface StepFlowController<TStepId extends string> {
  currentStep: TStepId;
  canAdvance: boolean;
  next: () => void;
  back: () => void;
  cancel: () => void;
}
```

## Anti-patterns to reject

- Ad-hoc popovers that bypass the central overlay manager.
- Inline expansion for content that should live in a dedicated viewer.
- Overlays that mutate parent state without returning a result.
- Wizard steps that mix primary form inputs and support content with equal
  visual rank.
- Badge clouds used to compensate for weak hierarchy.
- Multiple conflicting back behaviors depending on which screen happened to
  implement the local handler first.

## Success signals

The mental model is working when:

- an operator can predict dismissal behavior everywhere in the shell
- overlays simplify sequential code instead of multiplying callbacks
- dense screens still feel fast to scan
- support content helps orientation without becoming structural noise
- logs, artifacts, and credentials behave like first-class flows rather than
  incidental exceptions
