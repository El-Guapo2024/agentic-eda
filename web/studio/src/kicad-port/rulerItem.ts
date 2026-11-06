// The ruler of the Measure tool and of Interactive Offset: a port of common/preview_items/ruler_item.cpp (`KIGFX::PREVIEW::RULER_ITEM::ViewDraw`,
// `GetDimensionStrings`, `drawTicksAlongLine`, `drawBacksideTicks`, `getTickFormatForScale`) and of common/preview_items/preview_utils.cpp (`DimensionLabel`,
// `GetConstantGlyphHeight`, `DrawTextNextToCursor`), commit 8303b2ad. Pure: the strokes and the texts in world coordinates (nm, y down) for a view of `scale`
// pixels per nm; the board's painter and the library editors' overlay draw them.
//
//   - the line from the origin to the end, as wide as `0.8 x` the preview text's stroke;
//   - next to the end, in the quadrant that keeps it on screen (away from the ruler first), "x: ..", "y: ..", "r: .." and "θ: ..°" -- dx and dy signed, the angle
//     counterclockwise from the X axis (`-EDA_ANGLE( rulerVec )`), in the user's unit at a preview's precision (`DimensionLabel`);
//   - with ticks (the Measure tool, `m_showTicks` true by default): a tick every 1-2-5 step of at least 10 pixels, longer and labelled every major and mid step
//     (`|....:....|`, `|....|`, `|.|.|`), and on the other side the ruler halved by three back ticks;
//   - the end: an arrowhead of two 30 degree strokes (`m_showEndArrowHead`, Interactive Offset's ruler) or, without one, a short stroke back from the origin.
// Not drawn: the drop shadows (`getShadowLayer`, the outline in the shadow colour drawn under the strokes on the other overlay layer).
import type { LengthUnit } from "../state/units";

export type Vec = [number, number];

export interface RulerSeg {
  a: Vec;
  b: Vec;
  /** Line width, nm. */
  width: number;
}

export interface RulerText {
  text: string;
  /** Where the text is anchored: its left or right end (as `halign` says), centred vertically (`GR_TEXT_V_ALIGN_CENTER`). */
  at: Vec;
  halign: "left" | "right";
  /** Counterclockwise on screen, degrees (`EDA_ANGLE`, KiCad's sense). */
  angleDeg: number;
  /** Glyph height and stroke width, nm. */
  size: number;
  stroke: number;
}

export interface RulerDrawing {
  segs: RulerSeg[];
  texts: RulerText[];
}

export interface RulerOptions {
  units: LengthUnit;
  /** `m_showTicks`, default true. */
  showTicks?: boolean;
  /** `m_showEndArrowHead`, default false. */
  showEndArrowHead?: boolean;
  /** `m_DisplayInvertXAxis` / `m_DisplayInvertYAxis`: the signs of dx and dy as shown. */
  flipX?: boolean;
  flipY?: boolean;
}

/** The view the ruler is drawn in: pixels per nm, and the screen's corners in world coordinates (for the label's quadrant and the culling of ticks). */
export interface RulerView {
  scale: number;
  /** The visible part of the world, [x0, y0, x1, y1]. */
  viewport: readonly [number, number, number, number];
}

const MAX_TICK_DENSITY = 10; // min pixels between tick marks
const MID_TICK_LENGTH_FACTOR = 1.5;
const MAJOR_TICK_LENGTH_FACTOR = 2.5;

/** `GetConstantGlyphHeight( gal, aRelativeSize )` off a HiDPI canvas: 8 .. 18 px tall by relative size -3 .. 3, stroke 0.20 of it, line pitch 1.9. */
export function constantGlyph(scale: number, relativeSize = 0): { size: number; stroke: number; linePitch: number } {
  const sizes = [8, 10, 12, 14, 15, 16, 18];
  const height = sizes[3 + relativeSize]! / scale;
  return { size: height, stroke: height * 0.2, linePitch: height * 1.9 };
}

/** `getTickLineWidth`: 0.8 of the preview text's stroke (no shadow). */
const tickLineWidth = (stroke: number) => stroke * 0.8;

/** `KiROUND`-free unit conversion of `DimensionLabel`: the value in the user's unit at a preview's precision, with the unit when asked. */
export function dimensionLabel(prefix: string, valueNm: number, units: LengthUnit | "deg", includeUnits = true): string {
  let num: string;
  let unit: string;
  switch (units) {
    case "mm":
      num = (valueNm / 1e6).toFixed(3);
      unit = " mm";
      break;
    case "mil":
      num = (valueNm / 25_400).toFixed(1);
      unit = " mils";
      break;
    case "in":
      num = (valueNm / 25_400_000).toFixed(4);
      unit = " in";
      break;
    default:
      num = valueNm.toFixed(1);
      unit = "°";
  }
  // `wxString::Format( "%.3f", -0.0001 )` prints "-0.000": keep the sign C's printf keeps
  return `${prefix ? `${prefix}: ` : ""}${num}${includeUnits ? unit : ""}`;
}

