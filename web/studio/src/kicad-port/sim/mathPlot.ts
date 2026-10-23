// The simulator's plot, as KiCad's mathplot draws it (commit 8303b2ad):
//
//   common/widgets/mathplot.cpp   mpWindow (Fit, DoZoom, ZoomRect, ZoomUndo/Redo, AdjustLimitedView, ViewNeedsRefitting, RecomputeDesiredX/Y,
//                                 PerformMouseWheelAction, x2p/y2p/p2x/p2y), mpScaleX/mpScaleXLog/mpScaleY::recalculateTicks, computeSlaveTicks,
//                                 mpScaleXBase/mpScaleY::Plot, mpFXY::Plot, mpInfoLegend::Plot, mpScaleBase::ExtendDataRange, TransformToPlot
//   eeschema/sim/sim_plot_tab.cpp LIN_SCALE / LOG_SCALE / TIME_SCALE (formatLabels, getSISuffix, countDecimalDigits, formatFloat),
//                                 SIM_PLOT_TAB::updateAxes / prepareDCAxes / EnsureThirdYAxisExists / SetTraceData / UpdateAxisVisibility /
//                                 ResetScales / UpdateTraceStyle, CURSOR::doSetCoordX / Plot
//   eeschema/sim/sim_plot_colors.cpp SIM_PLOT_COLORS::FillDefaultColorList / GenerateColor
//
// mathplot keeps every layer in "plot coordinates": each scale maps its data range onto 0..1 (`TransformToPlot`), the window's bounding box is
// 0..1 on both axes (`UpdateBBox`), and a zoom moves the window's position and scale over that box. This module is the model -- scales, ticks,
// labels, the view and its zoom history, a cursor's value -- and `SimPlot.tsx` paints it on a canvas in the same order mathplot plots its
// layers. Text widths come from the canvas (`measure`), as mathplot asks the DC for them.

export type Align = "bottom" | "left" | "right" | "border_right";

export interface Scale {
  kind: "lin" | "log" | "time";
  unit: string;
  /** The name before `formatLabels` adds the unit (`m_base_axis_label`). */
  base: string;
  /** The name shown: `base (suffix)` for a linear scale. */
  name: string;
  align: Align;
  nameAlign: Align;
  minV: number;
  maxV: number;
  rangeSet: boolean;
  /** `SetAxisMinMax( lock, min, max )`. */
  locked: boolean;
  axisMin: number;
  axisMax: number;
  /** `SetMasterScale`: a Y scale whose ticks are placed where its master's are. */
  master: Scale | null;
  /** mathplot's `m_ticks`: true draws ticks, false the grid. */
  ticks: boolean;
  visible: boolean;
  /** TIME_SCALE's start and end time. */
  startTime: number;
  endTime: number;
  offset: number;
  scale: number;
  absVisibleMax: number;
  tickValues: number[];
  tickLabels: { pos: number; label: string; visible: boolean }[];
}

export function newScale(kind: Scale["kind"], unit: string, align: Align, base = ""): Scale {
  return {
    kind,
    unit,
    base,
    name: base,
    align,
    nameAlign: align,
    minV: 0,
    maxV: 0,
    rangeSet: false,
    locked: false,
    axisMin: 0,
    axisMax: 0,
    master: null,
    // LIN_SCALE and LOG_SCALE pass `ticks = false` to mathplot: a new plot shows its grid.
    ticks: false,
    visible: true,
    startTime: 0,
    endTime: 1,
    offset: 0,
    scale: 1,
    absVisibleMax: 0,
    tickValues: [],
    tickLabels: [],
  };
}

/** `mpScaleBase::ExtendDataRange` (TIME_SCALE: never past the simulation's own start and end). */
export function extendDataRange(s: Scale, min: number, max: number): void {
  if (!s.rangeSet) {
    s.minV = min;
    s.maxV = max;
    s.rangeSet = true;
  } else {
    s.minV = Math.min(min, s.minV);
    s.maxV = Math.max(max, s.maxV);
  }
  if (s.minV === s.maxV) {
    s.minV -= 1.0;
    s.maxV += 1.0;
  }
  if (s.kind === "time") {
    if (s.minV < s.startTime) s.minV = s.startTime;
    if (s.maxV > s.endTime) s.maxV = s.endTime;
  }
}

