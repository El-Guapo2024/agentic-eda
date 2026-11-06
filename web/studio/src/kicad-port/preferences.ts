// Preferences (`common.SuiteControl.openPreferences`, Ctrl+, -- `PANEL_MOUSE_SETTINGS`, "Mouse and Touchpad",
// common/dialogs/panel_mouse_settings.cpp) and the settings it edits (`COMMON_SETTINGS::m_Input`,
// common/settings/common_settings.cpp).
//
// Only the settings a canvas here actually honours are offered -- a control with no effect would be a fake:
//   * the scroll-wheel gestures: which modifier key (none / Ctrl / Shift / Alt) zooms, pans left/right and pans
//     up/down, one action per column, and whether zoom and left/right pan run reversed;
//   * "Automatically pan while moving object" and its speed (`input.auto_pan`, `input.auto_pan_acceleration`);
//   * what dragging with the left button does in the selection tools -- draw the selection area, drag selected objects, or drag any object (`input.mouse_left`,
//     `leftDragOutcome`), and with the middle and the right button -- pan, zoom or nothing (`input.drag_middle`, `input.drag_right`): every canvas honours them;
//   * "Pan on mouse movement with key:" (`input.motion_pan_modifier`, `motionPan`), every canvas;
//   * "Pan left/right with horizontal movement" (`input.horizontal_pan`): kept and set by the two Reset buttons as KiCad does, and, as in KiCad 8303b2ad, read by
//     nothing -- `WX_VIEW_CONTROLS::onWheel` always pans on a horizontal wheel ("The m_horizontalPan setting only controls whether a keyboard modifier can convert
//     vertical scroll into horizontal pan", and no code does);
//   * "Use zoom acceleration" and the zoom speed with its "Automatic" switch (`input.zoom_acceleration`,
//     `input.zoom_speed`, `input.zoom_speed_auto` -> `WX_VIEW_CONTROLS::LoadSettings`);
//   * the board editor's magnetic points (`PCBNEW_SETTINGS::m_MagneticItems`, "Snap to Pads", "Snap to Tracks and Vias", "Snap to graphics": the board editor's
//     Preferences > Editing Options), which decide what the drawing and move tools snap to (kicad-port/snapAnchors.ts);
//   * the board editor's "Step for rotate commands" (`PCBNEW_SETTINGS::m_RotationAngle`, `editing.rotation_angle`) and "Flip board items" (`m_FlipDirection`,
//     `editing.flip_left_right`), which R / Shift+R and F (and the menu's Rotate and Flip) read (kicad-port/pcbTransform.ts);
//   * the board editor's Origins & Axes (`PCBNEW_SETTINGS::m_Display.m_DisplayOrigin`, `m_DisplayInvertXAxis`, `m_DisplayInvertYAxis`): where X and Y are counted
//     from and which way they grow, wherever the board editor shows or takes a coordinate (kicad-port/originTransforms.ts);
//   * the schematic editor's "Allow unconstrained pin swaps" (`EESCHEMA_SETTINGS::m_Input.allow_unconstrained_pin_swaps`), without which Swap Pins is not offered;
//   * the symbol editor's Repeated Items, "Pitch of repeated pins" and "Label increment" (`SYMBOL_EDITOR_SETTINGS::m_Repeat`), which Repeat Pin (Insert) reads.
// Not offered: "Center and warp cursor on zoom" (`input.center_on_zoom`): it warps the real pointer to the middle of the canvas (`CenterOnCursor`), which a web page
// cannot do -- the browser gives a page no way to move the pointer -- so the wheel always zooms about the pointer, KiCad's other branch.
import { DEFAULT_LEFT_DRAG, DEFAULT_VIEW_CONTROL_SETTINGS, type DragAction, type LeftDragAction, type MotionPanModifier, type ScrollModifier, type ViewControlSettings } from "./viewControls";
import type { MagneticOption, PcbMagnetic } from "./snapAnchors";
import type { FlipDirection } from "./pcbTransform";
import { AcceleratingZoomController, CONSTANT_SCALE, ConstantZoomController, pickDefaultZoomController, type ZoomController } from "./zoomController";
import { isMac } from "../platform";
import type { TextGraphicsDefaults } from "../api/types";
import { DEFAULT_ARC_EDIT_MODE, type ArcEditMode } from "./pcbPointEdit";
import { DEFAULT_USER_LAYER_COUNT, sanitizeFpTextItems, sanitizeUserLayerNames, type FpTextItem } from "./fpFieldDefaults";
import { sanitizeDisplayOrigin, type DisplayOrigin } from "./originTransforms";

