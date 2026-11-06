// Grid *drawing* math (common/gal/graphics_abstraction_layer.h's GAL
// methods + common/gal/opengl/opengl_gal.cpp's DrawGrid: `gridMarks`, which
// components/canvas/gridPaint.ts draws for all four canvases, in the three
// styles of Display Options > Grid Display) and the real
// default PCB grid-size list (common/settings/app_settings.cpp
// APP_SETTINGS_BASE::DefaultGridSizeList, the "else" / Pcbnew branch) that
// N/Shift+N (common.Control.gridNext/gridPrev) cycles through.
//
// Grid *snapping* (anchor snap to pads/track ends, magnetic pads/tracks)
// is a separate port -- see components/canvas/gridHelper.ts /
// pcb_grid_helper.cpp -- this file is only "how the grid is drawn."

export type GridStyle = "dots" | "lines" | "small-cross";

/**
 * KiCad's real default PCB-editor grid list (app_settings.cpp
 * DefaultGridSizeList's final `else` branch -- used by the PCB editor and
 * footprint editor; eeschema/pl_editor/gerbview each get their own
 * different lists this app doesn't need). This is the literal declared
 * order, NOT sorted by physical size: a block of mil values (1000 mil
 * down to 1 mil) followed by a block of mm values (5.0 mm down to
 * 0.01 mm) -- 1 mil (0.0254 mm) sits right next to 5.0 mm, a deliberate
 * jump in KiCad's own shipped list. N/Shift+N (gridNext/gridPrev) index
 * through this array exactly as declared, so that jump is real, expected
 * behavior to reproduce, not a bug to "fix" by sorting.
 *
 * Values are nm (1 mil = 25_400 nm exactly); this app's board unit.
 */
export const DEFAULT_PCB_GRIDS_NM: readonly number[] = [
  25_400_000, // 1000 mil
  12_700_000, // 500 mil
  6_350_000, // 250 mil
  5_080_000, // 200 mil
  2_540_000, // 100 mil
  1_270_000, // 50 mil
  635_000, // 25 mil
  508_000, // 20 mil
  254_000, // 10 mil
  127_000, // 5 mil
  50_800, // 2 mil
  25_400, // 1 mil
  5_000_000, // 5.0 mm
  2_500_000, // 2.5 mm
  1_000_000, // 1.0 mm
  500_000, // 0.5 mm
  250_000, // 0.25 mm
  200_000, // 0.2 mm
  100_000, // 0.1 mm
  50_000, // 0.05 mm
  25_000, // 0.025 mm
  10_000, // 0.01 mm
];

/** GAL_DISPLAY_OPTIONS's own ctor default (gal_display_options.cpp) -- what KiCad actually ships with (GAL's own raw fallback of GRID_STYLE::LINES is overwritten by this before the first paint, via the settings-changed observer). */
export const DEFAULT_GRID_STYLE: GridStyle = "dots";
/** GAL_DISPLAY_OPTIONS default, pixels. */
export const DEFAULT_GRID_MIN_SPACING_PX = 10;
/** `grid.line_width`'s default (`m_gridLineWidth( 1.0 )`), pixels. */
export const DEFAULT_GRID_LINE_WIDTH_PX = 1;

/** `PANEL_GAL_OPTIONS`: "Grid thickness:" offers 0.5 to 10 pixels by 0.5 (`gridThicknessMin` / `Max` / `Step`). */
export const GRID_THICKNESS_CHOICES: readonly number[] = Array.from({ length: 20 }, (_, i) => 0.5 + i * 0.5);
/** "Minimum grid spacing:" spins from 5 to 200 pixels by 5 (`gridMinSpacingMin` / `Max` / `Step`). */
export const GRID_MIN_SPACING_RANGE = { min: 5, max: 200, step: 5 } as const;

/** A grid thickness as the choice box can hold it: the nearest of `GRID_THICKNESS_CHOICES` (the panel selects the entry equal to the setting). */
export function gridLineWidthChoice(px: number): number {
  if (!Number.isFinite(px)) return DEFAULT_GRID_LINE_WIDTH_PX;
  return GRID_THICKNESS_CHOICES.reduce((best, c) => (Math.abs(c - px) < Math.abs(best - px) ? c : best), GRID_THICKNESS_CHOICES[0]!);
}