/** `mpScaleBase::ResetDataRange` (TIME_SCALE: the simulation's start and end). */
export function resetDataRange(s: Scale): void {
  if (s.kind === "time") {
    s.minV = s.startTime;
    s.maxV = s.endTime;
    s.rangeSet = true;
  } else {
    s.rangeSet = false;
  }
}

/** `TransformToPlot`. */
export function toPlot(s: Scale, x: number): number {
  if (s.kind === "log") {
    const lo = Math.log10(s.minV);
    const hi = Math.log10(s.maxV);
    return (Math.log10(x) - lo) / (hi - lo);
  }
  return (x + s.offset) * s.scale;
}

/** `TransformFromPlot`. */
export function fromPlot(s: Scale, p: number): number {
  if (s.kind === "log") {
    const lo = Math.log10(s.minV);
    const hi = Math.log10(s.maxV);
    return 10 ** (p * (hi - lo) + lo);
  }
  return p / s.scale - s.offset;
}

// --------------------------------------------------------------------------------------------------------------- the window

export interface PlotView {
  scrX: number;
  scrY: number;
  marginTop: number;
  marginRight: number;
  marginBottom: number;
  marginLeft: number;
  posX: number;
  posY: number;
  scaleX: number;
  scaleY: number;
  desiredXmin: number;
  desiredXmax: number;
  desiredYmin: number;
  desiredYmax: number;
  yLocked: boolean;
  undo: [number, number, number, number][];
  redo: [number, number, number, number][];
}

/** `mpWindow`'s state after `SIM_PLOT_TAB`'s constructor: margins 30, 70, 45, 70 (top, right, bottom, left), `LimitView( true )`. */
export function newView(scrX = 640, scrY = 400): PlotView {
  return { scrX, scrY, marginTop: 30, marginRight: 70, marginBottom: 45, marginLeft: 70, posX: 0, posY: 0, scaleX: 1, scaleY: 1, desiredXmin: 0, desiredXmax: 1, desiredYmin: 0, desiredYmax: 1, yLocked: false, undo: [], redo: [] };
}

const TOP_BOTTOM_GAP = 0.03;
const LEFT_RIGHT_GAP = 0.0;
/** `mpWindow::zoomIncrementalFactor`. */
export const ZOOM_FACTOR = 1.1;
// `UpdateBBox`: the layers' box in plot coordinates.
const MIN_X = 0;
const MAX_X = 1;
const MIN_Y = 0;
const MAX_Y = 1;

export const x2p = (v: PlotView, x: number) => Math.trunc((x - v.posX) * v.scaleX);
export const y2p = (v: PlotView, y: number) => Math.trunc((v.posY - y) * v.scaleY);
export const p2x = (v: PlotView, px: number) => v.posX + px / v.scaleX;
export const p2y = (v: PlotView, py: number) => v.posY - py / v.scaleY;

export type Directions = "both" | "h" | "v";
const hasH = (d: Directions) => d !== "v";
const hasV = (d: Directions) => d !== "h";

/** `mpWindow::Fit( xMin, xMax, yMin, yMax, ..., directions )`. */
export function fitTo(v: PlotView, xMin: number, xMax: number, yMin: number, yMax: number, dirs: Directions = "both"): void {
  const dXmin = xMin;
  const dXmax = xMax;
  const dYmin = yMin;
  const dYmax = yMax;
  const xGap = Math.abs(xMax - xMin) * LEFT_RIGHT_GAP;
  const yGap = Math.abs(yMax - yMin) * TOP_BOTTOM_GAP;
  xMin -= xGap;
  xMax += xGap;
  yMin -= yGap;
  yMax += yGap;
  const w = v.scrX - v.marginLeft - v.marginRight;
  const h = v.scrY - v.marginTop - v.marginBottom;
  const sx = xMax - xMin !== 0 ? w / (xMax - xMin) : 1;
  const sy = yMax - yMin !== 0 ? h / (yMax - yMin) : 1;
  if (hasH(dirs)) {
    v.desiredXmin = dXmin;
    v.desiredXmax = dXmax;
    v.scaleX = sx;
    v.posX = xMin - v.marginLeft / sx;
  }
  if (hasV(dirs)) {
    v.desiredYmin = dYmin;
    v.desiredYmax = dYmax;
    v.scaleY = sy;
    v.posY = yMax + v.marginTop / sy;
  }
}