/** `COMMON_SETTINGS`' default `input.zoom_speed`: 5 on macOS, 1 elsewhere. */
export function defaultZoomSpeed(isMacPlatform: boolean): number {
  return isMacPlatform ? 5 : 1;
}

export type { DragAction, LeftDragAction, MotionPanModifier, ScrollModifier, FlipDirection };
export type ScrollRow = "zoom" | "panH" | "panV";

export interface Preferences {
  /** `input.scroll_modifier_zoom` / `_pan_h` / `_pan_v`: the key held while scrolling for each action. */
  scrollModifierZoom: ScrollModifier;
  scrollModifierPanH: ScrollModifier;
  scrollModifierPanV: ScrollModifier;
  /** `input.reverse_scroll_zoom` / `reverse_scroll_pan_h`. */
  reverseScrollZoom: boolean;
  reverseScrollPanH: boolean;
  /** `input.auto_pan`, default off. */
  autoPan: boolean;
  /** `input.auto_pan_acceleration`, 1..10, default 5. */
  autoPanAcceleration: number;
  /** `input.drag_middle` / `input.drag_right` (`MOUSE_DRAG_ACTION`): what dragging with that button does. Default pan for both. */
  dragMiddle: DragAction;
  dragRight: DragAction;
  /** `input.mouse_left` ("Left button drag:"): what a left drag does in the selection tools (`leftDragOutcome`). The studio's default is `DEFAULT_LEFT_DRAG`. */
  dragLeft: LeftDragAction;
  /** `input.motion_pan_modifier` ("Pan on mouse movement with key:"), default none. */
  motionPanModifier: MotionPanModifier;
  /** `input.horizontal_pan` ("Pan left/right with horizontal movement"), default off; on with the trackpad defaults. Read by nothing, as in KiCad. */
  horizontalPan: boolean;
  /** `input.zoom_acceleration`, default off (the platform controller ignores it on macOS, as in KiCad). */
  zoomAcceleration: boolean;
  /** `input.zoom_speed_auto`, default on; `input.zoom_speed` 1..10, default 5, used when it is off. */
  zoomSpeedAuto: boolean;
  zoomSpeed: number;
  /** `MAGNETIC_SETTINGS::pads`: snap to pads never / only while routing / always -- default "in track tool". */
  magneticPads: MagneticOption;
  /** `MAGNETIC_SETTINGS::tracks`: tracks and vias -- default "in track tool". */
  magneticTracks: MagneticOption;
  /** `MAGNETIC_SETTINGS::graphics`: snap to graphics -- default off. */
  magneticGraphics: boolean;
  /** `PCBNEW_SETTINGS::m_RotationAngle` ("Step for rotate commands", Preferences > PCB Editor > Editing Options): the degrees one R or Shift+R turns by. Default 90. */
  rotationStepDeg: number;
  /** `PCBNEW_SETTINGS::m_FlipDirection` ("Flip board items"): which way F turns a board item over. Default left/right (the `editing.flip_left_right` default). */
  flipDirection: FlipDirection;
  /** `EESCHEMA_SETTINGS::m_Input.allow_unconstrained_pin_swaps` ("Allow unconstrained pin swaps", Preferences > Schematic Editor > Editing Options): Swap Pins is offered and runs only with it on. Default off. */
  allowUnconstrainedPinSwaps: boolean;
  /** `SYMBOL_EDITOR_SETTINGS::m_Repeat.pin_step` ("Pitch of repeated pins:", Preferences > Symbol Editor > Editing Options > Repeated Items), mils: how far Repeat Pin puts the
   * next pin. Default 100; a multiple of 25 (`MIN_GRID`, "Force pin_step to a grid multiple"). */
  symRepeatPinStepMils: number;
  /** `m_Repeat.label_delta` ("Label increment:", -10 .. 10): what Repeat Pin adds to the pin's number and name. Default 1. */
  symRepeatLabelDelta: number;
  /** `SYMBOL_EDITOR_SETTINGS::m_dragPinsAlongWithEdges` ("Keep pins attached when dragging edges", `drag_pins_along_with_edges`): a rectangle side dragged by the point
   *  editor takes the pins on it along (`RECTANGLE_POINT_EDIT_BEHAVIOR::dragPinsOnEdge`). Default on. */
  symDragPinsWithEdges: boolean;
  /** `FOOTPRINT_EDITOR_SETTINGS::m_RotationAngle` (Preferences > Footprint Editor > Editing Options, "Step for rotate commands", UI-LEFT F8): the Footprint Editor's own
   *  step, the degrees one R or Shift+R turns by there. Default 90. */
  fpRotationStepDeg: number;
  /** `FOOTPRINT_EDITOR_SETTINGS::m_MagneticItems.pads` / `.graphics` ("Magnetic points": Snap to pads, Snap to graphics). Both on by default (`CAPTURE_ALWAYS`, true). */
  fpMagneticPads: boolean;
  fpMagneticGraphics: boolean;
  /** `FOOTPRINT_EDITOR_SETTINGS::m_ArcEditMode` ("Arc editing mode:"): 0 keep the centre and adjust the radius, 1 keep the endpoints or the start's direction, 2 keep the centre and
   *  radius and adjust the endpoints. Default 0. */
  fpArcEditMode: number;
  /** `m_DisplayInvertXAxis` / `m_DisplayInvertYAxis` (Preferences > Footprint Editor > Origins & Axes: "X Axis Increases left", "Y Axis Increases up"): how the
   *  Footprint Editor shows coordinates. Both off by default. */
  fpInvertXAxis: boolean;
  fpInvertYAxis: boolean;
  /** `PCBNEW_SETTINGS::m_Display.m_DisplayOrigin` (Preferences > PCB Editor > Origins & Axes, "Display Origin": Page origin, Drill/place file origin, Grid origin;
   *  `pcb_display.origin_mode`): where the board editor's X and Y are counted from (kicad-port/originTransforms.ts). Default the page (`PCB_ORIGIN_PAGE`). */
  pcbDisplayOrigin: DisplayOrigin;
  /** `m_Display.m_DisplayInvertXAxis` / `m_DisplayInvertYAxis` ("X Axis: Increases left", "Y Axis: Increases up"; `pcb_display.origin_invert_x_axis` / `_y_axis`). Both off by default. */
  pcbInvertXAxis: boolean;
  pcbInvertYAxis: boolean;
  /** `FOOTPRINT_EDITOR_SETTINGS::m_DesignSettings` (Preferences > Footprint Editor > Graphics Defaults: the line width and the text of each layer class); `null` is KiCad's
   *  factory table (`FACTORY_TEXT_GRAPHICS`). */
  fpTextGraphics: TextGraphicsDefaults | null;
  /** `FOOTPRINT_EDITOR_SETTINGS::m_DesignSettings.m_DefaultFPTextItems` (Preferences > Footprint Editor > Footprint Defaults, `design_settings.default_footprint_text_items`):
   *  the Reference's and Value's text, visibility and layer and the other texts a new footprint gets (kicad-port/fpFieldDefaults.ts); `null` is KiCad's own list. */
  fpDefaultTextItems: FpTextItem[] | null;
  /** `m_DesignSettings.GetUserDefinedLayerCount()` ("User layers:" of Preferences > Footprint Editor > User Layer Names, `design_settings.user_layer_count`), 0 .. 9.
   *  Default 4. */
  fpUserLayerCount: number;
  /** `m_DesignSettings.m_UserLayerNames` (`design_settings.default_footprint_layer_names`): a user layer's name in the Footprint Editor, by its canonical name. */
  fpUserLayerNames: Record<string, string>;
  /** `PCBNEW_SETTINGS::m_ArcEditMode` ("Arc editing mode:", Preferences > PCB Editor > Editing Options; `cycleArcEditMode` sets it too): how the point editor drags an arc. */
  pcbArcEditMode: ArcEditMode;
  /** `EESCHEMA_SETTINGS::m_Drawing.arc_edit_mode` ("Arc editing mode:", Preferences > Schematic Editor > Editing Options). */
  schArcEditMode: ArcEditMode;
  /** `PCBNEW_SETTINGS::m_ESCClearsNetHighlight` ("<ESC> clears net highlighting", `editing.esc_clears_net_highlight`): the selection tool's last Escape tier. Default on. */
  escClearsNetHighlight: boolean;
  /** `PCBNEW_SETTINGS::m_ShowCourtyardCollisions` ("Show courtyard collisions when moving/dragging", `editing.show_courtyard_collisions`). Default on. */
  showCourtyardCollisions: boolean;
  /** `PCB_DISPLAY_OPTIONS::m_RatsnestThickness` ("Ratsnest line thickness:", `pcb_display.ratsnest_thickness`), pixels, 0.5 .. 10 by 0.5. Default 0.5. */
  ratsnestThicknessPx: number;
  /** `EESCHEMA_SETTINGS::m_Input.esc_clears_net_highlight` ("<ESC> clears net highlighting" of the Schematic Editor). Default on. */
  schEscClearsNetHighlight: boolean;
  /** `EESCHEMA_SETTINGS::m_Drawing.default_repeat_offset_x` / `_y` ("Horizontal pitch:" / "Vertical pitch:" of Repeated Items), mils: what Repeat Last Item moves a copy by.
   *  Defaults 0 and 100. */
  schRepeatOffsetXMils: number;
  schRepeatOffsetYMils: number;
  /** `EESCHEMA_SETTINGS::m_Drawing.repeat_label_increment` ("Label increment:"): what Repeat Last Item adds to a label's number. Default 1. */
  schRepeatLabelIncrement: number;
  /** `EESCHEMA_SETTINGS::m_Appearance.footprint_preview` ("Show footprint previews in Symbol Chooser"). Default on. */
  schFootprintPreview: boolean;
}

