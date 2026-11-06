// A shape's dashed, dotted, dash-dot and dash-dot-dot line, cut into the pieces KiCad draws (UI-LEFT P5): `STROKE_PARAMS::Stroke` (common/stroke_params.cpp at
// 8303b2ad) over the shapes `EDA_SHAPE::makeEffectiveShapes( aEdgeOnly = true )` makes of a board or footprint graphic (common/eda_shape.cpp), as
// `PCB_PAINTER::draw( const PCB_SHAPE* )` (pcbnew/pcb_painter.cpp) strokes a shape whose line style is not solid: every piece a `DrawSegment( a, b, thickness )`.
//
// The lengths scale with the line width (`RENDER_SETTINGS::GetDashLength`, `GetDotLength`, `GetGapLength`, common/render_settings.cpp, `correction` 1.0): a dash
// is `max( dash ratio - 1, 1 ) * w`, a dot `max( 1 - 1, 0.2 ) * w`, a gap `max( gap ratio + 1, 1 ) * w`. The board editor takes the ratios from the board's plot
// settings (`PCB_BASE_FRAME`: `SetDashLengthRatio( GetPlotOptions().GetDashedLineDashRatio() )`, Board Setup > Formatting's Dashed lines, 12 and 3 by default);
// the plotter keeps 12 and 3 (`PCB_RENDER_SETTINGS`' own, plot_board_layers.cpp), which is what kicad-cli's Gerbers have.
//
// Each piece of a shape starts the pattern again: a rectangle's four sides, a polygon's edges, a curve's flattened segments (so a dashed curve of short
// segments draws one dash per segment, as KiCad's does), each arc of a rounded rectangle. A segment's last stroke is cut at its end (`ClipLine` against the
// segment's box); an arc's at its end angle. A dash on an arc is drawn as chords of half a degree, a dot as one chord.
//
// Pure: no canvas. Unit-tested in strokeDash.test.ts.
import type { Shape } from "../api/types";
import { bezierPolyline } from "./bezierPoly";
import { kicadArc } from "./lineEnding";

type P = readonly [number, number];
/** One stroke to draw, `[a, b]`, integer nanometres. */
export type DashPiece = [[number, number], [number, number]];

/** `LINE_STYLE` but solid: the styles `STROKE_PARAMS::Stroke` cuts (the file's `dash`, `dot`, `dash_dot`, `dash_dot_dot`). */
export type DashStyle = "dash" | "dot" | "dash_dot" | "dash_dot_dot";

/** `RENDER_SETTINGS::m_dashLengthRatio` / `m_gapLengthRatio`. */
export interface DashRatios {
  dash: number;
  gap: number;
}

/** "From ISO 128-2" (`RENDER_SETTINGS::RENDER_SETTINGS`). */
export const DEFAULT_DASH_RATIOS: DashRatios = { dash: 12, gap: 3 };

/** `constexpr double correction = 1.0; // Matches ISO 128-2`. */
const CORRECTION = 1.0;

export const dashLength = (w: number, r: DashRatios = DEFAULT_DASH_RATIOS): number => Math.max(r.dash - CORRECTION, 1.0) * w;
export const dotLength = (w: number): number => Math.max(1.0 - CORRECTION, 0.2) * w;
export const gapLength = (w: number, r: DashRatios = DEFAULT_DASH_RATIOS): number => Math.max(r.gap + CORRECTION, 1.0) * w;

/**
 * The shape dialog's "Line style:" choices (`lineTypeNames`, common/stroke_params.cpp, in `DIALOG_SHAPE_PROPERTIES`' combo) and the Properties panel's "Line Style"
 * (`ENUM_MAP<LINE_STYLE>`, common/eda_shape.cpp): the same five words. `LINE_STYLE::DEFAULT` is not one of them; the dialog shows it as Solid
 * (`if( style >= 0 && style < (int) lineTypeNames.size() ) .. else SetSelection( 0 )`) and its OK writes what it shows.
 */
export const LINE_STYLE_CHOICES: readonly { value: "solid" | DashStyle; label: string }[] = [
  { value: "solid", label: "Solid" },
  { value: "dash", label: "Dashed" },
  { value: "dot", label: "Dotted" },
  { value: "dash_dot", label: "Dash-Dot" },
  { value: "dash_dot_dot", label: "Dash-Dot-Dot" },
];

/** Is this a style `STROKE_PARAMS::Stroke` cuts? (`solid`, `default` and none are drawn as one stroke.) */
export function isDashStyle(style: string | undefined | null): style is DashStyle {
  return style === "dash" || style === "dot" || style === "dash_dot" || style === "dash_dot_dot";
}

