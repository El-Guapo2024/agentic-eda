// ERC pin-conflict matrix panel helpers -- eeschema/dialogs/panel_setup_pinmap.cpp
// (PANEL_SETUP_PINMAP) over eeschema/erc/erc_settings.cpp's pin map.
//
// The panel shows only the lower triangle of an 11x11 grid: "NC is not
// included in the pin map as it generates errors separately" (`#define
// PINMAP_TYPE_COUNT ( ELECTRICAL_PINTYPES_TOTAL - 1 )`), clicking a cell
// cycles its level `( level + 1 ) % 3` (OK -> Warning -> Error -> OK) and
// writes both `SetPinMapValue( y, x )` and `( x, y )` -- the symmetric edit
// the backend's `set_erc_pin_map_cell` verb performs.

/** `CommentERC_H` / `CommentERC_V` (erc.cpp), in `ELECTRICAL_PINTYPE` order. The 12th, "No Connection", is never shown in the panel. */
export const PIN_TYPE_LABELS: readonly string[] = [
  "Input Pin",
  "Output Pin",
  "Bidirectional Pin",
  "Tri-State Pin",
  "Passive Pin",
  "Free Pin",
  "Unspecified Pin",
  "Power Input Pin",
  "Power Output Pin",
  "Open Collector",
  "Open Emitter",
  "No Connection",
];

/** `PINMAP_TYPE_COUNT`. */
export const PINMAP_TYPE_COUNT = 11;

/** `PIN_ERROR`: 0 OK, 1 WARNING, 2 PP_ERROR. */
export type PinLevel = 0 | 1 | 2;

/** `changeErrorLevel`'s `level = ( level + 1 ) % 3`. */
export function nextLevel(level: number): PinLevel {
  return (((level % 3) + 3 + 1) % 3) as PinLevel;
}

/** `setDRCMatrixButtonState`'s tooltips. */
export function levelTooltip(level: number): string {
  if (level === 0) return "No error or warning";
  if (level === 1) return "Generate warning";
  return "Generate error";
}

/** A short non-color cue so the grid is readable without relying on color alone. */
export function levelGlyph(level: number): string {
  if (level === 0) return "✓";
  if (level === 1) return "!";
  return "×";
}

export interface PinMapCell {
  /** Row type index (the longer-labelled axis). */
  row: number;
  /** Column type index, always <= row (lower triangle only, as in the panel). */
  col: number;
  level: PinLevel;
}

/** The lower-triangle cells in the panel's drawing order (`for ii: for jj <= ii`). Missing/invalid matrix entries read as OK. */
export function triangleCells(matrix: readonly (readonly number[])[]): PinMapCell[] {
  const out: PinMapCell[] = [];
  for (let row = 0; row < PINMAP_TYPE_COUNT; row++) {
    for (let col = 0; col <= row; col++) {
      const v = matrix[row]?.[col] ?? 0;
      out.push({ row, col, level: (v === 1 || v === 2 ? v : 0) as PinLevel });
    }
  }
  return out;
}
