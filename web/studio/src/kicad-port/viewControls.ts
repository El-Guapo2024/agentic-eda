// Port of common/view/wx_view_controls.cpp's mouse-wheel and edge-autopan
// handling -- the two interactions common/view/view_controls.cpp's
// VC_SETTINGS configures (defaults below) and WX_VIEW_CONTROLS::onWheel /
// ::handleAutoPanning / ::onTimer implement. Pure functions over this
// app's ViewTransform; Canvas.tsx supplies the DOM event and owns any
// requestAnimationFrame/timer loop.
//
// Two things wx_view_controls.cpp does that this port deliberately does
// NOT do, both for the same reason -- a real OS pointer warp:
//   - onWheel's "center on cursor before zooming" branch (IsCursorWarping
//     Enabled(), from the `center_on_zoom` setting, default true in
//     source) calls CenterOnCursor(), which warps the pointer to the
//     canvas center. A browser page cannot move the real OS pointer, and
//     this app's own rules forbid it even where possible -- so this port
//     always takes the *other* branch (SetScale(scale, anchor) with the
//     anchor under the cursor, unchanged pointer), which is exactly the
//     "wheel zoom around the cursor" behavior the task asks for anyway.
//   - DRAG_ZOOMING (right/middle-drag-to-zoom) warping the pointer back
//     when the drag reaches the window edge ("infinite drag"). Plain
//     drag-pan/zoom still works; it just stops at the window edge instead
//     of wrapping, same tradeoff as above.
import { zoomAbout, panByWorldDelta, type ViewTransform } from "./view";
import type { ZoomController } from "./zoomController";

export type DragAction = "pan" | "zoom" | "none";
export type ScrollModifier = "none" | "ctrl" | "shift" | "alt";

/** common/view/view_controls.cpp VC_SETTINGS::Reset() + common/settings/common_settings.cpp's input.* PARAM defaults -- the subset this port actually uses. */
export interface ViewControlSettings {
  /** input.auto_pan, default false -- KiCad ships with edge auto-pan OFF; Preferences > Mouse and Touchpad turns it on. */
  autoPanEnabled: boolean;
  /** VC_SETTINGS::m_autoPanMargin, fraction of the screen's smaller-ish dimension (min of width,height scaled), default 0.02. */
  autoPanMargin: number;
  /** input.auto_pan_acceleration, default 5. */
  autoPanAcceleration: number;
  /** input.drag_middle / drag_right, default PAN for both. (drag_left defaults to DRAG_SELECTED/box-select, not a view-control action, so it isn't modeled here.) */
  dragMiddle: DragAction;
  dragRight: DragAction;
  /** input.scroll_modifier_zoom, default "none" (plain wheel zooms). */
  scrollModifierZoom: ScrollModifier;
  /** input.scroll_modifier_pan_h, default "ctrl". */
  scrollModifierPanH: ScrollModifier;
  /** input.reverse_scroll_zoom, default false. */
  reverseScrollZoom: boolean;
  /** input.reverse_scroll_pan_h, default false. */
  reverseScrollPanH: boolean;
}

export const DEFAULT_VIEW_CONTROL_SETTINGS: ViewControlSettings = {
  autoPanEnabled: false,
  autoPanMargin: 0.02,
  autoPanAcceleration: 5.0,
  dragMiddle: "pan",
  dragRight: "pan",
  scrollModifierZoom: "none",
  scrollModifierPanH: "ctrl",
  reverseScrollZoom: false,
  reverseScrollPanH: false,
};

export interface ScreenSize {
  width: number;
  height: number;
}

/** A DOM WheelEvent's relevant fields -- shiftKey/ctrlOrCmd/altKey are the already-platform-resolved modifier booleans (ctrlOrCmd = ctrlKey, or metaKey on macOS, exactly like actions/hotkeys.ts's eventToHotkey normalizes it, since macOS KiCad reads Cmd wherever Windows/Linux KiCad reads Ctrl). `x, y` are canvas-relative pixels, for the zoom anchor. */
export interface WheelInput {
  deltaX: number;
  deltaY: number;
  shiftKey: boolean;
  ctrlOrCmd: boolean;
  altKey: boolean;
  x: number;
  y: number;
}

export type WheelResultKind = "zoom" | "pan" | "pan-horizontal" | "unhandled";

export interface WheelResult {
  view: ViewTransform;
  kind: WheelResultKind;
}

/** wx_view_controls.cpp onWheel: `const double wheelPanSpeed = 0.001;`. */
const WHEEL_PAN_SPEED = 0.001;

/**
 * wxMouseEvent::GetWheelRotation() has no DOM equivalent: wx reports a
 * platform-native "detent" count (traditionally +-120 per physical wheel
 * click, continuous and smaller for precision/trackpad devices); DOM
 * WheelEvents report deltaX/deltaY pixels (or lines/pages per
 * `deltaMode`), with no separate "which axis is this" flag -- a single
 * event can carry both. This maps a DOM delta onto the same "rotation"
 * axis source's math expects: sign flipped so scrolling up/left (DOM
 * negative) is a positive rotation (wx: forward on the wheel = positive,
 * which `onWheel` treats as zoom-in / pan up), magnitude passed through
 * as-is (ConstantZoomController already clamps to +-100 same as source).
 */