/** The ratsnest thickness as `m_ratsnestThickness` (a `wxSpinCtrlDouble` of 0.5 .. 10 by 0.5) can hold it; anything else is the default 0.5. */
export function sanitizeRatsnestThickness(v: unknown, fallback = 0.5): number {
  if (typeof v !== "number" || !Number.isFinite(v)) return fallback;
  return Math.min(10, Math.max(0.5, Math.round(v * 2) / 2));
}

/** `PANEL_SYM_EDITING_OPTIONS::TransferDataFromWindow`: "Force pin_step to a grid multiple" of 25 mils (`MIN_GRID`), `KiROUND( pin_step / MIN_GRID ) * MIN_GRID`. */
export function sanitizePinStepMils(v: unknown, fallback = 100): number {
  if (typeof v !== "number" || !Number.isFinite(v)) return fallback;
  const r = v / 25;
  return (r < 0 ? -Math.round(-r) : Math.round(r)) * 25;
}

/**
 * The step of the rotate commands, as `PCBNEW_SETTINGS` keeps it: tenths of a degree, one turn at most (`rot % 3600`), and a step of nothing is not a step
 * (`if( aVal ) m_RotationAngle = ...` leaves the old one). Anything unusable is the default 90.
 */
export function sanitizeRotationStep(v: unknown, fallback = 90): number {
  if (typeof v !== "number" || !Number.isFinite(v)) return fallback;
  const tenths = Math.round(v * 10) % 3600;
  const norm = ((tenths % 3600) + 3600) % 3600;
  return norm === 0 ? fallback : norm / 10;
}

