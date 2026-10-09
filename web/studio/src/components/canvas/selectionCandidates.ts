// Wires kicad-port/selection.ts's pure GuessSelectionCandidates port to
// this app's actual board model: collects every selectable item under a
// click point (footprints via courtyard containment, everything else via
// itemHitTest.ts's distance tests), computes each one's "sloppiness" and
// approximate coverage area, and hands the pool to guessSelectionCandidates.
//
// This replaces Canvas.tsx's old two-tier "try a footprint, else try an
// item" priority with source's real model: every kind of item is a
// candidate at once, and GuessSelectionCandidates (or, failing that, the
// clarification menu) decides among them. A footprint still normally wins
// over a copper item it fully contains (its exact-hit slop is 0 same as a
// track right under the cursor, but its much larger area loses the
// size-ratio pass -- see pcb_selection_tool.cpp's own comment: "If the
// user clicked on a small item within a much larger one...").
import type { BoardState, Dimension, Pad, Part } from "../../api/types";
import { padIds } from "../../kicad-port/pcbItems";
import { distToPolyline, distToSegment, pointInPolygon, polygonArea, shapeArea, shapeBoundingBox, shapeHitDistance, textBoundingBox } from "./itemHitTest";
import { guessSelectionCandidates, type GuessCandidate } from "../../kicad-port/selection";
import { objectOn } from "../../kicad-port/appearance";
import { layerIsVisible, layerStateKey } from "../../kicad-port/layerPresets";

export type SelectableKind = "part" | "track" | "via" | "zone" | "shape" | "text" | "dimension" | "pad";

export interface SelectionCandidate extends GuessCandidate {
  kind: SelectableKind;
  id: string;
}

/**
 * pcbnew's real Selection Filter panel categories, narrowed to the kinds
 * this app actually has as distinct selectable items (no keepouts/points/
 * otherItems, which this app's model has no equivalent of). `footprints`/
 * `tracks`/`vias` predate this session; `zones`/`graphics`/`text` and
 * `dimensions` (task item 7) are new so every selectable kind has a real,
 * working toggle (panels/SelectionFilterPanel.tsx). `pads` (`m_filter.pads`):
 * a pad is an item of its own to click, highlight and read the properties
 * of -- the edit tools take its footprint (`FilterCollectorForFreePads`).
 */
export interface SelectionFilter {
  /** `PCB_SELECTION_FILTER_OPTIONS::lockedItems` ("Allow selection of locked items") -- default OFF, exactly like `m_filter.lockedItems = false` in `PCB_SELECTION_TOOL`'s constructor: a locked item (`L`, `set_locked`) can't be picked by a click/box until this is on. */
  lockedItems: boolean;
  footprints: boolean;
  tracks: boolean;
  vias: boolean;
  zones: boolean;
  graphics: boolean;
  text: boolean;
  dimensions: boolean;
  pads: boolean;
}

export const DEFAULT_SELECTION_FILTER: SelectionFilter = { lockedItems: false, footprints: true, tracks: true, vias: true, zones: true, graphics: true, text: true, dimensions: true, pads: true };

function filterAllows(filter: SelectionFilter, kind: SelectableKind): boolean {
  switch (kind) {
    case "part":
      return filter.footprints;
    case "track":
      return filter.tracks;
    case "via":
      return filter.vias;
    case "zone":
      return filter.zones;
    case "shape":
      return filter.graphics;
    case "text":
      return filter.text;
    case "dimension":
      return filter.dimensions;
    case "pad":
      return filter.pads;
  }
}

/** How far (um) `(x, y)` is outside a pad -- 0 or less on it: a round pad is a circle or a stadium, any other a box. */
export function padHitDistance(pad: Pad, x: number, y: number): number {
  const hw = pad.w / 2;
  const hh = pad.h / 2;
  if (pad.round) {
    // the two end circles of the stadium lie on the pad's long axis
    const r = Math.min(hw, hh);
    const [ax, ay, bx, by] = hw >= hh ? [pad.x - (hw - r), pad.y, pad.x + (hw - r), pad.y] : [pad.x, pad.y - (hh - r), pad.x, pad.y + (hh - r)];
    return distToSegment(x, y, ax, ay, bx, by) - r;
  }
  const dx = Math.max(Math.abs(x - pad.x) - hw, 0);
  const dy = Math.max(Math.abs(y - pad.y) - hh, 0);
  return dx === 0 && dy === 0 ? Math.max(Math.abs(x - pad.x) - hw, Math.abs(y - pad.y) - hh) : Math.hypot(dx, dy);
}

/**
 * pcb_selection_tool.cpp Selectable(): a hidden layer's items aren't
 * pickable, and in high-contrast mode an item not on the active layer
 * isn't either (`if (!onActiveLayer && type != MARKER) return false`).
 * Mirrors painter.ts's own `opts.layerVisible[item.layer]` lookup
 * exactly (including its real-name-vs-bucket-key quirk for non-copper
 * layers -- see that file) so "selectable" and "visible" never disagree.
 */
