// The board items GAPS.md item 24 (package P16) added -- text boxes, reference images, points and barcodes, and KiCad 10's grid items -- seen as one family by the code that
// walks every kind of item (what an id names, where `GetPosition()` puts it, its bounding box, which layer it is on, the verb that deletes it). The hubs that dispatch on the kind
// of an item (`itemKind`, `itemBounds`, the selection, the delete tool, the move tool's carry) ask this file for the kinds they do not know, so each holds a line for all of them.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardBarcode, BoardDrillMap, BoardDrillMark, BoardDrillProfile, BoardGridItem, BoardPoint, BoardRefImage, BoardTextBox, BoardViaStitch } from "../api/pcbItemTypes";
import { DEFAULT_PROFILE, mapBounds, mapHitDistance, outlineBox, outlineRings } from "./drillChart";
import type { BoardState, Cmd } from "../api/types";
import { barcodeBounds, barcodeHitDistance, barcodeLabel } from "./barcodeShapes";
import { imageBoxHitDistance } from "./refImage";
import { textBoxBounds, textBoxHitDistance } from "./pcbTextBox";
import { GRID_ITEM_LABEL, gridBounds, gridHitDistance } from "./gridItem";
import { stitchBounds, stitchHitDistance, stitchPosition, VIA_STITCH_LABEL } from "./viaStitch";

/** The kinds of item this file knows. */
export type ExtraKind = "textbox" | "image" | "point" | "barcode" | "grid" | "via_stitch" | "drill_map";

/** Every kind of the family, for the tables that list the kinds an item can be (what a plain drag of a clicked item picks up, ...). */
export const EXTRA_KINDS: readonly ExtraKind[] = ["textbox", "image", "point", "barcode", "grid", "via_stitch", "drill_map"];

export type Box = [number, number, number, number];

/** One item of the family, whatever its kind: what the generic code needs of it. */
export interface ExtraItem {
  kind: ExtraKind;
  id: string;
  layer: string;
  /** `BOARD_ITEM::GetPosition()`. */
  position: [number, number];
  /** `GetBoundingBox()`, border and ink included. */
  bounds: Box;
  /** The state record, for the code that knows the kind. */
  textBox?: BoardTextBox;
  image?: BoardRefImage;
  point?: BoardPoint;
  barcode?: BoardBarcode;
  /** KiCad 10's grid item: on no layer (`layer` is empty; `GetLayerSet` is every layer). */
  grid?: BoardGridItem;
  /** KiCad 10's via stitching: on no layer of its own (its vias are ordinary vias of the board). */
  stitch?: BoardViaStitch;
  /** KiCad 10's drill map, with what its hit test reads: the board's marks, the profile and the board outline. */
  map?: { map: BoardDrillMap; marks: BoardDrillMark[]; profile: BoardDrillProfile; outline: [number, number][][] };
}

/** `DEFAULT_PT_SIZE_MM` of `pcbnew/pcb_point.cpp`: the size of a point the Place Point tool makes, µm. */
export const DEFAULT_POINT_SIZE_NM = 1_000_000;

/** `PCB_POINT::GetBoundingBox`: `BOX2I::ByCenter( m_pos, { m_size, m_size } )`. */
export function pointBounds(p: Pick<BoardPoint, "x" | "y" | "size">): Box {
  const x0 = p.x - Math.trunc(p.size / 2);
  const y0 = p.y - Math.trunc(p.size / 2);
  return [x0, y0, x0 + p.size, y0 + p.size];
}

/**
 * How far `(x, y)` is from the point's mark (`PCB_POINT::HitTest`): the bars of its X (half the size either way from the centre, so the bars end at the corners of the box) or the disc of its
 * ring (a quarter of the size in radius, which `SHAPE_CIRCLE::Collide` takes as filled); 0 when it hits.
 */
export function pointHitDistance(p: Pick<BoardPoint, "x" | "y" | "size">, x: number, y: number): number {
  const half = Math.trunc(p.size / 2);
  const bar = (ax: number, ay: number, bx: number, by: number): number => {
    const [dx, dy] = [bx - ax, by - ay];
    const t = Math.max(0, Math.min(1, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)));
    return Math.hypot(x - (ax + t * dx), y - (ay + t * dy));
  };
  const disc = Math.max(0, Math.hypot(x - p.x, y - p.y) - Math.trunc(half / 2));
  return Math.min(bar(p.x - half, p.y - half, p.x + half, p.y + half), bar(p.x - half, p.y + half, p.x + half, p.y - half), disc);
}