/** `COMMON_SETTINGS` factory defaults (common_settings.cpp `input.*`): wheel zooms, Ctrl pans left/right, Shift pans up/down. */
export const DEFAULT_PREFERENCES: Preferences = {
  scrollModifierZoom: "none",
  scrollModifierPanH: "ctrl",
  scrollModifierPanV: "shift",
  reverseScrollZoom: false,
  reverseScrollPanH: false,
  autoPan: false,
  autoPanAcceleration: 5,
  dragMiddle: "pan",
  dragRight: "pan",
  dragLeft: DEFAULT_LEFT_DRAG,
  motionPanModifier: "none",
  horizontalPan: false,
  zoomAcceleration: false,
  zoomSpeedAuto: true,
  // `#ifdef __WXMAC__ default_zoom_speed = 5; #else default_zoom_speed = 1;` (common_settings.cpp, `input.zoom_speed`): a Zoom drag uses it whatever
  // `zoom_speed_auto` says (`WX_VIEW_CONTROLS::onMotion`)
  zoomSpeed: defaultZoomSpeed(isMac()),
  magneticPads: "track-tool",
  magneticTracks: "track-tool",
  magneticGraphics: false,
  rotationStepDeg: 90,
  flipDirection: "left_right",
  allowUnconstrainedPinSwaps: false,
  symRepeatPinStepMils: 100,
  symRepeatLabelDelta: 1,
  symDragPinsWithEdges: true,
  fpRotationStepDeg: 90,
  fpMagneticPads: true,
  fpMagneticGraphics: true,
  fpArcEditMode: 0,
  fpInvertXAxis: false,
  fpInvertYAxis: false,
  pcbDisplayOrigin: "page",
  pcbInvertXAxis: false,
  pcbInvertYAxis: false,
  fpTextGraphics: null,
  fpDefaultTextItems: null,
  fpUserLayerCount: DEFAULT_USER_LAYER_COUNT,
  fpUserLayerNames: {},
  pcbArcEditMode: DEFAULT_ARC_EDIT_MODE,
  schArcEditMode: DEFAULT_ARC_EDIT_MODE,
  escClearsNetHighlight: true,
  showCourtyardCollisions: true,
  ratsnestThicknessPx: 0.5,
  schEscClearsNetHighlight: true,
  schRepeatOffsetXMils: 0,
  schRepeatOffsetYMils: 100,
  schRepeatLabelIncrement: 1,
  schFootprintPreview: true,
};

