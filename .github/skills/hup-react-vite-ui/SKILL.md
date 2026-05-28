---
name: hup-react-vite-ui
description: "Build or refactor the HGP desktop UI with React, TypeScript, and Vite. Use when editing src-react or extending shared components. Priorities: reuse shared primitives first, then preserve the compact dark operator-facing style."
argument-hint: "[surface, component, or UI task]"
user-invocable: true
---

# HGP React Vite UI

Use this skill when working on the desktop shell frontend in `src-react`.
It is for React + TypeScript + Vite work that must stay aligned with the
project's reusable component kit and compact operator-facing visual language.

## When To Use

- Build or refactor screens under `src-react/src`
- Add or extend shared primitives under `src-react/src/components`
- Rework buttons, icon buttons, icons, fields, selects, badges, or panels
- Keep the shell dense, dark, compact, and tooling-oriented
- Preserve the visual design patterns inspired by Yaak and Hoppscotch

## Core Rules

- Priority 1: prefer existing shared primitives before introducing one-off markup
- Priority 2: keep the UI compact, operator-facing, dark, and limited to the
  established `5px` radii
- Priority 3: keep React state local and explicit unless a broader shared
  state is needed
- Priority 4: keep shell logic thin; runtime orchestration still belongs in
  Rust crates

## Procedure

1. Start from the smallest touched surface in `src-react/src`.
2. Review the [component kit reference](./references/component-kit.md).
3. Reuse or extend the shared primitives before inventing new control styles.
4. Keep new layout work aligned with the compact dark theme and dense spacing.
5. Validate with `npm run build --prefix src-react`.
6. If Tauri shell integration changed, rerun `cargo check -p desktop-shell -j 1 -q`.

## References

- [Component kit reference](./references/component-kit.md)
