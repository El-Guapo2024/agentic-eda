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
import { stitchFilteredAsVia } from "../../kicad-port/viaStitch";
import type { BoardState, Dimension, FieldInfo, Part } from "../../api/types";
import { fpTextById, padIds, padParent } from "../../kicad-port/pcbItems";
import { ancestors } from "../../kicad-port/groupTree";
import { artBounds, artHitDistance, artTextAsField, artTextShown, unionBox } from "../../kicad-port/fpArt";
import { fpItemsHitDistance } from "../../kicad-port/fpBoardItems";
import { fieldAsText, fieldShownByObjects } from "../../kicad-port/fpFields";
import { distToPolyline, distToSegment, shapeBoundingBox, shapeHitDistance, textBoundingBox, zoneArea, zoneContainsPoint, zoneEdgeDistance } from "./itemHitTest";
import { guessSelectionCandidates, type GuessCandidate } from "../../kicad-port/selection";
import { objectOn } from "../../kicad-port/appearance";
import { isAnyFill } from "../../kicad-port/shapeFill";
import { layerIsVisible, layerStateKey } from "../../kicad-port/layerPresets";
import { zonePickLayer } from "../../kicad-port/zoneLayers";
import { extraHitDistance, extraItems, type ExtraKind } from "../../kicad-port/pcbExtraItems";
import { gridArea } from "../../kicad-port/gridItem";
import { stitchArea } from "../../kicad-port/viaStitch";
import { cellArea, cellHitDistance, isSelectableCell } from "../../kicad-port/tables";
import { circleRing, coverageRatio, ringArea, type Ring } from "../../kicad-port/coverage";
import { boxRing, footprintHull, shapeCoverageArea, shapeRings, trackRings } from "../../kicad-port/footprintCoverage";
import { fieldStrokes, textBoxDistance, textHitSlop } from "../../kicad-port/textHit";
import { ARC_LOW_DEF_NM } from "../../kicad-port/coverage";
import { boardTextAsField, strokeRings, strokesBoxRing, textCoverageArea } from "../../kicad-port/textCoverage";
import { padCopperRings, padCoverageArea, padHitDistance as padShapeDistance, type PadLike } from "../../kicad-port/padHit";
import { viaMaxDiameter } from "../../kicad-port/viaPadstack";

export type SelectableKind = "part" | "track" | "via" | "zone" | "shape" | "text" | "dimension" | "pad" | "field" | "fptext" | "table" | "tablecell" | ExtraKind;

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
  /** `m_filter.points` (`PCB_POINT`) and `m_filter.otherItems` (a barcode). */
  points: boolean;
  otherItems: boolean;
  /** KiCad 10's `m_filter.gridItems` (Grid Items); absent is on. */
  gridItems?: boolean;
  /** `m_filter.keepouts` ("Rule Areas"): a zone that is a rule area is gated by this, not by `zones`; absent is on. */
  keepouts?: boolean;
}

export const DEFAULT_SELECTION_FILTER: SelectionFilter = { lockedItems: false, footprints: true, tracks: true, vias: true, zones: true, keepouts: true, graphics: true, text: true, dimensions: true, pads: true, points: true, otherItems: true, gridItems: true };

/** `itemPassesFilter`'s zone case: `( !m_filter.zones && !zone->GetIsRuleArea() ) || ( !m_filter.keepouts && zone->GetIsRuleArea() )` rejects. */
function zoneAllowed(filter: SelectionFilter, ruleArea: boolean): boolean {
  return ruleArea ? filter.keepouts !== false : filter.zones;
}

/**
 * `BOARD_ITEM::IsLocked()` as `itemPassesFilter` asks it: the item's own lock (a pad's and a field's is its footprint's, `lockedId`), or that of a group
 * above it (`if( group->AsEdaItem()->IsLocked() ) return true;`).
 */
function lockedForFilter(board: BoardState, locked: ReadonlySet<string>, lockedId: string): boolean {
  if (locked.has(lockedId)) return true;
  const groups = board.drawings?.groups ?? [];
  if (groups.length === 0 || locked.size === 0) return false;
  return ancestors(groups, lockedId).some((g) => locked.has(g.id));
}

/**
 * `PCB_SELECTION_TOOL::itemPassesFilter( aItem, aMultiSelect = true )` for what Select Connection walks (UI-LEFT P22): the item's row of the filter, and a locked
 * item only with "Locked items" on -- a pad's lock is its footprint's, and with `aMultiSelect` a pad of a locked footprint is locked too.
 */
export function itemPassesFilter(board: BoardState, filter: SelectionFilter, kind: SelectableKind, id: string): boolean {
  if (!filterAllows(filter, filterKindOf(board, kind, id))) return false;
  if (filter.lockedItems) return true;
  const lockedId = kind === "pad" ? (padParent(board, id) ?? id) : id;
  return !lockedForFilter(board, new Set(board.locked ?? []), lockedId);
}

/** The kind `itemPassesFilter` filters an item as (kicad-port/viaStitch.ts `stitchFilteredAsVia`: a via stitching with vias follows the Vias row). */
function filterKindOf(board: BoardState, kind: SelectableKind, id: string): SelectableKind {
  return kind === "via_stitch" && stitchFilteredAsVia(board, id) ? "via" : kind;
}

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
    // `PCB_SELECTION_TOOL::itemPassesFilter`: a footprint's field is text (`PCB_FIELD_T` shares `m_filter.text`), and so are a table and its cells.
    case "field":
    case "fptext":
    case "table":
    case "tablecell":
      return filter.text;
    // `PCB_TEXTBOX_T` shares `m_filter.text`, `PCB_REFERENCE_IMAGE_T` `m_filter.graphics`, `PCB_POINT_T` has `m_filter.points` and `PCB_BARCODE_T` falls to `m_filter.otherItems`.
    case "textbox":
      return filter.text;
    case "image":
      return filter.graphics;
    case "point":
      return filter.points;
    case "barcode":
      return filter.otherItems;
    // `PCB_GRID_ITEM_T` has `m_filter.gridItems` (KiCad 10).
    case "grid":
      return filter.gridItems !== false;
    // A generator (`PCB_GENERATOR_T`) has no case of its own: `m_filter.otherItems`; nor has KiCad 10's drill map (`PCB_DRILL_MAP_T`).
    case "via_stitch":
    case "drill_map":
      return filter.otherItems;
  }
}