type ScrollSet = Pick<Preferences, "scrollModifierZoom" | "scrollModifierPanH" | "scrollModifierPanV" | "reverseScrollZoom" | "reverseScrollPanH" | "horizontalPan">;

/** `onMouseDefaults`: the wheel zooms, Ctrl pans left/right, Shift pans up/down, nothing reversed, "Pan left/right with horizontal movement" off. */
export const MOUSE_SCROLL_DEFAULTS: ScrollSet = { scrollModifierZoom: "none", scrollModifierPanH: "ctrl", scrollModifierPanV: "shift", reverseScrollZoom: false, reverseScrollPanH: false, horizontalPan: false };
/** `onTrackpadDefaults`: Ctrl+scroll zooms, Shift pans left/right, the plain two-finger scroll pans up/down, "Pan left/right with horizontal movement" on. */
export const TRACKPAD_SCROLL_DEFAULTS: ScrollSet = { scrollModifierZoom: "ctrl", scrollModifierPanH: "shift", scrollModifierPanV: "none", reverseScrollZoom: false, reverseScrollPanH: false, horizontalPan: true };

const KEYS: Record<ScrollRow, "scrollModifierZoom" | "scrollModifierPanH" | "scrollModifierPanV"> = { zoom: "scrollModifierZoom", panH: "scrollModifierPanH", panV: "scrollModifierPanV" };
const ROWS: readonly ScrollRow[] = ["zoom", "panH", "panV"];
/** `assign_first_available`'s candidate order. */
const CANDIDATES: readonly ScrollModifier[] = ["none", "ctrl", "shift", "alt"];

