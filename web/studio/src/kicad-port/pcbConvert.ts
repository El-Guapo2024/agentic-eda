// Ports of pcbnew/tools/convert_tool.cpp (CONVERT_TOOL) and OUTSET_ROUTINE
// (pcbnew/tools/item_modification_routine.cpp) at 8303b2ad, minus the polygon
// building of "Create Polygon / Zone / Rule Area from Selection", which needs a
// polygon engine and lives in crates/ops/src/convert.rs (`POST /api/convert/polys`).
//
// What is here: the settings and their resolution (`CONVERT_SETTINGS`,
// `resolvedSettings` in CreatePolys), which menu entries a selection offers
// (CONVERT_TOOL::Init's conditions), "Create Lines" / "Create Tracks" from polygons
// and graphic lines, "Create Arc from Selection" (SegmentToArc), and the exact
// outsets of "Create Outsets from Selection" (OUTSET_ROUTINE::ProcessItem).
//
// Units: nanometres (the IR's), KiCad's integer geometry through pcbGeom.ts where
// rounding matters.

import type { BoardState, Cmd, CmdRouteTrack, CmdShape, Pad, PadShapeName, Part, Shape } from "../api/types";
import { arcCentralAngle, arcCenter, arcFromCenterAngle, arcRadius, kiRound, resize, rotatePoint, toIU, fromIU, vsub, type V } from "./pcbGeom";
import { itemKind } from "./pcbItems";
import { arcRouteTrack } from "./trackArc";

type P = [number, number];
const pt = (p: readonly [number, number]): P => [p[0], p[1]];
const xy = ([x, y]: readonly [number, number]) => ({ x, y });

// ------------------------------------------------------------------------ settings

/** `CONVERT_STRATEGY`. */
export type ConvertStrategy = "copy_linewidth" | "centerline" | "bounding_hull";

/** `CONVERT_SETTINGS`. */
export interface ConvertSettings {
  strategy: ConvertStrategy;
  /** `m_Gap`: the bounding hull's gap. */
  gapNm: number;
  /** `m_LineWidth`: the bounding hull's / copied line width (0 = the layer's default). */
  widthNm: number;
  /** `m_DeleteOriginals`. */
  deleteOriginals: boolean;
}

/** `CONVERT_TOOL::initUserSettings`. */
export const DEFAULT_CONVERT_SETTINGS: ConvertSettings = { strategy: "centerline", gapNm: 0, widthNm: 0, deleteOriginals: true };

/**
 * `resolvedSettings` of CreatePolys: a strategy other than the centerline with no width takes the layer's default line width, and a
 * positive hull gap also covers half of that width ("the gap is measured from the stroke's edge").
 */
export function resolveConvertSettings(s: ConvertSettings, layerLineWidthNm: number): ConvertSettings {
  const out = { ...s };
  if (out.strategy !== "centerline" && out.widthNm === 0) out.widthNm = layerLineWidthNm;
  if (out.strategy === "bounding_hull" && out.gapNm > 0) out.gapNm += Math.round(out.widthNm / 2);
  return out;
}

/** `COPY_LINEWIDTH`: the stroke width of the top-left stroked item ("pos.x smaller, or equal x and smaller y"). Null when none has a stroke. */
export function copiedLineWidth(board: BoardState, ids: readonly string[]): number | null {
  let best: { x: number; y: number; w: number } | null = null;
  for (const id of ids) {
    const s = board.drawings?.shapes.find((sh) => sh.id === id);
    if (!s) continue; // `HasLineStroke`: graphic shapes
    const [x, y] = shapePosition(s);
    if (!best || x < best.x || (best.x === x && y < best.y)) best = { x, y, w: s.stroke_width };
  }
  return best ? best.w : null;
}

/** `EDA_SHAPE::getPosition` of a shape: an arc's centre (`if( m_shape == SHAPE_T::ARC ) return getCenter();`, round 5 B5-12), a polygon's first point, else its start (a circle's is its centre). */
export function shapePosition(s: Shape): P {
  switch (s.kind) {
    case "arc":
      return fromIU(arcCenter({ start: toIU(s.start), mid: toIU(s.mid), end: toIU(s.end) }));
    case "segment":
    case "rect":
    case "bezier":
      return pt(s.start);
    case "circle":
      return pt(s.center);
    case "polygon":
      return s.pts[0] ? pt(s.pts[0]) : [0, 0];
  }
}

// ------------------------------------------------------------------- availability

/** Which "Create from Selection" entries a selection offers (CONVERT_TOOL::Init). */
export interface ConvertAvailability {
  poly: boolean;
  zone: boolean;
  keepout: boolean;
  lines: boolean;
  tracks: boolean;
  arc: boolean;
  outset: boolean;
  any: boolean;
}

