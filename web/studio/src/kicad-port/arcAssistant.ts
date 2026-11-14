// KiCad 25ec2335's `KIGFX::PREVIEW::ARC_ASSISTANT` (common/preview_items/arc_assistant.cpp: `ViewDraw`, `drawCenterGuides`, `drawEndpointGuides`) over the Draw Arc
// construction (kicad-port/arcGeom.ts, `ARC_GEOM_MANAGER`): the guides it draws through `DRAW_CONTEXT` (common/preview_items/draw_context.cpp) and the cursor strings it
// writes next to the cursor (`DrawTextNextToCursor`, `DimensionLabel`, common/preview_items/preview_utils.cpp; kicad-port/rulerItem.ts). Every editor's Draw Arc shows it:
// the schematic's and the Symbol Editor's (`EE_GRAPHIC_TOOL::drawManagedShape` adds `aBehavior.GetAssistant()` to the view), the board's and the Footprint Editor's
// (UI-LEFT-3 S3-28).
//
//   - the centre mode (`drawCenterGuides`): the start radius line (dimmed once the start is set), then -- while the start is being placed -- "r" and "θ" (the start
//     angle counterclockwise, `Normalize720`), or -- while the end is being placed -- the end radius line, a dimmed line from the centre to the cursor, "Δθ" (the
//     subtended angle) and "θ" (the end angle, `Normalize180`); the strings on the side of the cursor away from the centre. 25ec2335 draws the radius guide circle only
//     when `m_drawArc` is set, which nothing sets (8303b2ad drew it always);
//   - the modes that keep the clicked ends (`drawEndpointGuides`): the chord the first two clicks fixed (the cursor chasing its end until then; dimmed once fixed) and,
//     with a solution, the bisector guide from the chord's middle to the derived mid point (start-end-mid) or centre (start-end-centre, with dimmed lines from the
//     centre to the ends), a 4 px circle on it, "r" and "s" (the sagitta: the mid point's distance from the chord's middle); or the tangent ray (at least 100 px long)
//     and -- the direction being placed -- a dimmed line to the cursor and "θ" (the heading, `EDA_ANGLE( dir )` as is), else a dimmed line to the end and "r"; then
//     "Δθ" whenever there is a solution; the strings away from the chord's middle (or the start);
//   - `DrawLineWithAngleHighlight` draws a line whose angle is a multiple of 45 degrees in `getSpecialAngleColour` (green: (0.5, 1, 0.5) on a dark background, (0, 0.7,
//     0) on a light one); a dimmed line is at `PreviewOverlayDeemphAlpha( true )` = 0.5; dashes are 12 px apart and 6 px long (`DrawLineDashed( .., dashSize, dashSize
//     / 2 )`), a guide longer than 400 px cut there.
//
// Pure: points in the editor's internal units (nm, +y down), `scale` pixels per nm.
import { ARC_SET_START, arcHasPreview, arcPointIndex, arcStartAngle, arcStartRadiusEnd, arcEndRadiusEnd, arcSubtended, arcUsesExactEndpoints, angleOfVector, type ArcGeom } from "./arcGeom";
import { dimensionLabel, textNextToCursor, type RulerText } from "./rulerItem";
import type { LengthUnit } from "../state/units";

type Vec = [number, number];

export interface AssistantLine {
  a: Vec;
  b: Vec;
  /** `aDeEmphasised`: drawn at half alpha. */
  deemph: boolean;
  /** `DrawLineWithAngleHighlight` of a line at a multiple of 45 degrees: `getSpecialAngleColour`. */
  special: boolean;
  /** `DrawLineDashed( a, b, step, fill )`: a dash `fill` long every `step` (nm). */
  dash?: { step: number; fill: number };
}

export interface AssistantCircle {
  center: Vec;
  radius: number;
  deemph: boolean;
}

export interface ArcAssistantDrawing {
  lines: AssistantLine[];
  circles: AssistantCircle[];
  /** The cursor strings, top to bottom. */
  strings: string[];
  /** Where `DrawTextNextToCursor` writes them: the cursor and the quadrant (the side away from the guides). */
  cursor: Vec;
  quadrant: Vec;
}

