// Port of two track edits to the studio's polyline-track model:
//
//   Break Track   ROUTER_TOOL::InlineBreakTrack -> ROUTER::BreakSegmentOrArc
//                 -> LINE_PLACER::SplitAdjacentSegments / SplitAdjacentArcs
//                 (pcbnew/router/router_tool.cpp, pns_router.cpp, pns_line_placer.cpp)
//   Fillet Tracks EDIT_TOOL::FilletTracks (pcbnew/tools/edit_tool.cpp)
//
// KiCad's tracks are single segments (PCB_TRACK / PCB_ARC) chained end to
// end; this IR's `Track` is a whole polyline (or one arc). So "a segment" in
// the C++ is "a polyline's segment" here: a break splits the polyline at the
// point, a fillet rounds the corner between two consecutive segments of a
// polyline (and between the ends of two selected tracks that meet), and the
// result goes back as one `commit_route` (remove the old tracks, add the
// pieces and the new arcs).
//
// Maths run in IU (nm) like the rest of the pcbnew ports (pcbGeom.ts) and
// come back rounded to micrometres.

import type { Cmd, CmdRouteTrack } from "../api/types";
import { arcFromFillet, eq, fromIU, MIN_PRECISION_IU, norm, segApproxCollinear, segContains, segIntersectLines, segLength, segLineProject, segNearestPoint, segSquaredDistance, toIU, vsub, type Seg, type V } from "./pcbGeom";
import { arcRouteTrack, lineRouteTrack, splitArcAt, circumcircle, type P } from "./trackArc";

export interface EditTrack {
  id: string;
  net: string;
  layer: string;
  width: number;
  pts: readonly (readonly [number, number])[];
  arc_mid?: readonly [number, number] | null;
}

/** What shares a point with a track: other tracks' vertices, vias, pad centres (the connectivity `GetConnectedItemsAtAnchor` consults). */
export interface EditBoard {
  tracks: readonly EditTrack[];
  vias: readonly { net: string; x: number; y: number; from: string; to: string }[];
  /** Pad centres with their net and the copper layers they are on. */
  pads: readonly { net: string | null; x: number; y: number; layers: readonly string[] }[];
  /** Copper layers in stackup order (to expand a via's `from`..`to`). */
  layers: readonly string[];
}

const samePt = (a: readonly [number, number], b: readonly [number, number]): boolean => a[0] === b[0] && a[1] === b[1];

function viaSpans(board: EditBoard, v: { from: string; to: string }, layer: string): boolean {
  const i = board.layers.indexOf(layer);
  const a = board.layers.indexOf(v.from);
  const b = board.layers.indexOf(v.to);
  if (i < 0 || a < 0 || b < 0) return v.from === layer || v.to === layer;
  return i >= Math.min(a, b) && i <= Math.max(a, b);
}

/** Items (other than `exceptTrack`'s own vertex at `p`) joined at `p` on `layer`/`net`: tracks with a vertex there, vias, pads. */
export function itemsJoinedAt(board: EditBoard, p: readonly [number, number], layer: string, net: string): { tracks: { id: string; vertex: number }[]; vias: number; pads: number } {
  const tracks: { id: string; vertex: number }[] = [];
  for (const t of board.tracks) {
    if (t.layer !== layer || t.net !== net) continue;
    t.pts.forEach((q, i) => {
      // an arc track's interior points are tessellation, not joints
      if (t.arc_mid && i !== 0 && i !== t.pts.length - 1) return;
      if (samePt(q, p)) tracks.push({ id: t.id, vertex: i });
    });
  }
  const vias = board.vias.filter((v) => v.net === net && v.x === p[0] && v.y === p[1] && viaSpans(board, v, layer)).length;
  const pads = board.pads.filter((pad) => pad.net === net && pad.x === p[0] && pad.y === p[1] && pad.layers.includes(layer)).length;
  return { tracks, vias, pads };
}

// -------------------------------------------------------------- Break Track

export interface BreakTrackResult {
  ok: boolean;
  /** Why nothing happened (the C++ returns without a message when the break is refused). */
  message: string | null;
  cmd: Cmd | null;
}

/**
 * `PCB_GRID_HELPER::AlignToSegment`: `aligned` is the grid-snapped pointer; the break point is
 * where the segment crosses one of the four lines through it (horizontal, vertical, the
 * diagonals) if that is nearer to `aligned` than the closest end point is to the pointer,
 * else the closest end point.
 */
