// Grid *drawing* math (common/gal/graphics_abstraction_layer.h's GAL
// methods + common/gal/opengl/opengl_gal.cpp's DrawGrid) and the real
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
 * Values are µm (1 mil = 25.4 µm exactly); this app's board unit.
 */
export const DEFAULT_PCB_GRIDS_UM: readonly number[] = [
  25400, // 1000 mil
  12700, // 500 mil
  6350, // 250 mil
  5080, // 200 mil
  2540, // 100 mil
  1270, // 50 mil
  635, // 25 mil
  508, // 20 mil
  254, // 10 mil
  127, // 5 mil
  50.8, // 2 mil
  25.4, // 1 mil
  5000, // 5.0 mm
  2500, // 2.5 mm
  1000, // 1.0 mm
  500, // 0.5 mm
  250, // 0.25 mm
  200, // 0.2 mm
  100, // 0.1 mm
  50, // 0.05 mm
  25, // 0.025 mm
  10, // 0.01 mm
];

/** GAL_DISPLAY_OPTIONS's own ctor default (gal_display_options.cpp) -- what KiCad actually ships with (GAL's own raw fallback of GRID_STYLE::LINES is overwritten by this before the first paint, via the settings-changed observer). */
export const DEFAULT_GRID_STYLE: GridStyle = "dots";
/** GAL_DISPLAY_OPTIONS default, pixels. */
export const DEFAULT_GRID_MIN_SPACING_PX = 10;
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
 * changes anything when zoomed out far enough that `gridUm` is sub-pixel.
 *
 * `scalePxPerUm` is this app's ViewTransform.scale -- the same role as
 * source's `m_worldScale` (screen pixels per world unit).
 */
export function computeVisibleGridSize(gridUm: number, scalePxPerUm: number, style: GridStyle = DEFAULT_GRID_STYLE, tick = DEFAULT_GRID_TICK, minSpacingPx = DEFAULT_GRID_MIN_SPACING_PX): number {
  let spacing = gridUm;
  const threshold = (minSpacingPx / scalePxPerUm) * (style === "small-cross" ? 2 : 1);
  // source clamps each axis to >= 100 (world units) first as a degenerate-
  // input guard (grid size of exactly 0); not meaningful in µm terms at
  // any real zoom, kept only so gridUm=0 doesn't loop forever below.
  if (spacing <= 0) spacing = 100;
  // A view not fitted yet (scale 0) makes the threshold infinite: KiCad's
  // VIEW never has scale 0, here the first paint can race the first fit.
  // Return the base grid rather than loop forever; tick <= 1 never grows.
  if (!(scalePxPerUm > 0) || !Number.isFinite(threshold) || tick <= 1) return spacing;
  while (spacing <= threshold) spacing *= tick;
  return spacing;
}

/** opengl_gal.cpp DrawGrid: `(index % m_gridTick == 0)` picks the double-width "major" line/dot/cross at every `tick`-th line, indexed from the grid origin (0,0), not from whatever's on screen. */
export function isMajorGridLine(index: number, tick = DEFAULT_GRID_TICK): boolean {
  return index % tick === 0;
}

/** opengl_gal.cpp DrawGrid: `minorLineWidth = max(1px, gridLineWidth_setting) ...; majorLineWidth = minorLineWidth * 2`. `gridLineWidth` is a user display setting (gal_display_options default 1.0 world-scaled px); exposed as a ratio here rather than a literal pixel count since this app draws the grid directly in screen pixels (Canvas2D), not world units. */
export const MAJOR_GRID_LINE_WIDTH_RATIO = 2;