/** `isScrollModSetValid`: each action on its own column. */
export function scrollModSetValid(p: Pick<Preferences, "scrollModifierZoom" | "scrollModifierPanH" | "scrollModifierPanV">): boolean {
  return p.scrollModifierZoom !== p.scrollModifierPanH && p.scrollModifierPanH !== p.scrollModifierPanV && p.scrollModifierPanV !== p.scrollModifierZoom;
}

/**
 * `OnScrollRadioButton`: the user picks `modifier` for `row`. A Ctrl/Shift/Alt column can hold one action only, so
 * any other row that held `modifier` moves to the first column nobody holds (none, Ctrl, Shift, Alt in that order).
 * Picking "none" for a row leaves the others alone -- a clash of two "none"s is reported by `scrollModSetValid`.
 */
export function assignScrollModifier(prefs: Preferences, row: ScrollRow, modifier: ScrollModifier): Preferences {
  const next: Preferences = { ...prefs, [KEYS[row]]: modifier };
  if (modifier === "none") return next;
  for (const other of ROWS) {
    if (other === row || next[KEYS[other]] !== modifier) continue;
    const taken = (c: ScrollModifier) => ROWS.some((r) => next[KEYS[r]] === c);
    const free = CANDIDATES.find((c) => c !== modifier && !taken(c));
    if (free) next[KEYS[other]] = free;
  }
  return next;
}

/** The part of the settings `handleWheel` / the auto-pan loop read. */
export function toViewControlSettings(p: Preferences): ViewControlSettings {
  return {
    ...DEFAULT_VIEW_CONTROL_SETTINGS,
    autoPanEnabled: p.autoPan,
    autoPanAcceleration: p.autoPanAcceleration,
    dragMiddle: p.dragMiddle,
    dragRight: p.dragRight,
    dragLeft: p.dragLeft,
    motionPanModifier: p.motionPanModifier,
    scrollModifierZoom: p.scrollModifierZoom,
    scrollModifierPanH: p.scrollModifierPanH,
    reverseScrollZoom: p.reverseScrollZoom,
    reverseScrollPanH: p.reverseScrollPanH,
  };
}

/**
 * `WX_VIEW_CONTROLS::LoadSettings`: automatic speed -> the platform's controller (zoom acceleration only matters off macOS);
 * a manual speed -> an accelerating controller scaled by it, or a constant one at `zoom_speed * MANUAL_SCALE_FACTOR`.
 */
export function zoomControllerFor(p: Preferences, isMacPlatform: boolean): ZoomController {
  if (p.zoomSpeedAuto) return pickDefaultZoomController(isMacPlatform, p.zoomAcceleration);
  if (p.zoomAcceleration) return new AcceleratingZoomController(p.zoomSpeed);
  return new ConstantZoomController(CONSTANT_SCALE.MANUAL_FACTOR * p.zoomSpeed);
}

// ---------------------------------------------------------------------------------------------- persistence

export const PREFERENCES_STORAGE_KEY = "eda-studio.preferences.v1";