function toRotation(delta: number): number {
  return -delta;
}

/**
 * wx_view_controls.cpp WX_VIEW_CONTROLS::onWheel, ported condition-for-
 * condition. `screenSize` plays GetScreenPixelSize()'s role (the canvas's
 * own pixel size, NOT the delta/anchor position); `zoomController` is
 * whatever pickDefaultZoomController() selected (or a caller-chosen one).
 */
export function handleWheel(view: ViewTransform, screenSize: ScreenSize, input: WheelInput, settings: ViewControlSettings, zoomController: ZoomController): WheelResult {
  // "Native horizontal wheel events ... are always handled as horizontal
  // pan" regardless of modifiers/settings -- source picks this by
  // GetWheelAxis(); DOM has no such flag, so a diagonal/axis-dominant
  // delta is treated as whichever axis has the larger magnitude, same
  // practical effect for any real tilt-wheel/trackpad event (one axis
  // dominates; they're rarely exactly equal).
  if (Math.abs(input.deltaX) > Math.abs(input.deltaY)) {
    const rotation = toRotation(input.deltaX);
    const worldPerPx = 1 / view.scale;
    const scrollVecX = screenSize.width * worldPerPx * (rotation * WHEEL_PAN_SPEED);
    // source: `SetCenter(GetCenter() + VECTOR2D(scrollVec.x, 0))` -- NOT negated,
    // unlike every other pan branch below (this asymmetry is in source as-is).
    return { view: panByWorldDelta(view, -scrollVecX, 0), kind: "pan-horizontal" };
  }

  // "Shift beats control beats alt, we don't support more than one" --
  // nMods counts how many of the three are down; modifiers records only
  // the FIRST one found in that priority order.
  let nMods = 0;
  let modifier: ScrollModifier = "none";
  if (input.shiftKey) {
    nMods++;
    modifier = "shift";
  }
  if (input.ctrlOrCmd) {
    nMods++;
    if (modifier === "none") modifier = "ctrl";
  }
  if (input.altKey) {
    nMods++;
    if (modifier === "none") modifier = "alt";
  }

  // "When we have multiple mods, forward it for tool handling" -- source
  // lets the event propagate to the tool stack unhandled; this port has
  // no tool-level wheel bindings, so this is just a no-op for the caller.
  if (nMods > 1) return { view, kind: "unhandled" };

  if (modifier === settings.scrollModifierZoom) {
    const rotation = toRotation(input.deltaY) * (settings.reverseScrollZoom ? -1 : 1);
    const zoomScale = zoomController.getScaleForRotation(rotation);
    return { view: zoomAbout(view, input.x, input.y, zoomScale), kind: "zoom" };
  }

  const rotation = toRotation(input.deltaY);
  const worldPerPx = 1 / view.scale;
  const scrollVec = { x: screenSize.width * worldPerPx * (rotation * WHEEL_PAN_SPEED), y: screenSize.height * worldPerPx * (rotation * WHEEL_PAN_SPEED) };

  let dx = 0;
  let dy = 0;
  if (modifier === settings.scrollModifierPanH) {
    dx = settings.reverseScrollPanH ? scrollVec.x : -scrollVec.x;
  } else {
    dy = -scrollVec.y;
  }
  return { view: panByWorldDelta(view, dx, dy), kind: "pan" };
}

// ---------------------------------------------------------------- drag actions

/**
 * `WX_VIEW_CONTROLS::onButton`: what pressing `button` (the DOM's 1 is the middle one, 2 the right one) starts in an idle view -- DRAG_PANNING or DRAG_ZOOMING, as
 * `m_dragMiddle` / `m_dragRight` say, or nothing. The left button (0) is not a view control: `drag_left` decides between box selection and dragging items.
 */
export function dragActionOf(button: number, settings: Pick<ViewControlSettings, "dragMiddle" | "dragRight">): DragAction {
  if (button === 1) return settings.dragMiddle;
  if (button === 2) return settings.dragRight;
  return "none";
}

/**
 * `WX_VIEW_CONTROLS::onMotion` in DRAG_ZOOMING: the pointer moved `dyPx` pixels down the screen since the last motion event (`d.y = m_dragStartPoint.y - mousePos.y` is positive for
 * a move up, so dragging up zooms in) and the view is scaled by `exp( d.y * zoomSpeed * 0.001 )` about the point the press was at (`m_zoomStartPoint`), which stays where it is
 * on the screen. `zoomSpeed` is `input.zoom_speed`, 5 by default on macOS and 1 elsewhere (`defaultZoomSpeed`).
 */
export function dragZoomView(view: ViewTransform, anchorPx: { x: number; y: number }, dyPx: number, zoomSpeed: number): ViewTransform {
  const scale = Math.exp(-dyPx * zoomSpeed * 0.001);
  return zoomAbout(view, anchorPx.x, anchorPx.y, scale);
}

// ---------------------------------------------------------------- autopan

