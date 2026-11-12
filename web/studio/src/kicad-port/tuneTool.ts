// The length tuner's tool state and geometry: what the canvas keeps while a tuning pattern is being placed, the status box beside the cursor, the outline of a
// pattern and its four edit points. Pure: no store, no DOM. The server side is crates/cli/src/tune_api.rs (KiCad's `DRAWING_TOOL::PlaceTuningPattern` and
// `PCB_TUNING_PATTERN`, pcbnew/generators/pcb_tuning_pattern.cpp); unit-tested in tuneTool.test.ts.
//
//   TUNING_STATUS_VIEW_ITEM::ViewDraw   the box: "current length | min | max" over the length and the range, red where the length is outside it
//   PCB_TUNING_PATTERN::getOutline      the dashed outline of a pattern: the baseline offset by the amplitude (+ half the track), rounded all round
//   PCB_TUNING_PATTERN::MakeEditPoints  the four handles (the server sends their places; this decides which one a click is on)

import type { TuningPatterns, Nm } from "../api/types";
import type { PatternSide, TargetRange, TuneKind, TunePiece, TuneReadout, TuneSessionReply, TuneSettings, TuningPattern } from "../api/tuneTypes";
import { ATTOSECONDS_PER_PS, DELAY_UNCONSTRAINED_AS, LENGTH_UNCONSTRAINED_NM } from "../api/tuneTypes";
import { tuningPatternsOf } from "./setupPages";

/** `DrawState`'s `tune` variant: a tuning pattern on its way to the board. */
export interface TuneDrawState {
  kind: "tune";
  mode: TuneKind;
  layer: string;
  net: string;
  /** Where the pattern starts, on the track. */
  origin: [Nm, Nm];
  /** The new line: the track up to the pattern, the meander, the track after it (and what stays of the tracks it replaces). */
  pieces: TunePiece[];
  /** The tracks the preview takes the place of: not drawn while it is shown. */
  hide: string[];
  readout: TuneReadout | null;
  pattern: TuningPattern | null;
  /** The four edit points (`MakeEditPoints`). */
  handles: [Nm, Nm][];
  /** The id of the pattern of the board being edited through its edit points (`genStartEdit`), if it is not a new one. */
  editing?: string;
  /** Whether the cursor has moved the end of the pattern yet. */
  moved: boolean;
}

/** The `.kicad_pcb` word for a mode, and back. */
export function kindOfMode(mode: TuningPattern["mode"]): TuneKind {
  return mode === "diff_pair" ? "diffpair" : mode === "diff_pair_skew" ? "skew" : "single";
}

export function modeOfKind(kind: TuneKind): TuningPattern["mode"] {
  return kind === "diffpair" ? "diff_pair" : kind === "skew" ? "diff_pair_skew" : "single";
}

/** The draw state a start / move / step / settings / edit reply makes (the pieces and the readout are kept from before when the reply carries none). */
export function tuneStateFromReply(reply: TuneSessionReply, prev: TuneDrawState | null, base: { mode: TuneKind; layer: string; net?: string }): TuneDrawState {
  const pattern = reply.pattern ?? prev?.pattern ?? null;
  const origin: [Nm, Nm] = reply.origin ? [reply.origin.x, reply.origin.y] : pattern ? [pattern.origin.x, pattern.origin.y] : (prev?.origin ?? [0, 0]);
  return {
    kind: "tune",
    mode: base.mode,
    layer: pattern?.layer ?? base.layer,
    net: reply.net ?? prev?.net ?? base.net ?? "",
    origin,
    pieces: reply.preview ? reply.preview.pieces : (prev?.pieces ?? []),
    hide: reply.preview ? reply.preview.hide : (prev?.hide ?? []),
    readout: reply.readout ?? prev?.readout ?? null,
    pattern,
    handles: reply.handles ?? prev?.handles ?? [],
    editing: reply.editing ? (reply.pattern?.id ?? prev?.editing) : undefined,
    moved: reply.moved ?? prev?.moved ?? false,
  };
}

// ------------------------------------------------------------------------------------------------------------------------------------ the status box

export interface ReadoutBox {
  /** The three headings: the label, `min`, `max`. */
  header: [string, string, string];
  /** The three values, as the unit formatter prints them; `---` for a range the pattern does not have. */
  values: [string, string, string];
  /** Whether each value is red (outside the range) or green. */
  bad: [boolean, boolean, boolean];
}