/** `EDA_ANGLE( const VECTOR2D& )`, degrees: atan2 with its exact cases (the negative X axis is -180, x == y gives 45 / -135, x == -y -45 / 135). */
export function vectorAngleDeg(v: Vec): number {
  const [x, y] = v;
  if (x === 0 && y === 0) return 0;
  if (y === 0) return x >= 0 ? 0 : -180;
  if (x === 0) return y >= 0 ? 90 : -90;
  if (x === y) return x >= 0 ? 45 : -180 + 45;
  if (x === -y) return x >= 0 ? -45 : 180 - 45;
  return (Math.atan2(y, x) * 180) / Math.PI;
}

/** `RotatePoint( VECTOR2D&, EDA_ANGLE )`: x' = x cos + y sin, y' = y cos - x sin (with its exact quarter turns). */
export function rotatePoint(v: Vec, deg: number): Vec {
  const a = ((deg % 360) + 360) % 360;
  if (a === 0) return [v[0], v[1]];
  if (a === 90) return [v[1], -v[0]];
  if (a === 180) return [-v[0], -v[1]];
  if (a === 270) return [-v[1], v[0]];
  const r = (a * Math.PI) / 180;
  return [v[1] * Math.sin(r) + v[0] * Math.cos(r), v[1] * Math.cos(r) - v[0] * Math.sin(r)];
}

const norm = (v: Vec) => Math.hypot(v[0], v[1]);
/** `VECTOR2::Resize`. */
const resize = (v: Vec, len: number): Vec => {
  const n = norm(v);
  return n === 0 ? [0, 0] : [(v[0] / n) * len, (v[1] / n) * len];
};
const add = (a: Vec, b: Vec): Vec => [a[0] + b[0], a[1] + b[1]];
const sub = (a: Vec, b: Vec): Vec => [a[0] - b[0], a[1] - b[1]];
const inside = (p: Vec, box: readonly [number, number, number, number], margin: number) => p[0] >= box[0] - margin && p[0] <= box[2] + margin && p[1] >= box[1] - margin && p[1] <= box[3] + margin;

/** `getTickFormatForScale`: from 1 nm (2.54 nm for imperial units, to land on round mils), the tick space grows by 2, 2, 2.5 ... until ticks are 10 px apart. */
export function tickFormatForScale(scale: number, units: LengthUnit): { tickSpace: number; majorStep: number; midStep: number } {
  const formats = [
    { divisionBase: 2, majorStep: 10, midStep: 5 },
    { divisionBase: 2, majorStep: 5, midStep: 0 },
    { divisionBase: 2.5, majorStep: 2, midStep: 0 },
  ];
  let tickSpace = units === "mm" ? 1 : 2.54;
  let f = 0;
  for (let guard = 0; guard < 200 && tickSpace * scale < MAX_TICK_DENSITY; guard++) {
    f = (f + 1) % formats.length;
    tickSpace *= formats[f]!.divisionBase;
  }
  return { tickSpace, majorStep: formats[f]!.majorStep, midStep: formats[f]!.midStep };
}

/** `GetDimensionStrings`: x and y (signed, flipped as the display options flip them), r, and θ counterclockwise. */
export function rulerStrings(origin: Vec, end: Vec, o: RulerOptions): string[] {
  const v = sub(end, origin);
  const t: Vec = [o.flipX ? -v[0] : v[0], o.flipY ? -v[1] : v[1]];
  // `-EDA_ANGLE( rulerVec ).AsDegrees()`: the negative X axis reads 180 (its -180 negated)
  const theta = -vectorAngleDeg(v);
  return [dimensionLabel("x", t[0], o.units), dimensionLabel("y", t[1], o.units), dimensionLabel("r", norm(v), o.units), dimensionLabel("θ", theta === 0 ? 0 : theta, "deg")];
}

