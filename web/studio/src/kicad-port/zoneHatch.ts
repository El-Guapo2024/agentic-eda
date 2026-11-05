// A zone's outline as KiCad draws it: the border hatch (`ZONE_BORDER_DISPLAY_STYLE`: Line, Hatched, Fully hatched, and the pitch of the hatch), the lines
// `ZONE::HatchBorder` builds and `PCB_PAINTER::draw( const ZONE* )` strokes over the outline and its holes. It is display only: the copper is kicad-cli's.
//
// Ported from (8303b2ad):
//   pcbnew/zone.cpp                                  ZONE::HatchBorder, ZONE::SetBorderDisplayStyle, ZONE::GetDefaultHatchPitch
//   pcbnew/zones.h                                   ZONE_BORDER_HATCH_DIST_MM, ZONE_BORDER_HATCH_MINDIST_MM, ZONE_BORDER_HATCH_MAXDIST_MM
//   libs/kimath/src/geometry/shape_poly_set.cpp      SHAPE_POLY_SET::GenerateHatchLines, Contains, containsSingle
//   libs/kimath/src/geometry/seg.cpp                 SEG::IntersectsLine
//   libs/kimath/src/geometry/shape_line_chain.cpp    SHAPE_LINE_CHAIN_BASE::PointInside
//   libs/kimath/src/math/util.cpp                    rescale( int, int, int )
//
// Pure and dependency-free but for the layer numbering, so the geometry has unit tests (zoneHatch.test.ts).

import type { ZoneBorderStyle } from "../api/types";
import { pcbLayerId } from "./boardLayers";
import { kiRound } from "./pcbGeom";

type P = readonly [number, number];
export type HatchLine = [[number, number], [number, number]];

/** `ZONE_BORDER_HATCH_DIST_MM` (0.5 mm): `ZONE::GetDefaultHatchPitch()`, `ZONE_SETTINGS::m_BorderHatchPitch`'s default. */
export const ZONE_BORDER_HATCH_DIST_NM = 500_000;
/** `ZONE_BORDER_HATCH_MINDIST_MM` (0.1 mm). */
export const ZONE_BORDER_HATCH_MINDIST_NM = 100_000;
/** `ZONE_BORDER_HATCH_MAXDIST_MM` (2 mm). */
export const ZONE_BORDER_HATCH_MAXDIST_NM = 2_000_000;

/** `ZONE_SETTINGS::m_ZoneBorderDisplayStyle`'s default (`DIAGONAL_EDGE`), what a zone without the field (an older backend) is drawn with. */
export const DEFAULT_ZONE_BORDER_STYLE: ZoneBorderStyle = "edge";

/** The "Outline display:" choice (`m_OutlineDisplayCtrlChoices[] = { _("Line"), _("Hatched"), _("Fully hatched") }`), in its order: `NO_HATCH`, `DIAGONAL_EDGE`, `DIAGONAL_FULL`. */
export const ZONE_BORDER_STYLE_CHOICES: { value: ZoneBorderStyle; label: string }[] = [
  { value: "none", label: "Line" },
  { value: "edge", label: "Hatched" },
  { value: "full", label: "Fully hatched" },
];

/** `ZONE::SetBorderDisplayStyle`: the pitch is clamped to 0.1 .. 2 mm. */
export function clampBorderHatchPitch(pitchNm: number): number {
  return Math.min(Math.max(pitchNm, ZONE_BORDER_HATCH_MINDIST_NM), ZONE_BORDER_HATCH_MAXDIST_NM);
}

/**
 * `m_outlineHatchPitch.Validate( mmToIU( ZONE_BORDER_HATCH_MINDIST_MM ), mmToIU( ZONE_BORDER_HATCH_MAXDIST_MM ) )` (the three zone dialogs' `AcceptOptions` /
 * `TransferDataFromWindow`): `UNIT_BINDER::Validate`'s "%s must be at least %s." / "%s must be less than %s." with the label less its colon, or null. `fmt` is
 * `StringFromValue( .., aAddUnitLabel = true )` in the dialog's units.
 */
export function borderHatchPitchError(pitchNm: number, fmt: (nm: number) => string): string | null {
  if (pitchNm < ZONE_BORDER_HATCH_MINDIST_NM) return `Outline hatch pitch must be at least ${fmt(ZONE_BORDER_HATCH_MINDIST_NM)}.`;
  if (pitchNm > ZONE_BORDER_HATCH_MAXDIST_NM) return `Outline hatch pitch must be less than ${fmt(ZONE_BORDER_HATCH_MAXDIST_NM)}.`;
  return null;
}