const ofPoint = (p: BoardPoint): ExtraItem => ({ kind: "point", id: p.id, layer: p.layer, position: [p.x, p.y], bounds: pointBounds(p), point: p });

/** `REFERENCE_IMAGE::GetBoundingBox`: the image's box, centred on its position (`BOX2I::ByCenter`). */
export function refImageBounds(i: Pick<BoardRefImage, "x" | "y" | "w" | "h">): Box {
  const x0 = i.x - Math.trunc(i.w / 2);
  const y0 = i.y - Math.trunc(i.h / 2);
  return [x0, y0, x0 + i.w, y0 + i.h];
}

const ofBarcode = (b: BoardBarcode): ExtraItem => ({ kind: "barcode", id: b.id, layer: b.layer, position: [b.x, b.y], bounds: barcodeBounds(b), barcode: b });

const ofGrid = (g: BoardGridItem): ExtraItem => ({ kind: "grid", id: g.id, layer: "", position: [g.x, g.y], bounds: gridBounds(g), grid: g });

const ofStitch = (v: BoardViaStitch): ExtraItem => ({ kind: "via_stitch", id: v.id, layer: "", position: stitchPosition(v), bounds: stitchBounds(v), stitch: v });

/** `PCB_DRILL_MAP`: its position is its offset ("Position is therefore an offset applied to every mark"). */
const ofMap =
  (board: (Pick<BoardState, "drawings"> & Partial<Pick<BoardState, "outline" | "outline_polys">>) | null | undefined) =>
  (m: BoardDrillMap): ExtraItem => {
    const marks = board?.drawings?.drill_marks ?? [];
    const profile = board?.drawings?.drill_profile ?? DEFAULT_PROFILE;
    const outline = board ? outlineRings(board) : [];
    const bounds = mapBounds(m, marks, profile, board ? outlineBox(board) : null) ?? [m.offset.x, m.offset.y, m.offset.x, m.offset.y];
    return { kind: "drill_map", id: m.id, layer: m.layer, position: [m.offset.x, m.offset.y], bounds, map: { map: m, marks, profile, outline } };
  };

const ofImage = (i: BoardRefImage): ExtraItem => ({ kind: "image", id: i.id, layer: i.layer, position: [i.x, i.y], bounds: refImageBounds(i), image: i });

const ofTextBox = (b: BoardTextBox): ExtraItem => ({
  kind: "textbox",
  id: b.id,
  layer: b.layer,
  position: b.pts.length > 0 ? [b.pts[0]![0], b.pts[0]![1]] : [b.start[0], b.start[1]],
  bounds: textBoxBounds(b),
  textBox: b,
});

/** Every item of the family the board has, in the order the state lists them. */
export function extraItems(board: Pick<BoardState, "drawings"> | null | undefined): ExtraItem[] {
  return [...(board?.drawings?.text_boxes ?? []).map(ofTextBox), ...(board?.drawings?.images ?? []).map(ofImage), ...(board?.drawings?.points ?? []).map(ofPoint), ...(board?.drawings?.barcodes ?? []).map(ofBarcode), ...(board?.drawings?.grid_items ?? []).map(ofGrid), ...(board?.drawings?.via_stitches ?? []).map(ofStitch), ...(board?.drawings?.drill_maps ?? []).map(ofMap(board))];
}

/** The item of the family `id` names, or undefined. */
export function extraItemById(board: Pick<BoardState, "drawings"> | null | undefined, id: string): ExtraItem | undefined {
  const b = board?.drawings?.text_boxes?.find((t) => t.id === id);
  if (b) return ofTextBox(b);
  const i = board?.drawings?.images?.find((t) => t.id === id);
  if (i) return ofImage(i);
  const pt = board?.drawings?.points?.find((t) => t.id === id);
  if (pt) return ofPoint(pt);
  const bc = board?.drawings?.barcodes?.find((t) => t.id === id);
  if (bc) return ofBarcode(bc);
  const gr = board?.drawings?.grid_items?.find((t) => t.id === id);
  if (gr) return ofGrid(gr);
  const vs = board?.drawings?.via_stitches?.find((t) => t.id === id);
  if (vs) return ofStitch(vs);
  const dm = board?.drawings?.drill_maps?.find((t) => t.id === id);
  return dm ? ofMap(board)(dm) : undefined;
}