/** `mpWindow::Fit()`: the whole box. */
export function fit(v: PlotView): void {
  fitTo(v, MIN_X, MAX_X, MIN_Y, MAX_Y);
}

/** `mpWindow::AdjustLimitedView`. */
export function adjustLimitedView(v: PlotView, dirs: Directions = "both"): void {
  if (hasH(dirs)) {
    if (v.desiredXmin < MIN_X) {
      const d = MIN_X - v.desiredXmin;
      v.posX += d;
      v.desiredXmax += d;
      v.desiredXmin = MIN_X;
    }
    if (v.desiredXmax > MAX_X) {
      const d = v.desiredXmax - MAX_X;
      v.posX -= d;
      v.desiredXmin -= d;
      v.desiredXmax = MAX_X;
    }
  }
  if (hasV(dirs)) {
    if (v.desiredYmin < MIN_Y) {
      const d = MIN_Y - v.desiredYmin;
      v.posY += d;
      v.desiredYmax += d;
      v.desiredYmin = MIN_Y;
    }
    if (v.desiredYmax > MAX_Y) {
      const d = v.desiredYmax - MAX_Y;
      v.posY -= d;
      v.desiredYmin -= d;
      v.desiredYmax = MAX_Y;
    }
  }
}

/** `mpWindow::ViewNeedsRefitting`. */
function needsRefitting(v: PlotView, dirs: Directions): Directions | null {
  const xGap = Math.abs(MAX_X - MIN_X) * LEFT_RIGHT_GAP;
  const yGap = Math.abs(MAX_Y - MIN_Y) * TOP_BOTTOM_GAP;
  const h = hasH(dirs) && (v.desiredXmax > MAX_X + xGap || v.desiredXmin < MIN_X - xGap);
  const vv = hasV(dirs) && (v.desiredYmax > MAX_Y + yGap || v.desiredYmin < MIN_Y - yGap);
  return h && vv ? "both" : h ? "h" : vv ? "v" : null;
}

/** `mpWindow::pushZoomUndo`. */
function pushUndo(v: PlotView): void {
  v.undo.push([v.desiredXmin, v.desiredXmax, v.desiredYmin, v.desiredYmax]);
  v.redo = [];
}

/** `mpWindow::RecomputeDesiredX` / `RecomputeDesiredY`. */
function recomputeDesired(v: PlotView, h: boolean): void {
  if (h) {
    const spanX = (v.scrX - v.marginLeft - v.marginRight) / v.scaleX;
    const desired = spanX / (2 * LEFT_RIGHT_GAP + 1);
    const gap = desired * LEFT_RIGHT_GAP;
    v.desiredXmin = v.posX + v.marginLeft / v.scaleX + gap;
    v.desiredXmax = v.desiredXmin + desired;
  } else {
    const spanY = (v.scrY - v.marginTop - v.marginBottom) / v.scaleY;
    const desired = spanY / (2 * TOP_BOTTOM_GAP + 1);
    const gap = desired * TOP_BOTTOM_GAP;
    v.desiredYmax = v.posY - v.marginTop / v.scaleY - gap;
    v.desiredYmin = v.desiredYmax - desired;
  }
}

/** `mpWindow::DoZoom`: about `center` (pixels; none is the plot area's middle), by `factor` (> 1 zooms in). */
export function zoom(v: PlotView, factor: number, dirs: Directions = "both", center?: { x: number; y: number }): void {
  if (v.yLocked) {
    if (dirs === "v") return;
    dirs = "h";
  }
  pushUndo(v);
  let cx: number;
  let cy: number;
  if (!center) {
    cx = Math.trunc((v.scrX - v.marginLeft - v.marginRight) / 2) + v.marginLeft;
    cy = Math.trunc((v.scrY - v.marginTop - v.marginBottom) / 2) + v.marginTop;
  } else {
    cx = Math.min(Math.max(center.x, v.marginLeft), v.scrX - v.marginRight);
    cy = Math.min(Math.max(center.y, v.marginTop), v.scrY - v.marginBottom);
  }
  const MAX_SCALE = 1e6;
  const nsx = hasH(dirs) ? v.scaleX * factor : v.scaleX;
  const nsy = hasV(dirs) ? v.scaleY * factor : v.scaleY;
  if (nsx > MAX_SCALE || nsy > MAX_SCALE) return;
  if (hasH(dirs)) {
    const prior = p2x(v, cx);
    v.scaleX = nsx;
    v.posX = prior - cx / nsx;
    recomputeDesired(v, true);
  }
  if (hasV(dirs)) {
    const prior = p2y(v, cy);
    v.scaleY = nsy;
    v.posY = prior + cy / nsy;
    recomputeDesired(v, false);
  }
  adjustLimitedView(v, dirs);
  if (factor < 1.0) {
    const r = needsRefitting(v, dirs);
    if (r) fitTo(v, MIN_X, MAX_X, MIN_Y, MAX_Y, r);
  }
}

