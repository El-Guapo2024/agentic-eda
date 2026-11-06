// A board footprint's text boxes, reference images, points and barcodes (UI-LEFT P17) and its dimensions and tables (UI-LEFT PP14; crates/cli/src/fp_board_items_json.rs:
// `Part.items`), in board space like the footprint's art
// (kicad-port/fpArt.ts), each in the shape of the board's own item of its kind so the canvas draws it with the board's painters. What KiCad does with them, and this file ports:
//   - They are the footprint's (`FOOTPRINT::m_drawings`, `m_points`): a move, a turn or a flip of the footprint carries them (the backend placed them), and a click on one picks the
//     footprint (`PCB_SELECTION_TOOL::FilterCollectorForHierarchy` hands a footprint's child to its footprint), so [`fpItemsHitDistance`] feeds the footprint's candidate.
//   - `FOOTPRINT::GetBoundingBox` counts the text boxes, the barcodes, the dimensions and the tables and leaves the reference images out on the board ("We want the bitmap
//     bounding box just in the footprint editor"): [`fpItemsBounds`].
//   - Each is drawn as the board draws its kind -- the image under everything (`LAYER_DRAW_BITMAPS`), the others with the footprint -- while its layer and the footprint's side
//     are shown.
//   - Its points are snap anchors of the footprint (`PCB_GRID_HELPER::computeAnchors`: "Points are also pick-up points").
//
// Pure geometry, no React, no DOM: the painter (components/canvas/fpBoardItemsPaint.ts) and the selection ask here.

import type { BoardBarcode, BoardPoint, BoardRefImage, BoardTextBox } from "../api/pcbItemTypes";
import type { BoardState, BoardTable, Dimension, Part } from "../api/types";
import { barcodeBounds, barcodeHitDistance } from "./barcodeShapes";
import { layerIsVisible } from "./layerPresets";
import { pointHitDistance, refImageBounds } from "./pcbExtraItems";
import { imageBoxHitDistance } from "./refImage";
import { textBoxBounds, textBoxHitDistance } from "./pcbTextBox";
import { objectOn } from "./appearance";
import { cellHitDistance, isSelectableCell } from "./tables";

type Box = [number, number, number, number];

/** The footprint's items of each kind (none when it has none). */
export const fpTextBoxes = (p: Pick<Part, "items">): readonly BoardTextBox[] => p.items?.text_boxes ?? [];
export const fpImages = (p: Pick<Part, "items">): readonly BoardRefImage[] => p.items?.images ?? [];
export const fpPoints = (p: Pick<Part, "items">): readonly BoardPoint[] => p.items?.points ?? [];
export const fpBarcodes = (p: Pick<Part, "items">): readonly BoardBarcode[] => p.items?.barcodes ?? [];
export const fpDimensions = (p: Pick<Part, "items">): readonly Dimension[] => p.items?.dimensions ?? [];
export const fpTables = (p: Pick<Part, "items">): readonly BoardTable[] => p.items?.tables ?? [];

/** The distance from `(x, y)` to the segment `a`-`b`. */
function segmentDistance(x: number, y: number, a: readonly [number, number], b: readonly [number, number]): number {
  const [dx, dy] = [b[0] - a[0], b[1] - a[1]];
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((x - a[0]) * dx + (y - a[1]) * dy) / len2));
  return Math.hypot(x - (a[0] + t * dx), y - (a[1] + t * dy));
}

/** A dimension's hit (`PCB_DIMENSION_BASE::HitTest`: its shapes and its text): the nearest of its lines, or its text's anchor. */
export function fpDimensionHitDistance(d: Pick<Dimension, "lines" | "text_at">, x: number, y: number): number {
  let best = Math.hypot(x - d.text_at[0], y - d.text_at[1]);
  for (const [a, b] of d.lines) best = Math.min(best, segmentDistance(x, y, a, b));
  return best;
}

/** A dimension's box: its lines and its text's anchor. */
function dimensionBounds(d: Pick<Dimension, "lines" | "text_at">): Box {
  let b: Box = [d.text_at[0], d.text_at[1], d.text_at[0], d.text_at[1]];
  for (const [p, q] of d.lines) b = [Math.min(b[0], p[0], q[0]), Math.min(b[1], p[1], q[1]), Math.max(b[2], p[0], q[0]), Math.max(b[3], p[1], q[1])];
  return b;
}

