// World-space bounding boxes of the items each editor draws, keyed by the id its selection holds.
//
// KiCad answers "where is this selection / the document" with `EDA_ITEM::GetBoundingBox()` unioned over the
// items (`SELECTION::GetBoundingBox`, `EDA_DRAW_FRAME::GetDocumentExtents`, `BOARD::ComputeBoundingBox`); the
// studio's four canvases hold their items in four different models, so this module is the one place each is
// reduced to boxes. The common view actions read them: Zoom to Selected Objects / Pan to Center Selected Objects
// (common_tools.cpp doZoomFit / doCenter), Draw Bounding Boxes (the render settings' `m_drawBoundingBoxes`) and
// the lasso (a box tested against the lasso polygon, `KIGEOM::BoxHitTest`).
//
// Units are each editor's own internal space: micrometres, +y down (the board, the schematic, the footprint
// editor and -- after the one Y-flip `resolveLibPoint` does -- the symbol editor).
import type { BoardState, CmdShape, LibraryFootprint, LibraryPad, LibrarySymbol, LibrarySymbolGraphic, PointXY, Shape } from "../api/types";
import { shapeBoundingBox, textBoundingBox } from "../components/canvas/itemHitTest";
import { libPointToInternalUm, resolvePin, symbolTransformMatrix } from "../components/schematic/transform";
import { padIds } from "./pcbItems";

/** `[minX, minY, maxX, maxY]`. */
export type Box = readonly [number, number, number, number];
export type ItemBoxes = Map<string, Box>;

export function boxOfPoints(pts: readonly (readonly [number, number])[]): Box | null {
  if (pts.length === 0) return null;
  let x0 = Infinity,
    y0 = Infinity,
    x1 = -Infinity,
    y1 = -Infinity;
  for (const [x, y] of pts) {
    if (x < x0) x0 = x;
    if (y < y0) y0 = y;
    if (x > x1) x1 = x;
    if (y > y1) y1 = y;
  }
  return [x0, y0, x1, y1];
}

/** The smallest box holding all of `boxes`, or null for none. */
export function unionBoxes(boxes: Iterable<Box>): Box | null {
  let out: Box | null = null;
  for (const b of boxes) out = out ? [Math.min(out[0], b[0]), Math.min(out[1], b[1]), Math.max(out[2], b[2]), Math.max(out[3], b[3])] : b;
  return out;
}

/** `box` grown by `m` on every side. */
export function inflateBox(box: Box, m: number): Box {
  return [box[0] - m, box[1] - m, box[2] + m, box[3] + m];
}

/** The boxes of the ids in `ids` that `boxes` knows, unioned (a group id counts as its members -- see `pcbItemBoxes`). */
export function boxOfIds(boxes: ItemBoxes, ids: Iterable<string>): Box | null {
  const found: Box[] = [];
  for (const id of ids) {
    const b = boxes.get(id);
    if (b) found.push(b);
  }
  return unionBoxes(found);
}

// ------------------------------------------------------------------------------------------------ board

/** Every selectable board item: placed footprints (courtyard), tracks, vias, zones, graphics, text, dimensions and groups (the union of their members). */
export function pcbItemBoxes(board: BoardState): ItemBoxes {
  const out: ItemBoxes = new Map();
  for (const p of board.parts) {
    if (!p.placed) continue;
    if (p.courtyard) out.set(p.ref, p.courtyard);
    else if (p.at) {
      const pads = (p.pads ?? []).map((pad) => [pad.x, pad.y] as [number, number]);
      const b = boxOfPoints(pads.length > 0 ? pads : [[p.at[0], p.at[1]]]);
      if (b) out.set(p.ref, b);
    }
  }
  // A pad is an item of its own (`REF.NUMBER`, kicad-port/pcbItems.ts `padIds`): selectable, and Zoom to Selection finds it.
  for (const p of board.parts) {
    if (!p.placed || !p.pads?.length) continue;
    const ids = padIds(p);
    p.pads.forEach((pad, i) => out.set(ids[i]!, [pad.x - pad.w / 2, pad.y - pad.h / 2, pad.x + pad.w / 2, pad.y + pad.h / 2]));
  }
  for (const t of board.routing?.tracks ?? []) {
    const b = boxOfPoints(t.pts);
    if (b) out.set(t.id, inflateBox(b, t.width / 2));
  }
  for (const v of board.routing?.vias ?? []) out.set(v.id, [v.x - v.d / 2, v.y - v.d / 2, v.x + v.d / 2, v.y + v.d / 2]);
  for (const z of board.routing?.zones ?? []) {
    const b = boxOfPoints(z.outline);
    if (b) out.set(z.id, b);
  }
  for (const s of board.drawings?.shapes ?? []) out.set(s.id, shapeBoundingBox(s));
  for (const t of board.drawings?.texts ?? []) {
    const b = textBoundingBox(t);
    out.set(t.id, [b.x0, b.y0, b.x1, b.y1]);
  }
  for (const d of board.drawings?.dimensions ?? []) {
    const pts: [number, number][] = [d.text_at, ...d.lines.flatMap(([a, b]) => [a, b] as [number, number][])];
    const b = boxOfPoints(pts);
    if (b) out.set(d.id, b);
  }
  for (const g of board.drawings?.groups ?? []) {
    const b = boxOfIds(out, g.member_ids);
    if (b) out.set(g.id, b);
  }
  return out;
}