export function alignToSegment(pointer: V, aligned: V, seg: Seg, snap: boolean): V {
  if (!snap) return aligned;
  const tests: Seg[] = [
    { a: aligned, b: [aligned[0] + 1, aligned[1]] },
    { a: aligned, b: [aligned[0], aligned[1] + 1] },
    { a: aligned, b: [aligned[0] + 1, aligned[1] + 1] },
    { a: aligned, b: [aligned[0] + 1, aligned[1] - 1] },
  ];
  const points: V[] = [];
  for (const t of tests) {
    const hit = segIntersectLines(seg, t);
    if (hit && segSquaredDistance(seg, hit) <= 4) points.push(hit);
  }
  let nearest: V = aligned;
  let minD = Infinity;
  for (const pt of [seg.a, seg.b]) {
    const d = (pt[0] - pointer[0]) ** 2 + (pt[1] - pointer[1]) ** 2;
    if (d < minD) {
      minD = d;
      nearest = pt;
    }
  }
  for (const pt of points) {
    const d = (pt[0] - aligned[0]) ** 2 + (pt[1] - aligned[1]) ** 2;
    if (d < minD) {
      minD = d;
      nearest = pt;
    }
  }
  return nearest;
}

/**
 * `ROUTER_TOOL::InlineBreakTrack`: split `track` where the pointer is (`pointerUm`; `gridUm` for
 * the snap). Refused -- like `SplitAdjacentSegments` returning false -- when the point is an end
 * point or a vertex, or anything else (a track end, a via, a pad) is joined exactly there.
 */
export function breakTrack(board: EditBoard, track: EditTrack, pointerUm: readonly [number, number], gridUm: number): BreakTrackResult {
  const refuse = (message: string | null): BreakTrackResult => ({ ok: false, message, cmd: null });
  const pts = track.pts;
  if (pts.length < 2) return refuse(null);
  const pointer = toIU(pointerUm);
  const aligned: V = [Math.round(pointerUm[0] / gridUm) * gridUm * 1000, Math.round(pointerUm[1] / gridUm) * gridUm * 1000];
  const halfWidthSq = ((track.width * 1000) / 2) ** 2;

  if (track.arc_mid) {
    // Arc: the point on the arc nearest the pointer (the grid alignment of AlignToArc needs the arc/line intersections; the nearest point serves).
    const start = pts[0]!;
    const end = pts[pts.length - 1]!;
    const c = circumcircle([start[0], start[1]], [track.arc_mid[0], track.arc_mid[1]], [end[0], end[1]]);
    if (!c) return refuse(null);
    const dx = pointerUm[0] - c.cx;
    const dy = pointerUm[1] - c.cy;
    const len = Math.hypot(dx, dy) || 1;
    const at: P = [Math.round(c.cx + (c.r * dx) / len), Math.round(c.cy + (c.r * dy) / len)];
    const distToEnds = Math.min((at[0] - start[0]) ** 2 + (at[1] - start[1]) ** 2, (at[0] - end[0]) ** 2 + (at[1] - end[1]) ** 2);
    if (distToEnds * 1_000_000 < halfWidthSq) return refuse(null); // snapped onto an end point: a joint is there
    const joined = itemsJoinedAt(board, at, track.layer, track.net);
    if (joined.tracks.some((t) => t.id !== track.id) || joined.vias > 0 || joined.pads > 0) return refuse(null);
    const halves = splitArcAt([start[0], start[1]], [track.arc_mid[0], track.arc_mid[1]], [end[0], end[1]], at);
    if (!halves) return refuse(null);
    const [first, second] = halves;
    return {
      ok: true,
      message: null,
      cmd: { op: "commit_route", remove_track_ids: [track.id], remove_via_ids: [], tracks: [arcRouteTrack(track.net, track.layer, track.width, first.start, first.mid, first.end), arcRouteTrack(track.net, track.layer, track.width, second.start, second.mid, second.end)] },
    };
  }

  // The segment nearest the pointer.
  let best = 0;
  let bestD = Infinity;
  for (let i = 0; i + 1 < pts.length; i++) {
    const seg: Seg = { a: toIU(pts[i]!), b: toIU(pts[i + 1]!) };
    const d = segSquaredDistance(seg, pointer);
    if (d < bestD) {
      bestD = d;
      best = i;
    }
  }
  const seg: Seg = { a: toIU(pts[best]!), b: toIU(pts[best + 1]!) };

  // `snapToItem`: within half a track width of an end point the break point is that end point (and no joint-free split exists there).
  const dA = (pointer[0] - seg.a[0]) ** 2 + (pointer[1] - seg.a[1]) ** 2;
  const dB = (pointer[0] - seg.b[0]) ** 2 + (pointer[1] - seg.b[1]) ** 2;
  if (dA < halfWidthSq || dB < halfWidthSq) return refuse(null);

  const snapped = alignToSegment(pointer, aligned, seg, true);
  const at = fromIU(snapped);
  if (samePt(at, pts[best]!) || samePt(at, pts[best + 1]!)) return refuse(null);
  // `jt && jt->LinkCount() >= 1`: a joint exists at the point (another track's end, a via, a pad, or a vertex of this polyline).
  const joined = itemsJoinedAt(board, at, track.layer, track.net);
  if (joined.tracks.length > 0 || joined.vias > 0 || joined.pads > 0) return refuse(null);
  if (!segContains(seg, snapped)) return refuse(null);

  const left: P[] = [...pts.slice(0, best + 1).map((p): P => [p[0], p[1]]), at];
  const right: P[] = [at, ...pts.slice(best + 1).map((p): P => [p[0], p[1]])];
  return {
    ok: true,
    message: null,
    cmd: { op: "commit_route", remove_track_ids: [track.id], remove_via_ids: [], tracks: [lineRouteTrack(track.net, track.layer, track.width, left), lineRouteTrack(track.net, track.layer, track.width, right)] },
  };
}