/** The `strokes[]` of `STROKE_PARAMS::Stroke` and its `wrapAround`. */
function pattern(style: DashStyle, w: number, r: DashRatios): { strokes: number[]; wrap: number } {
  const dash = dashLength(w, r);
  const dot = dotLength(w);
  const gap = gapLength(w, r);
  switch (style) {
    case "dash":
      return { strokes: [dash, gap], wrap: 2 };
    case "dot":
      return { strokes: [dot, gap], wrap: 2 };
    case "dash_dot":
      return { strokes: [dash, gap, dot, gap], wrap: 4 };
    case "dash_dot_dot":
      return { strokes: [dash, gap, dot, gap, dot, gap], wrap: 6 };
  }
}

/** `KiROUND`: half away from zero (and no negative zero). */
const kiRound = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v)) + 0;

/** `clipOutCode` (libs/kimath/src/geometry/geometry_utils.cpp). */
function outCode(box: { x: number; y: number; r: number; b: number }, x: number, y: number): number {
  let code = y < box.y ? 2 : y > box.b ? 1 : 0;
  if (x < box.x) code |= 4;
  else if (x > box.r) code |= 8;
  return code;
}

/**
 * `ClipLine( const BOX2I*, int&, int&, int&, int& )`: Cohen-Sutherland, integer arithmetic as KiCad's (the 64-bit products divided with truncation). Returns
 * null when the line is outside the box ("Fast reject"), else the clipped ends.
 */
export function clipLine(box: { x: number; y: number; r: number; b: number }, a: P, b: P): [[number, number], [number, number]] | null {
  let [x1, y1] = a;
  let [x2, y2] = b;
  let o1 = outCode(box, x1, y1);
  let o2 = outCode(box, x2, y2);
  while (o1 || o2) {
    if (o1 & o2) return null;
    const out = o1 || o2;
    let x: number;
    let y: number;
    if (out & 1) {
      y = box.b;
      x = x1 + Math.trunc(((x2 - x1) * (y - y1)) / (y2 - y1));
    } else if (out & 2) {
      y = box.y;
      x = x1 + Math.trunc(((x2 - x1) * (y - y1)) / (y2 - y1));
    } else if (out & 8) {
      x = box.r;
      y = y1 + Math.trunc(((y2 - y1) * (x - x1)) / (x2 - x1));
    } else {
      x = box.x;
      y = y1 + Math.trunc(((y2 - y1) * (x - x1)) / (x2 - x1));
    }
    if (out === o1) {
      [x1, y1] = [x, y];
      o1 = outCode(box, x1, y1);
    } else {
      [x2, y2] = [x, y];
      o2 = outCode(box, x2, y2);
    }
  }
  return [
    [x1, y1],
    [x2, y2],
  ];
}

/** `STROKE_PARAMS::Stroke`'s `case SH_SEGMENT`: the strokes of one segment, from `start`, the pattern starting with a stroke; the last cut at the end. */
export function strokeSegment(start: P, end: P, style: DashStyle, width: number, ratios: DashRatios = DEFAULT_DASH_RATIOS, out: DashPiece[] = []): DashPiece[] {
  const { strokes, wrap } = pattern(style, width, ratios);
  // `BOX2I clip( start, KiROUND( end.x - start.x, end.y - start.y ) ); clip.Normalize();`
  const [w, h] = [kiRound(end[0] - start[0]), kiRound(end[1] - start[1])];
  const box = { x: Math.min(start[0], start[0] + w), y: Math.min(start[1], start[1] + h), r: Math.max(start[0], start[0] + w), b: Math.max(start[1], start[1] + h) };
  const theta = Math.atan2(end[1] - start[1], end[0] - start[0]);
  let [sx, sy] = [start[0], start[1]];
  for (let i = 0; i < 10000; i++) {
    // "Calculations MUST be done in doubles to keep from accumulating rounding errors as we go."
    const nx = sx + strokes[i % wrap]! * Math.cos(theta);
    const ny = sy + strokes[i % wrap]! * Math.sin(theta);
    const clipped = clipLine(box, [kiRound(sx), kiRound(sy)], [kiRound(nx), kiRound(ny)]);
    if (!clipped) break;
    if (i % 2 === 0) out.push(clipped);
    [sx, sy] = [nx, ny];
  }
  return out;
}

