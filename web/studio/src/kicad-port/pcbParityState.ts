// The small slice of editor state the "pcbnew parity" action batch needs
// (docs/parity/UI-ACTIONS.md), kept in one object (`StudioState.pcbx`, one
// `PCBX` reducer action) so the shared store file carries a single, easily
// merged hunk instead of a field per action.

/** The line width a freshly armed graphic tool starts at (Canvas.tsx's pre-existing constant, hoisted here so `incWidth`/`decWidth` can step it). */
export const DEFAULT_STROKE_WIDTH_UM = 150;

/** `ZONE_MODE` (drawing_tool.h) for the two source-zone-driven draws: `CUTOUT` subtracts the drawn polygon from `sourceId`, `SIMILAR` makes a new zone with `sourceId`'s settings and never opens the properties dialog (`ZONE_CREATE_HELPER::createZoneFromExisting`). Plain `ADD` is `null` (the dialog path, unchanged). */
export interface ZoneDrawMode {
  mode: "cutout" | "similar";
  sourceId: string;
}

/** Which pcbnew parity dialog is open (null = none). */
export type PcbDialog = "find_move" | "position_relative" | "place_footprint" | null;

/** `LEADER_MODE`: `DIRECT` = free angle, `DEG45`, `DEG90` (`PCBNEW_SETTINGS::m_AngleSnapMode`). */
export type AngleSnapMode = "direct" | "45" | "90";

export interface PcbParityState {
  /**
   * `EDIT_TOOL::Move`'s `moveIndividually` item list: the refs still waiting
   * for their turn after the one currently glued to the cursor (`orig_items[
   * itemIdx+1 ..]`). Empty = not moving individually. Tab (`skip`) and the
   * drop click both advance it; Escape clears it.
   */
  moveQueue: string[];
  /** True from `moveIndividually` until its last item is dropped/skipped -- what `skip` (Tab) is available for (source: `frame()->IsCurrentTool( PCB_ACTIONS::moveIndividually )`), including while the queue is empty on the last item. */
  movingIndividually: boolean;
  zoneDrawMode: ZoneDrawMode | null;
  pcbDialog: PcbDialog;
  /** `PCBNEW_SETTINGS::m_AngleSnapMode` -- starts at 45 deg, what the segment/rect tools always did before this became switchable. */
  angleSnapMode: AngleSnapMode;
  /** `DRAWING_TOOL::m_stroke`'s width: `incWidth`/`decWidth` step it by `WIDTH_STEP` while a graphic is being drawn. */
  drawStrokeWidthUm: number;
  /** A footprint name waiting to be opened in the Footprint Editor tab (`EditFpInFpEditor`): the Footprint Editor lives in its own store, so the action only records the request and `useFootprintEditHotkey` (mounted where both providers are in scope) performs it. */
  fpEditRequest: string | null;
  /** `MEANDER_SETTINGS` amplitude/spacing the length-tuning dialog edits (`lengthTuner.Ampl*`/`Spacing*` step them). */
  lengthTuner: { amplitudeUm: number; spacingUm: number };
  /** Bumped by `closeOutline` to ask the canvas to finish the zone/polygon being drawn with the points placed so far (the canvas owns the draw). */
  drawFinishRequest: number;
  /** `TOOL_MANAGER::GetMenuCursorPos()`: where the context menu was opened. The pointer is over the menu by the time an entry runs, so a cursor-driven action (Break Track) reads this instead of the live cursor; the next canvas press clears it. */
  menuCursorUm: { x: number; y: number } | null;
}

export const DEFAULT_PCB_PARITY: PcbParityState = {
  moveQueue: [],
  movingIndividually: false,
  zoneDrawMode: null,
  pcbDialog: null,
  angleSnapMode: "45",
  drawStrokeWidthUm: DEFAULT_STROKE_WIDTH_UM,
  fpEditRequest: null,
  lengthTuner: { amplitudeUm: 200, spacingUm: 400 },
  drawFinishRequest: 0,
  menuCursorUm: null,
};

/** `drawing_tool.cpp`: `#define WIDTH_STEP pcbIUScale.mmToIU( 0.1 )`. */
export const WIDTH_STEP_UM = 100;

/** `DRAWING_TOOL::DrawSegment`'s `incWidth`/`decWidth`: +/- one step, and the decrement is refused unless "(unsigned) m_stroke.GetWidth() > WIDTH_STEP" (never reaches 0). */
export function stepStrokeWidth(widthUm: number, dir: 1 | -1): number {
  if (dir === 1) return widthUm + WIDTH_STEP_UM;
  return widthUm > WIDTH_STEP_UM ? widthUm - WIDTH_STEP_UM : widthUm;
}

/** `PCB_VIEWER_TOOLS::NextLineMode`: DIRECT -> DEG45 -> DEG90 -> DIRECT. */
export function nextAngleSnapMode(mode: AngleSnapMode): AngleSnapMode {
  switch (mode) {
    case "direct":
      return "45";
    case "45":
      return "90";
    default:
      return "direct";
  }
}

/** `MEANDER_SETTINGS` constants (pns_meander.cpp): `m_minAmplitude = 200000`, `m_step = 50000` nm. */
export const TUNER_MIN_AMPLITUDE_UM = 200;
export const TUNER_STEP_UM = 50;

/** `MEANDER_PLACER_BASE::AmplitudeStep`: `max(amplitude + sign*step, minAmplitude)`. */
export function amplitudeStep(amplitudeUm: number, sign: 1 | -1): number {
  return Math.max(amplitudeUm + sign * TUNER_STEP_UM, TUNER_MIN_AMPLITUDE_UM);
}

/** `MEANDER_PLACER_BASE::SpacingStep`: `max(spacing + sign*step, trackWidth + clearance)`. */
export function spacingStep(spacingUm: number, sign: 1 | -1, trackWidthUm: number, clearanceUm: number): number {
  return Math.max(spacingUm + sign * TUNER_STEP_UM, trackWidthUm + clearanceUm);
}