/**
 * `CONVERT_TOOL::Init`'s conditions for the board editor:
 *   shapes        OnlyTypes( segment, rect, circle, arc, curve, text ) && SameLayer
 *   anyPolys      OnlyTypes( zone, polygon, rectangle )
 *   anyTracks     MoreThan( 0 ) && OnlyTypes( track, arc track, via ) && SameLayer
 *   toArcTypes    Count( 1 ) && OnlyTypes( track, arc track, segment )
 *   outsetTypes   OnlyTypes( pad, shape )
 * (Text is not a source here: the studio has no font outlines.)
 */
export function convertAvailability(board: BoardState, ids: readonly string[]): ConvertAvailability {
  const none: ConvertAvailability = { poly: false, zone: false, keepout: false, lines: false, tracks: false, arc: false, outset: false, any: false };
  if (ids.length === 0) return none;
  const shapes = ids.map((id) => board.drawings?.shapes.find((s) => s.id === id) ?? null);
  const kinds = ids.map((id) => itemKind(board, id));
  const layerOf = (i: number): string | null => shapes[i]?.layer ?? board.routing?.tracks.find((t) => t.id === ids[i])?.layer ?? null;
  const sameLayer = ids.every((_, i) => layerOf(i) !== null && layerOf(i) === layerOf(0));
  const shapeKinds = new Set(["segment", "rect", "circle", "arc", "bezier"]);
  const onlyShapes = shapes.every((s) => s !== null && shapeKinds.has(s.kind));
  const anyPolys = ids.every((_, i) => kinds[i] === "zone" || shapes[i]?.kind === "polygon" || shapes[i]?.kind === "rect");
  const anyTracks = kinds.every((k) => k === "track" || k === "via") && sameLayerTracks(board, ids);
  const graphicToTrack = shapes.every((s) => s !== null && (s.kind === "segment" || s.kind === "arc"));
  const poly = (onlyShapes && sameLayer) || anyPolys || anyTracks;
  const lines = anyPolys;
  const tracks = anyPolys || graphicToTrack;
  const arc = ids.length === 1 && (kinds[0] === "track" || shapes[0]?.kind === "segment");
  // `outsetTypes = { PCB_PAD_T, PCB_SHAPE_T }`
  const outset = ids.every((_, i) => shapes[i] !== null || kinds[i] === "pad");
  return { poly, zone: poly, keepout: poly, lines, tracks, arc, outset, any: poly || lines || tracks || arc || outset || ids.length > 0 };
}

/** `SameLayer()` over tracks (a via has no single layer: it joins the layer of the tracks next to it, so only tracks decide). */
function sameLayerTracks(board: BoardState, ids: readonly string[]): boolean {
  const layers = new Set<string>();
  for (const id of ids) {
    const t = board.routing?.tracks.find((x) => x.id === id);
    if (t) layers.add(t.layer);
  }
  return layers.size <= 1;
}

// ------------------------------------------------------------------ polygon sources

/**
 * The outlines of a polygon-like item: a rectangle's four corners, a polygon's points, a zone's outline and then each of its holes (`CreateLines` /
 * `CreateTracks`: `processPolySet( *zone->Outline(), .. )` takes `COutline( oi )` and then every `CHole( oi, hi )`).
 */
export function sourceRings(board: BoardState, id: string): P[][] | null {
  const s = board.drawings?.shapes.find((sh) => sh.id === id);
  if (s) {
    if (s.kind === "rect") return [[pt(s.start), [s.end[0], s.start[1]], pt(s.end), [s.start[0], s.end[1]]]];
    // a polygon's holes are rings too (`processPolySet` over `GetPolyShape()`, UI-LEFT-2 PP10)
    if (s.kind === "polygon") return s.pts.length >= 3 ? [s.pts, ...(s.holes ?? []).filter((h) => h.length >= 3)].map((ring) => ring.map(pt)) : null;
    return null;
  }
  const z = board.routing?.zones.find((zz) => zz.id === id);
  if (z && z.outline.length >= 3) return [z.outline, ...(z.holes ?? []).filter((h) => h.length >= 3)].map((ring) => ring.map((p) => [p[0], p[1]] as P));
  return null;
}

/** The sides and corner arcs of a rectangle source with a corner radius (`EDA_SHAPE::GetCornerRadius()`), null for anything else. */
function roundedSourcePieces(board: BoardState, id: string): ReturnType<typeof roundedRectPieces> | null {
  const s = board.drawings?.shapes.find((sh) => sh.id === id);
  if (s?.kind !== "rect" || !s.corner_radius || s.corner_radius <= 0) return null;
  const box: [number, number, number, number] = [Math.min(s.start[0], s.end[0]), Math.min(s.start[1], s.end[1]), Math.max(s.start[0], s.end[0]), Math.max(s.start[1], s.end[1])];
  // (A square whose radius is half its side is one full-circle arc, which `addGraphicChain` skips: nothing, not the four sides.)
  return roundedRectPieces(box, s.corner_radius);
}

/**
 * `GetBoardItemWidth` for a polygon-like source: a shape's own stroke width when it has one and is not solid-filled; nothing
 * for a zone.
 */