/** The number of a formatted value without its unit: `MessageTextFromValue( aValue, false )`. */
function withoutUnit(text: string): string {
  return text.replace(/\s*(mm|mil|in|ps|\u00b5m|um)$/, "");
}

/**
 * `TUNING_STATUS_VIEW_ITEM`: the length is red when it is outside `[min, max]`, the minimum when the length is below it, the maximum when it is above (`ViewDraw`).
 * The current value has its unit, the minimum and the maximum not (`SetCurrent`: `MessageTextFromValue( aCurrent, true, .. )`, `SetMinMax`: `.., false, ..`); no target
 * is `---` (`ClearMinMax`). `fmt` prints a length in nanometres.
 */
export function readoutBox(r: TuneReadout, fmt: (nm: number) => string): ReadoutBox {
  const um = (mm: number) => mm * 1e6; // mm to nm
  // a time domain pattern: the box reads the delay and the range of the delay, in picoseconds (`SetIsTimeDomain`)
  if (r.time_domain && r.delay) {
    const d = r.delay;
    const has = d.min != null && d.max != null;
    const below = has && d.current < (d.min as number);
    const above = has && d.current > (d.max as number);
    return { header: [d.label, "min", "max"], values: [fmtPs(d.current), d.min == null ? "---" : withoutUnit(fmtPs(d.min)), d.max == null ? "---" : withoutUnit(fmtPs(d.max))], bad: [below || above, below, above] };
  }
  const min = r.min, max = r.max;
  const has = min != null && max != null;
  const below = has && r.current < min;
  const above = has && r.current > max;
  return {
    header: [r.label, "min", "max"],
    values: [fmt(um(r.current)), min == null ? "---" : withoutUnit(fmt(um(min))), max == null ? "---" : withoutUnit(fmt(um(max)))],
    bad: [below || above, below, above],
  };
}

/** Where `TUNING_STATUS_VIEW_ITEM::ViewDraw` puts the box and its texts, in board nanometres, for the status item at `at` (the mouse position,
 *  `statusItem->SetPosition( GetMousePosition() )`); `px` turns screen pixels into nanometres at the view's zoom (`GetConstantGlyphHeight`: a size in pixels
 *  over the world scale, so the box is the same size on the screen at every zoom).
 *
 *  `GetConstantGlyphHeight( gal, -2 )` is the header's glyph (10 px), `( gal, -1 )` the values' (12 px); a glyph is as wide as it is high, its stroke a fifth of it, its
 *  line pitch 1.9 of it. `margin( KiROUND( glyphWidth * 0.4 ), glyphWidth )`, `size( glyphWidth * 25 + margin.x * 2, header + text )`, `offset( margin.x * 2, -( size.y +
 *  margin.y * 2 ) )`: the drop shadow box runs from `pos + offset - margin` to `pos + offset + size + margin` -- above and to the right of the cursor. The header row
 *  is at `pos + offset` (the label, then `min` 11 glyphs and a margin further, `max` 7 glyphs and a margin further), the values `KiROUND( headerLinePitch * 1.3 )`
 *  below it, each text left-aligned and centred on its row (`TEXT_ATTRIBUTES`' default vertical centre). The sizes are KiCad's for a canvas that is not HiDPI. */
export function statusBoxLayout(at: [number, number], px: (n: number) => number): {
  box: [number, number, number, number];
  header: { size: number; stroke: number; y: number; xs: [number, number, number] };
  values: { size: number; stroke: number; y: number; xs: [number, number, number] };
} {
  const headerH = 10, textH = 12;
  const glyph = textH;
  const margin = [Math.round(glyph * 0.4), glyph];
  const size = [glyph * 25 + margin[0]! * 2, headerH + textH];
  const offset = [margin[0]! * 2, -(size[1]! + margin[1]! * 2)];
  const x0 = offset[0]!, y0 = offset[1]!;
  const xs: [number, number, number] = [x0, x0 + glyph * 11 + margin[0]!, x0 + glyph * 11 + margin[0]! + glyph * 7 + margin[0]!];
  const valuesY = y0 + Math.round(headerH * 1.9 * 1.3);
  const X = (dx: number) => at[0] + px(dx);
  const Y = (dy: number) => at[1] + px(dy);
  return {
    box: [X(x0 - margin[0]!), Y(y0 - margin[1]!), X(x0 + size[0]! + margin[0]!), Y(y0 + size[1]! + margin[1]!)],
    header: { size: px(headerH), stroke: px(headerH) / 5, y: Y(y0), xs: xs.map(X) as [number, number, number] },
    values: { size: px(textH), stroke: px(textH) / 5, y: Y(valuesY), xs: xs.map(X) as [number, number, number] },
  };
}