/** A table's hit (`PCB_TABLE::HitTest`): the nearest of its cells that are items (0 inside one). */
export function fpTableHitDistance(t: Pick<BoardTable, "cells">, x: number, y: number): number {
  let best = Infinity;
  for (const c of t.cells) if (isSelectableCell(c)) best = Math.min(best, cellHitDistance(c, x, y));
  return best;
}

/** Whether the footprint's side is shown (Footprints Front / Back): everything of a footprint on a hidden side is hidden. */
function sideOn(layerVisible: Readonly<Record<string, boolean>>, p: Pick<Part, "side">): boolean {
  return objectOn(layerVisible, p.side === "bottom" ? "footprints_back" : "footprints_front");
}

/** The id of a footprint's item (`<ref>~<item id>`, crates/model/src/fp_items_place.rs `board_id`) split into the footprint's reference and the item's id; null for another id. */
export function parseFpItemId(id: string): { ref: string; item: string } | null {
  const k = id.indexOf("~");
  if (k <= 0) return null;
  const item = id.slice(k + 1);
  return /^(tbx|img|pnt|bar|dim|tbl)_/.test(item) ? { ref: id.slice(0, k), item } : null;
}

/** The reference images of the placed footprints whose side and layer are shown, with the footprint each is on: what the bitmap layer draws besides the board's own. */
export function shownFootprintImages(board: Pick<BoardState, "parts">, layerVisible: Readonly<Record<string, boolean>>): { image: BoardRefImage; part: Part }[] {
  const out: { image: BoardRefImage; part: Part }[] = [];
  for (const part of board.parts) {
    if (!part.placed || !sideOn(layerVisible, part)) continue;
    for (const image of fpImages(part)) if (layerIsVisible(layerVisible, image.layer)) out.push({ image, part });
  }
  return out;
}

/** The distance from `(x, y)` to the nearest of the footprint's shown items (0 inside one), Infinity when it has none: a click there picks the footprint. */
export function fpItemsHitDistance(part: Pick<Part, "items" | "side">, x: number, y: number, layerVisible: Readonly<Record<string, boolean>>): number {
  if (!part.items || !sideOn(layerVisible, part)) return Infinity;
  let best = Infinity;
  for (const t of fpTextBoxes(part)) if (layerIsVisible(layerVisible, t.layer)) best = Math.min(best, textBoxHitDistance(t, x, y));
  for (const b of fpBarcodes(part)) if (layerIsVisible(layerVisible, b.layer)) best = Math.min(best, barcodeHitDistance(b, x, y));
  for (const p of fpPoints(part)) if (layerIsVisible(layerVisible, p.layer) && objectOn(layerVisible, "ly_points")) best = Math.min(best, pointHitDistance(p, x, y));
  for (const i of fpImages(part)) if (layerIsVisible(layerVisible, i.layer) && objectOn(layerVisible, "bitmaps")) best = Math.min(best, imageBoxHitDistance(refImageBounds(i), x, y));
  for (const d of fpDimensions(part)) if (layerIsVisible(layerVisible, d.layer)) best = Math.min(best, fpDimensionHitDistance(d, x, y));
  for (const t of fpTables(part)) if (layerIsVisible(layerVisible, t.layer)) best = Math.min(best, fpTableHitDistance(t, x, y));
  return best;
}

/** `FOOTPRINT::GetBoundingBox`'s share of the items: the boxes of the text boxes, the barcodes, the dimensions and the tables (a reference image is left out on the board); null when
 * it has none. */
export function fpItemsBounds(part: Pick<Part, "items">): Box | null {
  let out: Box | null = null;
  const merge = (b: Box) => {
    out = out ? [Math.min(out[0], b[0]), Math.min(out[1], b[1]), Math.max(out[2], b[2]), Math.max(out[3], b[3])] : [b[0], b[1], b[2], b[3]];
  };
  for (const t of fpTextBoxes(part)) merge(textBoxBounds(t));
  for (const b of fpBarcodes(part)) merge(barcodeBounds(b));
  for (const d of fpDimensions(part)) merge(dimensionBounds(d));
  for (const t of fpTables(part)) if (t.bbox) merge([t.bbox[0], t.bbox[1], t.bbox[2], t.bbox[3]]);
  return out;
}