const ANGLE_EPSILON = 1e-9;

/** C's `std::remainder`: x - n y with n the nearest integer to x / y, halves to even. */
function remainder(x: number, y: number): number {
  const q = x / y;
  let n = Math.round(q);
  if (Math.abs(q - Math.trunc(q)) === 0.5) n = 2 * Math.round(q / 2);
  return x - n * y;
}

/** `angleIsSpecial( EDA_ANGLE( vec ) )`: within 1e-9 rad of a multiple of 45 degrees. */
export function angleIsSpecial(v: readonly [number, number]): boolean {
  const rad = (angleOfVector(v[0], v[1]) * Math.PI) / 180;
  return Math.abs(remainder(rad, Math.PI / 4)) < ANGLE_EPSILON;
}

/** `EDA_ANGLE::Normalize720`: into [-360, 360). */
function normalize720(a: number): number {
  while (a < -360) a += 360;
  while (a >= 360) a -= 360;
  return a;
}

/** `EDA_ANGLE::Normalize180`: into (-180, 180]. */
function normalize180(a: number): number {
  while (a <= -180) a += 360;
  while (a > 180) a -= 360;
  return a;
}

const kiRound = (v: number) => (v < 0 ? -Math.round(-v) : Math.round(v));

/** `ARC_ASSISTANT::ViewDraw` for the construction `g`; null when there is nothing to draw (`!HasPreview()`, or the first point only being placed). */
export function arcAssistant(g: ArcGeom, scale: number, units: LengthUnit): ArcAssistantDrawing | null {
  if (!arcHasPreview(g)) return null;
  return arcUsesExactEndpoints(g) ? endpointGuides(g, scale, units) : centerGuides(g, units);
}

function hl(a: Vec, b: Vec, deemph: boolean): AssistantLine {
  return { a, b, deemph, special: angleIsSpecial([b[0] - a[0], b[1] - a[1]]) };
}

function centerGuides(g: ArcGeom, units: LengthUnit): ArcAssistantDrawing {
  const origin: Vec = [g.origin[0], g.origin[1]];
  const cursor: Vec = [g.lastPoint[0], g.lastPoint[1]];
  const lines: AssistantLine[] = [];
  const strings: string[] = [];
  // "draw first radius line"
  lines.push(hl(origin, arcStartRadiusEnd(g), g.step > ARC_SET_START));
  if (g.step === ARC_SET_START) {
    // "haven't started the angle selection phase yet"
    const initAngle = normalize720(arcStartAngle(g));
    strings.push(dimensionLabel("r", g.radius, units), dimensionLabel("θ", initAngle, "deg"));
  } else {
    lines.push(hl(origin, arcEndRadiusEnd(g), false));
    const start = arcStartAngle(g);
    const subtended = arcSubtended(g);
    const normalizedEnd = normalize180(start + subtended);
    // "draw dimmed extender line to cursor"
    lines.push(hl(origin, cursor, true));
    strings.push(dimensionLabel("Δθ", subtended, "deg"), dimensionLabel("θ", normalizedEnd, "deg"));
  }
  // "place the text next to cursor, on opposite side from radius"
  return { lines, circles: [], strings, cursor, quadrant: [origin[0] - cursor[0], origin[1] - cursor[1]] };
}