export const zoomIn = (v: PlotView, dirs: Directions = "both", center?: { x: number; y: number }, factor = ZOOM_FACTOR) => zoom(v, factor, dirs, center);
/** `mpWindow::ZoomOut`. */
export const zoomOut = (v: PlotView, dirs: Directions = "both", center?: { x: number; y: number }, factor = ZOOM_FACTOR) => zoom(v, 1.0 / (factor === 0 ? 1.0 : factor), dirs, center);

/** `mpWindow::ZoomRect`: the rectangle between two pixel points, constrained to the plot area. */
export function zoomRect(v: PlotView, a: { x: number; y: number }, b: { x: number; y: number }): void {
  pushUndo(v);
  const cl = (p: { x: number; y: number }) => ({ x: Math.min(Math.max(p.x, v.marginLeft), v.scrX - v.marginRight), y: Math.min(Math.max(p.y, v.marginTop), v.scrY - v.marginBottom) });
  const p0 = cl(a);
  const p1 = cl(b);
  const x0 = p2x(v, p0.x);
  const y0 = p2y(v, p0.y);
  const x1 = p2x(v, p1.x);
  const y1 = p2y(v, p1.y);
  let yMin = Math.min(y0, y1);
  let yMax = Math.max(y0, y1);
  if (v.yLocked) {
    yMin = v.desiredYmin;
    yMax = v.desiredYmax;
  }
  fitTo(v, Math.min(x0, x1), Math.max(x0, x1), yMin, yMax);
  adjustLimitedView(v);
  const r = needsRefitting(v, "both");
  if (r) fitTo(v, MIN_X, MAX_X, MIN_Y, MAX_Y, r);
}

/** `mpWindow::OnFit`: a zoom step of its own, then `Fit`. */
export function fitWithUndo(v: PlotView): void {
  pushUndo(v);
  fit(v);
}

/** `mpWindow::ZoomUndo`. */
export function zoomUndo(v: PlotView): void {
  const z = v.undo.pop();
  if (!z) return;
  v.redo.push([v.desiredXmin, v.desiredXmax, v.desiredYmin, v.desiredYmax]);
  fitTo(v, z[0], z[1], z[2], z[3]);
  adjustLimitedView(v);
}

/** `mpWindow::ZoomRedo`. */
export function zoomRedo(v: PlotView): void {
  const z = v.redo.pop();
  if (!z) return;
  v.undo.push([v.desiredXmin, v.desiredXmax, v.desiredYmin, v.desiredYmax]);
  fitTo(v, z[0], z[1], z[2], z[3]);
  adjustLimitedView(v);
}

/** `mpWindow::SetXView` / `SetYView` from a pan of `dx`, `dy` pixels (`onMouseMove` with the middle button, the wheel's pan actions). */
export function pan(v: PlotView, dxPx: number, dyPx: number): void {
  if (dxPx) {
    const u = dxPx / v.scaleX;
    v.posX += u;
    v.desiredXmax += u;
    v.desiredXmin += u;
    adjustLimitedView(v, "h");
  }
  if (dyPx && !v.yLocked) {
    const u = -dyPx / v.scaleY;
    v.posY += u;
    v.desiredYmax += u;
    v.desiredYmin += u;
    adjustLimitedView(v, "v");
  }
}

/** A window resized: mathplot's next paint fits the desired view to the new size (`OnSize` -> `Fit( m_desired... )`). */
export function resize(v: PlotView, w: number, h: number): void {
  v.scrX = w;
  v.scrY = h;
  fitTo(v, v.desiredXmin, v.desiredXmax, v.desiredYmin, v.desiredYmax);
}

