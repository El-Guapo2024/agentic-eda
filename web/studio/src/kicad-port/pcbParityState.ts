// The small slice of editor state the "pcbnew parity" action batch needs
// (docs/parity/UI-ACTIONS.md), kept in one object (`StudioState.pcbx`, one
// `PCBX` reducer action) so the shared store file carries a single, easily
// merged hunk instead of a field per action.
import type { Cmd } from "../api/types";
import type { TuneHoverReply, TuneKind, TuneSettings } from "../api/tuneTypes";
import type { LayerPairSettings } from "./layerPairs";
import { DEFAULT_CUSTOM_TRACK_VIA, type CustomTrackVia } from "./routerVia";
import { DEFAULT_ARC_EDIT_MODE, type ArcEditMode } from "./pcbPointEdit";

/**
 * `DRAWING_TOOL::m_stroke`'s width once `incWidth`/`decWidth` have stepped it on `layer`. A graphic starts at the line thickness of its layer's class (Board Setup >
 * Text & Graphics > Defaults, `kicad-port/layerDefaults.ts`); a step is kept only while the active layer is `layer` and until a drawing tool is armed again.
 */
export interface DrawStrokeStep {
  layer: string;
  um: number;
}

/** `ZONE_MODE` (drawing_tool.h) for the two source-zone-driven draws: `CUTOUT` subtracts the drawn polygon from `sourceId`, `SIMILAR` makes a new zone with `sourceId`'s settings and never opens the properties dialog (`ZONE_CREATE_HELPER::createZoneFromExisting`). Plain `ADD` is `null` (the dialog path, unchanged). */
export interface ZoneDrawMode {
  /** KiCad 10's `ZONE_MODE::STITCH` (Draw Via Stitch Area) is an outline with no source zone (`sourceId` empty). */
  mode: "cutout" | "similar" | "stitch";
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
  /** `PCBNEW_SETTINGS::m_AngleSnapMode` -- starts free (`LEADER_MODE::DIRECT`, `editing.pcb_angle_snap_mode`'s default in pcbnew_settings.cpp; round 5's A5-4). */
  angleSnapMode: AngleSnapMode;
  /** `DRAWING_TOOL::m_stroke`'s width, once `incWidth`/`decWidth` have stepped it by `WIDTH_STEP` while a graphic is being drawn; null = the layer class's default. */
  drawStrokeStep: DrawStrokeStep | null;
  /** A footprint name waiting to be opened in the Footprint Editor tab (`EditFpInFpEditor`): the Footprint Editor lives in its own store, so the action only records the request and `useFootprintEditHotkey` (mounted where both providers are in scope) performs it. */
  fpEditRequest: string | null;
  /**
   * The length tuner's `MEANDER_SETTINGS` for each kind of tuning (`meanderSettings` of `DRAWING_TOOL::PlaceTuningPattern`), over the board's Length-tuning Patterns page: what the
   * Tuning Pattern dialog (Ctrl+L) set, and what a pattern left of them (the amplitude and spacing keys step them). Absent: the page's own.
   */
  tuneSettings: Partial<Record<TuneKind, TuneSettings>>;
  /** The Tuning Pattern dialog is open (`state.lengthTuningDialogOpen`) for this pattern of the board (`ShowPropertiesDialog`); `null` is the tool's own settings. */
  tuneDialogPattern: string | null;
  /** The status box of the track under the cursor with the tuner armed (`updateHoverStatus`), and where the cursor was. */
  tuneHover: { reply: TuneHoverReply; at: [number, number] } | null;
  /** The Generators Manager (`DIALOG_GENERATORS`) is open. */
  generatorsManagerOpen: boolean;
  /** Bumped by `closeOutline` to ask the canvas to finish the zone/polygon being drawn with the points placed so far (the canvas owns the draw). */
  drawFinishRequest: number;
  /** `TOOL_MANAGER::GetMenuCursorPos()`: where the context menu was opened. The pointer is over the menu by the time an entry runs, so a cursor-driven action (Break Track) reads this instead of the live cursor; the next canvas press clears it. */
  menuCursorNm: { x: number; y: number } | null;
  /** `PCB_EDIT_FRAME::m_layerPairSettings` (layer_pairs.cpp): the copper layer pair `V` switches between and its presets. Null = the default (the board's outer copper layers, no presets) until "Set Layer Pair..." changes it. */
  layerPairs: LayerPairSettings | null;
  /** `ROUTER_TOOL::m_startWithVia`: V was pressed with the route tool armed and no route drawn, so the next route starts with a via armed (R6-R5). */
  startWithVia?: boolean;
  /** `BOARD_DESIGN_SETTINGS::m_customDiffPair`: the "Differential Pair Dimensions..." dialog's width, gap and via gap (`SetCustomDiffPairWidth/Gap/ViaGap`; null until the dialog was first accepted). */
  customDiffPair: CustomDiffPair | null;
  /** `BOARD_DESIGN_SETTINGS::UseCustomDiffPairDimensions()`: the pairs routed from now on take `customDiffPair` rather than the net class's. Set by the pair menu's "Use Custom Values..." (`DIFF_PAIR_MENU::eventHandler`, kept on Cancel too), cleared by "Use Net Class Values"; the Route menu's dialog alone does not set it (`DpDimensionsDialog`). */
  useCustomDiffPair: boolean;
  /** `BOARD_DESIGN_SETTINGS::GetDiffPairIndex()`: the chosen entry of Pre-defined Sizes' Differential Pairs (`m_DiffPairDimensionsList`), 0 the net class's -- the pair
   * menu's entries and W while a pair is routed choose it (`SetDiffPairIndex`, which turns the custom dimensions off), r5 R5-5. */
  diffPairIndex: number;
  /** `BOARD_DESIGN_SETTINGS::m_customTrackWidth` / `m_customViaSize`: the Custom Track/Via Size dialog's values (`DIALOG_TRACK_VIA_SIZE`), and whether they are in use (`UseCustomTrackViaSize()`, the "Use Custom Values..." check of the width menu; choosing a width or via size of the list turns it off). */
  customTrackVia: CustomTrackVia;
  useCustomTrackVia: boolean;
  /** "Create Zone / Rule Area from Selection" (convert_tool.cpp `CreatePolys`): what the zone dialog does once the first outline's zone is added -- the other outlines the conversion made get the same settings, then the source items go. */
  zoneConvert: ZoneConvertPending | null;
  /** `PCBNEW_SETTINGS::m_ArcEditMode`: how editing an arc's handles recomputes the arc (kicad-port/pcbPointEdit.ts). */
  arcEditMode: ArcEditMode;
  /** `PCB_BASE_EDIT_FRAME::m_vertexEditorPane` is open ("Edit Corners..."): the floating table of the selected polygon's or zone's vertices. */
  vertexEditorOpen: boolean;
}