/** A time in picoseconds the way the unit formatter writes it: up to four decimals, no trailing zeros, with the unit. */
export function fmtPs(ps: number): string {
  const s = ps.toFixed(4).replace(/\.?0+$/, "");
  return `${s === "" || s === "-" ? "0" : s} ps`;
}

/** One line for the message panel / status bar while a pattern is placed: `current length 21.99 mm, target 22 mm (-0.01 mm), tuned`. */
export function readoutSummary(r: TuneReadout, fmt: (um: number) => string): string {
  const um = (mm: number) => mm * 1e6;
  if (r.time_domain && r.delay) {
    const d = r.delay;
    const parts = [`${d.label} ${fmtPs(d.current)}`];
    if (d.opt != null) parts.push(`${d.label === "current skew" ? "target skew" : "target"} ${fmtPs(d.opt)}${d.delta != null ? ` (${d.delta >= 0 ? "+" : "-"}${fmtPs(Math.abs(d.delta))})` : ""}`);
    parts.push(r.status === "tuned" ? "tuned" : r.status === "too_short" ? "too short" : "too long");
    if (r.constraint) parts.push(`from ${r.constraint}`);
    return parts.join(", ");
  }
  const parts = [`${r.label} ${fmt(um(r.current))}`];
  if (r.opt != null) parts.push(`${r.label === "current skew" ? "target skew" : "target"} ${fmt(um(r.opt))}${r.delta != null ? ` (${r.delta >= 0 ? "+" : "-"}${fmt(um(Math.abs(r.delta)))})` : ""}`);
  parts.push(r.status === "tuned" ? "tuned" : r.status === "too_short" ? "too short" : "too long");
  if (r.constraint) parts.push(`from ${r.constraint}`);
  return parts.join(", ");
}

// --------------------------------------------------------------------------------------------------------------------------------- settings

/** The meander settings a tuner starts from for a kind (Board Setup > Length-tuning Patterns), in the keys the server reads. */
export function pageSettings(patterns: TuningPatterns | undefined | null, kind: TuneKind): Required<Pick<TuneSettings, "min_amplitude" | "max_amplitude" | "spacing" | "corner_radius_percent" | "rounded" | "single_sided">> {
  const p = tuningPatternsOf(patterns);
  const s = kind === "single" ? p.single_track : kind === "diffpair" ? p.diff_pair : p.diff_pair_skew;
  return { min_amplitude: s.min_amplitude_nm, max_amplitude: s.max_amplitude_nm, spacing: s.spacing_nm, corner_radius_percent: s.corner_radius_percentage, rounded: s.corner_style === "round", single_sided: s.single_sided };
}

/** A pattern's settings in the keys the server reads (what the dialog edits and `update` takes). */
export function settingsOfPattern(p: TuningPattern): Required<TuneSettings> {
  return {
    min_amplitude: p.min_amplitude,
    max_amplitude: p.max_amplitude,
    spacing: p.spacing,
    corner_radius_percent: p.corner_radius_percent,
    rounded: p.rounded,
    single_sided: p.single_sided,
    initial_side: p.initial_side,
    override_custom_rules: p.override_custom_rules,
    target_length: p.target_length.opt >= LENGTH_UNCONSTRAINED_NM ? null : p.target_length,
    target_skew: p.target_skew,
    time_domain: p.is_time_domain ?? false,
    target_delay: !p.target_delay || p.target_delay.opt >= DELAY_UNCONSTRAINED_AS ? null : psRange(p.target_delay),
    target_skew_delay: psRange(p.target_skew_delay ?? { min: -100_000, opt: 0, max: 100_000 }),
  };
}

