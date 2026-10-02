// Pure ports of the keyboard cursor / pan / grid-preset / zoom-to-area math
// from KiCad's common tools. No React, no DOM: the action registry
// (actions/useActionRunner.ts) feeds these the live view and grid and
// dispatches the result.
//
//   common/tool/common_tools.cpp  COMMON_TOOLS::CursorControl  (cursor keys)
//   common/tool/common_tools.cpp  COMMON_TOOLS::PanControl     (Shift+arrows)
//   common/tool/common_tools.cpp  COMMON_TOOLS::GridPreset / GridFast1 /
//                                 GridFast2 / GridFastCycle
//   common/view/wx_view_controls.cpp WX_VIEW_CONTROLS::WarpMouseCursor
//   common/tool/zoom_tool.cpp     ZOOM_TOOL::selectRegion
import { MAX_SCALE, MIN_SCALE, screenToWorld, worldToScreen, type ViewTransform } from "./view";

export type CursorDir = "up" | "down" | "left" | "right";

export interface Pt {
  x: number;
  y: number;
}

/**
 * CursorControl's CURSOR_* cases: step the RAW cursor (GetRawCursorPosition(
 * false), i.e. no grid snap applied first) by one grid cell; the *_FAST
 * variants fall through after `gridSize *= 10`, so a fast step is ten cells.
 * `mirroredX` flips the horizontal sense (view->IsMirroredX()); this app
 * never mirrors its views, so callers pass false.
 */
export function cursorMove(cursor: Pt, gridUm: number | Pt, dir: CursorDir, fast: boolean, mirroredX = false): Pt {
  const g = typeof gridUm === "number" ? { x: gridUm, y: gridUm } : gridUm;
  const gx = fast ? g.x * 10 : g.x;
  const gy = fast ? g.y * 10 : g.y;
  switch (dir) {
    case "up":
      return { x: cursor.x, y: cursor.y - gy };
    case "down":
      return { x: cursor.x, y: cursor.y + gy };
    case "left":
      return { x: cursor.x - (mirroredX ? -gx : gx), y: cursor.y };
    case "right":
      return { x: cursor.x + (mirroredX ? -gx : gx), y: cursor.y };
  }
}

/** World-space centre of a view of `width` x `height` screen pixels (VIEW::GetCenter). */
export function viewCenter(view: ViewTransform, width: number, height: number): Pt {
  const [x, y] = screenToWorld(view, width / 2, height / 2);
  return { x, y };
}

/** VIEW::SetCenter: same scale, `center` moved to the middle of the screen. */
export function viewCenteredOn(view: ViewTransform, width: number, height: number, center: Pt): ViewTransform {
  return { scale: view.scale, x: width / 2 - center.x * view.scale, y: height / 2 - center.y * view.scale };
}

/**
 * PanControl: `center +/-= GAL grid size * 10` along one axis (the plain
 * current grid, NOT the selection grid CursorControl asks the grid helper
 * for), then `view->SetCenter(center)`.
 */
export function panByGrid(view: ViewTransform, width: number, height: number, gridUm: number, dir: CursorDir, mirroredX = false): ViewTransform {
  const c = viewCenter(view, width, height);
  const next = cursorMove(c, gridUm * 10, dir, false, mirroredX);
  return viewCenteredOn(view, width, height, next);
}

/**
 * WarpMouseCursor(aWorldCoordinates, aWarpView=true), the view half: when
 * the cursor's new screen position is outside the canvas, re-centre the view
 * on it (`m_view->SetCenter( clampedPosition )`); otherwise the view is left
 * alone. (The other half, warping the real OS pointer, is not possible from
 * a web page -- the crosshair is drawn from `state.cursorUm` instead.)
 * Returns the same `view` object when nothing changes.
 */
export function warpViewToInclude(view: ViewTransform, width: number, height: number, pos: Pt): ViewTransform {
  const [sx, sy] = worldToScreen(view, pos.x, pos.y);
  if (sx >= 0 && sx <= width && sy >= 0 && sy <= height) return view;
  return viewCenteredOn(view, width, height, pos);
}

/** app_settings.cpp GRID_SETTINGS defaults for pcbnew/footprint editor: `defaultGridIdx = 15` -> fast_grid_1; `defaultGridIdx + 1` -> fast_grid_2. Indexes into DEFAULT_PCB_GRIDS_UM (kicad-port/grid.ts). */
export const DEFAULT_FAST_GRID_1 = 15;
export const DEFAULT_FAST_GRID_2 = 16;

/** GridPreset(idx): `currentGrid = std::clamp( idx, 0, (int) m_grids.size() - 1 )`. */
export function gridPresetIndex(idx: number, gridCount: number): number {
  return Math.max(0, Math.min(gridCount - 1, idx));
}

/** GridFastCycle: if the current grid is fast grid 1, go to fast grid 2; anything else goes to fast grid 1. */
export function fastGridCycleTarget(currentIdx: number, fast1: number, fast2: number): number {
  return currentIdx === fast1 ? fast2 : fast1;
}

/**
 * ZOOM_TOOL::selectRegion's release handling. `a`/`b` are the two drag
 * corners in world space. A zero-width or zero-height box does nothing (the
 * source `break`s without touching the view -> returns null). Otherwise:
 *   ratio = max( |box.w / screenWorld.w|, |box.h / screenWorld.h| )
 *   left button  (zoom in):  scale = view.scale / ratio
 *   right button (zoom out): scale = view.scale * ratio
 *   then SetScale(scale) and SetCenter(box centre).
 */
export function zoomToAreaView(view: ViewTransform, width: number, height: number, a: Pt, b: Pt, rightButton = false): ViewTransform | null {
  const boxW = Math.abs(b.x - a.x);
  const boxH = Math.abs(b.y - a.y);
  if (boxW === 0 || boxH === 0 || view.scale <= 0 || width <= 0 || height <= 0) return null;
  const sW = width / view.scale; // view->ToWorld( clientSize, false ): a size, so no translation
  const sH = height / view.scale;
  const ratio = Math.max(Math.abs(boxW / sW), Math.abs(boxH / sH));
  const raw = rightButton ? view.scale * ratio : view.scale / ratio;
  const scale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, raw));
  const centre = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
  return viewCenteredOn({ ...view, scale }, width, height, centre);
}