/** `rescale( int aNumerator, int aValue, int aDenominator )`: `aNumerator * aValue / aDenominator` in 64-bit integers, rounded to nearest (half away from zero). */
export function rescaleInt(numerator: number, value: number, denominator: number): number {
  const product = numerator * value;
  if (Math.abs(product) < 2 ** 52) {
    const half = Math.trunc(denominator / 2);
    return Math.trunc((product < 0 !== denominator < 0 ? product - half : product + half) / denominator);
  }
  const p = BigInt(numerator) * BigInt(value);
  const d = BigInt(denominator);
  const h = d / 2n;
  return Number((p < 0n !== d < 0n ? p - h : p + h) / d);
}

/** `SHAPE_LINE_CHAIN_BASE::PointInside( aPt, aAccuracy <= 1 )`: the crossing test of a closed ring (a point on the edge is either side). */
export function pointInRing(ring: readonly P[], pt: P): boolean {
  const n = ring.length;
  if (n < 3) return false;
  let inside = false;
  for (let i = 0; i < n; ) {
    const p1 = ring[i++]!;
    const p2 = ring[i === n ? 0 : i]!;
    const dx = p2[0] - p1[0];
    const dy = p2[1] - p1[1];
    if (dy === 0) continue;
    const d = rescaleInt(dx, pt[1] - p1[1], dy);
    if (p1[1] >= pt[1] !== p2[1] >= pt[1] && pt[0] - p1[0] < d) inside = !inside;
  }
  return inside;
}

/**
 * `SHAPE_POLY_SET::Contains( aP, -1, 1, true )` for a zone's polygon (`containsSingle`): inside the outline and in none of its holes ("If the point is inside a hole
 * it is outside of the polygon").
 */
export function polygonContains(outline: readonly P[], holes: readonly (readonly P[])[], pt: P): boolean {
  if (!pointInRing(outline, pt)) return false;
  return !holes.some((h) => pointInRing(h, pt));
}

/**
 * `SEG::IntersectsLine( aSlope, aOffset, aIntersection )`: where the segment a-b meets the line `y = slope * x + offset`, or null. A vertical segment takes the line's
 * rounded y at its x; a segment parallel to the line (by the cross product with `( 1000, slope * 1000 )`, as KiCad builds the direction) meets it only when it
 * lies on it, at its midpoint.
 */
export function segIntersectsLine(a: P, b: P, slope: number, offset: number): [number, number] | null {
  const dirX = b[0] - a[0];
  const dirY = b[1] - a[1];
  if (dirX === 0) {
    const y = kiRound(slope * a[0] + offset);
    if (y >= Math.min(a[1], b[1]) && y <= Math.max(a[1], b[1])) return [a[0], y];
    return null;
  }
  // `VECTOR2L lineDir( 1000, static_cast<ecoord>( aSlope * 1000 ) )`: the cast truncates.
  const lineDirY = Math.trunc(slope * 1000);
  const crossProduct = dirX * lineDirY - dirY * 1000;
  if (crossProduct === 0) {
    const expectedY = slope * a[0] + offset;
    if (Math.abs(a[1] - expectedY) < 0.5) return [Math.trunc((a[0] + b[0]) / 2), Math.trunc((a[1] + b[1]) / 2)];
    return null;
  }
  const t = (slope * a[0] + offset - a[1]) / (dirY - slope * dirX);
  if (t >= 0 && t <= 1) return [kiRound(a[0] + t * dirX), kiRound(a[1] + t * dirY)];
  return null;
}

/**
 * `SHAPE_POLY_SET::GenerateHatchLines( aSlopes, aSpacing, aLineLength )` for one polygon (the outline and its holes): for each slope, the lines `y = slope * x + a`
 * every `spacing` across the box of the vertices, cut where they cross the contours; a piece whose middle is inside the polygon is kept whole when `lineLength`
 * is -1 or the piece is shorter than twice it, else as its two ends `lineLength` long (along x).
 */
