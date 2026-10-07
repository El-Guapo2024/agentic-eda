// The crosshair the GAL draws at the cursor, as line segments in screen pixels.
//
//   common/gal/cairo/cairo_gal.cpp   CAIRO_GAL_BASE::blitCursor
//   common/gal/opengl/opengl_gal.cpp OPENGL_GAL::blitCursor
//   include/gal/gal_display_options.h KIGFX::CROSS_HAIR_MODE
//   common/tool/common_tools.cpp     COMMON_TOOLS::CursorSmallCrosshairs / CursorFullCrosshairs /
//                                    Cursor45Crosshairs / ToggleCursor
//   include/gal/graphics_abstraction_layer.h  IsCursorEnabled() = m_isCursorEnabled || m_forceDisplayCursor
//
// Three modes: a small cross (`cursorSize = 80` px across), lines that span the whole window, and the
// two 45/135 degree diagonals that span it. The cursor is only drawn when a tool asked for it
// (`VIEW_CONTROLS::ShowCursor`, i.e. while a drawing / move / picker tool is running) or when the
// "Always show crosshairs" setting (`always_show_cursor`, on by default) forces it.

export type CrossHairMode = "small" | "full" | "diag45";

/** `const int cursorSize = 80;` -- the small cross is this many pixels from end to end. */
export const SMALL_CROSS_SIZE_PX = 80;

/** One line segment in screen pixels. */
export type Segment = readonly [x0: number, y0: number, x1: number, y1: number];

/**
 * The segments of the crosshair centred on screen point `(px, py)` in a window of `width` x `height` pixels.
 *
 * Full cross: `DrawLine( 0, p.y, m_screenSize.x, p.y )` and `DrawLine( p.x, 0, p.x, m_screenSize.y )`.
 * Diagonal: `diagonalSize = m_screenSize.x + m_screenSize.y` and the two lines run from `p - diagonalSize`
 * to `p + diagonalSize` ("Oversized but that's ok"); a canvas clips them to the window.
 * Small: half of `cursorSize` either side of the centre.
 */
export function crosshairSegments(mode: CrossHairMode, px: number, py: number, width: number, height: number): Segment[] {
  switch (mode) {
    case "full":
      return [
        [0, py, width, py],
        [px, 0, px, height],
      ];
    case "diag45": {
      const d = width + height;
      return [
        [px - d, py - d, px + d, py + d],
        [px - d, py + d, px + d, py - d],
      ];
    }
    case "small": {
      const h = SMALL_CROSS_SIZE_PX / 2;
      return [
        [px - h, py, px + h, py],
        [px, py - h, px, py + h],
      ];
    }
  }
}

/**
 * Whether the cursor is drawn: `IsCursorEnabled()` is `m_isCursorEnabled || m_forceDisplayCursor`. A tool enables the cursor
 * while it needs a position from the user; the idle selection tool does not (`VC_SETTINGS::Reset` -> `m_showCursor = false`).
 * `toolWantsCursor` is that, for this app: any tool other than the plain selection tool.
 */
export function cursorVisible(alwaysShowCursor: boolean, toolWantsCursor: boolean): boolean {
  return toolWantsCursor || alwaysShowCursor;
}

/** The `CROSS_HAIR_MODE` the three "Cursor ... Crosshairs" actions set, by action name suffix. */
export function crossHairModeForAction(action: "cursorSmallCrosshairs" | "cursorFullCrosshairs" | "cursor45Crosshairs"): CrossHairMode {
  switch (action) {
    case "cursorSmallCrosshairs":
      return "small";
    case "cursorFullCrosshairs":
      return "full";
    case "cursor45Crosshairs":
      return "diag45";
  }
}