export interface AutoPanState {
  /** World-space pan delta to apply *per tick* while the cursor stays here -- null when the cursor isn't in the autopan border at all (handleAutoPanning returning false with panDirection (0,0)). */
  panNm: { x: number; y: number } | null;
}

/**
 * wx_view_controls.cpp WX_VIEW_CONTROLS::handleAutoPanning, minus the
 * keyboard-cursor/drag-state special cases (source skips autopan while
 * DRAG_PANNING/DRAG_ZOOMING, and while the last move came from arrow-key
 * cursor control rather than the mouse -- this port only ever gets called
 * from a real pointer-move handler, so neither applies). Returns the
 * *border-relative* pan direction in screen pixels, pre-acceleration,
 * exactly like source's `m_panDirection`; call `computeAutoPanStep` with
 * it each timer tick to get the actual per-tick world-space pan.
 *
 * `cursorPx` is the pointer position relative to the canvas's own
 * top-left (0,0), same coordinate space as WheelInput.x/y.
 */
export function computeAutoPanDirection(cursorPx: { x: number; y: number }, screenSize: ScreenSize, margin = DEFAULT_VIEW_CONTROL_SETTINGS.autoPanMargin): { x: number; y: number } {
  // r2 D-V2: `int borderStart = std::min( m_settings.m_autoPanMargin * m_view->GetScreenPixelSize().x, ... )` truncates, and the pointer `p` is a
  // `VECTOR2I`: the border and the direction are whole pixels
  const borderStart = Math.max(Math.trunc(Math.min(margin * screenSize.width, margin * screenSize.height)), 2);
  const borderEndX = Math.trunc(screenSize.width) - borderStart;
  const borderEndY = Math.trunc(screenSize.height) - borderStart;
  const px = Math.trunc(cursorPx.x);
  const py = Math.trunc(cursorPx.y);

  let x = 0;
  let y = 0;
  if (px < borderStart) x = -(borderStart - px);
  else if (px > borderEndX) x = px - borderEndX;

  if (py < borderStart) y = -(borderStart - py);
  else if (py > borderEndY) y = py - borderEndY;

  return { x, y };
}

function vecLength(v: { x: number; y: number }): number {
  return Math.hypot(v.x, v.y);
}

/** VECTOR2D::Resize(length): same direction, given magnitude (zero vector stays zero -- source's EuclideanNorm()-based Resize no-ops on a zero vector too, since there's no direction to preserve). */
function resize(v: { x: number; y: number }, length: number): { x: number; y: number } {
  const len = vecLength(v);
  if (len === 0) return { x: 0, y: 0 };
  return { x: (v.x / len) * length, y: (v.y / len) * length };
}

/**
 * wx_view_controls.cpp WX_VIEW_CONTROLS::onTimer's AUTO_PANNING case, the
 * per-tick pan-step math (everything after "borderSize"/"accel"/"dir" are
 * computed): given the raw border-relative direction from
 * computeAutoPanDirection, scale it by distance-into-the-border (barely
 * past the border = barely any pan; at or beyond a half-border-width past
 * it = a full accelerated step) and convert screen pixels to a world-space
 * delta via the view's current scale (source's `m_view->ToWorld(dir, false)`).
 *
 * Returns null when `dir` is (0,0) (cursor isn't in the autopan border at
 * all) -- same as source's `handleAutoPanning` returning false and the
 * timer doing nothing that tick.
 */
export function computeAutoPanStep(dir: { x: number; y: number }, screenSize: ScreenSize, scalePxPerNm: number, margin = DEFAULT_VIEW_CONTROL_SETTINGS.autoPanMargin, acceleration = DEFAULT_VIEW_CONTROL_SETTINGS.autoPanAcceleration): { x: number; y: number } | null {
  if (dir.x === 0 && dir.y === 0) return null;

  const borderSize = Math.min(margin * screenSize.width, margin * screenSize.height);
  const accel = 0.5 + acceleration / 5.0;

  let resized = dir;
  const len = vecLength(dir);
  if (len >= borderSize) resized = resize(dir, borderSize * accel);
  else if (len > borderSize / 2) resized = resize(dir, borderSize);
  // else: "for a small mouse cursor dist to area, just use the distance" -- keep `dir` as-is.

  // source: `dir = m_view->ToWorld(dir, false); m_view->SetCenter(GetCenter() + dir);`
  // ToWorld(vec, false) divides a screen-space vector by scale; SetCenter
  // adds it directly (not negated -- panning *toward* the cursor's side,
  // same sense as a middle-drag dragging the board under the cursor).
  const worldPerPx = 1 / scalePxPerNm;
  return { x: resized.x * worldPerPx, y: resized.y * worldPerPx };
}

/** wx_view_controls.cpp onTimer: `m_panTimer.Start((int)(250.0 / 60.0), true)` -- a ~4.17ms one-shot, restarted every tick while AUTO_PANNING. In practice this just means "as fast as the host will schedule it"; this app drives the loop from requestAnimationFrame instead of a wx one-shot timer, so this constant is kept only for documentation/tests, not as a literal setTimeout delay. */
export const AUTO_PAN_TIMER_MS = 250 / 60;