export interface ZoneConvertPending {
  extraOutlines: [number, number][][];
  /** Delete commands for the converted sources (`Delete source objects after conversion`); empty when they stay. */
  deleteCmds: Cmd[];
}

export interface CustomDiffPair {
  widthNm: number;
  gapNm: number;
  viaGapNm: number;
  viaGapSameAsTrackGap: boolean;
}

export const DEFAULT_PCB_PARITY: PcbParityState = {
  moveQueue: [],
  movingIndividually: false,
  zoneDrawMode: null,
  pcbDialog: null,
  angleSnapMode: "direct",
  drawStrokeStep: null,
  fpEditRequest: null,
  tuneSettings: {},
  tuneDialogPattern: null,
  tuneHover: null,
  generatorsManagerOpen: false,
  drawFinishRequest: 0,
  menuCursorNm: null,
  layerPairs: null,
  customDiffPair: null,
  useCustomDiffPair: false,
  diffPairIndex: 0,
  customTrackVia: DEFAULT_CUSTOM_TRACK_VIA,
  useCustomTrackVia: false,
  zoneConvert: null,
  arcEditMode: DEFAULT_ARC_EDIT_MODE,
  vertexEditorOpen: false,
};

/** `drawing_tool.cpp`: `#define WIDTH_STEP pcbIUScale.mmToIU( 0.1 )`. */
export const WIDTH_STEP_NM = 100_000;

/** `DRAWING_TOOL::DrawSegment`'s `incWidth`/`decWidth`: +/- one step, and the decrement is refused unless "(unsigned) m_stroke.GetWidth() > WIDTH_STEP" (never reaches 0). */
export function stepStrokeWidth(widthNm: number, dir: 1 | -1): number {
  if (dir === 1) return widthNm + WIDTH_STEP_NM;
  return widthNm > WIDTH_STEP_NM ? widthNm - WIDTH_STEP_NM : widthNm;
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
