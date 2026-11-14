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
import { DEFAULT_FP_SELECTION_FILTER, type FpSelectionFilter } from "./fpAppearance";
import type { LayerPreset } from "./appearance";
import { DEFAULT_LEFT_DRAG, DEFAULT_VIEW_CONTROL_SETTINGS, type DragAction, type LeftDragAction, type MotionPanModifier, type ScrollModifier, type ViewControlSettings } from "./viewControls";
import type { MagneticOption, PcbMagnetic } from "./snapAnchors";
import type { FlipDirection } from "./pcbTransform";
import { AcceleratingZoomController, CONSTANT_SCALE, ConstantZoomController, pickDefaultZoomController, type ZoomController } from "./zoomController";
import { isMac } from "../platform";
import type { TextGraphicsDefaults } from "../api/types";
import { DEFAULT_ARC_EDIT_MODE, type ArcEditMode } from "./pcbPointEdit";
import { DEFAULT_USER_LAYER_COUNT, sanitizeFpTextItems, sanitizeUserLayerNames, type FpTextItem } from "./fpFieldDefaults";
import { sanitizeDisplayOrigin, type DisplayOrigin } from "./originTransforms";
import { DEFAULT_PCB_DISPLAY, sanitizeNetNamesMode, sanitizeTrackClearanceMode, type NetNamesMode, type TrackClearanceMode } from "./pcbDisplayOptions";
import type { LineMode } from "./schLineMode";
import type { SchColor } from "../api/schEditTypes";
import type { TemplateFieldName } from "../api/types";
import { DEFAULT_SIM_WHEEL_ACTIONS, sanitizeSimWheelActions, type SimWheelActions } from "./sim/simWheel";

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
  /** `input.mouse_left` ("Left button drag:"): what a left drag does in the selection tools (`leftDragOutcome`). Default `DEFAULT_LEFT_DRAG`, KiCad's DRAG_SELECTED. */
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
  /** `PCBNEW_SETTINGS::m_AllowFreePads` ("Allow free pads", Preferences > PCB Editor > Editing Options): "pads can be moved with respect to the rest of the footprint" --
   *  the edit tools keep a selected pad as itself (`FilterCollectorForFreePads`, kicad-port/freePads.ts). Default off. */
  allowFreePads: boolean;
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
  /** `FOOTPRINT_EDITOR_SETTINGS::m_SelectionFilter` (`editing.selection_filter`): the Footprint Editor's Selection Filter pane, kept between sessions (UI-LEFT-2 FF5). */
  fpSelectionFilter: FpSelectionFilter;
  /** `FOOTPRINT_EDITOR_SETTINGS::m_LayerPresets` (`pcb_display.layer_presets`): the person's own layer presets of the Footprint Editor's Appearance manager (UI-LEFT-2 FF5). */
  fpLayerPresets: LayerPreset[];
  /** `PCBNEW_SETTINGS::m_ArcEditMode` ("Arc editing mode:", Preferences > PCB Editor > Editing Options; `cycleArcEditMode` sets it too): how the point editor drags an arc. */
  pcbArcEditMode: ArcEditMode;
  /** `EESCHEMA_SETTINGS::m_Drawing.arc_edit_mode` ("Arc editing mode:", Preferences > Schematic Editor > Editing Options). */
  schArcEditMode: ArcEditMode;
  /** `PCBNEW_SETTINGS::m_ESCClearsNetHighlight` ("<ESC> clears net highlighting", `editing.esc_clears_net_highlight`): the selection tool's last Escape tier. Default on. */
  escClearsNetHighlight: boolean;
  /** `PCBNEW_SETTINGS::m_ShowCourtyardCollisions` ("Show courtyard collisions when moving/dragging", `editing.show_courtyard_collisions`). Default on. */
  showCourtyardCollisions: boolean;
  /** `PCBNEW_SETTINGS::m_AutoRefillZones` ("Automatically refill zones", `editing.auto_fill_zones`): after each edit the zones it damaged and the unfilled ones are filled
   *  (`BOARD_COMMIT::Push` -> `zoneFillDirty`; kicad-port/autoRefill.ts). Default off (UI-LEFT-2 PP10). */
  autoRefillZones: boolean;
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
  /** `EESCHEMA_SETTINGS::m_Drawing.new_power_symbols` ("Power Symbols:" of Defaults for New Objects): `POWER_SYMBOLS::DEFAULT` (0, follow the symbol's
   *  library), `GLOBAL` (1) or `LOCAL` (2) -- what Place Power converts a new power symbol to (`SCH_DRAWING_TOOLS::PlaceSymbol`). Default 0. */
  schNewPowerSymbols: 0 | 1 | 2;
  /** `PCBNEW_SETTINGS::m_Display.m_NetNames` ("Net names:" of Preferences > PCB Editor > Display Options, `pcb_display.net_names_mode`): 0 do not show, 1 on pads,
   *  2 on tracks, 3 on pads & tracks (the default) -- kicad-port/pcbDisplayOptions.ts, UI-LEFT-3 P3-1. */
  pcbNetNames: NetNamesMode;
  /** `m_Display.m_TrackClearance` ("Clearance Outlines" > "Tracks:", `pcb_display.track_clearance_mode`): default "Show when routing w/ via clearance at end" (UI-LEFT-3 P3-6). */
  pcbTrackClearance: TrackClearanceMode;
  /** `m_Display.m_PadClearance` ("Show pad clearance", `pcb_display.pad_clearance`). Default on. */
  pcbPadClearance: boolean;
  /** `EESCHEMA_SETTINGS::m_Input.drag_is_move` (the inverse of "Mouse drag performs Drag (G) operation", `input.drag_is_move`): a left drag of the selection in the
   *  selection tool moves it (M) rather than drags it (G, wires stretch). Default off (UI-LEFT-3 S3-26). */
  schDragIsMove: boolean;
  /** `m_Drawing.auto_start_wires` ("Automatically start wires on unconnected pins", `drawing.auto_start_wires`): a click of the selection tool on an unconnected pin,
   *  label, sheet pin, bus entry or wire end starts a wire there (`SCH_SELECTION_TOOL::autostartEvent`). Default on (UI-LEFT-3 S3-1). */
  schAutoStartWires: boolean;
  /** `m_Drawing.line_mode` (`drawing.line_mode`): the wire, bus and line tools' Line Mode (Free Angle, 90 deg, 45 deg) -- the toolbar's and Editing Options'
   *  "Line drawing mode:", kept between sessions as KiCad keeps it. Default `LINE_MODE_90` (UI-LEFT-3 S3-26). */
  schLineMode: LineMode;
  /** `m_AutoplaceFields.enable` / `.allow_rejustify` / `.align_to_grid` (Symbol Field Automatic Placement: "Automatically place symbol fields", "Allow field autoplace to
   *  change justification", "Always align autoplaced fields to the 50 mil grid"), sent with every command (`eda_engine::fields::AutoplacePrefs`). All on by
   *  default (UI-LEFT-3 S3-3). */
  schAutoplaceFields: boolean;
  schAutoplaceJustify: boolean;
  schAutoplaceAlign: boolean;
  /** `m_Drawing.default_sheet_border_color` / `default_sheet_background_color` ("Sheet border:", "Sheet background:" of Defaults for New Objects): the colours a sheet
   *  drawn gets (`SCH_DRAWING_TOOLS::DrawSheet`); null is `COLOR4D::UNSPECIFIED`, the layers' colours. Default null (UI-LEFT-3 S3-26). */
  schSheetBorderColor: SchColor | null;
  schSheetBackgroundColor: SchColor | null;
  /** `EESCHEMA_SETTINGS::m_RescueNeverShow` ("Never show Rescue Symbols tool", `system.never_show_rescue_dialog`): a legacy schematic brought in without its cache
   *  library is not offered Rescue Symbols (`OpenProjectFiles`); Rescue's "Never Show Again" sets it. Default off (UI-LEFT-3 S3-26). */
  schRescueNeverShow: boolean;
  /** `m_Appearance.default_font` (Preferences > Schematic Editor > Display Options > "Default font:", `appearance.default_font`): the face of every schematic text with none
   *  of its own -- fields, labels, texts, pins' names and numbers (`PIN_LAYOUT_CACHE`) -- and of the plots (`SetDefaultFont`). Default "KiCad Font" (UI-LEFT-3 S3-29). */
  schDefaultFont: string;
  /** `m_Selection.highlight_thickness` (Preferences > Schematic Editor > Display Options > Selection's "Highlight thickness:", `selection.highlight_thickness`, mils, 0 ..
   *  50 on the page, `KiROUND` on OK): how much wider than the item a brightened item's shadow is drawn, in screen pixels and in mils (`SCH_PAINTER::getShadowWidth(
   *  true )`), in the schematic and the Symbol Editor. Default 2. */
  schHighlightThicknessMils: number;
  /** `m_Drawing.field_names` (Preferences > Schematic Editor > Field Name Templates, `PANEL_TEMPLATE_FIELDNAMES` global): the person's own templates, offered with the
   *  project's (`TEMPLATES::resolveTemplates`: a project template of the same name wins) and alone in the Symbol Editor's Symbol Properties. Default none (UI-LEFT-3 C3-2). */
  schGlobalFieldNames: TemplateFieldName[];
  /** `m_Simulator.preferences.mouse_wheel_actions` (Preferences > Schematic Editor > Simulator): what the scroll wheel does on a simulator plot, per modifier
   *  (`mpWindow::onMouseWheel`). KiCad's mouse defaults (UI-LEFT-3 C3-2). */
  simWheel: SimWheelActions;
  /** `SYMBOL_EDITOR_SETTINGS::m_Defaults` (Preferences > Symbol Editor > Editing Options > Defaults for New Objects), mils: the line width of a new shape or text box
   *  (0: inherit the schematic's), the text size of a new text, text box and field, the length and the number and name sizes of the first new pin
   *  (`GetLastPinLength`, ...). Defaults 0, 50, 100, 50, 50 (UI-LEFT-3 C3-2). */
  symDefaultLineWidthMils: number;
  symDefaultTextSizeMils: number;
  symDefaultPinLengthMils: number;
  symDefaultPinNumSizeMils: number;
  symDefaultPinNameSizeMils: number;
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
  allowFreePads: false,
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
  fpSelectionFilter: DEFAULT_FP_SELECTION_FILTER,
  fpLayerPresets: [],
  pcbArcEditMode: DEFAULT_ARC_EDIT_MODE,
  schArcEditMode: DEFAULT_ARC_EDIT_MODE,
  escClearsNetHighlight: true,
  showCourtyardCollisions: true,
  autoRefillZones: false,
  ratsnestThicknessPx: 0.5,
  schEscClearsNetHighlight: true,
  schRepeatOffsetXMils: 0,
  schRepeatOffsetYMils: 100,
  schRepeatLabelIncrement: 1,
  schFootprintPreview: true,
  schNewPowerSymbols: 0,
  pcbNetNames: DEFAULT_PCB_DISPLAY.netNames,
  pcbTrackClearance: DEFAULT_PCB_DISPLAY.trackClearance,
  pcbPadClearance: DEFAULT_PCB_DISPLAY.padClearance,
  schDragIsMove: false,
  schAutoStartWires: true,
  schLineMode: 1,
  schAutoplaceFields: true,
  schAutoplaceJustify: true,
  schAutoplaceAlign: true,
  schSheetBorderColor: null,
  schSheetBackgroundColor: null,
  schRescueNeverShow: false,
  schDefaultFont: "KiCad Font",
  schHighlightThicknessMils: 2,
  schGlobalFieldNames: [],
  simWheel: DEFAULT_SIM_WHEEL_ACTIONS,
  symDefaultLineWidthMils: 0,
  symDefaultTextSizeMils: 50,
  symDefaultPinLengthMils: 100,
  symDefaultPinNumSizeMils: 50,
  symDefaultPinNameSizeMils: 50,
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

