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
import type { BoardState, Part } from "../../api/types";
import { distToPolyline, pointInPolygon, polygonArea, shapeArea, shapeBoundingBox, shapeHitDistance, textBoundingBox } from "./itemHitTest";
import { guessSelectionCandidates, type GuessCandidate } from "../../kicad-port/selection";

export type SelectableKind = "part" | "track" | "via" | "zone" | "shape" | "text";

export interface SelectionCandidate extends GuessCandidate {
  kind: SelectableKind;
  id: string;
}

/**
 * pcbnew's real Selection Filter panel categories, narrowed to the kinds
 * this app actually has as distinct selectable items (no separate "pads"
 * item -- a pad is only ever reached through its parent footprint here --
 * and no locked/keepouts/dimensions/points/otherItems, which this app's
 * model has no equivalent of). `footprints`/`tracks`/`vias` predate this
 * session; `zones`/`graphics`/`text` are new so every selectable kind has
 * a real, working toggle (panels/SelectionFilterPanel.tsx).
 */
export interface SelectionFilter {
  footprints: boolean;
  tracks: boolean;
  vias: boolean;
  zones: boolean;
  graphics: boolean;
  text: boolean;
}

export const DEFAULT_SELECTION_FILTER: SelectionFilter = { footprints: true, tracks: true, vias: true, zones: true, graphics: true, text: true };

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
  }
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
  if (layerVisible[layer] === false) return false;
  if (highContrast && activeLayer != null && layer !== activeLayer) return false;
  return true;
}

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
  const consider = (kind: SelectableKind, id: string, slopUm: number, areaUm2: number, layer: string | null) => {
    if (slopUm > toleranceUm) return;
    if (!filterAllows(filter, kind)) return;
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
    if (!p.placed || !p.courtyard) continue;
    const [x0, y0, x1, y1] = p.courtyard;
    if (xUm < x0 || xUm > x1 || yUm < y0 || yUm > y1) continue;
    consider("part", p.ref, 0, Math.max((x1 - x0) * (y1 - y0), 1), null);
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
    consider("shape", s.id, shapeHitDistance(s, xUm, yUm), shapeArea(s), s.layer);
  }
  for (const t of board.drawings?.texts ?? []) {
    const { x0, y0, x1, y1 } = textBoundingBox(t);
    const inside = xUm >= x0 && xUm <= x1 && yUm >= y0 && yUm <= y1;
    const d = inside ? 0 : Math.hypot(xUm - t.x, yUm - t.y) - (x1 - x0) / 2;
    consider("text", t.id, d, Math.max((x1 - x0) * (y1 - y0), 1), t.layer);
  }

  return out;
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
  const consider = (kind: SelectableKind, id: string, itemBox: Box, layer: string | null) => {
    if (!filterAllows(filter, kind)) return;
    if (!layerSelectable(layer, layerVisible, activeLayer, highContrast)) return;
    if (boxMatches(itemBox, selBox, crossing)) out.push({ kind, id });
  };

  for (const p of board.parts as Part[]) {
    if (!p.placed || !p.courtyard) continue;
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
    consider("shape", s.id, shapeBoundingBox(s), s.layer);
  }
  for (const t of board.drawings?.texts ?? []) {
    const { x0, y0, x1, y1 } = textBoundingBox(t);
    consider("text", t.id, [x0, y0, x1, y1], t.layer);
  }

  return out;
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