/** A minimum grid spacing as the spin control holds it: a whole number of pixels within 5 .. 200. */
export function gridMinSpacingValue(px: number): number {
  if (!Number.isFinite(px)) return DEFAULT_GRID_MIN_SPACING_PX;
  return Math.min(GRID_MIN_SPACING_RANGE.max, Math.max(GRID_MIN_SPACING_RANGE.min, Math.round(px)));
}
/** GAL::SetCoarseGrid(10) in the GAL ctor -- every 10th grid line is drawn at double width ("major" line), and is also the factor grid spacing is multiplied by each time it's still too dense to render (see computeVisibleGridSize). */
export const DEFAULT_GRID_TICK = 10;

/**
 * graphics_abstraction_layer.h GAL::GetVisibleGridSize(), byte-for-byte:
 * start from the real grid pitch; if it would render less than
 * `minSpacingPx` apart on screen (doubled for small-cross, since crosses
 * need more room than a dot/line to stay legible -- source's `if
 * (m_gridStyle == SMALL_CROSS) gridThreshold *= 2.0`), multiply by
 * `tick` and recheck, repeating until it's visible. This is "coarsen an
 * absurdly fine grid instead of drawing a solid smear" -- it only ever
 * changes anything when zoomed out far enough that `gridNm` is sub-pixel.
 *
 * `scalePxPerNm` is this app's ViewTransform.scale -- the same role as
 * source's `m_worldScale` (screen pixels per world unit).
 */
export function computeVisibleGridSize(gridNm: number, scalePxPerNm: number, style: GridStyle = DEFAULT_GRID_STYLE, tick = DEFAULT_GRID_TICK, minSpacingPx = DEFAULT_GRID_MIN_SPACING_PX): number {
  let spacing = gridNm;
  const threshold = (minSpacingPx / scalePxPerNm) * (style === "small-cross" ? 2 : 1);
  // source clamps each axis to >= 100 (world units) first as a degenerate-
  // input guard (grid size of exactly 0); not meaningful in nm terms at
  // any real zoom, kept only so gridNm=0 doesn't loop forever below.
  if (spacing <= 0) spacing = 100_000;
  // A view not fitted yet (scale 0) makes the threshold infinite: KiCad's
  // VIEW never has scale 0, here the first paint can race the first fit.
  // Return the base grid rather than loop forever; tick <= 1 never grows.
  if (!(scalePxPerNm > 0) || !Number.isFinite(threshold) || tick <= 1) return spacing;
  while (spacing <= threshold) spacing *= tick;
  return spacing;
}

/** opengl_gal.cpp DrawGrid: `(index % m_gridTick == 0)` picks the double-width "major" line/dot/cross at every `tick`-th line, indexed from the grid origin (0,0), not from whatever's on screen. */
export function isMajorGridLine(index: number, tick = DEFAULT_GRID_TICK): boolean {
  return index % tick === 0;
}

/** opengl_gal.cpp DrawGrid: `minorLineWidth = max(1px, gridLineWidth_setting) ...; majorLineWidth = minorLineWidth * 2`. `gridLineWidth` is a user display setting (gal_display_options default 1.0 world-scaled px); exposed as a ratio here rather than a literal pixel count since this app draws the grid directly in screen pixels (Canvas2D), not world units. */
export const MAJOR_GRID_LINE_WIDTH_RATIO = 2;

// ------------------------------------------------------------------------------------------------ the grid as drawn

/** A grid's X and Y, nm (`GAL::m_gridSize`). */
export interface GridSize2 {
  x: number;
  y: number;
}

/**
 * `GAL::GetVisibleGridSize` for a grid that need not be square: each size at least 100 world units, then -- while the smaller one would be drawn closer than the
 * minimum spacing (twice that for small crosses) -- both are multiplied by the tick. `scale` is screen pixels per nm (`m_worldScale`).
 */
export function visibleGridSize2(size: GridSize2, scale: number, style: GridStyle = DEFAULT_GRID_STYLE, minSpacingPx = DEFAULT_GRID_MIN_SPACING_PX, tick = DEFAULT_GRID_TICK): GridSize2 {
  let x = Math.max(100, size.x);
  let y = Math.max(100, size.y);
  const threshold = (minSpacingPx / scale) * (style === "small-cross" ? 2 : 1);
  if (!(scale > 0) || !Number.isFinite(threshold) || tick <= 1) return { x, y };
  while (Math.min(x, y) <= threshold) {
    x *= tick;
    y *= tick;
  }
  return { x, y };
}

/** One mark of the grid, in nm: a filled rectangle (a dot, the intersection of two lines), a small cross (two strokes `len` each way of its centre) or a line. */
export type GridMark =
  | { kind: "rect"; x: number; y: number; w: number; h: number }
  | { kind: "cross"; x: number; y: number; len: number; width: number }
  | { kind: "line"; x0: number; y0: number; x1: number; y1: number; width: number };