/** What the filter did to a click's items (`selectPoint`'s `preFilterCount`, `rejected`): the rows that refused one, and how many it let through. */
export interface FilterRejections {
  rows: Set<keyof SelectionFilter>;
  passed: number;
}

/** A fresh record for `collectSelectionCandidates`' `rejected`. */
export function newFilterRejections(): FilterRejections {
  return { rows: new Set(), passed: 0 };
}

/** `if( collector.GetCount() == 0 && preFilterCount > 0 ) HighlightSelectionFilter( rejected )`: the rows to flash, none when an item got through. */
export function rowsToFlash(r: FilterRejections): (keyof SelectionFilter)[] {
  return r.passed === 0 ? [...r.rows] : [];
}

/**
 * The row of the filter that refuses an item of `kind` (`itemPassesFilter`'s `aRejected`), for the flash of that row (`HighlightSelectionFilter`). A text, a field,
 * a text box, a table and a cell set none: their case is `if( !m_filter.text ) return false;` with no `aRejected->text`, so the Text row never flashes.
 */
export function rejectedRowOf(kind: SelectableKind, ruleArea = false): keyof SelectionFilter | null {
  switch (kind) {
    case "part":
      return "footprints";
    case "track":
      return "tracks";
    case "via":
      return "vias";
    case "pad":
      return "pads";
    case "zone":
      return ruleArea ? "keepouts" : "zones";
    case "shape":
    case "image":
      return "graphics";
    case "dimension":
      return "dimensions";
    case "point":
      return "points";
    case "grid":
      return "gridItems";
    case "barcode":
    case "via_stitch":
    case "drill_map":
      return "otherItems";
    default:
      return null;
  }
}

/** The box around a field's text, turned by its angle (`EDA_TEXT::GetTextBox` and the rotation): the same rough per-character width `textBoundingBox` uses. */
export function textBoundingBoxRotated(f: FieldInfo): { x0: number; y0: number; x1: number; y1: number } {
  const t = fieldAsText(f);
  const b = textBoundingBox(t);
  const rad = (t.angle / 1000) * (Math.PI / 180);
  if (rad === 0) return b;
  const [cos, sin] = [Math.cos(rad), Math.sin(rad)];
  const xs: number[] = [];
  const ys: number[] = [];
  for (const [x, y] of [
    [b.x0, b.y0],
    [b.x1, b.y0],
    [b.x1, b.y1],
    [b.x0, b.y1],
  ] as const) {
    const dx = x - t.x;
    const dy = y - t.y;
    xs.push(t.x + dx * cos + dy * sin);
    ys.push(t.y - dx * sin + dy * cos);
  }
  return { x0: Math.min(...xs), y0: Math.min(...ys), x1: Math.max(...xs), y1: Math.max(...ys) };
}

/**
 * How far `(x, y)` is outside a pad -- 0 or less on it: `PAD::HitTest`'s effective shape of each of its layers and its hole (kicad-port/padHit.ts): a circle, an
 * oval's stadium, a turned rectangle, a rounded rectangle by its corner arcs, a trapezoid, a chamfered rectangle or a custom pad by its copper's contours
 * (UI-LEFT-2 PP2: a rounded, chamfered or custom pad was its rectangle). A pad the state gives no size (an older backend) is its box.
 */
export function padHitDistance(pad: PadLike, x: number, y: number): number {
  return padShapeDistance(pad, x, y);
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
    // `PCB_SELECTION_TOOL::Selectable`: a grid item while Grids (`LAYER_SUBGRIDS`) is shown.
    case "grid":
      return objectOn(layerVisible, "subgrids");
    // A via stitching is drawn on `LAYER_VIA_STITCHING` (Via Stitching).
    case "via_stitch":
      return objectOn(layerVisible, "via_stitching");
    default:
      return true;
  }
}

const sideShown = (layerVisible: Record<string, boolean>, p: Pick<Part, "side">): boolean => objectOn(layerVisible, p.side === "bottom" ? "footprints_back" : "footprints_front");

/**
 * Every selectable item whose hit-test is within `toleranceNm` of
 * (xNm, yNm), with the slop/area/layer GuessSelectionCandidates needs.
 * `subtractiveOnly`, when set, is selectPoint's own pre-filter for the
 * subtractive (Ctrl/Cmd+Shift) modifier: "we only want items that are
 * selected" -- applied here rather than by the caller so every caller
 * gets it automatically.
 *
 * With `onePixelNm`, a field, a footprint's text or a board text is collected by its box, as the collector takes it, but measured by its strokes for the
 * heuristics (`hitTestDistance`, kicad-port/textHit.ts): a click in its box off its strokes is no hit of it when anything else is hit exactly; its area is
 * that of its strokes grown by the collector's accuracy (`GetCoverageArea`'s `TransformTextToPolySet( poly, textMargin, .. )`, UI-LEFT-2 PP2). Without it,
 * by its box.
 *
 * `rejected`, when given, gets the rows of the filter that refused an item that could be picked otherwise and the count it let through (`selectPoint`:
 * `Selectable` first, then `FilterCollectedItems( collector, false, &rejected )`): the caller flashes the rows when nothing got through (`rowsToFlash`).
 */