export function generateHatchLines(outline: readonly P[], holes: readonly (readonly P[])[], slopes: readonly number[], spacing: number, lineLength: number): HatchLine[] {
  const out: HatchLine[] = [];
  const contours = [outline, ...holes].filter((c) => c.length > 0);
  if (outline.length === 0 || spacing <= 0) return out;
  let minX = outline[0]![0];
  let maxX = minX;
  let minY = outline[0]![1];
  let maxY = minY;
  for (const c of contours)
    for (const [x, y] of c) {
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
  for (let slope of slopes) {
    let maxA: number;
    let minA: number;
    if (slope > 0) {
      maxA = kiRound(maxY - slope * minX);
      minA = kiRound(minY - slope * maxX);
    } else {
      maxA = kiRound(maxY - slope * maxX);
      minA = kiRound(minY - slope * minX);
    }
    // `min_a = ( min_a / aSpacing ) * aSpacing`: integer division, toward zero.
    minA = Math.trunc(minA / spacing) * spacing;
    for (let a = minA; a < maxA; a += spacing) {
      const points: [number, number][] = [];
      // `CIterateSegmentsWithHoles()`: every closed contour's segments, the outline first.
      for (const c of contours) {
        for (let i = 0; i < c.length; i++) {
          const pt = segIntersectsLine(c[i]!, c[(i + 1) % c.length]!, slope, a);
          if (!pt) continue;
          if (pt[0] < minX || pt[0] > maxX || pt[1] < minY || pt[1] > maxY) continue;
          points.push(pt);
        }
      }
      // "sort points in order of descending x (if more than 2)"
      if (points.length > 2) points.sort((p, q) => q[0] - p[0]);
      for (let ip = 0; ip + 1 < points.length; ip++) {
        const p1 = points[ip]!;
        const p2 = points[ip + 1]!;
        if (p1[0] === p2[0] && p1[1] === p2[1]) continue;
        const mid: [number, number] = [Math.trunc((p1[0] + p2[0]) / 2), Math.trunc((p1[1] + p2[1]) / 2)];
        if (!polygonContains(outline, holes, mid)) continue;
        let dx = p2[0] - p1[0];
        if (lineLength === -1 || Math.abs(dx) < 2 * lineLength) {
          out.push([
            [p1[0], p1[1]],
            [p2[0], p2[1]],
          ]);
        } else {
          const dy = p2[1] - p1[1];
          // `slope = dy / dx;` -- the loop's own copy of the slope is overwritten, as KiCad's is, for the rest of this slope's lines.
          slope = dy / dx;
          dx = dx > 0 ? lineLength : -lineLength;
          const x1 = kiRound(p1[0] + dx);
          const x2 = kiRound(p2[0] - dx);
          const y1 = kiRound(p1[1] + dx * slope);
          const y2 = kiRound(p2[1] - dx * slope);
          out.push([
            [p1[0], p1[1]],
            [x1, y1],
          ]);
          out.push([
            [p2[0], p2[1]],
            [x2, y2],
          ]);
        }
      }
    }
  }
  return out;
}

/** The fields of a zone `zoneHatchLines` reads. */
export interface HatchedZone {
  layer: string;
  outline: readonly P[];
  holes?: readonly (readonly P[])[];
  teardrop?: boolean;
  border_style?: ZoneBorderStyle;
  border_hatch_pitch?: number;
}

/**
 * `ZONE::HatchBorder`: the hatch lines of the zone's outline for its border style -- none for `NO_HATCH` (Line) or a pitch of 0; for `DIAGONAL_EDGE` (Hatched) short
 * lines `pitch` apart and `pitch` long at the edges; for `DIAGONAL_FULL` (Fully hatched) whole diagonals twice as far apart. The slope is by the zone's first layer
 * ("To have a better look, give a slope depending on the layer": 1 for an odd `PCB_LAYER_ID` -- the technical and user layers --, -1 for an even one -- copper),
 * 0.7 and -0.7 for a teardrop.
 */
export function zoneHatchLines(z: HatchedZone): HatchLine[] {
  const style = z.border_style ?? DEFAULT_ZONE_BORDER_STYLE;
  const pitch = z.border_hatch_pitch ?? ZONE_BORDER_HATCH_DIST_NM;
  if (style === "none" || pitch === 0 || z.outline.length === 0) return [];
  let lineLen = pitch;
  let spacing = pitch;
  if (style === "full") {
    spacing = pitch * 2;
    lineLen = -1;
  }
  const slopes = z.teardrop ? [0.7, -0.7] : pcbLayerId(z.layer) & 1 ? [1] : [-1];
  return generateHatchLines(z.outline, z.holes ?? [], slopes, spacing, lineLen);
}

/** `zoneHatchLines`, kept per zone outline object (`m_borderHatchLines` is rebuilt only when the outline or the style changes). */
const cache = new WeakMap<object, { key: string; lines: HatchLine[] }>();
export function cachedZoneHatchLines(z: HatchedZone): HatchLine[] {
  const key = `${z.layer}|${z.border_style ?? ""}|${z.border_hatch_pitch ?? ""}|${z.teardrop ? 1 : 0}|${z.holes?.length ?? 0}`;
  const hit = cache.get(z.outline);
  if (hit && hit.key === key) return hit.lines;
  const lines = zoneHatchLines(z);
  cache.set(z.outline, { key, lines });
  return lines;
}