export function sourceWidth(board: BoardState, id: string): number | null {
  const s = board.drawings?.shapes.find((sh) => sh.id === id);
  if (!s) return null;
  if (s.kind === "segment") return s.stroke_width;
  return s.stroke_width && !s.filled ? s.stroke_width : null;
}

/** The edges of a closed ring as segments, zero-length ones dropped (`addGraphicChain` / `addTrackChain`). */
export function ringEdges(ring: readonly P[]): Array<[P, P]> {
  const out: Array<[P, P]> = [];
  for (let i = 0; i < ring.length; i++) {
    const a = ring[i]!;
    const b = ring[(i + 1) % ring.length]!;
    if (a[0] !== b[0] || a[1] !== b[1]) out.push([a, b]);
  }
  return out;
}

export interface ConversionPlan {
  cmds: Cmd[];
  /** Items the plan deletes (so the caller can drop them from the selection). */
  removed: string[];
  /** True when nothing came out of the selection. */
  empty: boolean;
}

/**
 * `CONVERT_TOOL::CreateLines` for `convertToLines`: every polygon-like source becomes line segments on `layer`, with the source's width when
 * it has one and `fallbackWidthNm` otherwise; the sources are deleted when asked.
 */
export function planConvertToLines(board: BoardState, ids: readonly string[], layer: string, fallbackWidthNm: number, deleteOriginals: boolean): ConversionPlan {
  const cmds: Cmd[] = [];
  const removed: string[] = [];
  for (const id of ids) {
    // A rounded rectangle: `ROUNDRECT rrect( rect, graphic->GetCornerRadius(), true )`, its sides as lines and its corners as arcs (round 5 B5-8).
    const rounded = roundedSourcePieces(board, id);
    if (rounded) {
      const width = sourceWidth(board, id) ?? fallbackWidthNm;
      for (const piece of rounded) {
        if (piece.kind === "seg") cmds.push({ op: "add_shape", shape: { kind: "segment", layer, stroke_width: width, filled: false, start: xy(piece.a), end: xy(piece.b) } });
        else cmds.push({ op: "add_shape", shape: { kind: "arc", layer, stroke_width: width, filled: false, start: xy(fromIU(piece.arc.start)), mid: xy(fromIU(piece.arc.mid)), end: xy(fromIU(piece.arc.end)) } });
      }
      continue;
    }
    const rings = sourceRings(board, id);
    if (!rings) continue;
    const width = sourceWidth(board, id) ?? fallbackWidthNm;
    for (const ring of rings) {
      for (const [a, b] of ringEdges(ring)) {
        cmds.push({ op: "add_shape", shape: { kind: "segment", layer, stroke_width: width, filled: false, start: xy(a), end: xy(b) } });
      }
    }
  }
  if (cmds.length > 0 && deleteOriginals) {
    for (const id of ids) {
      removed.push(id);
      cmds.push(deleteCmd(board, id));
    }
  }
  return { cmds, removed, empty: cmds.length === 0 };
}

/** The command that deletes one item by id, whatever it is. */
export function deleteCmd(board: BoardState, id: string): Cmd {
  if (board.drawings?.shapes.some((s) => s.id === id)) return { op: "delete_shape", id };
  if (board.routing?.zones.some((z) => z.id === id)) return { op: "delete_zone", id };
  if (board.routing?.tracks.some((t) => t.id === id)) return { op: "delete_track", id };
  if (board.routing?.vias.some((v) => v.id === id)) return { op: "delete_via", id };
  if (board.drawings?.texts.some((t) => t.id === id)) return { op: "delete_text", id };
  return { op: "delete_shape", id };
}

/**
 * `CONVERT_TOOL::CreateLines` for `convertToTracks`: segments and arcs become tracks of the same width (`handleGraphicSeg`), polygon-like sources a closed track
 * run around each outline and hole (`addTrackChain`, `fallbackWidthNm` -- `PCB_TRACK`'s own 0.2 mm -- for a source with no width), all on `layer`. The tracks have
 * no net (`new PCB_TRACK( parent )`: net 0, `""` here; UI-LEFT-3 P3-20). The sources go when `deleteOriginals` (`m_userSettings.m_DeleteOriginals`).
 */
