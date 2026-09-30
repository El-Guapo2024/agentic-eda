// GAL-like layer color/order lookup.
//
// src/kicad/colors.json now holds the real "KiCad Default" theme (184
// entries, keyed by KiCad's own PCB_LAYER_ID/GAL_LAYER_ID names, e.g.
// "F_Cu", "LAYER_PCB_BACKGROUND" -- see tools/extract-colors.js).
// BUCKET_TO_KICAD_KEY maps this app's own small set of paint "buckets"
// (background/f_cu/b_cu/... -- see painter.ts) onto those real names.
//
// src/kicad/layers.json's extracted `drawOrder` is NOT used for the
// paint loop below: it's pcb_draw_panel_gal.cpp's GAL *overlay* order
// (vias/pads/ratsnest/selection/DRC-marker layers -- ~20 entries, none
// of them a copper or board-outline layer), a different, non-overlapping
// namespace from this app's simplified per-bucket painter. Reconciling
// the two properly means teaching the painter KiCad's full ~50-layer
// model instead of ~15 buckets -- left for a later pass; BUCKET_ORDER
// below is this app's own reasonable back-to-front order for the
// buckets it actually understands.

import colorsData from "../../kicad/colors.json";
import type { ColorsFile } from "../../kicad/types";
import { placeholderColor } from "../../kicad/placeholderColors";

const colors = colorsData as ColorsFile;

const BUCKET_TO_KICAD_KEY: Record<string, string> = {
  background: "LAYER_PCB_BACKGROUND",
  board_edge: "Edge_Cuts",
  margin: "Margin",
  f_cu: "F_Cu",
  b_cu: "B_Cu",
  in1_cu: "In1_Cu",
  in2_cu: "In2_Cu",
  f_mask: "F_Mask",
  b_mask: "B_Mask",
  f_silks: "F_SilkS",
  b_silks: "B_SilkS",
  f_courtyard: "F_CrtYd",
  b_courtyard: "B_CrtYd",
  f_fab: "F_Fab",
  b_fab: "B_Fab",
  pad_th: "LAYER_PAD_PLATEDHOLES",
  pad_netname: "LAYER_PAD_NETNAMES",
  via: "LAYER_VIA_HOLES",
  ratsnest: "LAYER_RATSNEST",
  grid: "LAYER_GRID",
  grid_axes: "LAYER_GRID_AXES",
  cursor: "LAYER_CURSOR",
  anchor: "LAYER_ANCHOR",
  selection: "LAYER_SELECTION_SHADOWS",
  drc_error: "LAYER_DRC_ERROR",
  drc_warning: "LAYER_DRC_WARNING",
};

// Back-to-front paint order for this app's buckets (see painter.ts).
export const BUCKET_ORDER = [
  "background",
  "b_cu",
  "in2_cu",
  "in1_cu",
  "f_cu",
  "b_mask",
  "f_mask",
  "b_silks",
  "f_silks",
  "b_courtyard",
  "f_courtyard",
  "b_fab",
  "f_fab",
  "board_edge",
  "ratsnest",
  "grid",
  "selection",
  "cursor",
];

export function drawOrder(): string[] {
  return BUCKET_ORDER;
}

export function layerColor(bucketOrRealKey: string): string {
  const realKey = BUCKET_TO_KICAD_KEY[bucketOrRealKey] ?? bucketOrRealKey;
  if (colors.meta.generated) {
    const hit = colors.colors[realKey];
    if (hit) return hit;
  }
  return placeholderColor(bucketOrRealKey);
}

/** Model copper layer name ("F.Cu") -> this app's bucket key ("f_cu"). */
export function copperColorKey(layerName: string): string {
  return layerName.toLowerCase().replace(/\./g, "_");
}
