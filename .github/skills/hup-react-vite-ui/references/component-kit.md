# Component Kit Reference

The current desktop UI kit lives under `src-react/src/components`.

## Shared Primitives

- `Button.tsx`: `Button` and `IconButton` for primary, secondary, ghost, and
  icon-only actions
- `Field.tsx`: `TextField`, `SelectField`, and `TextAreaField` for dense form
  controls
- `Icon.tsx`: inline SVG icon system used by buttons, rows, and fields
- `Surface.tsx`: `SurfacePanel` and `Badge` for consistent shell containers and
  small status labels

## Theme Rules

- Theme tokens live in `src-react/src/styles.css`
- The palette is dark monochrome: black and gray surfaces with restrained
  contrast
- Buttons, inputs, and containers default to `5px` border radii
- Density is deliberate: short labels, tight spacing, and tool-first layouts
- Yaak and Hoppscotch are the primary references for hierarchy and ergonomics

## Validation

- Build the frontend with `npm run build --prefix src-react`
- If shell bindings or frontend asset wiring changed, rerun
  `cargo check -p desktop-shell -j 1 -q`