function layerSelectable(layer: string | null, layerVisible: Record<string, boolean>, activeLayer: string | null, highContrast: boolean): boolean {
  if (layer == null) return true;
  if (!layerIsVisible(layerVisible, layer)) return false;
  // `activeLayer` is a state key ("f_silks"), an item's layer a KiCad name ("F.SilkS"): compare as keys.
  if (highContrast && activeLayer != null && layerStateKey(layer) !== activeLayer) return false;
  return true;
}

/**
 * `PCB_SELECTION_TOOL::Selectable`'s object tests: tracks, vias, pads and zones that the Objects tab has switched off, or turned to an opacity of 0,
 * cannot be picked (kicad-port/appearance.ts carries that in `layerVisible`'s `obj:` keys). A footprint on a hidden side is out as well, with its pads.
 */
function objectSelectable(kind: SelectableKind, layerVisible: Record<string, boolean>): boolean {
  switch (kind) {
    case "track":
      return objectOn(layerVisible, "tracks");
    case "via":
      return objectOn(layerVisible, "vias");
    case "pad":
      return objectOn(layerVisible, "pads");
    case "zone":
      return objectOn(layerVisible, "zones");
    default:
      return true;
  }
}

const sideShown = (layerVisible: Record<string, boolean>, p: Pick<Part, "side">): boolean => objectOn(layerVisible, p.side === "bottom" ? "footprints_back" : "footprints_front");

/**
 * Every selectable item whose hit-test is within `toleranceUm` of
 * (xUm, yUm), with the slop/area/layer GuessSelectionCandidates needs.
 * `subtractiveOnly`, when set, is selectPoint's own pre-filter for the
 * subtractive (Ctrl/Cmd+Shift) modifier: "we only want items that are
 * selected" -- applied here rather than by the caller so every caller
 * gets it automatically.
 */