// ------------------------------------------------------------- Fillet Tracks

export interface FilletTracksResult {
  ok: boolean;
  message: string | null;
  cmd: Cmd | null;
  /** How many fillet arcs were made. */
  arcs: number;
}

/** A straight segment of a selected polyline, with live end points (the C++ reads `GetStart()`/`GetEnd()` as the fillets land). */
interface Work {
  track: EditTrack;
  /** Index of the first vertex of this segment in `track.pts`. */
  seg: number;
  a: V;
  b: V;
}

/**
 * `EDIT_TOOL::FilletTracks`: round every corner between two selected straight track segments
 * with a radius-`radiusUm` arc. A corner qualifies when exactly one other item is connected at it
 * and that item is a selected straight segment; arcs in the selection are skipped.
 */
export function filletTracks(board: EditBoard, selected: readonly EditTrack[], radiusUm: number): FilletTracksResult {
  const refuse = (message: string): FilletTracksResult => ({ ok: false, message, cmd: null, arcs: 0 });
  const straight = selected.filter((t) => !t.arc_mid && t.pts.length >= 2);
  const segCount = straight.reduce((n, t) => n + t.pts.length - 1, 0);
  if (segCount < 2) return refuse("At least two straight track segments must be selected.");
  if (radiusUm === 0) return refuse("");

  const work: Work[] = [];
  for (const t of straight) {
    for (let i = 0; i + 1 < t.pts.length; i++) {
      const a = toIU(t.pts[i]!);
      const b = toIU(t.pts[i + 1]!);
      if (norm(vsub(b, a)) > 0) work.push({ track: t, seg: i, a, b });
    }
  }
  const radius = radiusUm * 1000;

  interface Op {
    t1: Work;
    t2: Work;
    t1Start: boolean;
    t2Start: boolean;
  }
  const ops: Op[] = [];
  let didOneAttemptFail = false;
  const processed = new Set<Work>();
  const selectedStraight = new Set(straight.map((t) => t.id));

  /** The items joined at `anchor` besides `w` itself: segments of the selected straight tracks (live `Work`s), everything else just counted. */
  const itemsAt = (anchor: V, w: Work): { selected: Work[]; others: number } => {
    const anchorUm = fromIU(anchor);
    const selected: Work[] = [];
    let foreign = 0;
    for (const t of board.tracks) {
      if (t.layer !== w.track.layer || t.net !== w.track.net) continue;
      if (t.arc_mid) {
        const ends = [t.pts[0]!, t.pts[t.pts.length - 1]!];
        foreign += ends.filter((e) => samePt(e, anchorUm)).length;
        continue;
      }
      for (let i = 0; i + 1 < t.pts.length; i++) {
        if (!samePt(t.pts[i]!, anchorUm) && !samePt(t.pts[i + 1]!, anchorUm)) continue;
        if (selectedStraight.has(t.id)) {
          const o = work.find((x) => x.track.id === t.id && x.seg === i);
          if (o && o !== w) selected.push(o);
        } else {
          foreign++;
        }
      }
    }
    const joined = itemsJoinedAt(board, anchorUm, w.track.layer, w.track.net);
    return { selected, others: selected.length + foreign + joined.vias + joined.pads };
  };

  const processFilletOp = (w: Work, atStart: boolean): void => {
    const anchor = atStart ? w.a : w.b;
    const { selected, others } = itemsAt(anchor, w);
    const other = selected[0];
    if (!other || processed.has(other)) return;
    if (others === 1) ops.push({ t1: w, t2: other, t1Start: atStart, t2Start: eq(other.a, anchor) });
    else didOneAttemptFail = true; // "there are other elements connected at that point"
  };

  for (const w of work) {
    processFilletOp(w, true);
    processFilletOp(w, false);
    processed.add(w);
  }

  const arcs: { track: EditTrack; start: V; mid: V; end: V }[] = [];
  let performed = false;
  for (const op of ops) {
    const { t1, t2 } = op;
    const trackOnStart = eq(t1.a, t2.a) || eq(t1.b, t2.a);
    const trackOnEnd = eq(t1.a, t2.b) || eq(t1.b, t2.b);
    if (trackOnStart && trackOnEnd) continue; // ignore duplicate tracks
    if (!(trackOnStart || trackOnEnd) || t1.track.layer !== t2.track.layer) continue;
    const s1: Seg = { a: t1.a, b: t1.b };
    const s2: Seg = { a: t2.a, b: t2.b };
    if (segApproxCollinear(s1, s2)) continue;
    const arc = arcFromFillet(s1, s2, radius);
    if (!arc) {
      didOneAttemptFail = true;
      continue;
    }
    let t1New: V | null = null;
    let t2New: V | null = null;
    const setIfOn = (which: 1 | 2, seg: Seg, p: V): boolean => {
      if (norm(vsub(segNearestPoint(seg, p), p)) < MIN_PRECISION_IU) {
        if (which === 1) t1New = p;
        else t2New = p;
        return true;
      }
      return false;
    };
    if (!setIfOn(1, s1, arc.start) && !setIfOn(2, s2, arc.start)) {
      didOneAttemptFail = true;
      continue;
    }
    if (!setIfOn(1, s1, arc.end) && !setIfOn(2, s2, arc.end)) {
      didOneAttemptFail = true;
      continue;
    }
    if (!t1New || !t2New) {
      didOneAttemptFail = true;
      continue;
    }
    arcs.push({ track: t1.track, start: arc.start, mid: arc.mid, end: arc.end });
    if (op.t1Start) t1.a = t1New;
    else t1.b = t1New;
    if (op.t2Start) t2.a = t2New;
    else t2.b = t2New;
    performed = true;
  }

  if (!performed) return refuse("Unable to fillet the selected track segments.");

  // Rebuild every track whose segments moved: consecutive segments stay one polyline while they still meet.
  const removeIds: string[] = [];
  const tracks: CmdRouteTrack[] = [];
  for (const t of straight) {
    const segs = work.filter((w) => w.track === t);
    const moved = segs.some((w) => !eq(w.a, toIU(t.pts[w.seg]!)) || !eq(w.b, toIU(t.pts[w.seg + 1]!)));
    if (!moved) continue;
    removeIds.push(t.id);
    let piece: V[] = [];
    const flush = (): void => {
      if (piece.length >= 2) tracks.push(lineRouteTrack(t.net, t.layer, t.width, piece.map((p) => fromIU(p))));
      piece = [];
    };
    for (const w of segs) {
      if (piece.length > 0 && eq(piece[piece.length - 1]!, w.a)) piece.push(w.b);
      else {
        flush();
        piece = [w.a, w.b];
      }
    }
    flush();
  }
  for (const a of arcs) tracks.push(arcRouteTrack(a.track.net, a.track.layer, a.track.width, fromIU(a.start), fromIU(a.mid), fromIU(a.end)));

  return {
    ok: true,
    message: didOneAttemptFail ? "Some of the track segments could not be filleted." : null,
    cmd: { op: "commit_route", remove_track_ids: removeIds, remove_via_ids: [], tracks },
    arcs: arcs.length,
  };
}

export { segLength, segLineProject };
