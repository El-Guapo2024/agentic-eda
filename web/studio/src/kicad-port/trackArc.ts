// KiCad-arc tracks (`PCB_ARC`) in the studio's IR: a `Track` whose `pts` are the
// tessellation of the arc and whose `arc_mid_offset` is the mid point relative
// to `pts[0]` (crates/model/src/ir.rs `Track::arc`). The track edits that
// create or reshape arcs (Break Track, Fillet Tracks, Mirror) need to build
// those payloads, so `tessellateArc` here is a line-for-line port of the
// Rust `tessellate_arc` (32 segments, endpoints kept exact): the backend only
// treats a track as an arc while its points still equal that tessellation
// (within 1 um), so the two must agree.
//
// Units: micrometres, like the IR.

import type { CmdRouteTrack, Track } from "../api/types";

export type P = [number, number];

/** `TRACK_ARC_SEGMENTS`. */
export const TRACK_ARC_SEGMENTS = 32;

/** Rust's `f64::round`: half away from zero (`Math.round` rounds half up). */
const roundAway = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v));

/** `eda_model::ir::tessellate_arc( start, mid, end, segments )`. */
export function tessellateArc(start: P, mid: P, end: P, segments = TRACK_ARC_SEGMENTS): P[] {
  const [sx, sy] = start;
  const [mx, my] = mid;
  const [ex, ey] = end;
  const d = 2 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
  if (Math.abs(d) < 1e-6) return [start, end];
  const ux = ((sx * sx + sy * sy) * (my - ey) + (mx * mx + my * my) * (ey - sy) + (ex * ex + ey * ey) * (sy - my)) / d;
  const uy = ((sx * sx + sy * sy) * (ex - mx) + (mx * mx + my * my) * (sx - ex) + (ex * ex + ey * ey) * (mx - sx)) / d;
  const r = Math.sqrt((sx - ux) ** 2 + (sy - uy) ** 2);
  const ang = (x: number, y: number): number => Math.atan2(y - uy, x - ux);
  const twoPi = Math.PI * 2;
  const norm = (a: number): number => ((a % twoPi) + twoPi) % twoPi;
  const a0 = ang(sx, sy);
  const a1 = ang(mx, my);
  const a2 = ang(ex, ey);
  let sweep = norm(a2 - a0);
  if (norm(a1 - a0) > sweep) sweep -= twoPi; // the short way round does not pass through mid
  const n = Math.max(segments, 1);
  const pts: P[] = [];
  for (let i = 0; i <= n; i++) {
    const a = a0 + sweep * (i / n);
    pts.push([roundAway(ux + r * Math.cos(a)), roundAway(uy + r * Math.sin(a))]);
  }
  pts[0] = [start[0], start[1]];
  pts[n] = [end[0], end[1]];
  return pts;
}

/** A `commit_route` track that is a KiCad arc from `start` through `mid` to `end`. */
export function arcRouteTrack(net: string, layer: string, width: number, start: P, mid: P, end: P): CmdRouteTrack {
  const pts = tessellateArc(start, mid, end);
  return {
    net,
    layer,
    width,
    pts: pts.map(([x, y]) => ({ x, y })),
    arc_mid_offset: { x: mid[0] - pts[0]![0], y: mid[1] - pts[0]![1] },
  };
}

/** A `commit_route` track that is a plain polyline. */
export function lineRouteTrack(net: string, layer: string, width: number, pts: readonly P[]): CmdRouteTrack {
  return { net, layer, width, pts: pts.map(([x, y]) => ({ x, y })) };
}

/** `(start, mid, end)` when the studio track is a KiCad arc. */
export function trackArcOf(t: Pick<Track, "pts" | "arc_mid">): { start: P; mid: P; end: P } | null {
  if (!t.arc_mid || t.pts.length < 2) return null;
  return { start: t.pts[0]!, mid: t.arc_mid, end: t.pts[t.pts.length - 1]! };
}

/** The circle through three points, or null when they are collinear. */
export function circumcircle(start: P, mid: P, end: P): { cx: number; cy: number; r: number } | null {
  const [sx, sy] = start;
  const [mx, my] = mid;
  const [ex, ey] = end;
  const d = 2 * (sx * (my - ey) + mx * (ey - sy) + ex * (sy - my));
  if (Math.abs(d) < 1e-6) return null;
  const cx = ((sx * sx + sy * sy) * (my - ey) + (mx * mx + my * my) * (ey - sy) + (ex * ex + ey * ey) * (sy - my)) / d;
  const cy = ((sx * sx + sy * sy) * (ex - mx) + (mx * mx + my * my) * (sx - ex) + (ex * ex + ey * ey) * (mx - sx)) / d;
  return { cx, cy, r: Math.sqrt((sx - cx) ** 2 + (sy - cy) ** 2) };
}

/**
 * `LINE_PLACER::SplitAdjacentArcs`: the two arcs a break at `at` leaves
 * (`ConstructFromStartEndCenter` keeping the circle and the direction). The
 * mid point of each half is the middle of its own sweep.
 */
export function splitArcAt(start: P, mid: P, end: P, at: P): [{ start: P; mid: P; end: P }, { start: P; mid: P; end: P }] | null {
  const c = circumcircle(start, mid, end);
  if (!c) return null;
  const twoPi = Math.PI * 2;
  const norm = (a: number): number => ((a % twoPi) + twoPi) % twoPi;
  const ang = (p: P): number => Math.atan2(p[1] - c.cy, p[0] - c.cx);
  const a0 = ang(start);
  // direction: the way round that passes through mid
  const toEnd = norm(ang(end) - a0);
  const ccw = norm(ang(mid) - a0) <= toEnd; // true: increasing angle reaches mid before end
  const sweepTo = (p: P): number => (ccw ? norm(ang(p) - a0) : -norm(a0 - ang(p)));
  const toAt = sweepTo(at);
  const total = ccw ? toEnd : -norm(a0 - ang(end));
  // `at` must lie between start and end on the arc
  if (Math.abs(toAt) <= 1e-12 || Math.abs(toAt) >= Math.abs(total) - 1e-12 || Math.sign(toAt) !== Math.sign(total)) return null;
  const point = (a: number): P => [roundAway(c.cx + c.r * Math.cos(a)), roundAway(c.cy + c.r * Math.sin(a))];
  const first = { start, mid: point(a0 + toAt / 2), end: at };
  const second = { start: at, mid: point(a0 + toAt + (total - toAt) / 2), end };
  return [first, second];
}