export function collectSelectionCandidates(
  board: BoardState,
  xNm: number,
  yNm: number,
  toleranceNm: number,
  filter: SelectionFilter,
  layerVisible: Record<string, boolean>,
  activeLayer: string | null,
  highContrast: boolean,
  selection: ReadonlySet<string>,
  subtractiveOnly: boolean,
  onePixelNm?: number,
  rejected?: FilterRejections
): SelectionCandidate[] {
  const out: SelectionCandidate[] = [];
  const locked = new Set(board.locked ?? []);
  // `textMargin = aCollector.GetGuide()->Accuracy()`: `KiROUND( 5 * OnePixelInIU() )`
  const accuracyNm = onePixelNm !== undefined && onePixelNm > 0 ? Math.round(5 * onePixelNm) : 0;
  // `guessSlopNm`: the slop the heuristics weigh when it is not the one the item is collected by (a text's strokes against its box)
  const consider = (kind: SelectableKind, id: string, slopNm: number, areaNm2: number, layer: string | null, lockedId: string = id, ruleArea = false, guessSlopNm?: number) => {
    if (slopNm > toleranceNm) return;
    // `Selectable` before the filter (`selectPoint`: "Remove unselectable items", then `FilterCollectedItems`)
    if (!objectSelectable(kind, layerVisible) || !layerSelectable(layer, layerVisible, activeLayer, highContrast)) return;
    // pcb_selection_tool.cpp itemPassesFilter: the lock first -- `!m_filter.lockedItems && aItem->IsLocked()` -> rejected (`aRejected->lockedItems`) -- but a
    // click may pick a lone pad of a locked footprint ("allow a single pad to be selected -- there are a lot of operations that require this").
    if (!filter.lockedItems && kind !== "pad" && lockedForFilter(board, locked, lockedId)) {
      rejected?.rows.add("lockedItems");
      return;
    }
    const filterKind = kind === "zone" ? kind : filterKindOf(board, kind, id);
    if (!(kind === "zone" ? zoneAllowed(filter, ruleArea) : filterAllows(filter, filterKind))) {
      const row = rejectedRowOf(filterKind, ruleArea);
      if (row) rejected?.rows.add(row);
      return;
    }
    if (rejected) rejected.passed++;
    if (subtractiveOnly && !selection.has(id)) return;
    out.push({ kind, id, slopNm: guessSlopNm ?? Math.max(slopNm, 0), areaNm2: Math.max(areaNm2, 1), layer });
  };
  // A field, a footprint's text or a board text: collected by the box of its strokes (`TextHitTest`), weighed by its strokes (`hitTestDistance`), its area
  // that of its strokes grown by the accuracy (`GetCoverageArea`); by the rough box without `onePixelNm`.
  const considerText = (kind: "field" | "fptext" | "text", f: FieldInfo, id: string, lockedId: string) => {
    if (onePixelNm !== undefined && onePixelNm > 0) {
      // nowhere near it (no glyph advances more than 1.72 of the size, a line is 1.6 sizes below the last): no need of its strokes
      const lines = (f.shown ?? f.text).split("\n");
      const reach = (Math.max(...lines.map((l) => l.length)) + 1) * 2 * f.h + lines.length * 1.6 * f.h + toleranceNm;
      if (Math.abs(xNm - f.x) > reach || Math.abs(yNm - f.y) > reach) return;
      const s = fieldStrokes(f);
      const d = textBoxDistance(s, xNm, yNm);
      if (d <= toleranceNm) consider(kind, id, d, Math.max(textCoverageArea(s, accuracyNm), 1), f.layer, lockedId, false, textHitSlop(s, xNm, yNm, onePixelNm));
      return;
    }
    const { x0, y0, x1, y1 } = textBoundingBoxRotated(f);
    const inside = xNm >= x0 && xNm <= x1 && yNm >= y0 && yNm <= y1;
    const d = inside ? 0 : Math.hypot(xNm - (x0 + x1) / 2, yNm - (y0 + y1) / 2) - Math.min(x1 - x0, y1 - y0) / 2;
    consider(kind, id, d, Math.max((x1 - x0) * (y1 - y0), 1), f.layer, lockedId);
  };

  // Footprints: containment-only (courtyard rect), same as this app's
  // pre-existing partsAt -- always an exact (slop 0) hit when it matches
  // at all, same effect as pcb_selection_tool.cpp's footprint bounding
  // hull test for a normal rectangular courtyard. Layer-agnostic (a
  // footprint spans many layers at once), matching GuessCandidate's
  // documented `layer: null` convention.
  for (const p of board.parts as Part[]) {
    const hit = partHit(p, xNm, yNm, layerVisible);
    if (!hit || hit.slopNm > toleranceNm) continue;
    consider("part", p.ref, hit.slopNm, hit.areaNm2, null);
  }

  // Pads: `PAD::IsLocked()` is the footprint's. A pad is a copper item on its footprint's side (a through-hole one is on every layer); its area is the
  // pad's own, so a click on it picks it over the footprint it sits in -- "if the user clicked on a small item within a much larger one, they want the small item".
  for (const p of board.parts as Part[]) {
    if (!p.placed || !p.pads?.length || !sideShown(layerVisible, p)) continue;
    const ids = padIds(p);
    p.pads.forEach((pad, i) => {
      // `GetCoverageArea` of a pad: the area of its copper (it was its board box)
      const d = padHitDistance(pad, xNm, yNm);
      if (d <= toleranceNm) consider("pad", ids[i]!, d, Math.max(padCoverageArea(pad), 1), pad.th ? null : p.side === "bottom" ? "B.Cu" : "F.Cu", p.ref);
    });
  }

  // Fields: a footprint's Reference, Value and user fields are items of their own. A hidden one cannot be picked, and one of a locked footprint is locked with it.
  for (const p of board.parts as Part[]) {
    if (!p.placed || !p.fields?.length) continue;
    for (const f of p.fields) {
      if (!f.visible || !f.text || !fieldShownByObjects(layerVisible, p, f)) continue;
      considerText("field", f, f.id, p.ref);
    }
  }
  // A footprint's own texts (UI-LEFT P15): `PCB_SELECTION_TOOL::Selectable` lets a footprint's `PCB_TEXT` be picked on the board ("Most footprint children can only be
  // selected in the footprint editor": its other graphics are picked as the footprint), shown as `artTextShown` says, locked with its footprint.
  for (const p of board.parts as Part[]) {
    if (!p.placed || !p.texts?.length) continue;
    for (const t of p.texts) {
      if (!t.text || !artTextShown(layerVisible, p, t)) continue;
      considerText("fptext", artTextAsField(t), t.id, p.ref);
    }
  }
  for (const t of board.routing?.tracks ?? []) {
    const d = distToPolyline(xNm, yNm, t.pts) - t.width / 2;
    // `GetCoverageArea` of a `PCB_TRACE_T` / `PCB_ARC_T`: `width * width`, however long the track (UI-LEFT P22; it was its length times its width)
    consider("track", t.id, d, Math.max(t.width * t.width, 1), t.layer);
  }
  for (const v of board.routing?.vias ?? []) {
    // `PCB_VIA::HitTest`: within any of its layers' diameters (`ForEachUniqueLayer`), the widest
    const d = Math.hypot(xNm - v.x, yNm - v.y) - viaMaxDiameter(v) / 2;
    // pcb_selection_tool.cpp: vias are artificially shrunk to their
    // drill radius squared (not pi*r^2) so they never out-rank a short
    // track segment passing under them.
    consider("via", v.id, d, Math.max((v.drill / 2) ** 2, 1), null);
  }
  for (const z of board.routing?.zones ?? []) {
    // "Most footprint children can only be selected in the footprint editor" (`PCB_SELECTION_TOOL::Selectable`): a footprint's zone is not picked on the board (UI-LEFT PP14)
    if (z.outline.length < 3 || z.parent) continue;
    // `PCB_COLLECTOR::Inspect`: a zone is hit on a side of its outline or of one of its holes (`HitTestForCorner( accuracy * 2 ) || HitTestForEdge( accuracy )`,
    // every contour), or inside its area -- inside the outline and in none of its holes (`zoneContainsPoint`, standing in for `HitTestFilledArea`, which
    // reads the fill; a rule area's is `m_Poly->Contains`). A click deep in a hole is outside the zone: its slop is the distance to the hole's side.
    const inside = zoneContainsPoint(z, xNm, yNm);
    const edgeDist = zoneEdgeDistance(z, xNm, yNm);
    // Zone borders are deliberately treated as "small" (source: "Zone
    // borders are very specific, so make them small") so clicking right
    // on the outline of a zone picks the outline, not the (usually much
    // bigger) fill.
    const nearEdge = edgeDist <= toleranceNm / 2;
    const d = inside ? 0 : edgeDist;
    const area = nearEdge ? 1 : zoneArea(z);
    consider("zone", z.id, d, area, zonePickLayer(z, layerVisible, activeLayer), z.id, !!z.is_rule_area);
  }
  for (const s of board.drawings?.shapes ?? []) {
    // A filled or hatched shape at opacity 0 cannot be picked (`options.m_FilledShapeOpacity == 0.0 && IsAnyFill()`).
    if (s.kind !== "segment" && s.kind !== "arc" && s.kind !== "bezier" && isAnyFill(s) && !objectOn(layerVisible, "shapes")) continue;
    // `GetCoverageArea` of a `PCB_SHAPE_T`: a line, an arc, a curve or an unfilled outline is `width * width`, a filled one its area (UI-LEFT P22)
    consider("shape", s.id, shapeHitDistance(s, xNm, yNm), shapeCoverageArea(s, isAnyFill(s)), s.layer);
  }
  for (const t of board.drawings?.texts ?? []) {
    // A board text as a footprint's (UI-LEFT-2 PP2: it was hit and weighed by its box): `PCB_TEXT::HitTest` is `TextHitTest`, `hitTestDistance` its strokes.
    if (onePixelNm !== undefined && onePixelNm > 0) {
      considerText("text", boardTextAsField(t), t.id, t.id);
      continue;
    }
    const { x0, y0, x1, y1 } = textBoundingBox(t);
    const inside = xNm >= x0 && xNm <= x1 && yNm >= y0 && yNm <= y1;
    const d = inside ? 0 : Math.hypot(xNm - t.x, yNm - t.y) - (x1 - x0) / 2;
    consider("text", t.id, d, Math.max((x1 - x0) * (y1 - y0), 1), t.layer);
  }
  // The text boxes, images, points and barcodes (GAPS.md item 24): a text box is hit anywhere in its bounding box (`PCB_TEXTBOX::HitTest`).
  for (const item of extraItems(board)) {
    const [x0, y0, x1, y1] = item.bounds;
    // A grid item is on no layer, and its area is the one it covers (`GetCoverageArea`).
    if (item.grid) consider(item.kind, item.id, extraHitDistance(item, xNm, yNm), Math.max(gridArea(item.grid), 1), null);
    else if (item.stitch) consider(item.kind, item.id, extraHitDistance(item, xNm, yNm), Math.max(stitchArea(item.stitch), 1), null);
    else consider(item.kind, item.id, extraHitDistance(item, xNm, yNm), Math.max((x1 - x0) * (y1 - y0), 1), item.layer);
  }
  // `PCB_TABLE`: a click finds a cell, not the table ("Tables should defer to their table cells"); a cell another one covers is not an item. The table is locked as a whole.
  for (const tb of board.drawings?.tables ?? []) {
    // KiCad 10: "A generated table says what the board says, so a click on it is a click on the table" -- a drill chart is picked whole.
    if (board.drawings?.drill_charts?.some((c) => c.table_id === tb.id)) {
      const cells = tb.cells.filter(isSelectableCell);
      if (cells.length > 0) consider("table", tb.id, Math.min(...cells.map((c) => cellHitDistance(c, xNm, yNm))), cells.reduce((a, c) => a + cellArea(c), 0), tb.layer);
      continue;
    }
    for (const c of tb.cells) {
      if (!isSelectableCell(c)) continue;
      consider("tablecell", c.id, cellHitDistance(c, xNm, yNm), cellArea(c), tb.layer, tb.id);
    }
  }
  // Task item 7: `lines` is a set of disconnected segments (extension
  // lines, crossbar/leader pieces, arrow barbs), not one polyline -- the
  // hit distance is the closest approach to any one of them.
  for (const dim of board.drawings?.dimensions ?? []) {
    const { d, box } = dimensionHit(dim, xNm, yNm);
    consider("dimension", dim.id, d, Math.max((box[2] - box[0]) * (box[3] - box[1]), 1), dim.layer);
  }

  return out;
}