export function planConvertToTracks(board: BoardState, ids: readonly string[], layer: string, fallbackWidthNm: number, deleteOriginals: boolean): ConversionPlan {
  const net = "";
  const tracks: CmdRouteTrack[] = [];
  const removedShapes: string[] = [];
  for (const id of ids) {
    const s = board.drawings?.shapes.find((sh) => sh.id === id);
    if (s?.kind === "segment") {
      tracks.push({ net, layer, width: s.stroke_width || fallbackWidthNm, pts: [xy(s.start), xy(s.end)] });
      removedShapes.push(id);
      continue;
    }
    if (s?.kind === "arc") {
      tracks.push(arcRouteTrack(net, layer, s.stroke_width || fallbackWidthNm, pt(s.start), pt(s.mid), pt(s.end)));
      removedShapes.push(id);
      continue;
    }
    const rounded = roundedSourcePieces(board, id);
    if (rounded) {
      const width = sourceWidth(board, id) ?? fallbackWidthNm;
      for (const piece of rounded) {
        if (piece.kind === "seg") tracks.push({ net, layer, width, pts: [xy(piece.a), xy(piece.b)] });
        else tracks.push(arcRouteTrack(net, layer, width, fromIU(piece.arc.start) as P, fromIU(piece.arc.mid) as P, fromIU(piece.arc.end) as P));
      }
      removedShapes.push(id);
      continue;
    }
    const rings = sourceRings(board, id);
    if (!rings) continue;
    const width = sourceWidth(board, id) ?? fallbackWidthNm;
    for (const ring of rings) {
      const edges = ringEdges(ring);
      for (const [a, b] of edges) tracks.push({ net, layer, width, pts: [xy(a), xy(b)] });
    }
    removedShapes.push(id);
  }
  const cmds: Cmd[] = [];
  if (tracks.length > 0) cmds.push({ op: "commit_route", remove_track_ids: [], remove_via_ids: [], tracks });
  const removed = tracks.length > 0 && deleteOriginals ? removedShapes : [];
  for (const id of removed) cmds.push(deleteCmd(board, id));
  return { cmds, removed, empty: tracks.length === 0 };
}

// ------------------------------------------------------------------------ SegmentToArc

/** `offsetRatio = 0.1` of `SegmentToArc`: the mid point sits this fraction of the length off the chord. */
export const ARC_OFFSET_RATIO = 0.1;

/**
 * The mid point `SegmentToArc` gives a straight item: the chord's centre moved along its normal (`Perpendicular()`, rotated +90 degrees)
 * by `offsetRatio * length`, "so that it's more obviously an arc".
 */
export function arcMidOfSegment(a: P, b: P): P {
  const A = toIU(a);
  const B = toIU(b);
  const d = vsub(B, A);
  // `SEG::Length()` is an int (`EuclideanNorm`, rounded), and `Resize` takes an int: the offset is truncated (round 5 B5-14).
  const length = Math.round(Math.hypot(d[0], d[1]));
  const normal = resize([-d[1], d[0]], Math.trunc(ARC_OFFSET_RATIO * length));
  // `SEG::Center()`: `A + ( B - A ) / 2`, the integer halving truncated toward zero.
  const centre: V = [A[0] + Math.trunc(d[0] / 2), A[1] + Math.trunc(d[1] / 2)];
  return fromIU([centre[0] + normal[0], centre[1] + normal[1]]);
}

/**
 * `CONVERT_TOOL::SegmentToArc` on the first selected item (the source stays): a segment shape becomes an arc shape, a track an arc
 * track and an arc track an arc shape. Null when the item is none of those (or is degenerate).
 *
 * The C++ builds the graphic arc from its centre, start and end, and `EDA_SHAPE` then sweeps from start to end by increasing angle,
 * which for the offset mid point described above is the long way round; the arc through the three points (what the comment and the
 * mid point say is meant) is made here.
 */
export function planSegmentToArc(board: BoardState, id: string): Cmd[] | null {
  const s = board.drawings?.shapes.find((sh) => sh.id === id);
  if (s?.kind === "segment") {
    if (s.start[0] === s.end[0] && s.start[1] === s.end[1]) return null;
    const mid = arcMidOfSegment(pt(s.start), pt(s.end));
    return [{ op: "add_shape", shape: { kind: "arc", layer: s.layer, stroke_width: s.stroke_width, filled: false, start: xy(s.start), mid: xy(mid), end: xy(s.end) } }];
  }
  // (A graphic arc would become an arc track, but the menu never offers that -- `toArcTypes` lists segments and tracks only -- and this
  // model's tracks need a net the arc shape does not have.)
  const t = board.routing?.tracks.find((x) => x.id === id);
  if (!t || (t.pts.length !== 2 && !t.arc_mid)) return null; // a track here is one straight segment or one arc
  const first = pt(t.pts[0]!);
  const last = pt(t.pts[t.pts.length - 1]!);
  if (t.arc_mid) {
    // An arc track becomes an arc shape.
    const mid: P = [t.arc_mid[0], t.arc_mid[1]];
    return [{ op: "add_shape", shape: { kind: "arc", layer: t.layer, stroke_width: t.width, filled: false, start: xy(first), mid: xy(mid), end: xy(last) } }];
  }
  if (first[0] === last[0] && first[1] === last[1]) return null;
  const mid = arcMidOfSegment(first, last);
  return [{ op: "commit_route", remove_track_ids: [], remove_via_ids: [], tracks: [arcRouteTrack(t.net, t.layer, t.width, first, mid, last)] }];
}

// -------------------------------------------------------------------------- outsets