/** `EDA_ANGLE( const VECTOR2D& )`, degrees: `atan2`, with KiCad's exact values on the axes and the diagonals (-180, not 180, along -x). */
export function edaAngleOf(x: number, y: number): number {
  if (x === 0 && y === 0) return 0;
  if (y === 0) return x >= 0 ? 0 : -180;
  if (x === 0) return y >= 0 ? 90 : -90;
  if (x === y) return x >= 0 ? 45 : -180 + 45;
  if (x === -y) return x >= 0 ? -45 : 180 - 45;
  return (Math.atan2(y, x) * 180) / Math.PI;
}

/** `EDA_ANGLE::Normalize`: into [0, 360). */
const normalize = (a: number): number => {
  let v = a;
  while (v < 0) v += 360;
  while (v >= 360) v -= 360;
  return v;
};

/**
 * `STROKE_PARAMS::Stroke`'s `case SH_ARC`, for the arc `SHAPE_ARC( center, p0, angle )` (an increasing angle turns clockwise on the screen, as KiCad's arcs run
 * from their start to their end; 360 is a ring): the arc is walked from `p0`'s angle to the end's by each stroke's length over the circumference. A dash (and
 * a dash-dot's dash) is drawn as chords of half a degree, a dot as one chord.
 */
export function strokeArc(center: P, p0: P, angleDeg: number, style: DashStyle, width: number, ratios: DashRatios = DEFAULT_DASH_RATIOS, out: DashPiece[] = []): DashPiece[] {
  const { strokes, wrap } = pattern(style, width, ratios);
  const r = Math.hypot(p0[0] - center[0], p0[1] - center[1]);
  if (r === 0) return out;
  const C = 2 * Math.PI * r;
  // `GetP1()`: the start turned by the angle (`SHAPE_ARC`'s constructor rounds it to the grid of integers).
  const a0rad = Math.atan2(p0[1] - center[1], p0[0] - center[0]) + (angleDeg * Math.PI) / 180;
  const p1: P = [center[0] + kiRound(r * Math.cos(a0rad)), center[1] + kiRound(r * Math.sin(a0rad))];
  let startAngle = edaAngleOf(p0[0] - center[0], p0[1] - center[1]);
  let arcEndAngle = edaAngleOf(p1[0] - center[0], p1[1] - center[1]);
  if (arcEndAngle === startAngle) arcEndAngle = startAngle + 360; // "ring, not null"
  if (startAngle > arcEndAngle) {
    if (arcEndAngle < 0) arcEndAngle = normalize(arcEndAngle);
    else startAngle = normalize(startAngle) - 360;
  }
  const rad = Math.PI / 180;
  const at = (deg: number): [number, number] => [center[0] + kiRound(r * Math.cos(deg * rad)), center[1] + kiRound(r * Math.sin(deg * rad))];
  const increment = 0.5;
  for (let i = 0; i < 10000 && startAngle < arcEndAngle; i++) {
    const theta = (360 * strokes[i % wrap]!) / C;
    const endAngle = Math.min(startAngle + theta, arcEndAngle);
    if (i % 2 === 0) {
      if (((style === "dash_dot" || style === "dash_dot_dot") && i % wrap === 0) || style === "dash") {
        for (let cur = startAngle; cur < endAngle; cur += increment) {
          out.push([at(cur), at(Math.min(cur + increment, endAngle))]);
        }
      } else {
        out.push([at(startAngle), at(endAngle)]);
      }
    }
    startAngle = endAngle;
  }
  return out;
}

/**
 * The pieces `EDA_SHAPE::makeEffectiveShapes( true )` makes of a rounded rectangle's outline (`ROUNDRECT::TransformToPolygon`, libs/kimath/src/geometry/
 * roundrect.cpp: from right of the top-left corner, clockwise on the screen; `MakeCornerArcCw90`, `MakeSideArcCw180` where a side has no length), each
 * stroked by itself.
 */