/** A delay target (attoseconds) in picoseconds, the unit the server's settings take. */
export function psRange(r: TargetRange): TargetRange {
  return { min: r.min / ATTOSECONDS_PER_PS, opt: r.opt / ATTOSECONDS_PER_PS, max: r.max / ATTOSECONDS_PER_PS };
}

/** What the tool carries to its next session (`meanderSettings.m_spacing = placer->MeanderSettings().m_spacing` ...): the pattern's settings, but not its target length -- that is a pattern's own. */
export function carriedSettings(p: TuningPattern, prev: TuneSettings | undefined): TuneSettings {
  const s = settingsOfPattern(p);
  return { ...prev, min_amplitude: s.min_amplitude, max_amplitude: s.max_amplitude, spacing: s.spacing, corner_radius_percent: s.corner_radius_percent, rounded: s.rounded, single_sided: s.single_sided, override_custom_rules: s.override_custom_rules };
}

export type TuneKey = "amplitude_up" | "amplitude_down" | "spacing_up" | "spacing_down";

/** The `pcbnew.lengthTuner.*` actions and the step each one is. */
export const TUNE_KEY_ACTIONS: Record<string, TuneKey> = {
  "pcbnew.lengthTuner.AmplIncrease": "amplitude_up",
  "pcbnew.lengthTuner.AmplDecrease": "amplitude_down",
  "pcbnew.lengthTuner.SpacingIncrease": "spacing_up",
  "pcbnew.lengthTuner.SpacingDecrease": "spacing_down",
};

// ----------------------------------------------------------------------------------------------------------------------------------- outline

type P = [number, number];

const sub = (a: P, b: P): P => [a[0] - b[0], a[1] - b[1]];
const len = (v: P) => Math.hypot(v[0], v[1]);

/** The unit normal of a segment on the `sign` side: `Perpendicular()` is `(-dy, dx)`, which is the side `SEG::Side` calls `1`. */
function normal(a: P, b: P, sign: 1 | -1): P {
  const d = sub(b, a);
  const l = len(d) || 1;
  return [(-d[1] / l) * sign, (d[0] / l) * sign];
}

/**
 * The arc round `centre` from direction `from` (a unit vector) to direction `to`, as points at most 15 degrees apart, the end points not included. `sweep` is the angle
 * turned through (radians, positive from `from` towards the next quarter turn); without one it is the short way from `from` to `to`.
 */
function arcPoints(centre: P, from: P, to: P, r: number, sweep?: number): P[] {
  const a0 = Math.atan2(from[1], from[0]);
  let da = sweep ?? Math.atan2(to[1], to[0]) - a0;
  if (sweep === undefined) {
    while (da > Math.PI) da -= 2 * Math.PI;
    while (da < -Math.PI) da += 2 * Math.PI;
  }
  const n = Math.max(1, Math.ceil(Math.abs(da) / (Math.PI / 12)));
  const out: P[] = [];
  for (let i = 1; i < n; i++) {
    const a = a0 + (da * i) / n;
    out.push([centre[0] + r * Math.cos(a), centre[1] + r * Math.sin(a)]);
  }
  return out;
}

function intersect(a: P, b: P, c: P, d: P): P | null {
  const r = sub(b, a);
  const s = sub(d, c);
  const den = r[0] * s[1] - r[1] * s[0];
  if (Math.abs(den) < 1e-9) return null;
  const t = ((c[0] - a[0]) * s[1] - (c[1] - a[1]) * s[0]) / den;
  return [a[0] + t * r[0], a[1] + t * r[1]];
}

/**
 * The polyline `d` away from `pts` on one side (`sign`: the side `SEG::Side` calls 1 or -1), with a round join where the line turns away from that side and the two offset
 * segments' intersection where it turns towards it (`SHAPE_LINE_CHAIN::OffsetLine` with `ROUND_ALL_CORNERS`).
 */
