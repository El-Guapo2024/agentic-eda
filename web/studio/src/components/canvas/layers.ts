// GAL-like layer color/order lookup. Prefers src/kicad/colors.json and
// layers.json once tools/extract-colors.js / extract-layers.js have
// populated them (`meta.generated === true`); until then falls back to
// src/kicad/placeholderColors.ts, which is NOT source-verified -- see
// that file's header comment.

import colorsData from "../../kicad/colors.json";
import layersData from "../../kicad/layers.json";
import type { ColorsFile, LayersFile } from "../../kicad/types";
import { placeholderColor } from "../../kicad/placeholderColors";

const colors = colorsData as ColorsFile;
const layersFile = layersData as LayersFile;

// Back-to-front. Reasonable for a 2-copper-layer board; not checked
// against pcb_draw_panel_gal.cpp (see layers.json's meta.note).
const FALLBACK_DRAW_ORDER = [
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
  "ratsnest",
  "grid",
  "selection",
  "cursor",
];

export function drawOrder(): string[] {
  return layersFile.meta.generated && layersFile.drawOrder.length > 0 ? layersFile.drawOrder : FALLBACK_DRAW_ORDER;
}

export function layerColor(key: string): string {
  if (colors.meta.generated) {
    const hit = colors.colors[key];
    if (hit) return hit;
  }
  return placeholderColor(key);
}

/** Model copper layer name ("F.Cu") -> color-table key ("f_cu"). */
export function copperColorKey(layerName: string): string {
  return layerName.toLowerCase().replace(/\./g, "_");
}