export function collectSelectionCandidates(
  board: BoardState,
  xUm: number,
  yUm: number,
  toleranceUm: number,
  filter: SelectionFilter,
  layerVisible: Record<string, boolean>,
  activeLayer: string | null,
  highContrast: boolean,
  selection: ReadonlySet<string>,
  subtractiveOnly: boolean
): SelectionCandidate[] {
  const out: SelectionCandidate[] = [];
  const locked = new Set(board.locked ?? []);
  const consider = (kind: SelectableKind, id: string, slopUm: number, areaUm2: number, layer: string | null, lockedId: string = id) => {
    if (slopUm > toleranceUm) return;
    if (!filterAllows(filter, kind) || !objectSelectable(kind, layerVisible)) return;
    // pcb_selection_tool.cpp itemPassesFilter: `!m_filter.lockedItems && aItem->IsLocked()` -> rejected.
    if (!filter.lockedItems && locked.has(lockedId)) return;
    if (!layerSelectable(layer, layerVisible, activeLayer, highContrast)) return;
    if (subtractiveOnly && !selection.has(id)) return;
    out.push({ kind, id, slopUm: Math.max(slopUm, 0), areaUm2: Math.max(areaUm2, 1), layer });
  };

  // Footprints: containment-only (courtyard rect), same as this app's
  // pre-existing partsAt -- always an exact (slop 0) hit when it matches
  // at all, same effect as pcb_selection_tool.cpp's footprint bounding
  // hull test for a normal rectangular courtyard. Layer-agnostic (a
  // footprint spans many layers at once), matching GuessCandidate's
  // documented `layer: null` convention.
  for (const p of board.parts as Part[]) {
    if (!p.placed || !p.courtyard || !sideShown(layerVisible, p)) continue;
    const [x0, y0, x1, y1] = p.courtyard;
    if (xUm < x0 || xUm > x1 || yUm < y0 || yUm > y1) continue;
    consider("part", p.ref, 0, Math.max((x1 - x0) * (y1 - y0), 1), null);
  }

  // Pads: `PAD::IsLocked()` is the footprint's. A pad is a copper item on its footprint's side (a through-hole one is on every layer); its area is the
  // pad's own, so a click on it picks it over the footprint it sits in -- "if the user clicked on a small item within a much larger one, they want the small item".
  for (const p of board.parts as Part[]) {
    if (!p.placed || !p.pads?.length || !sideShown(layerVisible, p)) continue;
    const ids = padIds(p);
    p.pads.forEach((pad, i) => {
      consider("pad", ids[i]!, padHitDistance(pad, xUm, yUm), Math.max(pad.w * pad.h, 1), pad.th ? null : p.side === "bottom" ? "B.Cu" : "F.Cu", p.ref);
    });
  }

  for (const t of board.routing?.tracks ?? []) {
    const d = distToPolyline(xUm, yUm, t.pts) - t.width / 2;
    const len = t.pts.reduce((acc, p, i) => (i === 0 ? 0 : acc + Math.hypot(p[0] - t.pts[i - 1]![0], p[1] - t.pts[i - 1]![1])), 0);
    consider("track", t.id, d, len * Math.max(t.width, 1), t.layer);
  }
  for (const v of board.routing?.vias ?? []) {
    const d = Math.hypot(xUm - v.x, yUm - v.y) - v.d / 2;
    // pcb_selection_tool.cpp: vias are artificially shrunk to their
    // drill radius squared (not pi*r^2) so they never out-rank a short
    // track segment passing under them.
    consider("via", v.id, d, Math.max((v.drill / 2) ** 2, 1), null);
  }
  for (const z of board.routing?.zones ?? []) {
    if (z.outline.length < 3) continue;
    const inside = pointInPolygon(xUm, yUm, z.outline);
    const edgeDist = distToPolyline(xUm, yUm, [...z.outline, z.outline[0]!]);
    // Zone borders are deliberately treated as "small" (source: "Zone
    // borders are very specific, so make them small") so clicking right
    // on the outline of a zone picks the outline, not the (usually much
    // bigger) fill.
    const nearEdge = edgeDist <= toleranceUm / 2;
    const d = inside ? 0 : edgeDist;
    const area = nearEdge ? 1 : polygonArea(z.outline);
    consider("zone", z.id, d, area, z.layer);
  }
  for (const s of board.drawings?.shapes ?? []) {
    // A filled shape at opacity 0 cannot be picked (`options.m_FilledShapeOpacity == 0.0 && IsAnyFill()`).
    if (s.kind !== "segment" && s.kind !== "arc" && s.kind !== "bezier" && s.filled && !objectOn(layerVisible, "shapes")) continue;
    consider("shape", s.id, shapeHitDistance(s, xUm, yUm), shapeArea(s), s.layer);
  }
  for (const t of board.drawings?.texts ?? []) {
    const { x0, y0, x1, y1 } = textBoundingBox(t);
    const inside = xUm >= x0 && xUm <= x1 && yUm >= y0 && yUm <= y1;
    const d = inside ? 0 : Math.hypot(xUm - t.x, yUm - t.y) - (x1 - x0) / 2;
    consider("text", t.id, d, Math.max((x1 - x0) * (y1 - y0), 1), t.layer);
  }
  // Task item 7: `lines` is a set of disconnected segments (extension
  // lines, crossbar/leader pieces, arrow barbs), not one polyline -- the
  // hit distance is the closest approach to any one of them.
  for (const dim of board.drawings?.dimensions ?? []) {
    const { d, box } = dimensionHit(dim, xUm, yUm);
    consider("dimension", dim.id, d, Math.max((box[2] - box[0]) * (box[3] - box[1]), 1), dim.layer);
  }

  return out;
}

/** Closest approach to any of a dimension's own line segments (or its
 * text anchor, treated as a point) and its overall bounding box -- shared
 * by the point hit-test above and `collectBoxSelection` below. */
function dimensionHit(dim: Dimension, xUm: number, yUm: number): { d: number; box: Box } {
  let d = Math.hypot(xUm - dim.text_at[0], yUm - dim.text_at[1]);
  let x0 = dim.text_at[0],
    y0 = dim.text_at[1],
    x1 = dim.text_at[0],
    y1 = dim.text_at[1];
  for (const [a, b] of dim.lines) {
    d = Math.min(d, distToSegment(xUm, yUm, a[0], a[1], b[0], b[1]));
    x0 = Math.min(x0, a[0], b[0]);
    y0 = Math.min(y0, a[1], b[1]);
    x1 = Math.max(x1, a[0], b[0]);
    y1 = Math.max(y1, a[1], b[1]);
  }
  return { d, box: [x0, y0, x1, y1] };
}

/**
 * The full selectPoint decision for a plain (non-drag) click: collect,
 * apply GuessSelectionCandidates (unless `skipHeuristics`, Alt held --
 * source skips straight to the disambiguation menu), return whatever's
 * left. 0 -> nothing hit; 1 -> apply the click modifier directly; >1 ->
 * the caller shows the clarification menu.
 */
export interface BoxSelectHit {
  kind: SelectableKind;
  id: string;
}

type Box = readonly [number, number, number, number];

function boxOf(pts: readonly (readonly [number, number])[]): Box {
  let x0 = Infinity,
    y0 = Infinity,
    x1 = -Infinity,
    y1 = -Infinity;
  for (const [x, y] of pts) {
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x);
    y1 = Math.max(y1, y);
  }
  return [x0, y0, x1, y1];
}

