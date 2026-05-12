---
name: hup-react-vite-ui
description: "Build or refactor the HUP desktop UI with React, TypeScript, and Vite. Use when editing apps/desktop/ui, extending reusable components, keeping the compact dark theme, or creating operator-facing screens inspired by Yaak and Hoppscotch."
argument-hint: "[surface, component, or UI task]"
user-invocable: true
---

# HUP React Vite UI

Use this skill when working on the desktop shell frontend in `apps/desktop/ui`.
It is for React + TypeScript + Vite work that must stay aligned with the
project's reusable component kit and compact operator-facing visual language.

## When To Use

- Build or refactor screens under `apps/desktop/ui/src`
- Add or extend shared primitives under `apps/desktop/ui/src/components`
- Rework buttons, icon buttons, icons, fields, selects, badges, or panels
- Keep the shell dense, dark, compact, and tooling-oriented
- Preserve the current references taken from Yaak and Hoppscotch

## Core Rules

- Prefer existing shared primitives before introducing one-off markup
- Keep the UI compact and operator-facing rather than marketing-driven
- Preserve the dark monochrome palette and `5px` border radii
- Keep React state local and explicit unless a broader shared state is needed
- Keep shell logic thin; runtime orchestration still belongs in Rust crates

## Procedure

1. Start from the smallest touched surface in `apps/desktop/ui/src`.
2. Review the [component kit reference](./references/component-kit.md).
3. Reuse or extend the shared primitives before inventing new control styles.
4. Keep new layout work aligned with the compact dark theme and dense spacing.
5. Validate with `npm run build --prefix apps/desktop/ui`.
6. If Tauri shell integration changed, rerun `cargo check -p desktop-shell -j 1 -q`.

## References

- [Component kit reference](./references/component-kit.md)
