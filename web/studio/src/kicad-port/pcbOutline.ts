// `BOARD::GetBoardPolygonOutlines` for the dogbone routine's "does this corner
// point into the board" test (item_modification_routine.cpp,
// DOGBONE_CORNER_ROUTINE::EnsureBoardOutline): the closed loops the Edge.Cuts
// graphics form. Segments, arcs and curves are chained end to end (within the
// same epsilon the outline converter uses, 100 nm -- 1 um here, the IR grid);
// rectangles, circles and polygons are loops by themselves. When the Edge.Cuts
// graphics form no closed loop the board's own outline polygon is used (the
// studio keeps the placement outline apart from drawn Edge.Cuts shapes).

import type { BoardState, Shape } from "../api/types";
import { tessellateArc } from "./trackArc";
import { bezierPolyline } from "./bezierPoly";

type P = [number, number];

function circleRing(center: P, rim: P): P[] {
  const r = Math.hypot(rim[0] - center[0], rim[1] - center[1]);
  const n = Math.max(24, Math.ceil(Math.PI / Math.acos(Math.max(0, 1 - 2 / Math.max(r, 3)))));
  const out: P[] = [];
  for (let i = 0; i < n; i++) {
    const a = (2 * Math.PI * i) / n;
    out.push([center[0] + r * Math.cos(a), center[1] + r * Math.sin(a)]);
  }
  return out;
}

/** Each chainable Edge.Cuts shape as an open polyline (segments, arcs, curves). */
function openPolylines(shapes: readonly Shape[]): P[][] {
  const out: P[][] = [];
  for (const s of shapes) {
    if (s.kind === "segment") out.push([[s.start[0], s.start[1]], [s.end[0], s.end[1]]]);
    else if (s.kind === "arc") out.push(tessellateArc([s.start[0], s.start[1]], [s.mid[0], s.mid[1]], [s.end[0], s.end[1]]));
    else if (s.kind === "bezier") out.push(bezierPolyline(s.start, s.c1, s.c2, s.end).map((p): P => [p[0], p[1]]));
  }
  return out;
}

/** Chain open polylines whose end points meet (within `eps` um) into closed rings; chains that never close are dropped. */
export function chainClosedRings(lines: readonly P[][], eps = 1): P[][] {
  const used = new Array<boolean>(lines.length).fill(false);
  const near = (a: P, b: P): boolean => Math.hypot(a[0] - b[0], a[1] - b[1]) <= eps;
  const rings: P[][] = [];
  for (let i = 0; i < lines.length; i++) {
    if (used[i]) continue;
    used[i] = true;
    let chain: P[] = lines[i]!.map((p): P => [p[0], p[1]]);
    let grew = true;
    while (grew) {
      grew = false;
      for (let j = 0; j < lines.length; j++) {
        if (used[j]) continue;
        const l = lines[j]!;
        const head = chain[0]!;
        const tail = chain[chain.length - 1]!;
        if (near(tail, l[0]!)) chain = [...chain, ...l.slice(1)];
        else if (near(tail, l[l.length - 1]!)) chain = [...chain, ...[...l].reverse().slice(1)];
        else if (near(head, l[l.length - 1]!)) chain = [...l.slice(0, -1), ...chain];
        else if (near(head, l[0]!)) chain = [...[...l].reverse().slice(0, -1), ...chain];
        else continue;
        used[j] = true;
        grew = true;
      }
    }
    if (chain.length >= 4 && near(chain[0]!, chain[chain.length - 1]!)) rings.push(chain.slice(0, -1));
  }
  return rings;
}

/** The board's outline loops: the Edge.Cuts graphics when they close, else the board outline polygon. */
export function boardOutlineRings(board: Pick<BoardState, "outline" | "drawings">): P[][] {
  const edge = (board.drawings?.shapes ?? []).filter((s) => s.layer === "Edge.Cuts");
  const rings: P[][] = [];
  for (const s of edge) {
    if (s.kind === "rect") rings.push([[s.start[0], s.start[1]], [s.end[0], s.start[1]], [s.end[0], s.end[1]], [s.start[0], s.end[1]]]);
    else if (s.kind === "circle") rings.push(circleRing([s.center[0], s.center[1]], [s.end[0], s.end[1]]));
    else if (s.kind === "polygon") rings.push(s.pts.map((p): P => [p[0], p[1]]));
  }
  rings.push(...chainClosedRings(openPolylines(edge)));
  if (rings.length === 0 && board.outline && board.outline.length >= 3) rings.push(board.outline.map((p): P => [p[0], p[1]]));
  return rings;
}