/** pcb_selection_tool.cpp SelectMultiple's `boxMode ? item->HitTest(selectionBox, containedMode)`: fully contained (window) or merely intersecting (crossing/touching). */
function boxMatches(itemBox: Box, selBox: Box, crossing: boolean): boolean {
  const [ix0, iy0, ix1, iy1] = itemBox;
  const [sx0, sy0, sx1, sy1] = selBox;
  return crossing ? ix0 <= sx1 && ix1 >= sx0 && iy0 <= sy1 && iy1 >= sy0 : ix0 >= sx0 && ix1 <= sx1 && iy0 >= sy0 && iy1 <= sy1;
}

/**
 * The box-select apply step (SelectRectArea/SelectMultiple), extended to
 * every selectable kind the Selection Filter allows -- this app's
 * pre-existing box select only ever considered footprints. No
 * GuessSelectionCandidates pass here: a box select has no "which one did
 * you mean" ambiguity, every matching item is taken (same as source).
 */
export function collectBoxSelection(board: BoardState, selBox: Box, crossing: boolean, filter: SelectionFilter, layerVisible: Record<string, boolean>, activeLayer: string | null, highContrast: boolean): BoxSelectHit[] {
  const out: BoxSelectHit[] = [];
  const padHits: BoxSelectHit[] = [];
  const locked = new Set(board.locked ?? []);
  const consider = (kind: SelectableKind, id: string, itemBox: Box, layer: string | null, lockedId: string = id) => {
    if (!filterAllows(filter, kind) || !objectSelectable(kind, layerVisible)) return;
    if (!filter.lockedItems && locked.has(lockedId)) return; // itemPassesFilter, as above

    if (!layerSelectable(layer, layerVisible, activeLayer, highContrast)) return;
    if (boxMatches(itemBox, selBox, crossing)) (kind === "pad" ? padHits : out).push({ kind, id });
  };

  for (const p of board.parts as Part[]) {
    if (!p.placed || !p.courtyard || !sideShown(layerVisible, p)) continue;
    consider("part", p.ref, p.courtyard, null);
  }
  for (const t of board.routing?.tracks ?? []) {
    const [x0, y0, x1, y1] = boxOf(t.pts);
    const half = t.width / 2;
    consider("track", t.id, [x0 - half, y0 - half, x1 + half, y1 + half], t.layer);
  }
  for (const v of board.routing?.vias ?? []) {
    const r = v.d / 2;
    consider("via", v.id, [v.x - r, v.y - r, v.x + r, v.y + r], null);
  }
  for (const z of board.routing?.zones ?? []) {
    if (z.outline.length < 3) continue;
    consider("zone", z.id, boxOf(z.outline), z.layer);
  }
  for (const s of board.drawings?.shapes ?? []) {
    if (s.kind !== "segment" && s.kind !== "arc" && s.kind !== "bezier" && s.filled && !objectOn(layerVisible, "shapes")) continue;
    consider("shape", s.id, shapeBoundingBox(s), s.layer);
  }
  for (const t of board.drawings?.texts ?? []) {
    const { x0, y0, x1, y1 } = textBoundingBox(t);
    consider("text", t.id, [x0, y0, x1, y1], t.layer);
  }
  for (const dim of board.drawings?.dimensions ?? []) {
    consider("dimension", dim.id, dimensionHit(dim, dim.text_at[0], dim.text_at[1]).box, dim.layer);
  }
  for (const p of board.parts as Part[]) {
    if (!p.placed || !p.pads?.length || !sideShown(layerVisible, p)) continue;
    const ids = padIds(p);
    p.pads.forEach((pad, i) => consider("pad", ids[i]!, [pad.x - pad.w / 2, pad.y - pad.h / 2, pad.x + pad.w / 2, pad.y + pad.h / 2], pad.th ? null : p.side === "bottom" ? "B.Cu" : "F.Cu", p.ref));
  }

  // `PCB_SELECTION_TOOL::SelectMultiple`: "If we selected nothing but pads, allow them to be selected" -- a box that takes a footprint (or anything else)
  // never takes the pads inside it as well.
  return out.length > 0 ? out : padHits;
}

export function pickSelectionCandidates(
  board: BoardState,
  xUm: number,
  yUm: number,
  toleranceUm: number,
  onePixelUm: number,
  filter: SelectionFilter,
  layerVisible: Record<string, boolean>,
  activeLayer: string | null,
  highContrast: boolean,
  selection: ReadonlySet<string>,
  subtractiveOnly: boolean,
  skipHeuristics: boolean
): SelectionCandidate[] {
  const all = collectSelectionCandidates(board, xUm, yUm, toleranceUm, filter, layerVisible, activeLayer, highContrast, selection, subtractiveOnly);
  if (skipHeuristics) return all;
  return guessSelectionCandidates(all, onePixelUm, activeLayer);
}