/**
 * A footprint's hit at (xNm, yNm), containment-only (courtyard rect) as `collectSelectionCandidates` takes it: inside, an exact hit; outside, the distance to
 * the nearest stroke of its own silkscreen or fab drawing (`FOOTPRINT::HitTest` counts its graphics); its area that of its bounding hull
 * (`FOOTPRINT::GetCoverageArea`, UI-LEFT P22). Null for a footprint that is not placed, has no courtyard or is on a side not shown.
 */
function partHit(p: Part, xNm: number, yNm: number, layerVisible: Record<string, boolean>): { slopNm: number; areaNm2: number } | null {
  if (!p.placed || !p.courtyard || !sideShown(layerVisible, p)) return null;
  const [x0, y0, x1, y1] = p.courtyard;
  const inside = xNm >= x0 && xNm <= x1 && yNm >= y0 && yNm <= y1;
  const slop = inside ? 0 : Math.min(artHitDistance(p, xNm, yNm, layerVisible), fpItemsHitDistance(p, xNm, yNm, layerVisible));
  return { slopNm: Math.max(slop, 0), areaNm2: Math.max(ringArea(partHull(p)), 1) };
}

/** The footprint a pad, a field or a footprint's own text belongs to (`GetParentFootprint()`); null for an item of the board. */
function parentFootprintOf(board: BoardState, c: Pick<SelectionCandidate, "kind" | "id">): string | null {
  switch (c.kind) {
    case "pad":
      return padParent(board, c.id);
    case "fptext":
      return fpTextById(board, c.id)?.part.ref ?? null;
    case "field":
      return board.parts.find((p) => p.fields?.some((f) => f.id === c.id))?.ref ?? null;
    default:
      return null;
  }
}

