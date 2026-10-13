// A footprint's graphics and texts on the board (crates/cli/src/fp_art_json.rs: `Part.graphics`, `Part.texts`): the silkscreen outline, the pin-1 mark, the fab body,
// the assembly drawings and the `${REFERENCE}` on the fab layer -- the items `FOOTPRINT::m_drawings` holds besides the fields. They are in board space like the pads,
// so a move, a turn or a flip of the footprint (the backend's, or the preview's) carries them.
//
// What KiCad does with them, and this file ports:
//   - They are not items of their own in the board editor: a click on one picks the footprint (`PCB_SELECTION_TOOL::Selectable` leaves a footprint's children to
//     the footprint; `FOOTPRINT::HitTest` and `GetBoundingBox( false )` count its graphics), so [`artHitDistance`] feeds the footprint's candidate and
//     [`artBounds`] its box.
//   - A graphic is on its layer, and shows when the layer does and the footprint's side does (`PCB_SHAPE::ViewGetLOD`: Footprints Front / Back); a text also needs the
//     Footprint Text row, and the References / Values rows for `${REFERENCE}` / `${VALUE}` (`PCB_TEXT::ViewGetLOD`).
//
// Pure geometry, no React: the painter (components/canvas/fpArtPaint.ts) and the selection (components/canvas/selectionCandidates.ts) both ask here.

import type { FieldInfo, FpGraphic, FpTextInfo, Part, Shape } from "../api/types";
import { shapeBoundingBox, shapeHitDistance } from "../components/canvas/itemHitTest";
import { objectOn } from "./appearance";
import { layerIsVisible } from "./layerPresets";

/** The graphics as complete `Shape`s (the wire leaves out an empty `id` and a `filled` that is false), kept per array so a repaint does not rebuild them. */
const completed = new WeakMap<readonly FpGraphic[], readonly ArtShape[]>();

/** A graphic as the painter and the hit test take it: a `Shape` with the layers it is on, the dash of its stroke and its hatch. */
export type ArtShape = Shape & { dash?: FpGraphic["dash"]; hatch?: FpGraphic["hatch"]; layers?: string[] };

export function graphicsOf(part: Pick<Part, "graphics">): readonly ArtShape[] {
  const raw = part.graphics;
  if (!raw?.length) return [];
  let out = completed.get(raw);
  if (!out) {
    out = raw.map((g) => ({ id: "", ...g, filled: g.filled ?? false }) as ArtShape);
    completed.set(raw, out);
  }
  return out;
}

/** Every layer a graphic is on (`layers`, else its one). */
export const layersOf = (g: Pick<ArtShape, "layer" | "layers">): readonly string[] => (g.layers?.length ? g.layers : [g.layer]);

/** Whether the Appearance panel's rows let a footprint's graphic show on `layer`: the layer's own switch and the footprint's side (`PCB_SHAPE::ViewGetLOD`). */
export function artLayerShown(layerVisible: Readonly<Record<string, boolean>>, part: Pick<Part, "side">, layer: string): boolean {
  return layerIsVisible(layerVisible, layer) && objectOn(layerVisible, part.side === "bottom" ? "footprints_back" : "footprints_front");
}

/** `PCB_TEXT::ViewGetLOD` for a footprint's text: its layer, its footprint's side, the Footprint Text row, and the References / Values rows for the two variables. */
export function artTextShown(layerVisible: Readonly<Record<string, boolean>>, part: Pick<Part, "side">, text: Pick<FpTextInfo, "layer" | "role">): boolean {
  if (!layerIsVisible(layerVisible, text.layer)) return false;
  if (!objectOn(layerVisible, part.side === "bottom" ? "footprints_back" : "footprints_front") || !objectOn(layerVisible, "footprint_text")) return false;
  if (text.role === "reference") return objectOn(layerVisible, "footprint_references");
  if (text.role === "value") return objectOn(layerVisible, "footprint_values");
  return true;
}

/** The text as the one-row field the canvas already draws, boxes and hit-tests (`fieldAsText` takes a `FieldInfo`). */
export function artTextAsField(t: FpTextInfo): FieldInfo {
  return { ...t, name: "", visible: true, lx: 0, ly: 0, langle: 0, custom: false };
}

/** The distance from `(x, y)` to the nearest stroke of a footprint's visible graphics, the stroke's half width taken off (0 on the stroke or inside a filled shape); Infinity when it has none. */
export function artHitDistance(part: Pick<Part, "graphics" | "side">, x: number, y: number, layerVisible: Readonly<Record<string, boolean>>): number {
  let best = Infinity;
  for (const g of graphicsOf(part)) {
    if (!layersOf(g).some((l) => artLayerShown(layerVisible, part, l))) continue;
    best = Math.min(best, Math.max(0, shapeHitDistance(g, x, y) - g.stroke_width / 2));
    if (best === 0) break;
  }
  return best;
}

/** The box `[x0, y0, x1, y1]` of a footprint's graphics, the strokes included (`FOOTPRINT::GetBoundingBox( false )` counts them, the texts it leaves out); null when it has none. */
export function artBounds(part: Pick<Part, "graphics">): [number, number, number, number] | null {
  let out: [number, number, number, number] | null = null;
  for (const g of graphicsOf(part)) {
    const [x0, y0, x1, y1] = shapeBoundingBox(g);
    const half = g.stroke_width / 2;
    out = out ? [Math.min(out[0], x0 - half), Math.min(out[1], y0 - half), Math.max(out[2], x1 + half), Math.max(out[3], y1 + half)] : [x0 - half, y0 - half, x1 + half, y1 + half];
  }
  return out;
}

/** The box of two boxes, either of which may be absent. */
export function unionBox(a: readonly [number, number, number, number] | null | undefined, b: readonly [number, number, number, number] | null | undefined): [number, number, number, number] | null {
  if (!a) return b ? [b[0], b[1], b[2], b[3]] : null;
  if (!b) return [a[0], a[1], a[2], a[3]];
  return [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[2], b[2]), Math.max(a[3], b[3])];
}

/**
 * The dash pattern of a stroke of `width` (`STROKE_PARAMS::Stroke`, `RENDER_SETTINGS::GetDashLength` / `GetDotLength` / `GetGapLength` at the 12 : 3 ratios of ISO 128-2):
 * a dash is `11 w` long, a dot `0.2 w`, a gap `4 w`; the round ends of the strokes add to what is drawn.
 */
export function dashPattern(dash: FpGraphic["dash"], width: number): number[] {
  const w = Math.max(width, 1);
  const [dashLen, dotLen, gap] = [11 * w, 0.2 * w, 4 * w];
  switch (dash) {
    case "dash":
      return [dashLen, gap];
    case "dot":
      return [dotLen, gap];
    case "dash_dot":
      return [dashLen, gap, dotLen, gap];
    case "dash_dot_dot":
      return [dashLen, gap, dotLen, gap, dotLen, gap];
    default:
      return [];
  }
}

/** The layers' paint order, back to front: the user layers, the fab layers, adhesive, paste, mask, the silkscreen on top (copper under all of them). */
export function layerRank(layer: string): number {
  if (/\.Cu$/.test(layer)) return 0;
  const order = ["Cmts.User", "Dwgs.User", "Eco1.User", "Eco2.User", "Margin", "B.Fab", "F.Fab", "B.Adhes", "F.Adhes", "B.Paste", "F.Paste", "B.Mask", "F.Mask", "B.SilkS", "F.SilkS"];
  const i = order.indexOf(layer);
  return i < 0 ? 1 : i + 2;
}