function endpointGuides(g: ArcGeom, scale: number, units: LengthUnit): ArcAssistantDrawing | null {
  const index = arcPointIndex(g);
  // "The first point is only being placed"
  if (index === 0 || !g.pts) return null;
  const mode = g.mode;
  const sol = g.solution?.valid ? g.solution : null;
  const start: Vec = [g.pts[0][0], g.pts[0][1]];
  const cursor: Vec = [g.lastPoint[0], g.lastPoint[1]];
  const pixel = 1 / (scale || 1);
  const dashSize = kiRound(12 * pixel);
  const lines: AssistantLine[] = [];
  const circles: AssistantCircle[] = [];
  const strings: string[] = [];
  // "Long guides, such as the line to a far away center, are cut so the dashes stay bounded"
  const dashedGuide = (from: Vec, to: readonly [number, number]) => {
    const dx = to[0] - from[0];
    const dy = to[1] - from[1];
    const length = Math.hypot(dx, dy);
    const limit = 400 * pixel;
    const end: Vec = length > limit ? [from[0] + dx * (limit / length), from[1] + dy * (limit / length)] : [to[0], to[1]];
    lines.push({ a: from, b: [kiRound(end[0]), kiRound(end[1])], deemph: false, special: false, dash: { step: dashSize, fill: Math.trunc(dashSize / 2) } });
  };
  let labelAnchor: Vec = start;
  if (mode === "start_end_mid" || mode === "start_end_center") {
    // "The chord is what the first two clicks fixed, and the cursor chases its end until then"
    const chordEnd: Vec = index === 1 ? cursor : [g.pts[1][0], g.pts[1][1]];
    const chordMid: Vec = [(start[0] + chordEnd[0]) / 2, (start[1] + chordEnd[1]) / 2];
    lines.push(hl(start, chordEnd, index > 1));
    labelAnchor = chordMid;
    if (index > 1 && sol) {
      const marked: Vec = mode === "start_end_mid" ? [sol.mid[0], sol.mid[1]] : [sol.center[0], sol.center[1]];
      // "Bisector guide and a marker on the point derived from the cursor"
      dashedGuide([kiRound(chordMid[0]), kiRound(chordMid[1])], marked);
      circles.push({ center: [kiRound(marked[0]), kiRound(marked[1])], radius: 4 * pixel, deemph: false });
      if (mode === "start_end_center") {
        const m: Vec = [kiRound(marked[0]), kiRound(marked[1])];
        lines.push({ a: m, b: [sol.start[0], sol.start[1]], deemph: true, special: false });
        lines.push({ a: m, b: [sol.end[0], sol.end[1]], deemph: true, special: false });
      }
      strings.push(dimensionLabel("r", sol.radius, units), dimensionLabel("s", Math.hypot(sol.mid[0] - chordMid[0], sol.mid[1] - chordMid[1]), units));
    }
  } else {
    // "The tangent ray is at least as long as the distance to the cursor so it reads as a direction"
    const dir = g.dir ?? [0, 0];
    const length = Math.hypot(dir[0], dir[1]);
    if (length > 0) {
      const l = Math.max(length, 100 * pixel) / length;
      dashedGuide(start, [start[0] + dir[0] * l, start[1] + dir[1] * l]);
    }
    if (index === 1) {
      lines.push(hl(start, cursor, true));
      strings.push(dimensionLabel("θ", angleOfVector(dir[0], dir[1]), "deg"));
    } else if (sol) {
      lines.push({ a: start, b: [sol.end[0], sol.end[1]], deemph: true, special: false });
      strings.push(dimensionLabel("r", sol.radius, units));
    }
  }
  if (sol) strings.push(dimensionLabel("Δθ", arcSubtended(g), "deg"));
  // "place the text next to cursor, on opposite side from the guides"
  return { lines, circles, strings, cursor, quadrant: [labelAnchor[0] - cursor[0], labelAnchor[1] - cursor[1]] };
}

/** The cursor strings of `d` as `DrawTextNextToCursor` lays them out, on the drawing layer or (`shadows`) the drop-shadow one. */
export function arcAssistantTexts(d: ArcAssistantDrawing, scale: number, shadows = false): RulerText[] {
  return d.strings.length === 0 ? [] : textNextToCursor(d.cursor, d.quadrant, d.strings, scale, shadows);
}

/** `DRAW_CONTEXT::getSpecialAngleColour`. */
export function specialAngleColour(backgroundDark: boolean): string {
  return backgroundDark ? "rgb(128, 255, 128)" : "rgb(0, 179, 0)";
}