/** `RULER_ITEM::ViewDraw` (the drawing layer, not the shadow one). `textWidth` measures a string at a glyph size (`StringBoundaryLimits`). */
export function rulerDrawing(origin: Vec, end: Vec, view: RulerView, o: RulerOptions, textWidth: (s: string, size: number) => number): RulerDrawing {
  const segs: RulerSeg[] = [];
  const texts: RulerText[] = [];
  const scale = view.scale;
  const g = constantGlyph(scale);
  const lineW = tickLineWidth(g.stroke);
  segs.push({ a: origin, b: end, width: lineW });
  const rulerVec = sub(end, origin);
  const strings = rulerStrings(origin, end, o);

  // the text quadrant: preferred away from the origin, else the first that fits on screen with no backward component (`dot >= 0`)
  const prefX = rulerVec[1] < 0 ? -1 : 1;
  const prefY = rulerVec[0] < 0 ? 1 : -1;
  const width = Math.max(...strings.map((s) => textWidth(s, g.size)));
  const height = g.linePitch * strings.length;
  const [vx0, vy0, vx1, vy1] = view.viewport;
  const screen = { w: (vx1 - vx0) * scale, h: (vy1 - vy0) * scale };
  const cursor = { x: (end[0] - vx0) * scale, y: (end[1] - vy0) * scale };
  const offsetX = 15;
  const offsetY = g.linePitch * scale;
  const fits = (sx: number, sy: number) => {
    const xStart = cursor.x + (sx < 0 ? offsetX : -offsetX);
    const [left, right] = sx < 0 ? [xStart, xStart + width * scale] : [xStart - width * scale, xStart];
    const [top, bottom] = sy > 0 ? [cursor.y - offsetY - height * scale, cursor.y - offsetY] : [cursor.y + offsetY, cursor.y + offsetY + height * scale];
    return left >= 0 && right <= screen.w && top >= 0 && bottom <= screen.h;
  };
  const candidates: Vec[] = [
    [prefX, prefY],
    [-prefX, prefY],
    [prefX, -prefY],
    [-prefX, -prefY],
  ];
  let chosen = candidates[0]!;
  let bestDot = -1;
  for (const c of candidates) {
    const dot = c[0] * prefX + c[1] * prefY;
    if (dot >= 0 && fits(c[0], c[1]) && dot > bestDot) {
      bestDot = dot;
      chosen = c;
    }
  }
  // `DrawTextNextToCursor`: 15 px beside the cursor, left-aligned on its right or right-aligned on its left; above it, shifted up by one more line than it has
  const pos: Vec = [end[0], end[1]];
  if (chosen[1] > 0) pos[1] -= g.linePitch * (strings.length + 1);
  const halign = chosen[0] < 0 ? "left" : "right";
  pos[0] += (chosen[0] < 0 ? 15 : -15) / scale;
  for (const s of strings) {
    pos[1] += g.linePitch;
    texts.push({ text: s, at: [pos[0], pos[1]], halign, angleDeg: 0, size: g.size, stroke: g.stroke });
  }

  const minorTickLen = 5 / scale;
  const majorTickLen = minorTickLen * MAJOR_TICK_LENGTH_FACTOR;

  if (o.showTicks ?? true) {
    // `drawTicksAlongLine( aView, origin, rulerVec, minorTickLen, ... )`
    const fmt = tickFormatForScale(scale, o.units);
    const fullMajor = minorTickLen * (MAJOR_TICK_LENGTH_FACTOR + 1);
    const tickLine = rotatePoint(rulerVec, 90);
    const numTicks = Math.ceil(norm(rulerVec) / fmt.tickSpace);
    const label = constantGlyph(scale, -1);
    const labelAngle = -vectorAngleDeg(tickLine);
    const labelOffset = resize(tickLine, fullMajor);
    const rulerAngle = vectorAngleDeg(rulerVec);
    const [labelHalign, labelTextAngle]: ["left" | "right", number] = rulerAngle > 0 ? ["left", labelAngle] : ["right", labelAngle + 180];
    for (let i = 0; i < numTicks; i++) {
      const tickPos = add(origin, resize(rulerVec, fmt.tickSpace * i));
      if (!inside(tickPos, view.viewport, fullMajor * 2)) continue;
      let length = minorTickLen;
      let drawLabel = false;
      if (i % fmt.majorStep === 0) {
        drawLabel = true;
        length *= MAJOR_TICK_LENGTH_FACTOR;
      } else if (fmt.midStep && i % fmt.midStep === 0) {
        drawLabel = true;
        length *= MID_TICK_LENGTH_FACTOR;
      }
      segs.push({ a: tickPos, b: add(tickPos, resize(tickLine, length)), width: label.stroke / 2 });
      if (drawLabel) texts.push({ text: dimensionLabel("", fmt.tickSpace * i, o.units, false), at: add(tickPos, labelOffset), halign: labelHalign, angleDeg: labelTextAngle, size: label.size, stroke: label.stroke });
    }
    // `drawBacksideTicks( aView, origin, rulerVec, majorTickLen, 2, ... )`
    const back = resize(rotatePoint(rulerVec, -90), majorTickLen);
    const backSpace = norm(rulerVec) / 2;
    for (let i = 0; i < 3; i++) {
      const p = add(origin, resize(rulerVec, backSpace * i));
      if (!inside(p, view.viewport, majorTickLen * 4)) continue;
      segs.push({ a: p, b: add(p, back), width: lineW });
    }
  }

  if (o.showEndArrowHead) {
    const head1 = resize(rotatePoint(rulerVec, 30), majorTickLen);
    const head2 = resize(rotatePoint(rulerVec, -30), majorTickLen);
    segs.push({ a: end, b: sub(end, head1), width: lineW });
    segs.push({ a: end, b: sub(end, head2), width: lineW });
  } else {
    // "draw the back of the origin crosshair"
    segs.push({ a: origin, b: add(origin, resize(rulerVec, -minorTickLen * MID_TICK_LENGTH_FACTOR)), width: lineW });
  }
  return { segs, texts };
}
