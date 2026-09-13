/**
 * The one place the Console decides how large an icon is.
 *
 * Lucide takes a numeric prop, so a CSS token can never reach an icon; a
 * stylesheet-only scale is why the previous one went unused while every call
 * site invented its own number.
 *
 * Which step a call site takes:
 *
 * - `xs` beside text, inside compact controls, and for row or toolbar actions.
 * - `sm` a symbol in its own slot: row identity, operation status, a topology
 *   node, a close control.
 * - `md` pane-level controls and dialog icons.
 * - `lg` the illustration in a catalog pane's empty state, and brand tiles.
 * - `xl` the illustration in a detail pane's empty state, and the boot mark.
 */
export const iconSize = {
  xs: 14,
  sm: 16,
  md: 18,
  lg: 22,
  xl: 26,
} as const;
