// A board footprint's text boxes, reference images, points and barcodes (UI-LEFT P17; crates/cli/src/fp_board_items_json.rs: `Part.items`), in board space like the footprint's art
// (kicad-port/fpArt.ts), each in the shape of the board's own item of its kind so the canvas draws it with the board's painters. What KiCad does with them, and this file ports:
//   - They are the footprint's (`FOOTPRINT::m_drawings`, `m_points`): a move, a turn or a flip of the footprint carries them (the backend placed them), and a click on one picks the
//     footprint (`PCB_SELECTION_TOOL::FilterCollectorForHierarchy` hands a footprint's child to its footprint), so [`fpItemsHitDistance`] feeds the footprint's candidate.
//   - `FOOTPRINT::GetBoundingBox` counts the text boxes and the barcodes and leaves the reference images out on the board ("We want the bitmap bounding box just in the
//     footprint editor"): [`fpItemsBounds`].
//   - Each is drawn as the board draws its kind -- the image under everything (`LAYER_DRAW_BITMAPS`), the others with the footprint -- while its layer and the footprint's side
//     are shown.
//   - Its points are snap anchors of the footprint (`PCB_GRID_HELPER::computeAnchors`: "Points are also pick-up points").
//
// Pure geometry, no React, no DOM: the painter (components/canvas/fpBoardItemsPaint.ts) and the selection ask here.

import type { BoardBarcode, BoardPoint, BoardRefImage, BoardTextBox } from "../api/pcbItemTypes";
import type { BoardState, Part } from "../api/types";
import { barcodeBounds, barcodeHitDistance } from "./barcodeShapes";
import { layerIsVisible } from "./layerPresets";
import { pointHitDistance, refImageBounds } from "./pcbExtraItems";
import { imageBoxHitDistance } from "./refImage";
import { textBoxBounds, textBoxHitDistance } from "./pcbTextBox";
import { objectOn } from "./appearance";

type Box = [number, number, number, number];

/** The footprint's items of each kind (none when it has none). */
export const fpTextBoxes = (p: Pick<Part, "items">): readonly BoardTextBox[] => p.items?.text_boxes ?? [];
export const fpImages = (p: Pick<Part, "items">): readonly BoardRefImage[] => p.items?.images ?? [];
export const fpPoints = (p: Pick<Part, "items">): readonly BoardPoint[] => p.items?.points ?? [];
export const fpBarcodes = (p: Pick<Part, "items">): readonly BoardBarcode[] => p.items?.barcodes ?? [];

/** Whether the footprint's side is shown (Footprints Front / Back): everything of a footprint on a hidden side is hidden. */
function sideOn(layerVisible: Readonly<Record<string, boolean>>, p: Pick<Part, "side">): boolean {
  return objectOn(layerVisible, p.side === "bottom" ? "footprints_back" : "footprints_front");
}

/** The id of a footprint's item (`<ref>~<item id>`, crates/model/src/fp_items_place.rs `board_id`) split into the footprint's reference and the item's id; null for another id. */
export function parseFpItemId(id: string): { ref: string; item: string } | null {
  const k = id.indexOf("~");
  if (k <= 0) return null;
  const item = id.slice(k + 1);
  return /^(tbx|img|pnt|bar)_/.test(item) ? { ref: id.slice(0, k), item } : null;
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
  return best;
}

/** `FOOTPRINT::GetBoundingBox`'s share of the items: the boxes of the text boxes and the barcodes (a reference image is left out on the board); null when it has none. */
export function fpItemsBounds(part: Pick<Part, "items">): Box | null {
  let out: Box | null = null;
  const merge = (b: Box) => {
    out = out ? [Math.min(out[0], b[0]), Math.min(out[1], b[1]), Math.max(out[2], b[2]), Math.max(out[3], b[3])] : [b[0], b[1], b[2], b[3]];
  };
  for (const t of fpTextBoxes(part)) merge(textBoxBounds(t));
  for (const b of fpBarcodes(part)) merge(barcodeBounds(b));
  return out;
}