/** `OUTSET_ROUTINE::PARAMETERS`. */
export interface OutsetParams {
  outsetNm: number;
  roundCorners: boolean;
  useSourceLayers: boolean;
  useSourceWidths: boolean;
  layer: string;
  lineWidthNm: number;
  /** `gridRounding`: round rectangular outsets outwards to multiples of this (null = off). */
  gridRoundingNm: number | null;
  deleteSourceItems: boolean;
}

/** The dialog's start values for the board editor (`outset_params_pcb_edit`: 1 mm, rounded corners, copy layers, on Edge.Cuts); the line width is the board's
 * Edge.Cuts thickness (`GetLineThickness( Edge_Cuts )`, round 5 B5-15), which the caller fills in -- 0.05 mm is KiCad's own default for it. */
export const DEFAULT_OUTSET_PARAMS: OutsetParams = { outsetNm: 1_000_000, roundCorners: true, useSourceLayers: true, useSourceWidths: true, layer: "Edge.Cuts", lineWidthNm: 50_000, gridRoundingNm: null, deleteSourceItems: false };

/** `CONVERT_TOOL::CreateLines`' widths for a source with none: `addGraphicChain` and `addTrackChain` set one only `if( aWidth && *aWidth > 0 )`, so the
 * constructors' stand -- `PCB_SHAPE`'s `DEFAULT_LINE_WIDTH` (0.10 mm) and `PCB_TRACK`'s 0.2 mm (round 5 B5-11). */
export const DEFAULT_CONVERTED_LINE_WIDTH_NM = 100_000;
export const DEFAULT_CONVERTED_TRACK_WIDTH_NM = 200_000;

export interface OutsetResult {
  add: CmdShape[];
  /** Source shapes to delete (`deleteSourceItems` with at least one success and no failure so far). */
  remove: string[];
  successes: number;
  failures: number;
  /** `GetStatusMessage()`. */
  message: string | null;
}

const floorTo = (v: number, g: number): number => Math.floor(v / g) * g;
const ceilTo = (v: number, g: number): number => Math.ceil(v / g) * g;

type Mk = (partial: Record<string, unknown>) => CmdShape;

/**
 * A footprint pad as `OUTSET_ROUTINE::ProcessItem`'s `PCB_PAD_T` case reads it at 8303b2ad: `GetShape( ALL_LAYERS )` and `GetSize( ALL_LAYERS )`
 * (the front of a padstack: "TODO(JE) padstacks"), `GetPosition()` (the hole's position, the shape offset not added), `GetOrientation()` and
 * `GetRoundRectCornerRadius( ALL_LAYERS )`.
 */
export interface OutsetPad {
  /** The pad's id (`REF.NUM[#k]`). */
  id: string;
  shape: PadShapeName;
  /** Un-turned size, nm. */
  size: [number, number];
  /** `GetPosition()`, nm. */
  pos: [number, number];
  /** `GetOrientation()`: KiCad's angle on the board, degrees counter-clockwise on screen. */
  orientationDeg: number;
  /** `GetRoundRectCornerRadius`, nm. */
  cornerRadiusNm: number;
}

/** What `OUTSET_ROUTINE` is run on: the selection's graphic shapes and pads, in selection order. */
export type OutsetSource = { kind: "shape"; shape: Shape } | { kind: "pad"; pad: OutsetPad };

/** `PAD::GetLayer()` (`BOARD_ITEM::m_layer`): a pad never sets it, so it is the constructor's `F_Cu` on either side -- the layer "use source layers" puts a pad's outset on. */
export const PAD_SOURCE_LAYER = "F.Cu";

/**
 * The `OutsetPad` of a board pad (the studio state's `Pad` of a placed footprint): its front shape and size, the hole's position, and its board angle
 * (`PAD::GetOrientation`, the pad's own angle in the footprint turned with the footprint: `pcbProperties.ts` `padOrientation`). Null for a pad the state
 * gives no shape for (an older backend).
 */
export function outsetPadOf(id: string, pad: Pad, part: Pick<Part, "rot" | "side">): OutsetPad | null {
  if (!pad.size || !pad.shape) return null;
  const sense = part.side === "bottom" ? -1 : 1;
  const orientationDeg = sense * -(pad.rot ?? 0) - (part.rot ?? 0);
  // `PADSTACK::RoundRectRadius`: `KiROUND( std::min( size.x, size.y ) * RoundRectRadiusRatio() )`.
  const cornerRadiusNm = pad.shape === "round_rect" ? kiRound(Math.min(pad.size[0], pad.size[1]) * (pad.ratio ?? 0.25)) : 0;
  return { id, shape: pad.shape, size: [pad.size[0], pad.size[1]], pos: [pad.px ?? pad.x, pad.py ?? pad.y], orientationDeg, cornerRadiusNm };
}

/** `OUTSET_ROUTINE::ProcessItem` over `shapes` in order (the success / failure counters run across all of them, as in the C++). */
export function outsetShapes(shapes: readonly Shape[], params: OutsetParams): OutsetResult {
  return outsetItems(
    shapes.map((shape) => ({ kind: "shape", shape })),
    params
  );
}

