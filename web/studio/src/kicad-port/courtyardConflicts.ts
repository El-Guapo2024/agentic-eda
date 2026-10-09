// The courtyard conflicts of a move in progress (pcbnew/drc/drc_interactive_courtyard_clearance.cpp, `testCourtyardClearances`), over the studio's
// rectangular courtyards: while footprints are being moved, the ones (moved or not) whose courtyards collide are flagged `COURTYARD_CONFLICT`, and
// `PCB_PAINTER::draw( const FOOTPRINT* )` fills their courtyards on `LAYER_CONFLICTS_SHADOW` ("Colliding Courtyards" in the Appearance panel).
//
// Not ported: a through-hole pad's hole inside another footprint's courtyard (`testPadAgainstCourtyards`; the board model has no drill size), and the
// courtyard clearance of the DRC rules (`m_largestCourtyardClearance`: this checks overlap, clearance 0, as the move tool itself does -- "Currently, do not use
// DRC engine for calculation time reasons").
import type { BoardState, Part, Zone } from "../api/types";
import { carryPoint, splitCarried, type CarryPreview } from "./pcbCarry";

export type Box = readonly [number, number, number, number];

/** The move in progress (`StudioState.movePreview`): a carry, plus Pack and Move's per-footprint shift and which kind of move it is. */
export type MovePreview = CarryPreview & { kind?: string; perRefOffsetUm?: Record<string, [number, number]> };

/** `SHAPE_POLY_SET::Collide` for two boxes: they overlap by more than touching. */
export function boxesOverlap(a: Box, b: Box): boolean {
  return a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1];
}

/** The courtyard box of `p` after the move in progress: the corners go through the transform the painter draws the footprint with, the box is theirs. */
export function movedBox(p: Pick<Part, "ref" | "courtyard" | "at">, preview: MovePreview): Box | null {
  if (!p.courtyard) return null;
  const [x0, y0, x1, y1] = p.courtyard;
  const corners: [number, number][] = [
    [x0, y0],
    [x1, y0],
    [x1, y1],
    [x0, y1],
  ];
  let pts: [number, number][];
  if (preview.kind === "pcb") {
    pts = corners.map((c) => carryPoint(preview, c));
  } else {
    // `drawFootprint`'s own transform: turn / flip about the anchor, then move (Pack and Move adds its per-footprint shift).
    const own = preview.perRefOffsetUm?.[p.ref];
    const [dx, dy] = [preview.dxUm + (own?.[0] ?? 0), preview.dyUm + (own?.[1] ?? 0)];
    const [ax, ay] = p.at ?? [(x0 + x1) / 2, (y0 + y1) / 2];
    const theta = (-(preview.rotateQuarterTurns ?? 0) * Math.PI) / 2;
    const [cos, sin] = [Math.round(Math.cos(theta)), Math.round(Math.sin(theta))];
    pts = corners.map(([x, y]) => {
      let u = x - ax;
      const v = y - ay;
      if (preview.flipped) u = -u;
      return [ax + u * cos - v * sin + dx, ay + u * sin + v * cos + dy];
    });
  }
  const xs = pts.map((q) => q[0]);
  const ys = pts.map((q) => q[1]);
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
}

/** Whether a polygon (a rule area's outline) and a box meet: a corner of one inside the other, or their edges crossing. */
export function polygonHitsBox(poly: readonly (readonly [number, number])[], b: Box): boolean {
  const inside = (x: number, y: number) => {
    let c = false;
    for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
      const [xi, yi] = poly[i]!;
      const [xj, yj] = poly[j]!;
      if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) c = !c;
    }
    return c;
  };
  if (poly.some(([x, y]) => x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3])) return true;
  if (inside(b[0], b[1]) || inside(b[2], b[1]) || inside(b[2], b[3]) || inside(b[0], b[3])) return true;
  const edges: [number, number, number, number][] = [
    [b[0], b[1], b[2], b[1]],
    [b[2], b[1], b[2], b[3]],
    [b[2], b[3], b[0], b[3]],
    [b[0], b[3], b[0], b[1]],
  ];
  const ccw = (ax: number, ay: number, bx: number, by: number, cx: number, cy: number) => (cy - ay) * (bx - ax) > (by - ay) * (cx - ax);
  const cross = (a: readonly [number, number], b2: readonly [number, number], c: readonly [number, number], d: readonly [number, number]) =>
    ccw(a[0], a[1], c[0], c[1], d[0], d[1]) !== ccw(b2[0], b2[1], c[0], c[1], d[0], d[1]) && ccw(a[0], a[1], b2[0], b2[1], c[0], c[1]) !== ccw(a[0], a[1], b2[0], b2[1], d[0], d[1]);
  for (let i = 0; i < poly.length; i++) {
    const [p, q] = [poly[i]!, poly[(i + 1) % poly.length]!];
    if (edges.some(([x0, y0, x1, y1]) => cross(p, q, [x0, y0], [x1, y1]))) return true;
  }
  return false;
}

/**
 * `DRC_INTERACTIVE_COURTYARD_CLEARANCE::testCourtyardClearances` for a move in progress: a footprint being moved and one that is not conflict when their
 * courtyards on the same side overlap; a footprint being moved and a rule area that keeps footprints out conflict when the area meets the courtyard. Both parties
 * of a conflict are flagged (`UpdateConflicts( view, true )`). Returns the refs of the footprints in conflict and the ids of the rule areas.
 */
export function courtyardConflicts(board: BoardState, preview: MovePreview | null): { parts: Set<string>; zones: Set<string> } {
  const parts = new Set<string>();
  const zones = new Set<string>();
  if (!preview) return { parts, zones };
  const split = splitCarried(board, preview.refs);
  const moving = split.moving.parts.filter((p) => p.placed && p.courtyard);
  const still = split.still.parts.filter((p) => p.placed && p.courtyard);
  const keepouts = (board.routing?.zones ?? []).filter((z: Zone) => z.is_rule_area && z.keepout_footprints && z.outline.length >= 3);
  for (const m of moving) {
    const mb = movedBox(m, preview);
    if (!mb) continue;
    for (const o of still) {
      if ((o.side === "bottom") !== (m.side === "bottom")) continue;
      if (boxesOverlap(mb, o.courtyard as Box)) {
        parts.add(m.ref);
        parts.add(o.ref);
      }
    }
    for (const z of keepouts) {
      if (polygonHitsBox(z.outline, mb)) {
        parts.add(m.ref);
        zones.add(z.id);
      }
    }
  }
  return { parts, zones };
}