/** How an editor draws its grid (Display Options > Grid Display): `grid.style`, `grid.line_width`, `grid.min_spacing`. */
export interface GridDisplay {
  style: GridStyle;
  lineWidthPx: number;
  minSpacingPx: number;
}

export const DEFAULT_GRID_DISPLAY: GridDisplay = { style: DEFAULT_GRID_STYLE, lineWidthPx: DEFAULT_GRID_LINE_WIDTH_PX, minSpacingPx: DEFAULT_GRID_MIN_SPACING_PX };

export interface GridDrawSpec {
  size: GridSize2;
  origin: readonly [number, number];
  style: GridStyle;
  /** `grid.line_width`, pixels. */
  lineWidthPx: number;
  /** `grid.min_spacing`, pixels. */
  minSpacingPx: number;
  tick?: number;
}

/**
 * `OPENGL_GAL::DrawGrid`: the marks of the grid over the screen (`widthPx` x `heightPx` at the view `scale`, `x` / `y` its offset -- world = (screen - offset) / scale).
 *
 *   - the lines are `max( 1, line_width )` pixels wide, every tick-th line (counted from the grid origin) twice that;
 *   - the index range is the screen's corners rounded to the visible grid (`KiROUND`), one more on each side ("Ensure the grid fills the screen");
 *   - DOTS: the horizontal lines (`j`, as wide as row `j` is) and the vertical lines (`i`) drawn through a stencil that keeps only where they cross -- so each dot is a
 *     rectangle `width( i )` wide and `width( j )` high;
 *   - LINES: the same lines, all of them;
 *   - SMALL_CROSS: at each crossing two strokes of `2 * width` each way, `width` being the major one only where both `i` and `j` are ticks.
 */
export function gridMarks(view: { x: number; y: number; scale: number }, widthPx: number, heightPx: number, spec: GridDrawSpec): GridMark[] {
  const tick = spec.tick ?? DEFAULT_GRID_TICK;
  if (!(view.scale > 0) || !(spec.size.x > 0) || !(spec.size.y > 0)) return [];
  const g = visibleGridSize2(spec.size, view.scale, spec.style, spec.minSpacingPx, tick);
  const px = 1 / view.scale;
  const minor = Math.max(1, spec.lineWidthPx) * px;
  const major = minor * MAJOR_GRID_LINE_WIDTH_RATIO;
  const [ox, oy] = spec.origin;
  const worldStart = [(0 - view.x) / view.scale, (0 - view.y) / view.scale];
  const worldEnd = [(widthPx - view.x) / view.scale, (heightPx - view.y) / view.scale];
  const kiRound = (v: number) => (v < 0 ? -Math.round(-v) : Math.round(v));
  let sx = kiRound((worldStart[0]! - ox) / g.x);
  let ex = kiRound((worldEnd[0]! - ox) / g.x);
  let sy = kiRound((worldStart[1]! - oy) / g.y);
  let ey = kiRound((worldEnd[1]! - oy) / g.y);
  if (sx > ex) [sx, ex] = [ex, sx];
  if (sy > ey) [sy, ey] = [ey, sy];
  sx--;
  ex++;
  sy--;
  ey++;
  const isTick = (n: number) => n % tick === 0;
  const out: GridMark[] = [];
  if (spec.style === "small-cross") {
    for (let j = sy; j <= ey; j++) {
      const y = j * g.y + oy;
      for (let i = sx; i <= ex; i++) {
        const w = isTick(i) && isTick(j) ? major : minor;
        out.push({ kind: "cross", x: i * g.x + ox, y, len: 2 * w, width: w });
      }
    }
    return out;
  }
  if (spec.style === "lines") {
    for (let j = sy; j <= ey; j++) {
      const y = j * g.y + oy;
      out.push({ kind: "line", x0: sx * g.x + ox, y0: y, x1: ex * g.x + ox, y1: y, width: isTick(j) ? major : minor });
    }
    for (let i = sx; i <= ex; i++) {
      const x = i * g.x + ox;
      out.push({ kind: "line", x0: x, y0: sy * g.y + oy, x1: x, y1: ey * g.y + oy, width: isTick(i) ? major : minor });
    }
    return out;
  }
  for (let i = sx; i <= ex; i++) {
    const x = i * g.x + ox;
    const w = isTick(i) ? major : minor;
    for (let j = sy; j <= ey; j++) {
      const h = isTick(j) ? major : minor;
      out.push({ kind: "rect", x: x - w / 2, y: j * g.y + oy - h / 2, w, h });
    }
  }
  return out;
}