/**
 * The version of what `savePreferences` writes (`schemaVersion`), as KiCad's settings files carry theirs (`JSON_SETTINGS::m_schemaVersion`, migrated by
 * `registerMigration`). 2 (2026-11-10, UI-LEFT-2 CC3): the default of "Left button drag:" became KiCad's DRAG_SELECTED. The studio stores its whole preferences
 * on every start, so before 2 a stored `dragLeft: "drag_any"` is the old default as often as a choice; it is read as the new default, the way
 * `COMMON_SETTINGS::migrateSchema1to2` rewrites `input.mouse_left` from what an older file said. "Draw selection rectangle" and "Drag selected objects", which
 * only a person could have chosen, are kept, and so is anything saved from version 2 on.
 */
export const PREFERENCES_SCHEMA_VERSION = 2;

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
const channel = (v: unknown) => typeof v === "number" && Number.isInteger(v) && v >= 0 && v <= 255;
/** A colour as `SchColor` keeps it, or null (`COLOR4D::UNSPECIFIED`) for anything else. */
const schColor = (v: unknown): SchColor | null => {
  const c = v as Partial<SchColor> | null;
  return c && typeof c === "object" && channel(c.r) && channel(c.g) && channel(c.b) && channel(c.a) ? { r: c.r!, g: c.g!, b: c.b!, a: c.a! } : null;
};