/**
 * The edit tools' client filter on the items under the cursor (`EDIT_TOOL::Drag`: `FilterCollectorForFreePads( aCollector )` then
 * `FilterCollectorForHierarchy( aCollector, true )`; Move, Rotate, Flip, Properties and the rest have the same two): a pad stands for its footprint
 * (`FilterCollectorForFreePads`: "if( !aCollector.HasItem( item->GetParent() ) ) to_add.insert( item->GetParent() ); aCollector.Remove( item )"), and a
 * field, a text or a pad of a footprint that is itself under the cursor goes ("Remove children of selected items": with `aMultiselect` every item of the
 * collector is a CANDIDATE, so a child of one is dropped). Groups are left as they are here: the callers take a group's member as the member.
 */
export function applyToolFilter(board: BoardState, candidates: readonly SelectionCandidate[], xNm: number, yNm: number, layerVisible: Record<string, boolean>): SelectionCandidate[] {
  const out: SelectionCandidate[] = [];
  const parts = new Set(candidates.filter((c) => c.kind === "part").map((c) => c.id));
  for (const c of candidates) {
    if (c.kind !== "pad") {
      out.push(c);
      continue;
    }
    const ref = padParent(board, c.id);
    if (!ref || parts.has(ref)) continue;
    const p = board.parts.find((q) => q.ref === ref);
    const hit = p ? partHit(p, xNm, yNm, layerVisible) : null;
    // the footprint of a pad on a side not shown, or of one without a courtyard, still stands for the pad: it is what the tool works on
    out.push({ kind: "part", id: ref, slopNm: hit?.slopNm ?? c.slopNm, areaNm2: hit?.areaNm2 ?? c.areaNm2, layer: null });
    parts.add(ref);
  }
  return out.filter((c) => {
    const parent = parentFootprintOf(board, c);
    return parent === null || !parts.has(parent);
  });
}

/** Closest approach to any of a dimension's own line segments (or its
 * text anchor, treated as a point) and its overall bounding box -- shared
 * by the point hit-test above and `collectBoxSelection` below. */