// --------------------------------------------------------------------------------------------------------------- ticks and labels

/** `getSISuffix` (sim_plot_tab.cpp): the prefix for the value, from atto to giga (the loop stops before tera). */
export function siSuffix(x: number, unit: string): { power: number; suffix: string } {
  const powers: [number, string][] = [[-18, "a"], [-15, "f"], [-12, "p"], [-9, "n"], [-6, "u"], [-3, "m"], [0, ""], [3, "k"], [6, "M"], [9, "G"], [12, "T"], [14, "P"]];
  if (x === 0.0) return { power: 0, suffix: unit };
  for (let i = 0; i < 11 - 1; i++) {
    const [e, s] = powers[i] ?? [0, ""];
    const r = 10 ** e;
    if (Math.abs(x) >= r && Math.abs(x) < r * 1000.0) return { power: e, suffix: s + unit };
  }
  return { power: 0, suffix: unit };
}

/** `countDecimalDigits`. */
export function countDecimalDigits(x: number, maxDigits: number): number {
  if (Number.isNaN(x)) return 0;
  const sig = (k: number) => {
    while (k !== 0 && k % 10 === 0) k = Math.trunc(k / 10);
    let n = 0;
    while (k !== 0) {
      n++;
      k = Math.trunc(k / 10);
    }
    return n;
  };
  // `(int)( ( x - floor( x ) ) * pow( 10.0, maxDigits ) )`
  const k = Math.trunc((x - Math.floor(x)) * 10 ** maxDigits) | 0;
  return Math.min(sig(k), sig(k + 1));
}

/** `formatFloat`. */
export const formatFloat = (x: number, digits: number) => x.toFixed(digits);

/** `LIN_SCALE::formatLabels` / `LOG_SCALE::formatLabels`. */
export function formatLabels(s: Scale): void {
  if (s.kind === "log") {
    for (const l of s.tickLabels) {
      const { power, suffix } = siSuffix(l.pos, s.unit);
      const sf = 10 ** power;
      const k = countDecimalDigits(l.pos / sf, 3);
      l.label = formatFloat(l.pos / sf, k) + suffix;
      l.visible = true;
    }
    return;
  }
  const { power, suffix } = siSuffix(s.absVisibleMax, s.unit);
  const sf = 10 ** power;
  let digits = 0;
  for (const l of s.tickLabels) digits = Math.max(digits, countDecimalDigits(l.pos / sf, 3));
  let duplicate = false;
  do {
    for (let i = 0; i < s.tickLabels.length; i++) {
      const l = s.tickLabels[i]!;
      l.label = formatFloat(l.pos / sf, digits);
      l.visible = true;
      if (i > 0 && l.label === s.tickLabels[i - 1]!.label) duplicate = true;
    }
  } while (duplicate && ++digits <= 6);
  s.name = `${s.base} (${suffix})`;
}

function visibleX(s: Scale, v: PlotView): [number, number] {
  return [fromPlot(s, p2x(v, v.marginLeft)), fromPlot(s, p2x(v, v.scrX - v.marginRight))];
}

function visibleY(s: Scale, v: PlotView): [number, number] {
  return [fromPlot(s, p2y(v, v.scrY - v.marginBottom)), fromPlot(s, p2y(v, v.marginTop))];
}

function bestStep(lo: number, hi: number): number {
  let minErr = 1e12;
  let best = 1.0;
  for (let i = 10; i <= 20; i += 2) {
    const cur = Math.abs(hi - lo) / i;
    const base = 10 ** Math.floor(Math.log10(cur));
    const stepInt = Math.floor(cur / base) * base;
    const err = Math.abs(cur - stepInt);
    if (err < minErr) {
      minErr = err;
      best = stepInt;
    }
  }
  return best;
}

