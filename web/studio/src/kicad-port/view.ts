// Pure port of common/view/view.cpp's VIEW class -- just the screen<->world
// transform math and the handful of operations the rest of this app needs
// (SetScale-about-an-anchor, fit-to-bounds). No wxWidgets/GAL/drawing code,
// no React: this is the "GAL-like layer" worldToScreen/screenToWorld lived
// in before (components/canvas/view.ts, now a thin re-export of this
// module) plus the real KiCad zoom-clamp/anchor behavior.
//
// Our ViewTransform is algebraically the same idea as VIEW's own state
// (m_center/m_scale) but expressed as a direct screen-affine transform
// (scale, x, y) instead of (center, scale) -- screenPx = worldUm * scale +
// (x, y). The two are interchangeable; this shape is what painter.ts's
// Canvas2D calls (ctx.translate/ctx.scale) want directly.

export interface ViewTransform {
  /** Screen pixels per board µm. KiCad's VIEW::m_scale (world-to-screen), just expressed in our board unit instead of KiCad's internal nm. */
  scale: number;
  /** Screen-space translation: screenPx = worldUm * scale + (x, y). */
  x: number;
  y: number;
}

export function worldToScreen(v: ViewTransform, xUm: number, yUm: number): [number, number] {
  return [xUm * v.scale + v.x, yUm * v.scale + v.y];
}

export function screenToWorld(v: ViewTransform, sx: number, sy: number): [number, number] {
  return [(sx - v.x) / v.scale, (sy - v.y) / v.scale];
}

/** A hairline width in µm that renders as `px` screen pixels at the current zoom -- Canvas2D's stand-in for SVG's `vector-effect: non-scaling-stroke`. */
export function hairlineUm(v: ViewTransform, px = 1): number {
  return v.scale > 0 ? px / v.scale : px;
}

export interface Bounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export function boundsOfPoints(points: Array<[number, number]>): Bounds | null {
  if (points.length === 0) return null;
  let minX = Infinity,
    minY = Infinity,
    maxX = -Infinity,
    maxY = -Infinity;
  for (const [x, y] of points) {
    if (x < minX) minX = x;
    if (y < minY) minY = y;
    if (x > maxX) maxX = x;
    if (y > maxY) maxY = y;
  }
  return { minX, minY, maxX, maxY };
}

/** A view transform that fits `bounds` inside `width`x`height` screen pixels, with `padPx` margin. Matches ACTIONS::zoomFitScreen/zoomFitObjects's intent (common_tools.cpp's own fit code computes a similar center+scale from a bounding box and the canvas size); there is no separate "page" vs "objects" distinction here since this app has no fixed worksheet page the way KiCad's zoomFitScreen targets. */
export function fitTransform(bounds: Bounds, width: number, height: number, padPx = 30): ViewTransform {
  const w = Math.max(bounds.maxX - bounds.minX, 1);
  const h = Math.max(bounds.maxY - bounds.minY, 1);
  const scale = Math.min((width - 2 * padPx) / w, (height - 2 * padPx) / h);
  return {
    scale,
    x: (width - w * scale) / 2 - bounds.minX * scale,
    y: (height - h * scale) / 2 - bounds.minY * scale,
  };
}

// view.cpp: `m_minScale( 0.2 ), m_maxScale( 50000.0 )` -- these are
// VIEW::m_scale clamp bounds, but m_scale there is GAL's internal "zoom
// factor" relative to a world unit of 1 nanometre (SetWorldUnitLength(1e-9
// / 0.0254) in graphics_abstraction_layer.cpp), not a screen-px-per-µm
// ratio the way our ViewTransform.scale is. The two "scale" variables
// measure different things (KiCad's bakes in a 25,400,000:1 nm-per-inch
// normalization our µm-based board doesn't have), so 0.2/50000 would not
// mean "the same zoom range" if copied verbatim -- there is no unit-free
// way to port this pair of literal numbers, only the *purpose* (clamp to
// a sane range so scale can never reach 0 or run away to infinity). These
// bounds serve that same purpose, sized for a µm world: 1e-5 shows roughly
// a 100-metre span across a 1000px window at the low end, 50 lets a 0.1mm
// pad fill ~5000px at the high end -- comparable headroom to KiCad's own
// range, not the same numbers.
export const MIN_SCALE = 1e-5;
export const MAX_SCALE = 50;

/**
 * view.cpp VIEW::SetScale(aScale, aAnchor): change scale while keeping
 * `aAnchor`'s screen position fixed. Expressed here directly in screen
 * space (`px, py` is both the anchor's current screen position and where
 * it stays) since that's what every caller already has at hand (mouse
 * position); this is algebraically identical to source's
 * `delta = ToWorld(ToScreen(anchor)) - anchor; SetCenter(center - delta)`
 * dance, which likewise just solves for "keep this screen point fixed."
 *
 * Does NOT implement the `center_on_zoom`/`m_warpCursor` branch
 * (wx_view_controls.cpp onWheel: `if (IsCursorWarpingEnabled()) { CenterOnCursor(); SetScale(...); }`)
 * -- that branch first warps the real OS pointer to the canvas center.
 * This app never moves the real pointer (a hard rule, and also not
 * something a web page can do), so the non-warping anchor-preserving
 * branch below is used unconditionally; see PARITY-pcb.md.
 */
export function zoomAbout(v: ViewTransform, px: number, py: number, factor: number): ViewTransform {
  const scale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, v.scale * factor));
  return {
    scale,
    x: px - (px - v.x) * (scale / v.scale),
    y: py - (py - v.y) * (scale / v.scale),
  };
}

/** Pan by a screen-space pixel delta (middle/right-drag, autopan, arrow-key pan). */
export function panByScreenDelta(v: ViewTransform, dxPx: number, dyPx: number): ViewTransform {
  return { ...v, x: v.x + dxPx, y: v.y + dyPx };
}

/** Pan by a world-space (board µm) delta -- wx_view_controls.cpp's onWheel/onScroll/handleAutoPanning all compute a world-space delta (via ToWorld) and call `m_view->SetCenter(m_view->GetCenter() + delta)`; moving the center by +worldDelta is the same as moving the screen origin by -worldDelta*scale. */
export function panByWorldDelta(v: ViewTransform, dxUm: number, dyUm: number): ViewTransform {
  return panByScreenDelta(v, -dxUm * v.scale, -dyUm * v.scale);
}
