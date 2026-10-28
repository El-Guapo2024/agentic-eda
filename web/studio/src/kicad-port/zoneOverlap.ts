// `getOverlappingZones` (pcbnew/tools/board_editor_control.cpp) on the client, for the Zone Priority menu's enable test (`ZONE_PRIORITY_CONTEXT_MENU::update`: Raise
// is enabled when an overlapping zone has a higher priority, Lower when one has a lower). The same test as `crates/ops/src/board_control.rs::overlapping_zones`,
// which the command runs:
//   - another copper zone (not a rule area, not a teardrop) that shares a layer of the set (`( candidate->GetLayerSet() & aZone->GetLayerSet() ).any()`);
//   - whose box meets this one's;
//   - whose area collides with this one's (`Outline()->Collide( .. )`, over the triangulation, so nothing in a hole: an edge of one, holes' edges included, touches
//     an edge of the other, or a corner of one is in the other -- inside its outline and not in a hole, `containsSingle`).
// Integer orientation in BigInt: the coordinates are nm, and their products pass 2^53.
//
// Pure: no React, no DOM. Unit-tested in zoneOverlap.test.ts.
import type { Zone } from "../api/types";
import { zoneLayers } from "./zoneLayers";

type P = readonly [number, number];
type OverlapZone = Pick<Zone, "id" | "layer" | "outline" | "is_rule_area" | "teardrop" | "priority"> & { layers?: readonly string[]; holes?: readonly (readonly P[])[] };

function orient(a: P, b: P, c: P): number {
  const v = (BigInt(Math.round(b[0])) - BigInt(Math.round(a[0]))) * (BigInt(Math.round(c[1])) - BigInt(Math.round(a[1]))) - (BigInt(Math.round(b[1])) - BigInt(Math.round(a[1]))) * (BigInt(Math.round(c[0])) - BigInt(Math.round(a[0])));
  return v > 0n ? 1 : v < 0n ? -1 : 0;
}

const onSegment = (a: P, b: P, p: P): boolean => p[0] >= Math.min(a[0], b[0]) && p[0] <= Math.max(a[0], b[0]) && p[1] >= Math.min(a[1], b[1]) && p[1] <= Math.max(a[1], b[1]);

/** `SEG::Collide( aSeg, 0 )`: the segments share a point. */
function segmentsTouch(p1: P, p2: P, q1: P, q2: P): boolean {
  const o1 = orient(p1, p2, q1), o2 = orient(p1, p2, q2), o3 = orient(q1, q2, p1), o4 = orient(q1, q2, p2);
  if (o1 * o2 < 0 && o3 * o4 < 0) return true;
  return (o1 === 0 && onSegment(p1, p2, q1)) || (o2 === 0 && onSegment(p1, p2, q2)) || (o3 === 0 && onSegment(q1, q2, p1)) || (o4 === 0 && onSegment(q1, q2, p2));
}

/** Whether `p` is on an edge of `ring`. */
function onRing(ring: readonly P[], p: P): boolean {
  return ring.some((a, i) => {
    const b = ring[(i + 1) % ring.length]!;
    return orient(a, b, p) === 0 && onSegment(a, b, p);
  });
}

/** `SHAPE_LINE_CHAIN::PointInside`: even-odd, a point on an edge counts as inside. */
function ringContains(ring: readonly P[], p: P): boolean {
  if (onRing(ring, p)) return true;
  let inside = false;
  for (let i = 0; i < ring.length; i++) {
    const a = ring[i]!, b = ring[(i + 1) % ring.length]!;
    if (a[1] > p[1] !== b[1] > p[1]) {
      const side = orient(a, b, p);
      if ((b[1] > a[1] && side > 0) || (b[1] < a[1] && side < 0)) inside = !inside;
    }
  }
  return inside;
}

const box = (ring: readonly P[]): [number, number, number, number] => ring.reduce<[number, number, number, number]>((m, [x, y]) => [Math.min(m[0], x), Math.min(m[1], y), Math.max(m[2], x), Math.max(m[3], y)], [Infinity, Infinity, -Infinity, -Infinity]);

function edgesTouch(a: readonly P[], b: readonly P[]): boolean {
  return a.some((p1, i) => b.some((q1, j) => segmentsTouch(p1, a[(i + 1) % a.length]!, q1, b[(j + 1) % b.length]!)));
}

const holesOf = (z: OverlapZone): readonly (readonly P[])[] => (z.holes ?? []).filter((h) => h.length >= 3);

/** `SHAPE_POLY_SET::Contains`: in the outline and not inside a hole (a point on a hole's edge is in). */
function zoneContains(z: OverlapZone, p: P): boolean {
  return ringContains(z.outline, p) && !holesOf(z).some((h) => ringContains(h, p) && !onRing(h, p));
}

/** Whether the areas of two zones meet on a layer they share (`BOARD::TestZoneIntersection`, `getOverlappingZones`). */
export function zonesOverlap(a: OverlapZone, b: OverlapZone): boolean {
  if (a.outline.length < 3 || b.outline.length < 3) return false;
  const lb = zoneLayers(b);
  if (!zoneLayers(a).some((l) => lb.includes(l))) return false;
  const [ax0, ay0, ax1, ay1] = box(a.outline), [bx0, by0, bx1, by1] = box(b.outline);
  if (!(ax0 <= bx1 && bx0 <= ax1 && ay0 <= by1 && by0 <= ay1)) return false;
  const ra = [a.outline, ...holesOf(a)], rb = [b.outline, ...holesOf(b)];
  return ra.some((x) => rb.some((y) => edgesTouch(x, y))) || rb.flat().some((p) => zoneContains(a, p)) || ra.flat().some((p) => zoneContains(b, p));
}

/** `getOverlappingZones( board, zone )`: the other copper zones whose areas meet this one's on a shared layer. */
export function overlappingZones<Z extends OverlapZone>(zones: readonly Z[], zone: OverlapZone): Z[] {
  return zones.filter((other) => other.id !== zone.id && !other.is_rule_area && !other.teardrop && zonesOverlap(zone, other));
}