/** `mpScaleXBase::Plot`'s transform and `mpScaleX` / `mpScaleXLog::recalculateTicks`. */
export function xTicks(s: Scale, v: PlotView): void {
  s.offset = -s.minV;
  s.scale = 1.0 / (s.maxV - s.minV);
  const [lo, hi] = visibleX(s, v);
  s.tickValues = [];
  s.tickLabels = [];
  if (s.kind === "log") {
    const minDecade = 10 ** Math.floor(Math.log10(s.minV));
    const maxDecade = 10 ** Math.ceil(Math.log10(s.maxV));
    let decades = Math.log(hi / lo) / Math.log(10);
    let step = 10.0;
    if (minDecade === 0.0 || !Number.isFinite(minDecade)) return;
    while (decades - 2 >= v.scrX / 96.0) {
      step *= 10.0;
      decades = Math.log(hi / lo) / Math.log(step);
      if (!Number.isFinite(decades)) break;
    }
    for (let d = minDecade; d <= maxDecade && s.tickValues.length < 10000; d *= step) {
      s.tickLabels.push({ pos: d, label: "", visible: true });
      for (let dd = d; dd < d * step; dd += d) {
        if (decades < 2) s.tickLabels.push({ pos: dd, label: "", visible: true });
        s.tickValues.push(dd);
      }
    }
    formatLabels(s);
    return;
  }
  s.absVisibleMax = Math.max(Math.abs(lo), Math.abs(hi));
  let step = bestStep(lo, hi);
  let n = Math.floor((hi - lo) / step);
  while (n - 2.0 >= v.scrX / 96.0) {
    step *= 2;
    n = Math.floor((hi - lo) / step);
  }
  let x = Math.floor(lo / step) * step;
  let zero = 1e8;
  let guard = 0;
  while (x < hi && guard++ < 1000) {
    s.tickValues.push(x);
    zero = Math.min(zero, Math.abs(x));
    x += step;
  }
  if (zero <= step) s.tickValues = s.tickValues.map((t) => t - zero);
  s.tickLabels = s.tickValues.map((t) => ({ pos: t, label: "", visible: true }));
  formatLabels(s);
}

/** `mpScaleY::Plot`'s transform and `recalculateTicks` (a locked scale's own range, a slave's `computeSlaveTicks`). */
export function yTicks(s: Scale, v: PlotView): void {
  s.offset = -s.minV;
  s.scale = 1.0 / (s.maxV - s.minV);
  let lo: number;
  let hi: number;
  if (s.locked) {
    lo = s.axisMin;
    hi = s.axisMax;
    s.offset = -s.axisMin;
    s.scale = 1.0 / (s.axisMax - s.axisMin);
  } else if (s.master) {
    slaveTicks(s, v);
    formatLabels(s);
    return;
  } else {
    [lo, hi] = visibleY(s, v);
  }
  s.absVisibleMax = Math.max(Math.abs(lo), Math.abs(hi));
  s.tickValues = [];
  s.tickLabels = [];
  let step = bestStep(lo, hi);
  let n = Math.floor((hi - lo) / step);
  while (n >= v.scrY / 32.0) {
    step *= 2;
    n = Math.floor((hi - lo) / step);
  }
  let y = Math.floor(lo / step) * step;
  let zero = 1e8;
  let i = 0;
  while (y <= hi && i < 1000) {
    s.tickValues.push(y);
    zero = Math.min(zero, Math.abs(y));
    y += step;
    i++;
  }
  if (i === 1000) s.tickValues = [];
  if (zero <= step) s.tickValues = s.tickValues.map((t) => t - zero);
  s.tickLabels = s.tickValues.map((t) => ({ pos: t, label: "", visible: true }));
  formatLabels(s);
}

/** `mpScaleY::computeSlaveTicks`: ticks where the master's are, with a round step of this scale's own. */
function slaveTicks(s: Scale, v: PlotView): void {
  const m = s.master!;
  if (m.tickValues.length < 2) return;
  s.tickValues = [];
  s.tickLabels = [];
  const p0 = toPlot(m, m.tickValues[0]!);
  const p1 = toPlot(m, m.tickValues[1]!);
  s.scale = 1.0 / (s.maxV - s.minV);
  s.offset = -s.minV;
  const y0 = p0 / s.scale;
  const y1 = p1 / s.scale;
  const dy = y1 - y0;
  const exponent = Math.floor(Math.log10(dy));
  const base = dy / 10 ** exponent;
  const dyScaled = (Math.ceil(2.0 * base) / 2.0) * 10 ** exponent;
  let [minvv] = visibleY(s, v);
  minvv = Math.floor(minvv / dyScaled) * dyScaled;
  s.scale = 1.0 / (s.maxV - s.minV);
  s.scale *= dy / dyScaled;
  s.offset = p0 / s.scale - minvv;
  s.absVisibleMax = 0;
  for (const t of m.tickValues) {
    const val = fromPlot(s, toPlot(m, t));
    s.tickValues.push(val);
    s.tickLabels.push({ pos: val, label: "", visible: true });
    s.absVisibleMax = Math.max(s.absVisibleMax, Math.abs(val));
  }
}