/**
 * `OUTSET_ROUTINE::ProcessItem` over the selected shapes and pads in order, the success / failure counters running across all of them. A pad is never
 * deleted: `item_removal_handler` removes only an item with no parent footprint ("If you do an outset on a FP pad, do you really want to delete the parent?").
 */
export function outsetItems(items: readonly OutsetSource[], params: OutsetParams): OutsetResult {
  const add: CmdShape[] = [];
  const remove: string[] = [];
  let successes = 0;
  let failures = 0;
  const d = params.outsetNm;

  for (const item of items) {
    // `layer = useSourceLayers ? aItem.GetLayer() : layer`; `GetBoardItemWidth` has no width for a pad, so it takes the dialog's.
    const layer = params.useSourceLayers ? (item.kind === "shape" ? item.shape.layer : PAD_SOURCE_LAYER) : params.layer;
    let width = params.lineWidthNm;
    if (params.useSourceWidths && item.kind === "shape") {
      const s = item.shape;
      const w = s.kind === "segment" ? s.stroke_width : s.stroke_width && !s.filled ? s.stroke_width : null;
      if (w != null) width = w;
    }
    const mk: Mk = (partial) => ({ layer, stroke_width: width, filled: false, ...partial }) as unknown as CmdShape;
    const seg = (a: P, b: P) => add.push(mk({ kind: "segment", start: xy(a), end: xy(b) }));
    const arc = (a: { start: V; mid: V; end: V }) => {
      if (a.start[0] === a.end[0] && a.start[1] === a.end[1]) return;
      if (arcRadius(a) === 0) return;
      add.push(mk({ kind: "arc", start: xy(fromIU(a.start)), mid: xy(fromIU(a.mid)), end: xy(fromIU(a.end)) }));
    };
    const rect = (x0: number, y0: number, x1: number, y1: number) => {
      let [ax, ay, bx, by] = [x0, y0, x1, y1];
      if (params.gridRoundingNm) {
        // `GetRectRoundedToGridOutwards`: the top-left corner down, the opposite one up, to grid multiples.
        const g = params.gridRoundingNm;
        [ax, ay, bx, by] = [floorTo(ax, g), floorTo(ay, g), ceilTo(bx, g), ceilTo(by, g)];
      }
      add.push(mk({ kind: "rect", start: xy([ax, ay]), end: xy([bx, by]) }));
    };
    const circleOrRect = (c: P, r: number) => {
      if (params.roundCorners) add.push(mk({ kind: "circle", center: xy(c), end: xy([c[0] + r, c[1]]) }));
      else rect(c[0] - r, c[1] - r, c[0] + r, c[1] + r);
    };

    if (item.kind === "pad") {
      const pad = item.pad;
      switch (pad.shape) {
        case "rect":
        case "round_rect":
        case "oval": {
          // `BOX2I box{ pad.GetPosition() - pad_size / 2, pad_size }; box.Inflate( outset )`.
          const [sx, sy] = pad.size;
          const bx0 = pad.pos[0] - Math.trunc(sx / 2) - d;
          const by0 = pad.pos[1] - Math.trunc(sy / 2) - d;
          const bw = sx + 2 * d;
          const bh = sy + 2 * d;
          if (bw <= 0 || bh <= 0) {
            failures++;
            break;
          }
          let radius = d;
          if (pad.shape === "round_rect") radius += pad.cornerRadiusNm;
          else if (pad.shape === "oval") radius += Math.trunc(Math.min(sx, sy) / 2);
          radius = params.roundCorners ? Math.max(radius, 0) : 0;
          // `ROUNDRECT rrect( box, radius ); rrect.TransformToPolygon( poly ); poly.Rotate( pad.GetOrientation(), pad.GetPosition() ); addPoly( poly )`.
          const turn = (p: V): V => rotatePoint(p, toIU(pad.pos), pad.orientationDeg);
          const box: [number, number, number, number] = [bx0, by0, bx0 + bw, by0 + bh];
          const r = Math.max(0, Math.min(radius, Math.trunc(Math.min(bw, bh) / 2)));
          if (r === 0) {
            // A chain with no arc: one polygon (`addPolygonalChain`).
            const ring: V[] = [
              [bx0, by0],
              [bx0 + bw, by0],
              [bx0 + bw, by0 + bh],
              [bx0, by0 + bh],
            ];
            add.push(mk({ kind: "polygon", pts: ring.map((p) => xy(fromIU(turn(toIU(p))))) }));
          } else {
            for (const piece of roundedRectPieces(box, r, turn)) {
              if (piece.kind === "seg") seg(piece.a, piece.b);
              else arc(piece.arc);
            }
          }
          successes++;
          break;
        }
        case "circle": {
          // `pad.GetSize( ALL_LAYERS ).x / 2 + outset`, about `GetPosition()`.
          const r = Math.trunc(pad.size[0] / 2) + d;
          if (r <= 0) {
            failures++;
            break;
          }
          circleOrRect(pt(pad.pos), r);
          successes++;
          break;
        }
        default:
          // Trapezoids ("not handled yet"), chamfered rectangles and custom pads: no exact outset, neither a success nor a failure.
          break;
      }
      // (`DeleteItem` on a pad reaches `item_removal_handler`, which leaves an item with a parent footprint alone.)
      continue;
    }

    const s = item.shape;
    switch (s.kind) {
      case "rect": {
        // `box.Inflate( outset )` on the (normalised) rectangle; no width or height left = failure.
        const x0 = Math.min(s.start[0], s.end[0]) - d;
        const y0 = Math.min(s.start[1], s.end[1]) - d;
        const x1 = Math.max(s.start[0], s.end[0]) + d;
        const y1 = Math.max(s.start[1], s.end[1]) + d;
        if (x1 - x0 <= 0 || y1 - y0 <= 0) {
          failures++;
          break;
        }
        // `cornerRadius = pcb_shape.GetCornerRadius(); if( m_params.roundCorners ) cornerRadius = std::max( cornerRadius + m_params.outsetDistance, 0 );` (round 5 B5-8)
        const radius = params.roundCorners ? Math.max((s.corner_radius ?? 0) + d, 0) : (s.corner_radius ?? 0);
        if (radius > 0) {
          for (const piece of roundedRectPieces(params.gridRoundingNm ? gridOut(x0, y0, x1, y1, params.gridRoundingNm) : [x0, y0, x1, y1], radius)) {
            if (piece.kind === "seg") seg(piece.a, piece.b);
            else arc(piece.arc);
          }
        } else {
          rect(x0, y0, x1, y1);
        }
        successes++;
        break;
      }
      case "circle": {
        const r0 = Math.round(Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1]));
        const r = r0 + d;
        if (r <= 0) {
          failures++;
          break;
        }
        circleOrRect(pt(s.center), r);
        successes++;
        break;
      }
      case "segment": {
        if (d <= 0) {
          failures++;
          break;
        }
        const a = pt(s.start);
        const b = pt(s.end);
        const A = toIU(a);
        const B = toIU(b);
        const dir = vsub(B, A);
        if (params.roundCorners) {
          // `SHAPE_SEGMENT( seg, outset * 2 )` as a chain: two sides and two half-circle caps.
          const ext = resize(dir, d);
          const perp: V = [-ext[1], ext[0]];
          const a1: V = [A[0] + perp[0], A[1] + perp[1]];
          const a2: V = [A[0] - perp[0], A[1] - perp[1]];
          const b1: V = [B[0] + perp[0], B[1] + perp[1]];
          const b2: V = [B[0] - perp[0], B[1] - perp[1]];
          seg(fromIU(a1), fromIU(b1));
          seg(fromIU(b2), fromIU(a2));
          arc({ start: b1, mid: [B[0] + ext[0], B[1] + ext[1]], end: b2 });
          arc({ start: a2, mid: [A[0] - ext[0], A[1] - ext[1]], end: a1 });
        } else {
          // The oriented rectangle around the line, as a polygon (no arcs: `addPolygonalChain`).
          const ext = resize(dir, d);
          const perp: V = [-ext[1], ext[0]];
          const ring: V[] = [
            [A[0] - ext[0] + perp[0], A[1] - ext[1] + perp[1]],
            [A[0] - ext[0] - perp[0], A[1] - ext[1] - perp[1]],
            [B[0] + ext[0] - perp[0], B[1] + ext[1] - perp[1]],
            [B[0] + ext[0] + perp[0], B[1] + ext[1] + perp[1]],
          ];
          add.push(mk({ kind: "polygon", pts: ring.map((p) => xy(fromIU(p))) }));
        }
        successes++;
        break;
      }
      case "arc": {
        // Only an arc whose radius is at least the outset ("gets rather complicated if this isn't true"); anything else is neither a success nor a failure.
        const a = { start: toIU(pt(s.start)), mid: toIU(pt(s.mid)), end: toIU(pt(s.end)) };
        const radius = arcRadius(a);
        if (radius >= d) {
          const centre = arcCenter(a);
          const angle = arcCentralAngle(a);
          const startNorm = resize(vsub(a.start, centre), d);
          const outer = arcFromCenterAngle(centre, [a.start[0] + startNorm[0], a.start[1] + startNorm[1]], angle);
          const inner = arcFromCenterAngle(centre, [a.start[0] - startNorm[0], a.start[1] - startNorm[1]], angle);
          // End caps: half circles about the arc's ends, from the outer / inner end (`SHAPE_ARC{ P1, outer.GetP1(), ANGLE_180 }`, `SHAPE_ARC{ P0, inner.GetP0(), ANGLE_180 }`).
          const capEnd = arcFromCenterAngle(a.end, outer.end, 180);
          const capStart = arcFromCenterAngle(a.start, inner.start, 180);
          arc(outer);
          arc(capEnd);
          if (arcRadius(inner) > 0) arc({ start: inner.end, mid: inner.mid, end: inner.start });
          arc(capStart);
          successes++;
        }
        break;
      }
      default:
        // Other shapes are not supported with exact outsets (polygons and curves).
        break;
    }
    // "It would be nice if we could differentiate which items went with which ... err on the side of safety."
    if (params.deleteSourceItems && successes > 0 && failures === 0) remove.push(s.id);
  }

  let message: string | null = null;
  if (successes === 0) message = "Unable to outset the selected items.";
  else if (failures > 0) message = "Some of the items could not be outset.";
  return { add, remove, successes, failures, message };
}