/** The magnetic settings the board editor's snapping reads, with `allLayers` (the toggle of `common.Control.magneticSnapToggle`, Shift+S, kept in the studio state). */
export function toPcbMagnetic(p: Pick<Preferences, "magneticPads" | "magneticTracks" | "magneticGraphics">, allLayers: boolean): PcbMagnetic {
  return { pads: p.magneticPads, tracks: p.magneticTracks, graphics: p.magneticGraphics, allLayers };
}

/** Whatever was stored, made into valid preferences: unknown or malformed fields fall back to their defaults, a clashing wheel assignment to the defaults. */
/** "Highlight thickness:" as taken on OK: `m_highlightWidthCtrl` is a spin box of 0 .. 50, `KiROUND( GetValue() )`; anything else keeps `fallback`. */
export function sanitizeHighlightThickness(v: unknown, fallback: number): number {
  return typeof v === "number" && Number.isFinite(v) ? Math.min(50, Math.max(0, Math.round(v))) : fallback;
}

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
    allowFreePads: bool(o.allowFreePads, d.allowFreePads),
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
    // `SetIfPresent`: each row the stored filter has, over the defaults
    fpSelectionFilter: Object.fromEntries(Object.entries(DEFAULT_FP_SELECTION_FILTER).map(([k, v]) => [k, bool((o.fpSelectionFilter as Record<string, unknown> | undefined)?.[k], v)])) as unknown as FpSelectionFilter,
    fpLayerPresets: Array.isArray(o.fpLayerPresets)
      ? (o.fpLayerPresets as LayerPreset[]).filter((p) => p && typeof p.name === "string" && Array.isArray(p.layers) && Array.isArray(p.renderLayers)).map((p) => ({ ...p, readOnly: false, flipBoard: !!p.flipBoard, activeLayer: typeof p.activeLayer === "string" ? p.activeLayer : null }))
      : d.fpLayerPresets,
    pcbArcEditMode: arcEditMode(o.pcbArcEditMode, d.pcbArcEditMode),
    schArcEditMode: arcEditMode(o.schArcEditMode, d.schArcEditMode),
    escClearsNetHighlight: bool(o.escClearsNetHighlight, d.escClearsNetHighlight),
    showCourtyardCollisions: bool(o.showCourtyardCollisions, d.showCourtyardCollisions),
    autoRefillZones: bool(o.autoRefillZones, d.autoRefillZones),
    ratsnestThicknessPx: sanitizeRatsnestThickness(o.ratsnestThicknessPx, d.ratsnestThicknessPx),
    schEscClearsNetHighlight: bool(o.schEscClearsNetHighlight, d.schEscClearsNetHighlight),
    schRepeatOffsetXMils: finiteInt(o.schRepeatOffsetXMils, d.schRepeatOffsetXMils),
    schRepeatOffsetYMils: finiteInt(o.schRepeatOffsetYMils, d.schRepeatOffsetYMils),
    schRepeatLabelIncrement: clampInt(o.schRepeatLabelIncrement, -100000, 100000, d.schRepeatLabelIncrement),
    schFootprintPreview: bool(o.schFootprintPreview, d.schFootprintPreview),
    // `PARAM_ENUM<POWER_SYMBOLS>( .., POWER_SYMBOLS::DEFAULT, POWER_SYMBOLS::DEFAULT, POWER_SYMBOLS::LOCAL )`: out of range is the default
    schNewPowerSymbols: o.schNewPowerSymbols === 1 || o.schNewPowerSymbols === 2 ? o.schNewPowerSymbols : d.schNewPowerSymbols,
    pcbNetNames: sanitizeNetNamesMode(o.pcbNetNames, d.pcbNetNames),
    pcbTrackClearance: sanitizeTrackClearanceMode(o.pcbTrackClearance, d.pcbTrackClearance),
    pcbPadClearance: bool(o.pcbPadClearance, d.pcbPadClearance),
    schDragIsMove: bool(o.schDragIsMove, d.schDragIsMove),
    schAutoStartWires: bool(o.schAutoStartWires, d.schAutoStartWires),
    schLineMode: o.schLineMode === 0 || o.schLineMode === 1 || o.schLineMode === 2 ? o.schLineMode : d.schLineMode,
    schAutoplaceFields: bool(o.schAutoplaceFields, d.schAutoplaceFields),
    schAutoplaceJustify: bool(o.schAutoplaceJustify, d.schAutoplaceJustify),
    schAutoplaceAlign: bool(o.schAutoplaceAlign, d.schAutoplaceAlign),
    schSheetBorderColor: schColor(o.schSheetBorderColor),
    schSheetBackgroundColor: schColor(o.schSheetBackgroundColor),
    schRescueNeverShow: bool(o.schRescueNeverShow, d.schRescueNeverShow),
    schDefaultFont: typeof o.schDefaultFont === "string" && o.schDefaultFont.trim() ? o.schDefaultFont : d.schDefaultFont,
    schHighlightThicknessMils: sanitizeHighlightThickness(o.schHighlightThicknessMils, d.schHighlightThicknessMils),
    schGlobalFieldNames: Array.isArray(o.schGlobalFieldNames)
      ? (o.schGlobalFieldNames as unknown[]).filter((t): t is TemplateFieldName => !!t && typeof (t as TemplateFieldName).name === "string").map((t) => ({ name: t.name, visible: !!t.visible, url: !!t.url }))
      : d.schGlobalFieldNames,
    simWheel: sanitizeSimWheelActions(o.simWheel),
    // `PARAM<int>`: whole mils (`IUToMils( .GetIntValue() )`), none negative
    symDefaultLineWidthMils: clampInt(o.symDefaultLineWidthMils, 0, 1000, d.symDefaultLineWidthMils),
    symDefaultTextSizeMils: clampInt(o.symDefaultTextSizeMils, 1, 10000, d.symDefaultTextSizeMils),
    symDefaultPinLengthMils: clampInt(o.symDefaultPinLengthMils, 0, 10000, d.symDefaultPinLengthMils),
    symDefaultPinNumSizeMils: clampInt(o.symDefaultPinNumSizeMils, 1, 10000, d.symDefaultPinNumSizeMils),
    symDefaultPinNameSizeMils: clampInt(o.symDefaultPinNameSizeMils, 1, 10000, d.symDefaultPinNameSizeMils),
  };
  return scrollModSetValid(p) ? p : { ...p, ...MOUSE_SCROLL_DEFAULTS };
}

/** What was stored by an older studio, brought to `PREFERENCES_SCHEMA_VERSION` (see there): before 2, `dragLeft: "drag_any"` is the old default. */
export function migratePreferences(raw: unknown): unknown {
  if (!raw || typeof raw !== "object") return raw;
  const o = raw as Record<string, unknown>;
  const version = typeof o.schemaVersion === "number" ? o.schemaVersion : 1;
  if (version < 2 && o.dragLeft === "drag_any") return { ...o, dragLeft: DEFAULT_LEFT_DRAG };
  return raw;
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
    return text ? sanitizePreferences(migratePreferences(JSON.parse(text))) : DEFAULT_PREFERENCES;
  } catch {
    return DEFAULT_PREFERENCES;
  }
}

export function savePreferences(storage: PreferencesStorage | null | undefined, prefs: Preferences): void {
  try {
    storage?.setItem(PREFERENCES_STORAGE_KEY, JSON.stringify({ ...prefs, schemaVersion: PREFERENCES_SCHEMA_VERSION }));
  } catch {
    /* storage unavailable: the preferences still apply for this session */
  }
}