/** Every id of the family (for the sets that tell new items from old ones). */
export function extraItemIds(board: Pick<BoardState, "drawings"> | null | undefined): string[] {
  return [...(board?.drawings?.text_boxes ?? []).map((b) => b.id), ...(board?.drawings?.images ?? []).map((i) => i.id), ...(board?.drawings?.points ?? []).map((p) => p.id), ...(board?.drawings?.barcodes ?? []).map((b) => b.id), ...(board?.drawings?.grid_items ?? []).map((g) => g.id), ...(board?.drawings?.via_stitches ?? []).map((v) => v.id), ...(board?.drawings?.drill_maps ?? []).map((m) => m.id)];
}

/** How far the point `(x, y)` is from the item, for a click's "sloppiness": 0 when it hits (`HitTest`). */
export function extraHitDistance(item: ExtraItem, x: number, y: number): number {
  if (item.textBox) return textBoxHitDistance(item.textBox, x, y);
  // `PCB_REFERENCE_IMAGE::HitTest`: `KIGEOM::BoxHitTest( point, GetBoundingBox(), accuracy )` -- anywhere in the box.
  if (item.image) return imageBoxHitDistance(item.bounds, x, y);
  if (item.point) return pointHitDistance(item.point, x, y);
  // `PCB_BARCODE::HitTest`: inside the box of the symbol or of its text, turned with the barcode.
  if (item.barcode) return barcodeHitDistance(item.barcode, x, y);
  // `PCB_GRID_ITEM::HitTest`: anywhere it covers, turned as it is.
  if (item.grid) return gridHitDistance(item.grid, x, y);
  // `PCB_VIA_STITCH::HitTest`: `m_outline.Collide( aPosition, aAccuracy )`.
  if (item.stitch) return stitchHitDistance(item.stitch, x, y);
  // `PCB_DRILL_MAP::HitTest`: its marks and the outline it draws.
  if (item.map) return mapHitDistance(item.map.map, item.map.marks, item.map.profile, item.map.outline, x, y, 0);
  return Infinity;
}

/** The verb that deletes the item (`EDIT_TOOL::DeleteItems`). */
export function extraDeleteCmd(item: Pick<ExtraItem, "kind" | "id">): Cmd {
  switch (item.kind) {
    case "image":
      return { op: "pcb_item", verb: "delete_ref_image", id: item.id };
    case "point":
      return { op: "pcb_item", verb: "delete_point", id: item.id };
    case "textbox":
      return { op: "pcb_item", verb: "delete_text_box", id: item.id };
    case "barcode":
      return { op: "pcb_item", verb: "delete_barcode", id: item.id };
    case "grid":
      return { op: "pcb_item", verb: "delete_grid_item", id: item.id };
    // `PCB_VIA_STITCH::Remove`: the stitching and every via it made.
    case "via_stitch":
      return { op: "pcb_item", verb: "delete_via_stitch", id: item.id };
    case "drill_map":
      return { op: "pcb_item", verb: "delete_drill_map", id: item.id };
  }
}

/** The row of the disambiguation menu for the item (`GetItemDescription`: "PCB text box '%s' on %s"). */
export function extraItemLabel(board: Pick<BoardState, "drawings"> | null | undefined, id: string): string {
  const item = extraItemById(board, id);
  if (item?.textBox) {
    const text = item.textBox.shown ?? item.textBox.text;
    return `Text box "${text.length > 24 ? `${text.slice(0, 23)}...` : text}" [${item.layer}]`;
  }
  // `PCB_REFERENCE_IMAGE::GetItemDescription`
  if (item?.image) return `Reference Image [${item.layer}]`;
  // `PCB_POINT::GetItemDescription`
  if (item?.point) return `Point [${item.layer}]`;
  // `PCB_BARCODE::GetItemDescription`
  if (item?.barcode) return barcodeLabel(item.barcode);
  // `PCB_GRID_ITEM::GetItemDescription`
  if (item?.grid) return GRID_ITEM_LABEL;
  if (item?.stitch) return VIA_STITCH_LABEL;
  // `PCB_DRILL_MAP::GetItemDescription`: "Drill Map on %s".
  if (item?.map) return `Drill Map on ${item.layer}`;
  return id;
}