/** The board's whole extent (`BOARD::ComputeBoundingBox`): the outline and everything on it. */
export function pcbContentBox(board: BoardState, boxes: ItemBoxes = pcbItemBoxes(board)): Box | null {
  const outline = board.outline ? boxOfPoints(board.outline) : null;
  return unionBoxes([...(outline ? [outline] : []), ...boxes.values()]);
}

// (The schematic's items and their bounds are the catalog of components/schematic/schItems.ts; actions/editorAdapter.ts reads it.)

// ------------------------------------------------------------------------------------------------ footprint editor

/** A pad's box: its copper size around `at + offset`, turned by its rotation (a pad is small, so the box of the rotated rectangle is what matters). */
export function padBox(pad: LibraryPad): Box {
  const rad = (pad.rot / 1000) * (Math.PI / 180);
  const cos = Math.abs(Math.cos(rad));
  const sin = Math.abs(Math.sin(rad));
  const [w, h] = pad.size;
  const hw = (w * cos + h * sin) / 2;
  const hh = (w * sin + h * cos) / 2;
  const cx = pad.at.x + pad.offset.x;
  const cy = pad.at.y + pad.offset.y;
  return [cx - hw, cy - hh, cx + hw, cy + hh];
}

/** A library graphic (`{x, y}` points, an optional id) as the display `Shape` the board's hit-test helpers read (point pairs). */
export function cmdShapeToShape(g: CmdShape): Shape {
  const pt = (p: PointXY): [number, number] => [p.x, p.y];
  const common = { id: g.id ?? "", layer: g.layer, stroke_width: g.stroke_width, filled: g.filled };
  switch (g.kind) {
    case "segment":
      return { ...common, kind: "segment", start: pt(g.start), end: pt(g.end) };
    case "rect":
      return { ...common, kind: "rect", start: pt(g.start), end: pt(g.end) };
    case "circle":
      return { ...common, kind: "circle", center: pt(g.center), end: pt(g.end) };
    case "arc":
      return { ...common, kind: "arc", start: pt(g.start), mid: pt(g.mid), end: pt(g.end) };
    case "polygon":
      return { ...common, kind: "polygon", pts: g.pts.map(pt) };
    case "bezier":
      return { ...common, kind: "bezier", start: pt(g.start), c1: pt(g.c1), c2: pt(g.c2), end: pt(g.end) };
  }
}

/** The pads, graphics and text of the open footprint, by id. */
export function footprintItemBoxes(fp: LibraryFootprint): ItemBoxes {
  const out: ItemBoxes = new Map();
  for (const p of fp.pads) if (p.id) out.set(p.id, padBox(p));
  for (const g of fp.graphics) if (g.id) out.set(g.id, shapeBoundingBox(cmdShapeToShape(g)));
  for (const t of fp.texts) {
    if (!t.id) continue;
    const b = textBoundingBox({ id: t.id, content: t.content, x: t.at.x, y: t.at.y, angle: t.angle, layer: t.layer, size: t.size_um, stroke_width: t.stroke_width, justify: t.justify, mirror: t.mirror });
    out.set(t.id, [b.x0, b.y0, b.x1, b.y1]);
  }
  return out;
}

// ------------------------------------------------------------------------------------------------ symbol editor

const mmToUm = ([x, y]: [number, number]): [number, number] => libPointToInternalUm([x, y]);

function graphicBox(g: LibrarySymbolGraphic): Box | null {
  switch (g.kind) {
    case "rectangle":
      return boxOfPoints([mmToUm([g.start.x, g.start.y]), mmToUm([g.end.x, g.end.y])]);
    case "polyline":
      return boxOfPoints(g.pts.map((p) => mmToUm([p.x, p.y])));
    case "circle": {
      const [cx, cy] = mmToUm([g.center.x, g.center.y]);
      const r = g.radius_mm * 1000;
      return [cx - r, cy - r, cx + r, cy + r];
    }
    case "arc":
      return boxOfPoints([mmToUm([g.start.x, g.start.y]), mmToUm([g.mid.x, g.mid.y]), mmToUm([g.end.x, g.end.y])]);
    case "text": {
      const [x, y] = mmToUm([g.at.x, g.at.y]);
      const size = g.size_mm * 1000;
      const w = Math.max(g.text.length, 1) * size * 0.6;
      return [x - w / 2, y - size / 2, x + w / 2, y + size / 2];
    }
  }
}

/** The pins and graphics of the open symbol (the ones on `unit` / `bodyStyle` or on all of them, `0`), by id, in the editor's internal space. */
export function symbolItemBoxes(sym: LibrarySymbol, unit: number, bodyStyle: number): ItemBoxes {
  const out: ItemBoxes = new Map();
  const shown = (u: number, b: number) => (u === 0 || u === unit) && (b === 0 || b === bodyStyle);
  const identity = symbolTransformMatrix(0, null);
  for (const pin of sym.pins) {
    if (!pin.id || !shown(pin.unit, pin.body_style)) continue;
    const rp = resolvePin({ number: pin.number, name: pin.name, electrical_type: pin.electrical_type, shape: pin.shape, at: [pin.at.x, pin.at.y], angle_deg: pin.angle_deg, length_mm: pin.length_mm, unit: pin.unit, body_style: pin.body_style, hidden: pin.hidden }, identity, [0, 0]);
    const b = boxOfPoints([rp.root, rp.tip]);
    if (b) out.set(pin.id, b);
  }
  for (const g of sym.graphics) {
    if (!g.id || !shown(g.unit, g.body_style)) continue;
    const b = graphicBox(g);
    if (b) out.set(g.id, b);
  }
  return out;
}
