// Pure ports of the view-framing half of COMMON_TOOLS and the zoom list the zoom presets index.
//
//   common/tool/common_tools.cpp  COMMON_TOOLS::doZoomFit (ZOOM_FIT_SELECTION), doCenter (CENTER_SELECTION /
//                                 CENTER_CONTENTS), ZoomPreset / doZoomToPreset
//   include/zoom_defines.h        ZOOM_LIST_PCBNEW, ZOOM_LIST_EESCHEMA
//   common/settings/app_settings.cpp  APP_SETTINGS_BASE::DefaultZoomList (eeschema and the symbol editor get the
//                                 eeschema list, every other editor the pcbnew one)
//   common/eda_draw_frame.cpp     EDA_DRAW_FRAME::UpdateZoomSelectBox (the preset nearest the current zoom)
//   common/advanced_config.cpp    m_ScreenDPI = 91
//   include/gal/graphics_abstraction_layer.h  m_worldScale = m_screenDPI * m_worldUnitLength * m_zoomFactor
//
// KiCad names a zoom by the GAL zoom factor `Z`: the number of pixels one inch of the drawing takes is
// `Z * screenDPI`, whatever the editor's internal unit. Here a view's `scale` is screen pixels per um, so
// `scale = Z * screenDPI / 25400`.
import { MAX_SCALE, MIN_SCALE, type ViewTransform } from "./view";
import { viewCenteredOn } from "./cursorControl";
import type { Box } from "./itemBoxes";

/** `ADVANCED_CFG::m_ScreenDPI` default. */
export const SCREEN_DPI = 91;

/** A GAL zoom factor as this app's view scale (pixels per um). */
export function zoomFactorToScale(zoomFactor: number): number {
  return (zoomFactor * SCREEN_DPI) / 25_400;
}

/** The GAL zoom factor of a view scale (what `GAL::GetZoomFactor` would say). */
export function scaleToZoomFactor(scale: number): number {
  return (scale * 25_400) / SCREEN_DPI;
}

/** `ZOOM_LIST_PCBNEW` -- the board editor, the footprint editor and the footprint viewer. */
export const ZOOM_LIST_PCBNEW: readonly number[] = [0.13, 0.22, 0.35, 0.6, 1.0, 1.5, 2.2, 3.5, 5.0, 8.0, 13.0, 20.0, 35.0, 50.0, 80.0, 130.0, 220.0, 300.0];

/** `ZOOM_LIST_EESCHEMA` -- the schematic editor and the symbol editor. */
export const ZOOM_LIST_EESCHEMA: readonly number[] = [0.05, 0.07, 0.1, 0.15, 0.2, 0.3, 0.5, 0.7, 1.0, 1.5, 2.0, 3.0, 4.5, 6.5, 10.0, 15.0, 20.0, 30.0, 45.0, 65.0, 100.0];

/** `DefaultZoomList()`: which list an editor's zoom presets come from. */
export function zoomListFor(tab: "pcb" | "schematic" | "footprint" | "symbol"): readonly number[] {
  return tab === "schematic" || tab === "symbol" ? ZOOM_LIST_EESCHEMA : ZOOM_LIST_PCBNEW;
}

/**
 * `doZoomFit`'s margin: "Reserve enough margin to limit the amount of the view that might be obscured behind the
 * infobar" -- 1.04 normally, 1.10 on a canvas shorter than 768 px.
 */
export function fitMarginFactor(canvasHeightPx: number): number {
  return canvasHeightPx < 768 ? 1.1 : 1.04;
}

/** `doZoomFit( ZOOM_FIT_ALL )`'s bigger margin for the library editors: "Leave a bigger margin for library editors & viewers" -- 1.48 in the symbol and footprint editors. */
export const LIBRARY_EDITOR_FIT_MARGIN = 1.48;

/**
 * `doZoomFit( ZOOM_FIT_SELECTION )`: the view that frames `box`, centred on it, with the margin above.
 *
 * `view->SetScale( 1.0 )` first makes `ToWorld( clientSize )` the size of the canvas in world units at scale 1, and
 * `scale = view->GetScale() / max( |vsize.x / screenSize.x|, |vsize.y / screenSize.y| )` is then the scale at which the
 * box fills the canvas -- here that is `min( width / boxW, height / boxH )` pixels per um. A box with no width or no
 * height is replaced by `defaultBox` (`if( bBox.GetWidth() == 0 || bBox.GetHeight() == 0 ) bBox = defaultBox`).
 */
export function zoomFitBox(box: Box, defaultBox: Box, width: number, height: number, margin: number = fitMarginFactor(height)): ViewTransform | null {
  if (!(width > 0) || !(height > 0)) return null;
  let b = box;
  if (b[2] - b[0] === 0 || b[3] - b[1] === 0) b = defaultBox;
  const bw = Math.abs(b[2] - b[0]);
  const bh = Math.abs(b[3] - b[1]);
  if (bw === 0 || bh === 0) return null;
  const raw = Math.min(width / bw, height / bh);
  // "if the scale isn't finite (most likely due to an empty canvas) ... quit out of trying to zoom to fit"
  if (!Number.isFinite(raw) || raw <= 0) return null;
  const scale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, raw / margin));
  return viewCenteredOn({ scale, x: 0, y: 0 }, width, height, { x: (b[0] + b[2]) / 2, y: (b[1] + b[3]) / 2 });
}

/** `doCenter`: the same zoom, the view re-centred on `(x, y)` (`getView()->SetCenter( bBox.Centre() )`). */
export function centerViewOn(view: ViewTransform, width: number, height: number, at: { x: number; y: number }): ViewTransform | null {
  if (!(width > 0) || !(height > 0) || !(view.scale > 0)) return null;
  return viewCenteredOn(view, width, height, at);
}

/** The centre of a box. */
export function boxCentre(box: Box): { x: number; y: number } {
  return { x: (box[0] + box[2]) / 2, y: (box[1] + box[3]) / 2 };
}

/**
 * `doZoomToPreset( idx, false )`: `idx` 0 is "Zoom Auto" (the caller runs zoom-fit-screen), `idx` N the Nth entry of
 * the zoom list; the zoom is set about the view centre (`getView()->SetScale( scale )`, anchor = the view centre).
 * An index past the end of the list stays at the last entry rather than reading outside it.
 */
export function zoomPresetScale(list: readonly number[], idx: number): { auto: true } | { auto: false; scale: number } {
  if (idx <= 0 || list.length === 0) return { auto: true };
  const factor = list[Math.min(idx, list.length) - 1]!;
  return { auto: false, scale: Math.max(MIN_SCALE, Math.min(MAX_SCALE, zoomFactorToScale(factor))) };
}

/** The view with `scale` set about the centre of a `width` x `height` canvas. */
export function setScaleAboutCentre(view: ViewTransform, width: number, height: number, scale: number): ViewTransform {
  if (!(view.scale > 0)) return view;
  const f = scale / view.scale;
  const px = width / 2;
  const py = height / 2;
  return { scale, x: px - (px - view.x) * f, y: py - (py - view.y) * f };
}

/**
 * `UpdateZoomSelectBox` / `ZOOM_MENU::update`: the list entry nearest the current zoom, as the 1-based index
 * `zoomPreset` takes (0 stands for Auto, shown when the list is empty).
 */
export function nearestZoomPreset(list: readonly number[], zoomFactor: number): number {
  let best = 0;
  let bestErr = 1e9;
  list.forEach((z, i) => {
    const err = Math.abs(z - zoomFactor) / zoomFactor;
    if (err < bestErr) {
      bestErr = err;
      best = i + 1;
    }
  });
  return best;
}
