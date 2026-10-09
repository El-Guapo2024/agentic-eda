// What the board editor's snapping looks at: the items of the board as the grid helper sees them -- footprints, pads, track segments, vias, graphic shapes, zones,
// dimensions and text -- each with the layers it is on, a bounding box, and the idealised geometry (a zero-width segment, circle, arc or box) that
// `GetBoardIntersectable` gives the ones that can be intersected and extended.
//
// This is the stand-in for the KIGFX::VIEW that pcbnew/tools/pcb_grid_helper.cpp queries (`view->Query( area, items )`): `queryScene` returns the items whose box
// overlaps an area, minus the ones to skip. The studio's board state has a polyline for a track and a pad as a position and a size; here every segment of a
// polyline is an item of its own (`PCB_TRACK` is one segment), and a pad is a circle, an oval or a rectangle.
//
// Built from `BoardState` (api/types.ts, the type only: this module has no runtime dependency on the app) and cached per board object by `sceneOf`.
import type { BoardState, BoardText, Dimension, Pad, Part, Shape, Track, Via, Zone } from "../api/types";
import { arcThrough, box, circle, seg, type ArcG, type Box, type Geom, type Pt } from "./snapGeom";
import { shapeBoundingBox, textBoundingBox } from "../components/canvas/itemHitTest";

export type SnapItemType = "footprint" | "pad" | "track" | "via" | "shape" | "zone" | "dimension" | "text";

export interface SnapItemBase {
  /** Unique within the scene: the board id of the item, with `#k` after it for the k-th segment of a polyline track. */
  id: string;
  /** The board-level id the item belongs to (a track's id for each of its segments, the footprint's reference for its pads): what a skip list and the selection name. */
  owner: string;
  type: SnapItemType;
  /** `GetLayerSet()` as layer names: a copper item its layer, a through-hole pad or a through via every copper layer. */
  layers: readonly string[];
  /** `GetBoundingBox()`: the area `view->Query` matches the item by. */
  bbox: Box;
  /** The idealised geometry `GetBoardIntersectable` returns, for the item kinds that have one. */
  geom?: Geom;
  /** `HitTest( pos, 0 )`. */
  hit(p: Pt): boolean;
}

export interface FootprintItem extends SnapItemBase {
  type: "footprint";
  /** `FOOTPRINT::GetPosition()`. */
  at: Pt;
  /** `GetBoundingBox( false ).Centre()`. */
  center: Pt;
  pads: PadItem[];
}

export interface PadItem extends SnapItemBase {
  type: "pad";
  at: Pt;
  w: number;
  h: number;
  round: boolean;
  th: boolean;
  /** The reference of the footprint the pad is on. */
  footprint: string;
}

export interface TrackItem extends SnapItemBase {
  type: "track";
  start: Pt;
  end: Pt;
  /** `PCB_TRACK::GetCenter()`: the middle of the segment (of the chord, for an arc). */
  center: Pt;
  width: number;
  arc?: ArcG;
}

export interface ViaItem extends SnapItemBase {
  type: "via";
  at: Pt;
  d: number;
}

export interface ShapeItem extends SnapItemBase {
  type: "shape";
  shape: Shape;
}

export interface ZoneItem extends SnapItemBase {
  type: "zone";
  outline: readonly Pt[];
}

export interface DimensionItem extends SnapItemBase {
  type: "dimension";
  dim: Dimension;
}

export interface TextItem extends SnapItemBase {
  type: "text";
  at: Pt;
}

export type SnapItem = FootprintItem | PadItem | TrackItem | ViaItem | ShapeItem | ZoneItem | DimensionItem | TextItem;

export interface SnapScene {
  items: readonly SnapItem[];
}

/** The part of the board state the scene reads (a test builds one by hand; a `BoardState` is one). */
export interface SceneBoard {
  layers?: readonly string[];
  parts: ReadonlyArray<Pick<Part, "ref" | "placed"> & Partial<Pick<Part, "at" | "side" | "pads" | "courtyard">>>;
  routing?: { tracks: readonly Track[]; vias: readonly Via[]; zones: readonly Zone[] } | null;
  drawings?: { shapes: readonly Shape[]; texts: readonly BoardText[]; dimensions: readonly Dimension[] } | null;
}

// ------------------------------------------------------------------------------------------------------------------------------------- hit tests

