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
// Units: micrometres (the IR's), with KiCad's integer geometry done in nm through
// pcbGeom.ts where rounding matters.

import type { BoardState, Cmd, CmdRouteTrack, CmdShape, Shape } from "../api/types";
import { arcCentralAngle, arcCenter, arcFromCenterAngle, arcRadius, resize, toIU, fromIU, vsub, type V } from "./pcbGeom";
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
  gapUm: number;
  /** `m_LineWidth`: the bounding hull's / copied line width (0 = the layer's default). */
  widthUm: number;
  /** `m_DeleteOriginals`. */
  deleteOriginals: boolean;
}

/** `CONVERT_TOOL::initUserSettings`. */
export const DEFAULT_CONVERT_SETTINGS: ConvertSettings = { strategy: "centerline", gapUm: 0, widthUm: 0, deleteOriginals: true };

/**
 * `resolvedSettings` of CreatePolys: a strategy other than the centerline with no width takes the layer's default line width, and a
 * positive hull gap also covers half of that width ("the gap is measured from the stroke's edge").
 */
export function resolveConvertSettings(s: ConvertSettings, layerLineWidthUm: number): ConvertSettings {
  const out = { ...s };
  if (out.strategy !== "centerline" && out.widthUm === 0) out.widthUm = layerLineWidthUm;
  if (out.strategy === "bounding_hull" && out.gapUm > 0) out.gapUm += Math.round(out.widthUm / 2);
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

/** `BOARD_ITEM::GetPosition` of a shape: its start (a rectangle / segment / arc / curve), centre (circle) or first point (polygon). */
function shapePosition(s: Shape): P {
  switch (s.kind) {
    case "segment":
    case "rect":
    case "arc":
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
  const outset = shapes.every((s) => s !== null);
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

/** The outlines of a polygon-like item: a rectangle's four corners, a polygon's points, a zone's outline (`CreateLines` / `CreateTracks`). */
export function sourceRings(board: BoardState, id: string): P[][] | null {
  const s = board.drawings?.shapes.find((sh) => sh.id === id);
  if (s) {
    if (s.kind === "rect") return [[pt(s.start), [s.end[0], s.start[1]], pt(s.end), [s.start[0], s.end[1]]]];
    if (s.kind === "polygon") return s.pts.length >= 3 ? [s.pts.map(pt)] : null;
    return null;
  }
  const z = board.routing?.zones.find((zz) => zz.id === id);
  if (z && z.outline.length >= 3) return [z.outline.map((p) => [p[0], p[1]] as P)];
  return null;
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
 * it has one and `fallbackWidthUm` otherwise; the sources are deleted when asked.
 */
export function planConvertToLines(board: BoardState, ids: readonly string[], layer: string, fallbackWidthUm: number, deleteOriginals: boolean): ConversionPlan {
  const cmds: Cmd[] = [];
  const removed: string[] = [];
  for (const id of ids) {
    const rings = sourceRings(board, id);
    if (!rings) continue;
    const width = sourceWidth(board, id) ?? fallbackWidthUm;
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
 * `CONVERT_TOOL::CreateLines` for `convertToTracks`: segments and arcs become tracks of the same width, polygon-like sources a closed track
 * run around each outline (`fallbackWidthUm` for a source with no width). Every track needs a net in this model (`net`), where KiCad
 * leaves the new tracks without one. The sources go when `deleteOriginals`.
 */
export function planConvertToTracks(board: BoardState, ids: readonly string[], layer: string, net: string, fallbackWidthUm: number, deleteOriginals: boolean): ConversionPlan {
  const tracks: CmdRouteTrack[] = [];
  const removedShapes: string[] = [];
  for (const id of ids) {
    const s = board.drawings?.shapes.find((sh) => sh.id === id);
    if (s?.kind === "segment") {
      tracks.push({ net, layer, width: s.stroke_width || fallbackWidthUm, pts: [xy(s.start), xy(s.end)] });
      removedShapes.push(id);
      continue;
    }
    if (s?.kind === "arc") {
      tracks.push(arcRouteTrack(net, layer, s.stroke_width || fallbackWidthUm, pt(s.start), pt(s.mid), pt(s.end)));
      removedShapes.push(id);
      continue;
    }
    const rings = sourceRings(board, id);
    if (!rings) continue;
    const width = sourceWidth(board, id) ?? fallbackWidthUm;
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
  const length = Math.hypot(d[0], d[1]);
  const normal = resize([-d[1], d[0]], ARC_OFFSET_RATIO * length);
  const centre: V = [Math.trunc((A[0] + B[0]) / 2), Math.trunc((A[1] + B[1]) / 2)];
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
  outsetUm: number;
  roundCorners: boolean;
  useSourceLayers: boolean;
  useSourceWidths: boolean;
  layer: string;
  lineWidthUm: number;
  /** `gridRounding`: round rectangular outsets outwards to multiples of this (null = off). */
  gridRoundingUm: number | null;
  deleteSourceItems: boolean;
}

/** The dialog's start values for the board editor (`outset_params_pcb_edit`: 1 mm, rounded corners, copy layers, on Edge.Cuts, 0.05 mm). */
export const DEFAULT_OUTSET_PARAMS: OutsetParams = { outsetUm: 1000, roundCorners: true, useSourceLayers: true, useSourceWidths: true, layer: "Edge.Cuts", lineWidthUm: 50, gridRoundingUm: null, deleteSourceItems: false };

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

/** `OUTSET_ROUTINE::ProcessItem` over `shapes` in order (the success / failure counters run across all of them, as in the C++). */
export function outsetShapes(shapes: readonly Shape[], params: OutsetParams): OutsetResult {
  const add: CmdShape[] = [];
  const remove: string[] = [];
  let successes = 0;
  let failures = 0;
  const d = params.outsetUm;

  for (const s of shapes) {
    const layer = params.useSourceLayers ? s.layer : params.layer;
    let width = params.lineWidthUm;
    if (params.useSourceWidths) {
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
      if (params.gridRoundingUm) {
        // `GetRectRoundedToGridOutwards`: the top-left corner down, the opposite one up, to grid multiples.
        const g = params.gridRoundingUm;
        [ax, ay, bx, by] = [floorTo(ax, g), floorTo(ay, g), ceilTo(bx, g), ceilTo(by, g)];
      }
      add.push(mk({ kind: "rect", start: xy([ax, ay]), end: xy([bx, by]) }));
    };
    const circleOrRect = (c: P, r: number) => {
      if (params.roundCorners) add.push(mk({ kind: "circle", center: xy(c), end: xy([c[0] + r, c[1]]) }));
      else rect(c[0] - r, c[1] - r, c[0] + r, c[1] + r);
    };

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
        // The IR rectangle has no corner radius, so a radius only comes from rounding the outset's corners.
        const radius = params.roundCorners ? Math.max(d, 0) : 0;
        if (radius > 0) {
          for (const piece of roundedRectPieces(params.gridRoundingUm ? gridOut(x0, y0, x1, y1, params.gridRoundingUm) : [x0, y0, x1, y1], radius)) {
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
          const ext = resize(dir, d * 1000);
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
          const ext = resize(dir, d * 1000);
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
        if (radius >= d * 1000) {
          const centre = arcCenter(a);
          const angle = arcCentralAngle(a);
          const startNorm = resize(vsub(a.start, centre), d * 1000);
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

/** The straight sides and corner arcs of a rounded rectangle (`ROUNDRECT::TransformToPolygon`), the radius clamped to half the smaller side. */
function roundedRectPieces(box: readonly [number, number, number, number], radiusUm: number): Array<{ kind: "seg"; a: P; b: P } | { kind: "arc"; arc: { start: V; mid: V; end: V } }> {
  const [x0, y0, x1, y1] = box;
  const r = Math.max(0, Math.min(radiusUm, Math.floor(Math.min(x1 - x0, y1 - y0) / 2)));
  const out: Array<{ kind: "seg"; a: P; b: P } | { kind: "arc"; arc: { start: V; mid: V; end: V } }> = [];
  if (r === 0) return out;
  const xEdge = x1 - x0 - 2 * r;
  const yEdge = y1 - y0 - 2 * r;
  if (xEdge > 0) {
    out.push({ kind: "seg", a: [x0 + r, y0], b: [x1 - r, y0] });
    out.push({ kind: "seg", a: [x1 - r, y1], b: [x0 + r, y1] });
  }
  if (yEdge > 0) {
    out.push({ kind: "seg", a: [x1, y0 + r], b: [x1, y1 - r] });
    out.push({ kind: "seg", a: [x0, y1 - r], b: [x0, y0 + r] });
  }
  const corner = (cx: number, cy: number, from: number): void => {
    // A quarter circle about (cx, cy), starting at angle `from` (degrees, y down), turning clockwise on screen.
    const at = (deg: number): V => [Math.round(toIU([cx, cy])[0] + r * 1000 * Math.cos((deg * Math.PI) / 180)), Math.round(toIU([cx, cy])[1] + r * 1000 * Math.sin((deg * Math.PI) / 180))];
    out.push({ kind: "arc", arc: { start: at(from), mid: at(from + 45), end: at(from + 90) } });
  };
  corner(x0 + r, y0 + r, 180); // top left: from the left side up to the top
  corner(x1 - r, y0 + r, 270); // top right
  corner(x1 - r, y1 - r, 0); // bottom right
  corner(x0 + r, y1 - r, 90); // bottom left
  return out;
}