const MODIFIERS: readonly ScrollModifier[] = ["none", "ctrl", "shift", "alt"];
const isModifier = (v: unknown): v is ScrollModifier => typeof v === "string" && (MODIFIERS as readonly string[]).includes(v);
const dragAction = (v: unknown, fallback: DragAction): DragAction => (v === "pan" || v === "zoom" || v === "none" ? v : fallback);
const leftDrag = (v: unknown, fallback: LeftDragAction): LeftDragAction => (v === "select" || v === "drag_selected" || v === "drag_any" ? v : fallback);
const motionPanKey = (v: unknown, fallback: MotionPanModifier): MotionPanModifier => (v === "none" || v === "alt" || v === "ctrl" || v === "shift" ? v : fallback);
const clampInt = (v: unknown, lo: number, hi: number, fallback: number) => (typeof v === "number" && Number.isFinite(v) ? Math.min(hi, Math.max(lo, Math.round(v))) : fallback);
const bool = (v: unknown, fallback: boolean) => (typeof v === "boolean" ? v : fallback);
const finiteInt = (v: unknown, fallback: number) => (typeof v === "number" && Number.isFinite(v) ? Math.round(v) : fallback);
const arcEditMode = (v: unknown, fallback: ArcEditMode): ArcEditMode =>
  v === "keep_center_adjust_angle_radius" || v === "keep_center_ends_adjust_angle" || v === "keep_endpoints_or_start_direction" ? v : fallback;
const magnetic = (v: unknown, fallback: MagneticOption): MagneticOption => (v === "never" || v === "track-tool" || v === "always" ? v : fallback);

/** The magnetic settings the board editor's snapping reads, with `allLayers` (the toggle of `common.Control.magneticSnapToggle`, Shift+S, kept in the studio state). */
export function toPcbMagnetic(p: Pick<Preferences, "magneticPads" | "magneticTracks" | "magneticGraphics">, allLayers: boolean): PcbMagnetic {
  return { pads: p.magneticPads, tracks: p.magneticTracks, graphics: p.magneticGraphics, allLayers };
}