const dist = (p: Pt, q: Pt) => Math.hypot(p[0] - q[0], p[1] - q[1]);

function distToSegment(p: Pt, a: Pt, b: Pt): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  if (len2 === 0) return dist(p, a);
  const t = Math.min(1, Math.max(0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2));
  return dist(p, [a[0] + t * dx, a[1] + t * dy]);
}

function insidePolygon(p: Pt, pts: readonly Pt[]): boolean {
  let inside = false;
  for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
    const [xi, yi] = pts[i]!;
    const [xj, yj] = pts[j]!;
    if (yi > p[1] !== yj > p[1] && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

function distToPolyline(p: Pt, pts: readonly Pt[], closed: boolean): number {
  let best = Infinity;
  const n = pts.length;
  for (let i = 0; i + 1 < n; i++) best = Math.min(best, distToSegment(p, pts[i]!, pts[i + 1]!));
  if (closed && n > 2) best = Math.min(best, distToSegment(p, pts[n - 1]!, pts[0]!));
  return best;
}

function distToArc(p: Pt, a: ArcG): number {
  const ang = Math.atan2(p[1] - a.c[1], p[0] - a.c[0]);
  const twoPi = Math.PI * 2;
  const rel = (((ang - a.a0) % twoPi) + twoPi) % twoPi;
  const within = a.da >= 0 ? rel <= a.da : rel === 0 || twoPi - rel <= -a.da;
  if (within) return Math.abs(dist(p, a.c) - a.r);
  const s: Pt = [a.c[0] + a.r * Math.cos(a.a0), a.c[1] + a.r * Math.sin(a.a0)];
  const e: Pt = [a.c[0] + a.r * Math.cos(a.a0 + a.da), a.c[1] + a.r * Math.sin(a.a0 + a.da)];
  return Math.min(dist(p, s), dist(p, e));
}

const padHalf = (w: number, h: number) => [w / 2, h / 2] as const;

/** `PAD::HitTest( pos, 0 )`: the point is inside the pad's shape. */
export function padHit(pad: Pick<Pad, "x" | "y" | "w" | "h" | "round">, p: Pt): boolean {
  const [hx, hy] = padHalf(pad.w, pad.h);
  const dx = p[0] - pad.x;
  const dy = p[1] - pad.y;
  if (!pad.round) return Math.abs(dx) <= hx && Math.abs(dy) <= hy;
  if (pad.w === pad.h) return Math.hypot(dx, dy) <= hx;
  // A stadium: the distance to the segment between the two cap centres is at most the cap radius.
  if (pad.w > pad.h) return distToSegment(p, [pad.x - (hx - hy), pad.y], [pad.x + (hx - hy), pad.y]) <= hy;
  return distToSegment(p, [pad.x, pad.y - (hy - hx)], [pad.x, pad.y + (hy - hx)]) <= hx;
}

// -------------------------------------------------------------------------------------------------------------------------------------- building

const COPPER = (layers: readonly string[] | undefined): readonly string[] => (layers && layers.length > 0 ? layers.filter((l) => l.endsWith(".Cu")) : ["F.Cu", "B.Cu"]);

/** `PCB_SHAPE` -> `GetBoardIntersectable`: a segment, circle, arc or rectangle (a polygon and a Bezier have none). */
function shapeGeom(s: Shape): Geom | undefined {
  switch (s.kind) {
    case "segment":
      return seg(s.start, s.end);
    case "circle":
      return circle(s.center, dist(s.center, s.end));
    case "arc":
      return arcThrough(s.start, s.mid, s.end) ?? undefined;
    case "rect":
      return box(s.start[0], s.start[1], s.end[0], s.end[1]);
    default:
      return undefined;
  }
}

function shapeHit(s: Shape): (p: Pt) => boolean {
  const half = s.stroke_width / 2;
  switch (s.kind) {
    case "segment":
      return (p) => distToSegment(p, s.start, s.end) <= half;
    case "circle": {
      const r = dist(s.center, s.end);
      return (p) => (s.filled ? dist(p, s.center) <= r + half : Math.abs(dist(p, s.center) - r) <= half);
    }
    case "arc": {
      const a = arcThrough(s.start, s.mid, s.end);
      return (p) => (a ? distToArc(p, a) <= half : false);
    }
    case "rect": {
      const corners: Pt[] = [s.start, [s.end[0], s.start[1]], s.end, [s.start[0], s.end[1]]];
      const b = box(s.start[0], s.start[1], s.end[0], s.end[1]);
      return (p) => (s.filled && p[0] >= b.x0 && p[0] <= b.x1 && p[1] >= b.y0 && p[1] <= b.y1) || distToPolyline(p, corners, true) <= half;
    }
    case "polygon":
      return (p) => (s.filled && insidePolygon(p, s.pts)) || distToPolyline(p, s.pts, true) <= half;
    case "bezier":
      return (p) => distToPolyline(p, [s.start, s.c1, s.c2, s.end], false) <= half;
  }
}

function boxOf(b: [number, number, number, number]): Box {
  return box(b[0], b[1], b[2], b[3]);
}

function padItem(part: SceneBoard["parts"][number], pad: Pad, id: string, copper: readonly string[]): PadItem {
  const side = part.side === "bottom" ? "B.Cu" : "F.Cu";
  return {
    id,
    owner: part.ref,
    type: "pad",
    layers: pad.th ? copper : [side],
    bbox: box(pad.x - pad.w / 2, pad.y - pad.h / 2, pad.x + pad.w / 2, pad.y + pad.h / 2),
    hit: (p) => padHit(pad, p),
    at: [pad.x, pad.y],
    w: pad.w,
    h: pad.h,
    round: pad.round,
    th: pad.th,
    footprint: part.ref,
  };
}

function trackItems(t: Track): TrackItem[] {
  const out: TrackItem[] = [];
  const half = t.width / 2;
  const make = (suffix: number, start: Pt, end: Pt, arc?: ArcG): TrackItem => ({
    id: `${t.id}#${suffix}`,
    owner: t.id,
    type: "track",
    layers: [t.layer],
    bbox: arc ? box(Math.min(start[0], end[0], arc.c[0] - arc.r), Math.min(start[1], end[1], arc.c[1] - arc.r), Math.max(start[0], end[0], arc.c[0] + arc.r), Math.max(start[1], end[1], arc.c[1] + arc.r)) : box(start[0] - half, start[1] - half, end[0] + half, end[1] + half),
    geom: arc ?? seg(start, end),
    hit: arc ? (p) => distToArc(p, arc) <= half : (p) => distToSegment(p, start, end) <= half,
    start,
    end,
    center: [(start[0] + end[0]) / 2, (start[1] + end[1]) / 2],
    width: t.width,
    arc,
  });
  if (t.arc_mid && t.pts.length >= 2) {
    const start = t.pts[0]!;
    const end = t.pts[t.pts.length - 1]!;
    const arc = arcThrough(start, t.arc_mid, end);
    // An arc's box is its circle's box: a safe, slightly generous bound.
    if (arc) return [make(0, start, end, arc)];
  }
  for (let i = 0; i + 1 < t.pts.length; i++) {
    const a = t.pts[i]!;
    const b = t.pts[i + 1]!;
    const item = make(i, a, b);
    item.bbox = box(Math.min(a[0], b[0]) - half, Math.min(a[1], b[1]) - half, Math.max(a[0], b[0]) + half, Math.max(a[1], b[1]) + half);
    out.push(item);
  }
  return out;
}

/** Everything the snapping of a board looks at, in drawing order. */
export function buildScene(board: SceneBoard): SnapScene {
  const copper = COPPER(board.layers);
  const items: SnapItem[] = [];

  for (const part of board.parts) {
    if (!part.placed || !part.at) continue;
    const at: Pt = [part.at[0], part.at[1]];
    const side = part.side === "bottom" ? "B.Cu" : "F.Cu";
    const pads: PadItem[] = [];
    const counts = new Map<string, number>();
    for (const pad of part.pads ?? []) {
      const n = (counts.get(pad.num) ?? 0) + 1;
      counts.set(pad.num, n);
      pads.push(padItem(part, pad, n === 1 ? `${part.ref}.${pad.num}` : `${part.ref}.${pad.num}#${n}`, copper));
    }
    const fpBox = part.courtyard ? boxOf([part.courtyard[0], part.courtyard[1], part.courtyard[2], part.courtyard[3]]) : pads.length > 0 ? boxOf([Math.min(...pads.map((q) => q.bbox.x0)), Math.min(...pads.map((q) => q.bbox.y0)), Math.max(...pads.map((q) => q.bbox.x1)), Math.max(...pads.map((q) => q.bbox.y1))]) : box(at[0], at[1], at[0], at[1]);
    const fp: FootprintItem = {
      id: part.ref,
      owner: part.ref,
      type: "footprint",
      layers: [side],
      bbox: fpBox,
      hit: (p) => p[0] >= fpBox.x0 && p[0] <= fpBox.x1 && p[1] >= fpBox.y0 && p[1] <= fpBox.y1,
      at,
      center: [(fpBox.x0 + fpBox.x1) / 2, (fpBox.y0 + fpBox.y1) / 2],
      pads,
    };
    items.push(fp, ...pads);
  }

  for (const zone of board.routing?.zones ?? []) {
    const xs = zone.outline.map((p) => p[0]);
    const ys = zone.outline.map((p) => p[1]);
    if (zone.outline.length === 0) continue;
    items.push({
      id: zone.id,
      owner: zone.id,
      type: "zone",
      layers: [zone.layer],
      bbox: box(Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)),
      hit: (p) => distToPolyline(p, zone.outline, true) <= 0,
      outline: zone.outline,
    });
  }

  for (const t of board.routing?.tracks ?? []) items.push(...trackItems(t));

  for (const v of board.routing?.vias ?? []) {
    items.push({
      id: v.id,
      owner: v.id,
      type: "via",
      layers: copper,
      bbox: box(v.x - v.d / 2, v.y - v.d / 2, v.x + v.d / 2, v.y + v.d / 2),
      hit: (p) => dist(p, [v.x, v.y]) <= v.d / 2,
      at: [v.x, v.y],
      d: v.d,
    });
  }

  for (const s of board.drawings?.shapes ?? []) {
    const bb = shapeBoundingBox(s);
    const half = s.stroke_width / 2;
    items.push({ id: s.id, owner: s.id, type: "shape", layers: [s.layer], bbox: box(bb[0] - half, bb[1] - half, bb[2] + half, bb[3] + half), geom: shapeGeom(s), hit: shapeHit(s), shape: s });
  }

  for (const d of board.drawings?.dimensions ?? []) {
    const pts = d.lines.flatMap(([a, b]) => [a, b]);
    const all: Pt[] = pts.length > 0 ? pts : [d.start, d.end];
    const xs = all.map((p) => p[0]);
    const ys = all.map((p) => p[1]);
    items.push({
      id: d.id,
      owner: d.id,
      type: "dimension",
      layers: [d.layer],
      bbox: box(Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)),
      hit: (p) => d.lines.some(([a, b]) => distToSegment(p, a, b) <= d.stroke_width / 2),
      dim: d,
    });
  }

  for (const t of board.drawings?.texts ?? []) {
    const b = textBoundingBox(t);
    items.push({ id: t.id, owner: t.id, type: "text", layers: [t.layer], bbox: box(b.x0, b.y0, b.x1, b.y1), hit: (p) => p[0] >= b.x0 && p[0] <= b.x1 && p[1] >= b.y0 && p[1] <= b.y1, at: [t.x, t.y] });
  }

  return { items };
}

const cache = new WeakMap<object, SnapScene>();

/** The scene of a board, built once per board object (the studio replaces the whole object on every revision). */
export function sceneOf(board: SceneBoard | BoardState): SnapScene {
  const hit = cache.get(board);
  if (hit) return hit;
  const scene = buildScene(board as SceneBoard);
  cache.set(board, scene);
  return scene;
}

/**
 * `view->Query( aArea, items )`: the items whose box overlaps `area`, in scene order. `skip` holds item ids and owners to leave out (a skipped item takes its children
 * with it, as `skipItem` recurses: a footprint takes its pads, a track its segments).
 */
export function queryScene(scene: SnapScene, area: Box, skip?: ReadonlySet<string>): SnapItem[] {
  const out: SnapItem[] = [];
  for (const item of scene.items) {
    const b = item.bbox;
    if (b.x1 < area.x0 || b.x0 > area.x1 || b.y1 < area.y0 || b.y0 > area.y1) continue;
    if (skip && (skip.has(item.id) || skip.has(item.owner))) continue;
    out.push(item);
  }
  return out;
}