export function offsetSide(pts: readonly P[], d: number, sign: 1 | -1): P[] {
  const out: P[] = [];
  const segs: { a: P; b: P; n: P }[] = [];
  for (let i = 0; i + 1 < pts.length; i++) {
    if (pts[i]![0] === pts[i + 1]![0] && pts[i]![1] === pts[i + 1]![1]) continue;
    const n = normal(pts[i]!, pts[i + 1]!, sign);
    segs.push({ a: [pts[i]![0] + n[0] * d, pts[i]![1] + n[1] * d], b: [pts[i + 1]![0] + n[0] * d, pts[i + 1]![1] + n[1] * d], n });
  }
  segs.forEach((s, i) => {
    if (i === 0) out.push(s.a);
    else {
      const prev = segs[i - 1]!;
      const hit = intersect(prev.a, prev.b, s.a, s.b);
      const along = (prev.b[0] - prev.a[0]) * (s.a[0] - prev.b[0]) + (prev.b[1] - prev.a[1]) * (s.a[1] - prev.b[1]);
      // the offset segments cross: the line turned towards this side (the intersection is behind the end of the previous one)
      if (hit && along < 0) out.push(hit);
      else {
        const vertex: P = [prev.b[0] - prev.n[0] * d, prev.b[1] - prev.n[1] * d];
        out.push(prev.b, ...arcPoints(vertex, prev.n, s.n, d), s.a);
      }
    }
    if (i === segs.length - 1) out.push(s.b);
  });
  return out;
}

/** A closed outline `d` all round the polyline `pts`: both sides and a round cap at each end (`OffsetLineChain( .., ROUND_ALL_CORNERS, .., false )`). */
export function offsetOutline(pts: readonly P[], d: number): P[] {
  if (pts.length < 2) return [];
  const left = offsetSide(pts, d, -1);
  const right = offsetSide(pts, d, 1);
  if (left.length === 0 || right.length === 0) return [];
  const first = pts[0]!;
  const last = pts[pts.length - 1]!;
  const n0 = normal(pts[0]!, pts.find((q) => q[0] !== first[0] || q[1] !== first[1]) ?? pts[1]!, 1);
  const rev = [...pts].reverse();
  const n1 = normal(rev[0]!, rev.find((q) => q[0] !== last[0] || q[1] !== last[1]) ?? rev[1]!, 1);
  // from the right side's start round the first end to the left side's start; along the left side; round the last end; back along the right side
  // half a turn each, the way round that passes behind the first point and beyond the last one
  const startCap = arcPoints(first, n0, [-n0[0], -n0[1]], d, Math.PI);
  const endCap = arcPoints(last, n1, [-n1[0], -n1[1]], d, Math.PI);
  return [right[0]!, ...startCap, ...left, ...endCap, ...right.slice().reverse()];
}

/** `getOutline`: the dashed outline of a pattern, one polygon for each baseline (a pair's two are drawn over each other). Micrometres. */
export function patternOutline(p: TuningPattern): P[][] {
  const base = (p.base_line.length >= 2 ? p.base_line : [p.origin, p.end]).map((q): P => [q.x, q.y]);
  const width = p.last_track_width;
  const gap = p.last_diff_pair_gap;
  const pair = p.mode === "diff_pair";
  const baselineOffset = pair ? (gap + width) / 2 : 0;
  // `minAllowedAmplitude`: a round corner needs the track's width, a chamfered one KiCad's `width * tan( 1 - tan( 22.5 deg ) )`
  const minAllowed = p.rounded ? baselineOffset + width : baselineOffset + width * Math.tan(1 - Math.tan((22.5 * Math.PI) / 180));
  const clamped = Math.max(p.max_amplitude, minAllowed);
  const coupled = (p.base_line_coupled ?? []).map((q): P => [q.x, q.y]);
  const lines: P[][] = pair && coupled.length >= 2 ? [base, coupled] : [base];
  if (p.single_sided) {
    // only the side the first meander is on: `initial_side >= 0 ? right : left`
    const sign: 1 | -1 = p.initial_side === "left" ? -1 : 1;
    return lines.map((line) => {
      const outer = pair ? clamped + width + gap / 2 : clamped + width / 2;
      const inner = pair ? width / 2 : 0;
      const far = offsetSide(line, outer, sign);
      const near = inner > 0 ? offsetSide(line, inner, sign) : line;
      return [...far, ...near.slice().reverse()];
    });
  }
  const amplitude = pair ? clamped + gap / 2 + width : clamped + width / 2;
  return lines.map((line) => offsetOutline(line, amplitude));
}

