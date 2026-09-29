// NOT extracted from KiCad. src/kicad/colors.json is still empty (see
// its `meta.note`), and the canvas needs *something* to paint with in
// the meantime. These are hand-picked, loosely inspired by KiCad's
// general look (dark copper-on-black board), but the exact hex values
// have not been checked against common/settings/builtin_color_themes.h.
//
// Once tools/extract-colors.js has been run somewhere with git access to
// ~/ws/kicad-mirror and src/kicad/colors.json's `meta.generated` is
// true, `resolveLayerColor` below should read from that file instead --
// this module is a fallback path for lookups it can't (yet) answer, not
// a permanent theme.
export const PLACEHOLDER_COLORS: Record<string, string> = {
  background: "#131313",
  board_edge: "#cccc00",
  f_cu: "#c83434",
  b_cu: "#3434c8",
  in1_cu: "#c8c834",
  in2_cu: "#34c86a",
  f_silks: "#f0f0f0",
  b_silks: "#dbb0d8",
  f_mask: "#43364880",
  b_mask: "#00143c80",
  f_courtyard: "#a35db0",
  b_courtyard: "#4b6bab",
  f_fab: "#9698a3",
  b_fab: "#ffcc00",
  pad_th: "#c5c5a0",
  via: "#c9ccd1",
  ratsnest: "#e8e08a99",
  grid: "#454545",
  grid_major: "#5b5b5b",
  cursor: "#e0e0e0",
  selection: "#ffd84a",
  anchor: "#00ffff",
} as const;

export function placeholderColor(key: string, fallback = "#888888"): string {
  return PLACEHOLDER_COLORS[key] ?? fallback;
}