function gridOut(x0: number, y0: number, x1: number, y1: number, g: number): [number, number, number, number] {
  return [floorTo(x0, g), floorTo(y0, g), ceilTo(x1, g), ceilTo(y1, g)];
}

type Piece = { kind: "seg"; a: P; b: P } | { kind: "arc"; arc: { start: V; mid: V; end: V } };

/**
 * A rounded rectangle as the shapes `addChain` (OUTSET_ROUTINE) and `addGraphicChain` (CreateLines) make of `ROUNDRECT::TransformToPolygon`'s
 * chain: its straight segments in chain order (top, right, bottom, left; zero-length ones dropped), then its arcs (`MakeCornerArcCw90` NE, SE, SW,
 * NW, or the two `MakeSideArcCw180` halves of an oval: E and W when only the width has a straight edge, N and S when only the height has). The radius
 * is clamped to half the smaller side (`ROUNDRECT::ROUNDRECT`). A rectangle that is all corner (a circle: one 360-degree arc whose ends meet) gives
 * nothing, as `arc.GetP0() == arc.GetP1()` skips it in both callers. `rotate` turns every point about a centre (`SHAPE_POLY_SET::Rotate`, a pad's
 * orientation).
 */
function roundedRectPieces(box: readonly [number, number, number, number], radiusNm: number, rotate?: (p: V) => V): Piece[] {
  const [x0, y0, x1, y1] = box;
  const w = x1 - x0;
  const h = y1 - y0;
  const r = Math.max(0, Math.min(radiusNm, Math.trunc(Math.min(w, h) / 2)));
  const out: Piece[] = [];
  const xEdge = w - 2 * r;
  const yEdge = h - 2 * r;
  if (r === 0 || xEdge < 0 || yEdge < 0 || w <= 0 || h <= 0) return out;
  if (xEdge === 0 && yEdge === 0) return out; // the circle
  const R = rotate ?? ((p: V) => p);
  const segs: Array<[V, V]> = [];
  const arcs: Array<{ start: V; mid: V; end: V }> = [];
  // `inner_rect{ p0 + ( r, r ), x_edge, y_edge }`; `KIGEOM::GetPoint( inner_rect, dir )` is its corner or side middle in that direction.
  const [ix0, iy0, ix1, iy1] = [x0 + r, y0 + r, x0 + r + xEdge, y0 + r + yEdge];
  const quarter = (c: V, start: V) => arcs.push(arcFromCenterAngle(c, start, 90));
  const half = (c: V, start: V) => arcs.push(arcFromCenterAngle(c, start, 180));
  if (xEdge > 0) {
    segs.push([[x0 + r, y0], [x0 + r + xEdge, y0]]); // top
    if (yEdge > 0) {
      quarter([ix1, iy0], [ix1, iy0 - r]); // NE
      segs.push([[x1, y0 + r], [x1, y0 + r + yEdge]]); // right
      quarter([ix1, iy1], [ix1 + r, iy1]); // SE
    } else {
      half([ix1, Math.trunc((iy0 + iy1) / 2)], [ix1, iy0 - r]); // E
    }
    segs.push([[x0 + r + xEdge, y1], [x0 + r, y1]]); // bottom
    if (yEdge > 0) {
      quarter([ix0, iy1], [ix0, iy1 + r]); // SW
      segs.push([[x0, y0 + r + yEdge], [x0, y0 + r]]); // left
      quarter([ix0, iy0], [ix0 - r, iy0]); // NW
    } else {
      half([ix0, Math.trunc((iy0 + iy1) / 2)], [ix0, iy1 + r]); // W
    }
  } else {
    // "x_edge is 0 but y_edge is not, so it's an oval the other way up": the right side, then the closing left side.
    half([Math.trunc((ix0 + ix1) / 2), iy0], [ix0 - r, iy0]); // N
    segs.push([[x1, y0 + r], [x1, y0 + r + yEdge]]);
    half([Math.trunc((ix0 + ix1) / 2), iy1], [ix1 + r, iy1]); // S
    segs.push([[x0, y0 + r + yEdge], [x0, y0 + r]]);
  }
  for (const [a, b] of segs) if (a[0] !== b[0] || a[1] !== b[1]) out.push({ kind: "seg", a: fromIU(R(toIU(a))), b: fromIU(R(toIU(b))) });
  for (const a of arcs) out.push({ kind: "arc", arc: { start: R(a.start), mid: R(a.mid), end: R(a.end) } });
  return out;
}