/** Whether `p` is inside the closed polygon `poly` (even-odd). */
function insidePolygon(p: P, poly: readonly P[]): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i]!;
    const [xj, yj] = poly[j]!;
    if (yi > p[1] !== yj > p[1] && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

/** Where on `a`..`b` (0..1) it crosses the edges of the closed polygon `poly`. */
function crossings(a: P, b: P, poly: readonly P[]): number[] {
  const out: number[] = [];
  const r = sub(b, a);
  for (let i = 0; i < poly.length; i++) {
    const c = poly[i]!;
    const d = poly[(i + 1) % poly.length]!;
    const s = sub(d, c);
    const den = r[0] * s[1] - r[1] * s[0];
    if (Math.abs(den) < 1e-12) continue;
    const t = ((c[0] - a[0]) * s[1] - (c[1] - a[1]) * s[0]) / den;
    const u = ((c[0] - a[0]) * r[1] - (c[1] - a[1]) * r[0]) / den;
    if (t > 0 && t < 1 && u >= 0 && u <= 1) out.push(t);
  }
  return out;
}

/**
 * The outline of the union of closed polygons (`SHAPE_POLY_SET::BooleanAdd`), as the pieces of each polygon's edges that lie outside every other one: what
 * `merged.Outline( 0 )` draws when the polygons overlap. Open polylines.
 */
export function unionOutline(polys: readonly (readonly P[])[]): P[][] {
  const out: P[][] = [];
  polys.forEach((poly, i) => {
    const others = polys.filter((_, j) => j !== i);
    let run: P[] = [];
    const flush = () => {
      if (run.length >= 2) out.push(run);
      run = [];
    };
    for (let k = 0; k < poly.length; k++) {
      const a = poly[k]!;
      const b = poly[(k + 1) % poly.length]!;
      const ts = [0, 1, ...others.flatMap((o) => crossings(a, b, o))].sort((x, y) => x - y);
      const at = (t: number): P => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
      for (let m = 0; m + 1 < ts.length; m++) {
        const mid = at((ts[m]! + ts[m + 1]!) / 2);
        if (others.every((o) => !insidePolygon(mid, o))) {
          if (run.length === 0) run.push(at(ts[m]!));
          run.push(at(ts[m + 1]!));
        } else flush();
      }
    }
    flush();
  });
  return out;
}

/**
 * What `ViewDraw` draws of `getOutline()`: one closed outline, or for a pair the outline of the union of its two (`merged.BooleanAdd( poly, polyCoupled )`, then
 * `merged.Outline( 0 )`; both sides or, single-sided, `chain1` and `chain2`) -- the pieces of each that lie outside the other (UI-LEFT-2 RR5). A pair whose two
 * outlines do not meet has both drawn (KiCad draws the first only).
 */
export function patternOutlineDrawn(p: TuningPattern): { pts: P[]; closed: boolean }[] {
  const polys = patternOutline(p).filter((q) => q.length >= 3);
  if (polys.length < 2) return polys.map((pts) => ({ pts, closed: true }));
  return unionOutline(polys).map((pts) => ({ pts, closed: false }));
}

/** The pattern's baseline, as dashed segments are drawn along it (`ViewDraw`: `m_baseLine`, or the line from the origin to the end). */
export function patternBaselines(p: TuningPattern): P[][] {
  const line = (pts: readonly { x: Nm; y: Nm }[]): P[] => pts.map((q): P => [q.x, q.y]);
  const out: P[][] = [p.base_line.length >= 2 ? line(p.base_line) : [[p.origin.x, p.origin.y], [p.end.x, p.end.y]]];
  if (p.mode === "diff_pair" && (p.base_line_coupled?.length ?? 0) >= 2) out.push(line(p.base_line_coupled!));
  return out;
}

/** Which edit point `at` is on (the nearest one within `radiusNm`), or null. */
export function handleAt(handles: readonly [Nm, Nm][], at: { x: Nm; y: Nm }, radiusNm: Nm): number | null {
  let best: number | null = null;
  let bestD = radiusNm;
  handles.forEach((h, i) => {
    const d = Math.hypot(h[0] - at.x, h[1] - at.y);
    if (d <= bestD) {
      bestD = d;
      best = i;
    }
  });
  return best;
}

/** What the sides of the settings dialog's "Initial side" choice are. */
export const SIDES: PatternSide[] = ["left", "right"];