function strokeRoundRect(x0: number, y0: number, x1: number, y1: number, radius: number, style: DashStyle, width: number, ratios: DashRatios, out: DashPiece[]): void {
  const [w, h] = [x1 - x0, y1 - y0];
  const r = radius;
  const xe = w - 2 * r;
  const ye = h - 2 * r;
  if (xe < 0 || ye < 0 || r < 0 || w <= 0 || h <= 0) return;
  if (xe === 0 && ye === 0) {
    // "It's a circle": `SHAPE_ARC( m_p0 + ( r, r ), m_p0 + ( -r, 0 ), ANGLE_360 )`, KiCad's own start point.
    strokeArc([x0 + r, y0 + r], [x0 - r, y0], 360, style, width, ratios, out);
    return;
  }
  const seg = (a: P, b: P) => strokeSegment(a, b, style, width, ratios, out);
  const arc = (c: P, from: P, sweep: number) => strokeArc(c, from, sweep, style, width, ratios, out);
  if (xe > 0) {
    seg([x0 + r, y0], [x0 + r + xe, y0]);
    if (ye > 0) {
      arc([x1 - r, y0 + r], [x1 - r, y0], 90);
      seg([x1, y0 + r], [x1, y0 + r + ye]);
      arc([x1 - r, y1 - r], [x1, y1 - r], 90);
    } else {
      arc([x1 - r, y0 + r], [x1 - r, y0], 180);
    }
    seg([x1 - r, y1], [x0 + r, y1]);
    if (ye > 0) {
      arc([x0 + r, y1 - r], [x0 + r, y1], 90);
      seg([x0, y1 - r], [x0, y0 + r]);
      arc([x0 + r, y0 + r], [x0, y0 + r], 90);
    } else {
      arc([x0 + r, y0 + r], [x0 + r, y1], 180);
    }
  } else {
    // "an oval the other way up"
    seg([x0, y1 - r], [x0, y0 + r]);
    arc([x0 + r, y0 + r], [x0, y0 + r], 180);
    seg([x1, y0 + r], [x1, y1 - r]);
    arc([x0 + r, y1 - r], [x1, y1 - r], 180);
  }
}

/** What a shape's line needs to be cut: the style, the width the pieces are measured by (`getLineThickness( GetWidth() )`), the ratios. */
export interface DashSpec {
  style: DashStyle;
  width: number;
  ratios?: DashRatios;
}

/**
 * Every stroke of a dashed board or footprint graphic: `makeEffectiveShapes( true )`'s pieces of the shape -- a line; an arc (`SHAPE_ARC( m_arcCenter, m_start,
 * GetArcAngle() )` in KiCad's order of its ends); a rectangle's four sides (`GetRectCorners`), or a rounded one's sides and corner arcs; a circle as a ring from
 * its `end` point; a polygon's edges, the closing one too; a curve's flattened segments (`buildBezierToSegmentsPointsList( getMaxError() )`) -- each through
 * `STROKE_PARAMS::Stroke`.
 */
export function dashPieces(s: Shape, spec: DashSpec, bezierMaxError: number): DashPiece[] {
  const { style, width } = spec;
  const ratios = spec.ratios ?? DEFAULT_DASH_RATIOS;
  const out: DashPiece[] = [];
  if (width <= 0) return out;
  switch (s.kind) {
    case "segment":
      strokeSegment(s.start, s.end, style, width, ratios, out);
      break;
    case "arc": {
      const k = kicadArc(s.start, s.mid, s.end);
      if (!k) {
        strokeSegment(s.start, s.end, style, width, ratios, out);
        break;
      }
      const c: P = [kiRound(k.centre[0]), kiRound(k.centre[1])];
      const a0 = edaAngleOf(k.start[0] - c[0], k.start[1] - c[1]);
      let a1 = edaAngleOf(k.end[0] - c[0], k.end[1] - c[1]);
      // `EDA_SHAPE::CalcArcAngles`: the end at or past the start.
      if (a1 === a0) a1 = a0 + 360;
      while (a1 < a0) a1 += 360;
      strokeArc(c, k.start, a1 - a0, style, width, ratios, out);
      break;
    }
    case "rect": {
      const [x0, y0] = [Math.min(s.start[0], s.end[0]), Math.min(s.start[1], s.end[1])];
      const [x1, y1] = [Math.max(s.start[0], s.end[0]), Math.max(s.start[1], s.end[1])];
      const radius = s.corner_radius ?? 0;
      if (radius > 0) {
        strokeRoundRect(x0, y0, x1, y1, radius, style, width, ratios, out);
      } else {
        // `GetRectCorners()`: start, (end.x, start.y), end, (start.x, end.y).
        const pts: P[] = [s.start, [s.end[0], s.start[1]], s.end, [s.start[0], s.end[1]]];
        for (let i = 0; i < 4; i++) strokeSegment(pts[i]!, pts[(i + 1) % 4]!, style, width, ratios, out);
      }
      break;
    }
    case "circle":
      strokeArc(s.center, s.end, 360, style, width, ratios, out);
      break;
    case "polygon":
      for (let i = 0; i < s.pts.length; i++) strokeSegment(s.pts[i]!, s.pts[(i + 1) % s.pts.length]!, style, width, ratios, out);
      break;
    case "bezier": {
      const pts = bezierPolyline(s.start, s.c1, s.c2, s.end, bezierMaxError);
      for (let i = 1; i < pts.length; i++) strokeSegment(pts[i - 1]!, pts[i]!, style, width, ratios, out);
      break;
    }
  }
  return out;
}