/** Whatever was stored, made into valid preferences: unknown or malformed fields fall back to their defaults, a clashing wheel assignment to the defaults. */
export function sanitizePreferences(raw: unknown): Preferences {
  const o = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const d = DEFAULT_PREFERENCES;
  const p: Preferences = {
    scrollModifierZoom: isModifier(o.scrollModifierZoom) ? o.scrollModifierZoom : d.scrollModifierZoom,
    scrollModifierPanH: isModifier(o.scrollModifierPanH) ? o.scrollModifierPanH : d.scrollModifierPanH,
    scrollModifierPanV: isModifier(o.scrollModifierPanV) ? o.scrollModifierPanV : d.scrollModifierPanV,
    reverseScrollZoom: bool(o.reverseScrollZoom, d.reverseScrollZoom),
    reverseScrollPanH: bool(o.reverseScrollPanH, d.reverseScrollPanH),
    autoPan: bool(o.autoPan, d.autoPan),
    autoPanAcceleration: clampInt(o.autoPanAcceleration, 1, 10, d.autoPanAcceleration),
    dragMiddle: dragAction(o.dragMiddle, d.dragMiddle),
    dragRight: dragAction(o.dragRight, d.dragRight),
    dragLeft: leftDrag(o.dragLeft, d.dragLeft),
    motionPanModifier: motionPanKey(o.motionPanModifier, d.motionPanModifier),
    horizontalPan: bool(o.horizontalPan, d.horizontalPan),
    zoomAcceleration: bool(o.zoomAcceleration, d.zoomAcceleration),
    zoomSpeedAuto: bool(o.zoomSpeedAuto, d.zoomSpeedAuto),
    zoomSpeed: clampInt(o.zoomSpeed, 1, 10, d.zoomSpeed),
    magneticPads: magnetic(o.magneticPads, d.magneticPads),
    magneticTracks: magnetic(o.magneticTracks, d.magneticTracks),
    magneticGraphics: bool(o.magneticGraphics, d.magneticGraphics),
    rotationStepDeg: sanitizeRotationStep(o.rotationStepDeg, d.rotationStepDeg),
    flipDirection: o.flipDirection === "top_bottom" ? "top_bottom" : d.flipDirection,
    allowUnconstrainedPinSwaps: bool(o.allowUnconstrainedPinSwaps, d.allowUnconstrainedPinSwaps),
    symRepeatPinStepMils: sanitizePinStepMils(o.symRepeatPinStepMils, d.symRepeatPinStepMils),
    symRepeatLabelDelta: clampInt(o.symRepeatLabelDelta, -10, 10, d.symRepeatLabelDelta),
    symDragPinsWithEdges: bool(o.symDragPinsWithEdges, d.symDragPinsWithEdges),
    fpRotationStepDeg: sanitizeRotationStep(o.fpRotationStepDeg, d.fpRotationStepDeg),
    fpMagneticPads: bool(o.fpMagneticPads, d.fpMagneticPads),
    fpMagneticGraphics: bool(o.fpMagneticGraphics, d.fpMagneticGraphics),
    fpArcEditMode: clampInt(o.fpArcEditMode, 0, 2, d.fpArcEditMode),
    fpInvertXAxis: bool(o.fpInvertXAxis, d.fpInvertXAxis),
    fpInvertYAxis: bool(o.fpInvertYAxis, d.fpInvertYAxis),
    pcbDisplayOrigin: sanitizeDisplayOrigin(o.pcbDisplayOrigin),
    pcbInvertXAxis: bool(o.pcbInvertXAxis, d.pcbInvertXAxis),
    pcbInvertYAxis: bool(o.pcbInvertYAxis, d.pcbInvertYAxis),
    fpTextGraphics: o.fpTextGraphics && typeof o.fpTextGraphics === "object" ? (o.fpTextGraphics as TextGraphicsDefaults) : d.fpTextGraphics,
    fpDefaultTextItems: sanitizeFpTextItems(o.fpDefaultTextItems),
    fpUserLayerCount: clampInt(o.fpUserLayerCount, 0, 9, d.fpUserLayerCount),
    fpUserLayerNames: sanitizeUserLayerNames(o.fpUserLayerNames),
    pcbArcEditMode: arcEditMode(o.pcbArcEditMode, d.pcbArcEditMode),
    schArcEditMode: arcEditMode(o.schArcEditMode, d.schArcEditMode),
    escClearsNetHighlight: bool(o.escClearsNetHighlight, d.escClearsNetHighlight),
    showCourtyardCollisions: bool(o.showCourtyardCollisions, d.showCourtyardCollisions),
    ratsnestThicknessPx: sanitizeRatsnestThickness(o.ratsnestThicknessPx, d.ratsnestThicknessPx),
    schEscClearsNetHighlight: bool(o.schEscClearsNetHighlight, d.schEscClearsNetHighlight),
    schRepeatOffsetXMils: finiteInt(o.schRepeatOffsetXMils, d.schRepeatOffsetXMils),
    schRepeatOffsetYMils: finiteInt(o.schRepeatOffsetYMils, d.schRepeatOffsetYMils),
    schRepeatLabelIncrement: clampInt(o.schRepeatLabelIncrement, -100000, 100000, d.schRepeatLabelIncrement),
    schFootprintPreview: bool(o.schFootprintPreview, d.schFootprintPreview),
  };
  return scrollModSetValid(p) ? p : { ...p, ...MOUSE_SCROLL_DEFAULTS };
}

/** The slice of `Storage` used, so tests can pass a plain object. */
export interface PreferencesStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** The saved preferences, or the defaults when nothing usable is saved or storage is unavailable (private window, blocked site data). */
export function loadPreferences(storage: PreferencesStorage | null | undefined): Preferences {
  try {
    const text = storage?.getItem(PREFERENCES_STORAGE_KEY);
    return text ? sanitizePreferences(JSON.parse(text)) : DEFAULT_PREFERENCES;
  } catch {
    return DEFAULT_PREFERENCES;
  }
}

export function savePreferences(storage: PreferencesStorage | null | undefined, prefs: Preferences): void {
  try {
    storage?.setItem(PREFERENCES_STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    /* storage unavailable: the preferences still apply for this session */
  }
}