function dimensionHit(dim: Dimension, xNm: number, yNm: number): { d: number; box: Box } {
  let d = Math.hypot(xNm - dim.text_at[0], yNm - dim.text_at[1]);
  let x0 = dim.text_at[0],
    y0 = dim.text_at[1],
    x1 = dim.text_at[0],
    y1 = dim.text_at[1];
  for (const [a, b] of dim.lines) {
    d = Math.min(d, distToSegment(xNm, yNm, a[0], a[1], b[0], b[1]));
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
  const consider = (kind: SelectableKind, id: string, itemBox: Box, layer: string | null, lockedId: string = id, ruleArea = false) => {
    if (!(kind === "zone" ? zoneAllowed(filter, ruleArea) : filterAllows(filter, filterKindOf(board, kind, id))) || !objectSelectable(kind, layerVisible)) return;
    if (!filter.lockedItems && lockedForFilter(board, locked, lockedId)) return; // itemPassesFilter, as above (a box is a multi-select: no lone-pad exception)

    if (!layerSelectable(layer, layerVisible, activeLayer, highContrast)) return;
    if (boxMatches(itemBox, selBox, crossing)) (kind === "pad" ? padHits : out).push({ kind, id });
  };

  for (const p of board.parts as Part[]) {
    if (!p.placed || !p.courtyard || !sideShown(layerVisible, p)) continue;
    // The footprint's box holds its graphics (`FOOTPRINT::GetBoundingBox( false )`).
    consider("part", p.ref, unionBox(p.courtyard, artBounds(p)) ?? p.courtyard, null);
  }
  for (const t of board.routing?.tracks ?? []) {
    const [x0, y0, x1, y1] = boxOf(t.pts);
    const half = t.width / 2;
    consider("track", t.id, [x0 - half, y0 - half, x1 + half, y1 + half], t.layer);
  }
  for (const v of board.routing?.vias ?? []) {
    const r = viaMaxDiameter(v) / 2;
    consider("via", v.id, [v.x - r, v.y - r, v.x + r, v.y + r], null);
  }
  for (const z of board.routing?.zones ?? []) {
    if (z.outline.length < 3 || z.parent) continue;
    consider("zone", z.id, boxOf(z.outline), zonePickLayer(z, layerVisible, activeLayer), z.id, !!z.is_rule_area);
  }
  for (const s of board.drawings?.shapes ?? []) {
    if (s.kind !== "segment" && s.kind !== "arc" && s.kind !== "bezier" && isAnyFill(s) && !objectOn(layerVisible, "shapes")) continue;
    consider("shape", s.id, shapeBoundingBox(s), s.layer);
  }
  for (const t of board.drawings?.texts ?? []) {
    const { x0, y0, x1, y1 } = textBoundingBox(t);
    consider("text", t.id, [x0, y0, x1, y1], t.layer);
  }
  for (const dim of board.drawings?.dimensions ?? []) {
    consider("dimension", dim.id, dimensionHit(dim, dim.text_at[0], dim.text_at[1]).box, dim.layer);
  }
  for (const item of extraItems(board)) consider(item.kind, item.id, item.bounds, item.grid || item.stitch ? null : item.layer);
  // A window takes tables, whole (`PCB_TABLE::HitTest( aRect, aContained, aAccuracy )` on the box round the first and the last cell), not their cells.
  for (const tb of board.drawings?.tables ?? []) {
    consider("table", tb.id, tb.bbox, tb.layer);
  }
  for (const p of board.parts as Part[]) {
    if (!p.placed || !p.pads?.length || !sideShown(layerVisible, p)) continue;
    const ids = padIds(p);
    p.pads.forEach((pad, i) => consider("pad", ids[i]!, [pad.x - pad.w / 2, pad.y - pad.h / 2, pad.x + pad.w / 2, pad.y + pad.h / 2], pad.th ? null : p.side === "bottom" ? "B.Cu" : "F.Cu", p.ref));
  }
  // A field is taken by a box that holds it; its footprint taking it too (the footprint carries its fields) leaves it out (`FilterCollectorForHierarchy`).
  const fieldHits: BoxSelectHit[] = [];
  for (const p of board.parts as Part[]) {
    if (!p.placed || !p.fields?.length) continue;
    for (const f of p.fields) {
      if (!f.visible || !f.text || !fieldShownByObjects(layerVisible, p, f)) continue;
      if (!filterAllows(filter, "field") || (!filter.lockedItems && locked.has(p.ref))) continue;
      if (!layerSelectable(f.layer, layerVisible, activeLayer, highContrast)) continue;
      const { x0, y0, x1, y1 } = textBoundingBoxRotated(f);
      if (boxMatches([x0, y0, x1, y1], selBox, crossing)) fieldHits.push({ kind: "field", id: f.id });
    }
  }
  // and a footprint's own text likewise (`REF~t<k>`)
  for (const p of board.parts as Part[]) {
    if (!p.placed || !p.texts?.length) continue;
    for (const t of p.texts) {
      if (!t.text || !artTextShown(layerVisible, p, t)) continue;
      if (!filterAllows(filter, "fptext") || (!filter.lockedItems && locked.has(p.ref))) continue;
      if (!layerSelectable(t.layer, layerVisible, activeLayer, highContrast)) continue;
      const { x0, y0, x1, y1 } = textBoundingBoxRotated(artTextAsField(t));
      if (boxMatches([x0, y0, x1, y1], selBox, crossing)) fieldHits.push({ kind: "fptext", id: t.id });
    }
  }
  const takenParts = new Set(out.filter((h) => h.kind === "part").map((h) => h.id));
  out.push(...fieldHits.filter((h) => !takenParts.has(h.kind === "fptext" ? h.id.slice(0, h.id.lastIndexOf("~t")) : h.id.slice(0, h.id.indexOf(":")))));

  // `PCB_SELECTION_TOOL::SelectMultiple`: "If we selected nothing but pads, allow them to be selected" -- a box that takes a footprint (or anything else)
  // never takes the pads inside it as well.
  return out.length > 0 ? out : padHits;
}

/**
 * The boxes of a footprint's own texts (`PCB_TEXT`, not its fields): what `GetBoundingHull` takes of them (`TransformShapeToPolygon( .., ARC_LOW_DEF,
 * ERROR_OUTSIDE )` = `buildBoundingHull`: the box of their strokes, grown by the error, turned with them; it was a rough box of 0.6 of the size a character).
 */
function ownTextBoxes(p: Part): Ring[] {
  const out: Ring[] = [];
  for (const t of p.texts ?? []) {
    if (!t.text) continue;
    const ring = strokesBoxRing(fieldStrokes(artTextAsField(t)), ARC_LOW_DEF_NM);
    if (ring) out.push(ring);
  }
  return out;
}

/** `FOOTPRINT::GetBoundingHull` of a placed part (kicad-port/footprintCoverage.ts). */
function partHull(p: Part): Ring {
  return footprintHull(p, ownTextBoxes(p));
}

/**
 * `FOOTPRINT::CoverageRatio( aCollector )`: how much of `part`'s hull its pads (`TransformPadsToPolySet`, each layer's copper by its real shape) and its
 * texts and shown fields (`TransformFPShapesToPolySet( .., textMargin, ARC_LOW_DEF, ERROR_OUTSIDE, true, false, false )`: their strokes grown by the
 * collector's accuracy and the error, UI-LEFT-2 PP2 -- they were boxes) cover together with what else the click found: a field, text, text box, shape, barcode,
 * track or via not of this footprint (`TransformShapeToPolygon`: a text the box of its strokes, `buildBoundingHull`), another footprint's hull.
 */
export function footprintCoverageRatio(board: BoardState, ref: string, pool: readonly SelectionCandidate[], accuracyNm: number): number {
  const part = board.parts.find((p) => p.ref === ref);
  if (!part?.placed) return 0;
  const hull = partHull(part);
  const covered: Ring[] = [];
  for (const pad of part.pads ?? []) covered.push(...padCopperRings(pad));
  // `if( aErrorLoc == ERROR_OUTSIDE ) aClearance += aMaxError; textShape.Inflate( aClearance, .. )`
  const textMargin = accuracyNm + ARC_LOW_DEF_NM;
  for (const t of part.texts ?? []) if (t.text) covered.push(...strokeRings(fieldStrokes(artTextAsField(t)), textMargin));
  for (const f of part.fields ?? []) {
    if (!f.visible || !f.text) continue;
    covered.push(...strokeRings(fieldStrokes(f), textMargin));
  }
  const ownFields = new Set((part.fields ?? []).map((f) => f.id));
  const ownTexts = new Set((part.texts ?? []).map((t) => t.id));
  const boxOfText = (f: FieldInfo) => {
    const ring = strokesBoxRing(fieldStrokes(f), ARC_LOW_DEF_NM);
    if (ring) covered.push(ring);
  };
  for (const c of pool) {
    switch (c.kind) {
      case "part":
        if (c.id !== ref) {
          const other = board.parts.find((p) => p.ref === c.id);
          if (other?.placed) covered.push(partHull(other));
        }
        break;
      case "field": {
        if (ownFields.has(c.id)) break;
        for (const p of board.parts) {
          const f = p.fields?.find((x) => x.id === c.id);
          if (f) {
            boxOfText(f);
            break;
          }
        }
        break;
      }
      // another footprint's own text (`PCB_TEXT_T` whose parent is not this footprint)
      case "fptext": {
        if (ownTexts.has(c.id)) break;
        const hit = fpTextById(board, c.id);
        if (hit) boxOfText(artTextAsField(hit.text));
        break;
      }
      case "text": {
        const t = board.drawings?.texts.find((x) => x.id === c.id);
        if (t) boxOfText(boardTextAsField(t));
        break;
      }
      case "shape": {
        const sh = board.drawings?.shapes.find((x) => x.id === c.id);
        if (sh) covered.push(...shapeRings(sh, isAnyFill(sh)));
        break;
      }
      case "track": {
        const t = board.routing?.tracks.find((x) => x.id === c.id);
        if (t) covered.push(...trackRings(t.pts, t.width));
        break;
      }
      case "via": {
        const v = board.routing?.vias.find((x) => x.id === c.id);
        if (v) covered.push(circleRing(v.x, v.y, viaMaxDiameter(v) / 2));
        break;
      }
      case "textbox":
      case "barcode": {
        const item = extraItems(board).find((x) => x.id === c.id);
        if (item) covered.push(boxRing(item.bounds[0], item.bounds[1], item.bounds[2], item.bounds[3]));
        break;
      }
      default:
        break;
    }
  }
  return coverageRatio(hull, covered);
}

/**
 * `RequestSelection` with the edit tools' client filter, for the item under the cursor when nothing is selected (M, R, F, D, E, Delete ...): the items
 * collected, `applyToolFilter`, then the heuristics -- in `selectPoint`'s order (the client filter runs before `GuessSelectionCandidates`). Over a footprint's
 * fab reference this takes the footprint, as a click there takes the text.
 */
export function pickToolCandidates(
  board: BoardState,
  xNm: number,
  yNm: number,
  toleranceNm: number,
  onePixelNm: number,
  filter: SelectionFilter,
  layerVisible: Record<string, boolean>,
  activeLayer: string | null,
  highContrast: boolean,
  selection: ReadonlySet<string>
): SelectionCandidate[] {
  const all = applyToolFilter(board, collectSelectionCandidates(board, xNm, yNm, toleranceNm, filter, layerVisible, activeLayer, highContrast, selection, false, onePixelNm), xNm, yNm, layerVisible);
  const accuracy = Math.round(5 * onePixelNm);
  return guessSelectionCandidates(all, onePixelNm, activeLayer, highContrast, (c, pool) => footprintCoverageRatio(board, c.id, pool, accuracy));
}

/** Which client filter `RequestSelection` runs: the edit tools' (`FilterCollectorForFreePads` + `FilterCollectorForHierarchy`), none (`EDIT_TOOL::Properties`), or `EDIT_TOOL::Drag`'s. */
export type ToolPickMode = "tool" | "plain" | "drag";

/**
 * `EDIT_TOOL::Drag`'s client filter after the free-pad and hierarchy filters, for a hover selection (pcbnew/tools/edit_tool.cpp at 8303b2ad): with a track or a via
 * under the cursor the candidates are first trimmed by the heuristics ("First trim down selection to active layer, tracks vs zones, etc."), then "If we have a knee
 * between two tracks, or a via attached to two tracks, then drop the selection to a single item": two tracks that meet at an end leave the first, two tracks and a
 * via they both end on leave the via. ("Remove non-footprints so box-selection will drag footprints" is for a selection that is not the hover one: the caller's.)
 * Returns what is left and what the knees took away (gone for good: `aCollector.Remove`, not `Transfer`).
 */
export function applyDragFilter(board: BoardState, candidates: readonly SelectionCandidate[], guess: (c: SelectionCandidate[]) => SelectionCandidate[]): { kept: SelectionCandidate[]; removed: Set<string> } {
  const removed = new Set<string>();
  const isTrack = (c: SelectionCandidate) => c.kind === "track";
  if (!candidates.some((c) => c.kind === "track" || c.kind === "via")) return { kept: [...candidates], removed };
  let kept = candidates.length > 1 ? guess([...candidates]) : [...candidates];
  const tracks = kept.filter(isTrack);
  const vias = kept.filter((c) => c.kind === "via");
  const ends = (id: string): readonly (readonly [number, number])[] => {
    const t = board.routing?.tracks.find((x) => x.id === id);
    return t && t.pts.length > 0 ? [t.pts[0]!, t.pts[t.pts.length - 1]!] : [];
  };
  // `connected( track, pt )`: `track->GetStart() == pt || track->GetEnd() == pt`
  const connected = (id: string, pt: readonly [number, number] | undefined) => !!pt && ends(id).some((e) => e[0] === pt[0] && e[1] === pt[1]);
  if (tracks.length === 2 && vias.length === 0) {
    const [a, b] = [tracks[0]!.id, tracks[1]!.id];
    if (ends(b).some((pt) => connected(a, pt))) removed.add(b);
  } else if (tracks.length === 2 && vias.length === 1) {
    const via = board.routing?.vias.find((v) => v.id === vias[0]!.id);
    const at = via ? ([via.x, via.y] as const) : undefined;
    if (connected(tracks[0]!.id, at) && connected(tracks[1]!.id, at)) {
      removed.add(tracks[0]!.id);
      removed.add(tracks[1]!.id);
    }
  }
  kept = kept.filter((c) => !removed.has(c.id));
  return { kept, removed };
}

/**
 * `RequestSelection( aClientFilter )` with nothing selected: `selectPoint` on the cursor with the tool's client filter, then the heuristics. `guessed` is what is
 * left (one item: the tool takes it; several: `doSelectionMenu` asks); `all` is every candidate the client filter let through, for the menu's "Show More
 * Choices..." (`HasAdditionalItems()` / `Combine()`: what the heuristics set aside comes back).
 */
export function toolCandidateSets(
  board: BoardState,
  xNm: number,
  yNm: number,
  toleranceNm: number,
  onePixelNm: number,
  filter: SelectionFilter,
  layerVisible: Record<string, boolean>,
  activeLayer: string | null,
  highContrast: boolean,
  selection: ReadonlySet<string>,
  mode: ToolPickMode,
  rejected?: FilterRejections
): { guessed: SelectionCandidate[]; all: SelectionCandidate[] } {
  const collected = collectSelectionCandidates(board, xNm, yNm, toleranceNm, filter, layerVisible, activeLayer, highContrast, selection, false, onePixelNm, rejected);
  const accuracy = Math.round(5 * onePixelNm);
  const guess = (cands: SelectionCandidate[]) => guessSelectionCandidates(cands, onePixelNm, activeLayer, highContrast, (c, pool) => footprintCoverageRatio(board, c.id, pool, accuracy));
  if (mode === "plain") return { guessed: guess(collected), all: collected };
  let all = applyToolFilter(board, collected, xNm, yNm, layerVisible);
  let left = all;
  if (mode === "drag") {
    const { kept, removed } = applyDragFilter(board, all, guess);
    all = all.filter((c) => !removed.has(c.id));
    left = kept;
  }
  return { guessed: left.length > 1 ? guess(left) : left, all };
}

export function pickSelectionCandidates(
  board: BoardState,
  xNm: number,
  yNm: number,
  toleranceNm: number,
  onePixelNm: number,
  filter: SelectionFilter,
  layerVisible: Record<string, boolean>,
  activeLayer: string | null,
  highContrast: boolean,
  selection: ReadonlySet<string>,
  subtractiveOnly: boolean,
  skipHeuristics: boolean,
  rejected?: FilterRejections
): SelectionCandidate[] {
  const all = collectSelectionCandidates(board, xNm, yNm, toleranceNm, filter, layerVisible, activeLayer, highContrast, selection, subtractiveOnly, onePixelNm, rejected);
  if (skipHeuristics) return all;
  // `textMargin = aCollector.GetGuide()->Accuracy()`: `KiROUND( 5 * OnePixelInIU() )`
  const accuracy = Math.round(5 * onePixelNm);
  return guessSelectionCandidates(all, onePixelNm, activeLayer, highContrast, (c, pool) => footprintCoverageRatio(board, c.id, pool, accuracy));
}