// --------------------------------------------------------------------------------------------------------------- colours

export type Rgb = [number, number, number];

/** `SIM_PLOT_COLORS::FillDefaultColorList`: background, foreground, axes, then the trace colours. */
export function colorList(dark: boolean): Rgb[] {
  const head: Rgb[] = dark ? [[0, 0, 0], [255, 255, 255], [130, 130, 130]] : [[255, 255, 255], [0, 0, 0], [130, 130, 130]];
  return [
    ...head,
    [0xe4, 0x1a, 0x1c],
    [0x37, 0x7e, 0xb8],
    [0x4d, 0xaf, 0x4a],
    [0x98, 0x4e, 0xa3],
    [0xff, 0x7f, 0x00],
    [0xff, 0xff, 0x33],
    [0xa6, 0x56, 0x28],
    [0xf7, 0x81, 0xbf],
    [0x66, 0xc2, 0xa5],
    [0xfc, 0x8d, 0x62],
    [0x8d, 0xa0, 0xcb],
    [0xe7, 0x8a, 0xc3],
    [0xa6, 0xd8, 0x54],
    [0xff, 0xd9, 0x2f],
    [0xe5, 0xc4, 0x94],
    [0xb3, 0xb3, 0xb3],
  ];
}

export const cssRgb = (c: Rgb) => `rgb(${c[0]}, ${c[1]}, ${c[2]})`;

/** `COLOR4D( wxString )` of `rgb(r, g, b)`, `rgba(...)` or `#rrggbb`. */
export function parseColor(s: string): Rgb | null {
  const m = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/i.exec(s.trim());
  if (m) return [Number(m[1]), Number(m[2]), Number(m[3])];
  const h = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i.exec(s.trim());
  if (h) return [Number.parseInt(h[1]!, 16), Number.parseInt(h[2]!, 16), Number.parseInt(h[3]!, 16)];
  return null;
}

/** `SIM_PLOT_COLORS::GenerateColor`: the first trace colour no trace of the tab has, else one by the number of traces. */
export function generateColor(used: Rgb[], dark: boolean): Rgb {
  const list = colorList(dark);
  const same = (a: Rgb, b: Rgb) => a[0] === b[0] && a[1] === b[1] && a[2] === b[2];
  for (let i = 3; i < list.length; i++) if (!used.some((u) => same(u, list[i]!))) return list[i]!;
  return list[3 + (used.length % (list.length - 3))]!;
}

/** `COLOR4D::Mix( aColor, aFactor )`: this colour by `factor`, `other` by the rest. */
export function mix(c: Rgb, other: Rgb, factor: number): Rgb {
  return [other[0] * (1 - factor) + c[0] * factor, other[1] * (1 - factor) + c[1] * factor, other[2] * (1 - factor) + c[2] * factor];
}

/** `COLOR4D::Distance` on 0..1 channels. */
export function distance(a: Rgb, b: Rgb): number {
  const d = (i: 0 | 1 | 2) => (a[i] - b[i]) / 255;
  return d(0) * d(0) + d(1) * d(1) + d(2) * d(2);
}

// --------------------------------------------------------------------------------------------------------------- cursors

/** `CURSOR::doSetCoordX`: the trace's value at `x` by linear interpolation; NaN off the data (or with fewer than two points). */
export function valueAt(xs: readonly number[], ys: readonly number[], x: number, start = 0, end = xs.length): number {
  if (xs.length <= 1 || start >= end) return Number.NaN;
  // std::upper_bound
  let lo = start;
  let hi = end;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (xs[mid]! <= x) lo = mid + 1;
    else hi = mid;
  }
  const maxIdx = lo;
  const minIdx = maxIdx - 1;
  if (minIdx < start || maxIdx >= end || maxIdx >= xs.length) return Number.NaN;
  const lx = xs[minIdx]!;
  const rx = xs[maxIdx]!;
  const ly = ys[minIdx]!;
  const ry = ys[maxIdx]!;
  return ly + ((ry - ly) / (rx - lx)) * (x - lx);
}
