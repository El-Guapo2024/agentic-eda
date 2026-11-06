// The Footprint Editor tab's own state (GAPS.md #8). A footprint
// definition is a genuinely separate document from the board (its own
// undo domain -- "footprint_editor", see api/client.ts's postUndo/
// postRedo -- its own selection, its own pan/zoom), so this is a small,
// self-contained useReducer store of exactly the same shape state/
// store.tsx uses for the board, rather than growing that already-large
// file with a second, unrelated document to track. Kept deliberately
// simple relative to store.tsx: no net highlight, no layer visibility
// panel, no clipboard beyond the one pad-properties copy/paste slot
// pad_tool.cpp's own Copy/Paste Pad Properties needs.
import React, { createContext, useCallback, useContext, useEffect, useReducer, useRef } from "react";
import type { Cmd, CmdShape, CmdText, FootprintPropertiesFields, LibraryFootprint, LibraryPad, PointXY, Nm } from "../api/types";
import { downloadFootprintKicadMod, fetchFootprint, fetchFootprintLibraryNames, postCarry, postCmd, postRedo, postUndo, type CarryStep } from "../api/client";
import { readClipboardText, writeClipboardText } from "../api/libraryClient";
import { fpClipboardKind, fpPlainTextPaste } from "../kicad-port/fpClipboard";
import { LONG_TEXT_QUESTION } from "../kicad-port/plainTextPaste";
import { ensureFootprintInProject } from "../api/libraryClient";
import { duplicatePads, uniqueFootprintName } from "../kicad-port/fpEditActions";
import { isTableLibrary, newFootprintLibrary, newFootprintReadOnlyWarning, uniqueNewFootprintName } from "../kicad-port/librarySave";
import { fetchLibraryItemsNow, fetchWritableLibraries, saveToLibrary } from "../api/librarySaveClient";
import { getLibraryTree } from "./libraryTree";
import { FIRST_LAST_PAD_NUMBER } from "../kicad-port/padNumbering";
import { DEFAULT_PAD_MASTER, importPadSettings, settingsOf, type PadSettings } from "../kicad-port/padSettings";
import { DEFAULT_ENUMERATE_PARAMS, enumerateCommit, type EnumerateParams, type EnumerateState } from "../kicad-port/padEnumeration";
import { editPadCommand, enterContrast, exitContrast, padEditLayer } from "../kicad-port/padEditMode";
import { boardCopyKey } from "../api/fpEditorTypes";
import type { ViewTransform } from "./store";
import type { ArcGeom } from "../kicad-port/arcGeom";
import type { BezierGeom } from "../kicad-port/bezierGeom";
import type { AngleSnapMode } from "../kicad-port/pcbParityState";
import type { FlipDirectionWire, FpItemWire } from "../api/fpItemTypes";
import { fpAllIds, fpGroupCmd, fpLeaves, fpMirrorCmd, fpMoveCmd, fpRotateCmd, fpUngroupCmd, type Snap } from "../kicad-port/fpItems";
import type { FpDimDraw, FpDimKind, FpDrawnTable } from "../kicad-port/fpDrawTools";
import type { BoardTextBox, CmdBarcode } from "../api/pcbItemTypes";
import type { CmdZone } from "../api/types";
import { snapGridNm } from "../components/canvas/gridHelper";
import { getFpDefaultTextItems, getFpGraphicsDefaults } from "./fpPrefs";
import { isFactoryNewFootprint, newFootprintTexts } from "../kicad-port/fpFieldDefaults";

export type FpToolId =
  | "select" | "move" | "pad" | "draw_segment" | "draw_arc" | "draw_bezier" | "draw_rect" | "draw_circle" | "draw_polygon" | "text" | "anchor" | "enumerate"
  // `DRAWING_TOOL`'s tools for the items of `eda_model::fp_items` (kicad-port/fpDrawTools.ts): Draw Dimension of each kind, Draw Rule Area, Draw Text Box, Place Point,
  // Draw Table, Add Barcode, Place Reference Image
  | "dim_aligned" | "dim_orthogonal" | "dim_center" | "dim_radial" | "dim_leader" | "rule_area" | "textbox" | "point" | "table" | "barcode" | "ref_image";

/** The dimension kind each Draw Dimension tool draws. */
export const FP_DIM_TOOL_KIND: Partial<Record<FpToolId, FpDimKind>> = { dim_aligned: "aligned", dim_orthogonal: "orthogonal", dim_center: "center", dim_radial: "radial", dim_leader: "leader" };

/** The tools of the fp_items kinds: Escape drops what one holds and keeps it armed (each one's `cleanup()`); a second Escape leaves it. */
export const FP_ITEM_TOOLS: ReadonlySet<FpToolId> = new Set<FpToolId>(["dim_aligned", "dim_orthogonal", "dim_center", "dim_radial", "dim_leader", "rule_area", "textbox", "point", "table", "barcode", "ref_image"]);

export const FP_TOOL_MESSAGES: Record<FpToolId, string> = {
  select: "Select item(s)",
  move: "Move item(s)",
  pad: "Pad: click to place, number auto-increments",
  draw_segment: "Line: click start, then end",
  draw_arc: "Arc: click the centre, then the start point, then the end point (/ switches the direction)",
  draw_bezier: "Bezier: click the start, control 1, the end, then control 2 (the curve then continues from its end); double-click to finish, Esc to cancel",
  draw_rect: "Rectangle: click one corner, then the opposite one",
  draw_circle: "Circle: click center, then a point on the edge",
  draw_polygon: "Polygon: click points, Enter/double-click to finish, Esc to cancel",
  text: "Click to place text",
  anchor: "Place the footprint anchor: click the new origin of the footprint",
  enumerate: "Renumber pads: click each pad in the order it should be numbered; click a numbered pad again to take its number back; double-click to finish, Esc to cancel",
  dim_aligned: "Aligned dimension: click the start, the end, then the height",
  dim_orthogonal: "Orthogonal dimension: click the start, the end, then the height (the cursor's side sets horizontal or vertical)",
  dim_center: "Center dimension: click the centre, then the end of an arm",
  dim_radial: "Radial dimension: click the centre, then a point on the circle",
  dim_leader: "Leader: click the arrow's point, then the end of the line; the text is asked for next",
  rule_area: "Rule area: click points, Enter/double-click to finish, Esc to cancel",
  textbox: "Text box: click one corner, then the opposite one",
  point: "Point: click to put a point down (Esc ends the tool)",
  table: "Table: click the first corner, then the opposite one to size the grid",
  barcode: "Barcode: click to place it (Esc ends the tool)",
  ref_image: "Reference image: click to choose an image file, then click to put it down",
};

/**
 * What a tool of the fp_items kinds holds between its clicks (a polygon's or a box's corners are `drawState`'s): a dimension at SET_END or SET_HEIGHT (`DrawDimension`), a
 * table's first corner (`DrawTable`), the image the cursor holds by its centre (`PlaceReferenceImage`).
 */
export type FpItemDraw =
  | { tool: "dimension"; draw: FpDimDraw }
  | { tool: "table"; origin: [Nm, Nm] }
  | { tool: "ref_image"; name: string; data: string; img: HTMLImageElement; w: Nm; h: Nm };

/** An item a tool made whose Properties dialog is open on it before it is added: OK adds it (one `fp_item add`), Cancel drops it. */
export type FpPendingItem =
  | { kind: "textbox"; box: BoardTextBox }
  | { kind: "barcode"; barcode: CmdBarcode }
  | { kind: "table"; drawn: FpDrawnTable }
  | { kind: "zone"; zone: CmdZone };

/** Same "click to add a point(s), commit on the last one" shape the PCB tab's own `DrawState` uses, narrowed to what the footprint editor's graphics tools need (no route/zone/measure concept here). `arc`/`bezier`: the construction managers' state, see the PCB tab's `DrawState`. */
export type FpDrawState = { kind: "shape"; shapeKind: "segment" | "arc" | "rect" | "circle" | "polygon" | "bezier"; pts: [Nm, Nm][]; arc?: ArcGeom; bezier?: BezierGeom };

export interface FpMovePreview {
  refs: string[];
  /** The kind of the item a drag started on. Optional because a Duplicate pick-up carries a mixed selection (pads + graphics + text); the commit looks each ref up either way. */
  kind?: string;
  dxNm: number;
  dyNm: number;
  rotateQuarterTurns?: number;
}

export interface FootprintEditorState {
  /** The footprint currently open, or `null` before anything has been opened this session. */
  name: string | null;
  /** The last successful GET /api/footprint?name=... for `name`. */
  footprint: LibraryFootprint | null;
  error: string | null;
  view: ViewTransform;
  viewInitialized: boolean;
  /** Bumped by Zoom to Fit (`REQUEST_FIT`): the canvas fits the view to the footprint again, whether or not the view was moved by hand since. */
  fitRequest: number;
  selection: Set<string>;
  activeTool: FpToolId;
  drawState: FpDrawState | null;
  movePreview: FpMovePreview | null;
  moveOriginNm: { x: number; y: number } | null;
  cursorNm: { x: number; y: number } | null;
  gridNm: number;
  gridVisible: boolean;
  /** Which of F.SilkS/F.Fab/F.CrtYd a graphics/text tool draws on. */
  activeLayer: string;
  /** `common.Control.highContrastMode` (`PCB_CONTROL::HighContrastMode`): what is not on the active layer is drawn dimmed. */
  highContrast: boolean;
  /** "E" on a selected pad, or double-click -- which pad's Pad Properties dialog is open. */
  padPropertiesId: string | null;
  footprintPropertiesOpen: boolean;
  /** `pcbnew.PadTool.enumeratePads`: the "Renumber Pads" parameter dialog (`DIALOG_ENUM_PADS`) is open. OK arms the click-to-number tool. */
  renumberDialogOpen: boolean;
  /**
   * `BOARD_DESIGN_SETTINGS::m_Pad_Master`: the default pad. Place Pad creates pads from it and pushes the pad it placed back into it,
   * Copy Pad Properties to Default stores the selected pad's settings here, Paste Default Pad Properties to Selected applies them, and
   * Default Pad Properties edits it directly. It belongs to the board settings, not the footprint, so it survives opening another footprint.
   */
  defaultPad: PadSettings;
  /** `DIALOG_ENUM_PADS`'s `s_lastUsedParams`: the parameters the last Renumber Pads run used. */
  enumerateParams: EnumerateParams;
  /** The running click-to-number tool (`pcbnew.PadTool.enumeratePads`), or `null`. Nothing is sent to the backend until it finishes. */
  enumerate: EnumerateState | null;
  /** The footprint the library tree has selected (`GetLibTree()->GetSelectedLibId()`), which the library actions work on; `null` = none (the open footprint is the target). */
  treeSelection: string | null;
  /** `FOOTPRINT_EDITOR_CONTROL::m_copiedFootprint`: the footprint Copy / Cut put aside for Paste. */
  copiedFootprint: LibraryFootprint | null;
  /** Dialogs of the pad / library actions. */
  padTableOpen: boolean;
  defaultPadOpen: boolean;
  pushPadOpen: boolean;
  loadFromBoardOpen: boolean;
  /** `pcbnew.Array.createArray`: the Create Array dialog (`DIALOG_CREATE_ARRAY` with the pad numbering panels, `enableArrayNumbering`). */
  createArrayOpen: boolean;
  /** The board part this footprint was loaded from (`LoadFootprintFromBoard`), so "Insert footprint into PCB" knows what to update. */
  linkedPart: string | null;
  /** `PAD_TOOL::m_lastPadNumber`: the number the last placed (or duplicated-with-increment) pad took, which the pad tool and `Duplicate and Increment` continue from. `null`
   * until one is set -- "1", as `PAD_TOOL::Reset( MODEL_RELOAD )` sets it for each footprint loaded (padNumbering.ts). */
  lastPadNumber: string | null;
  /** `EDIT_TOOL::Duplicate` hands the copies straight to `doMoveSelection` and, if that is cancelled, `commit.Revert()`s them: true while the freshly duplicated items are glued to the cursor, so Escape undoes the duplicate and a drop keeps it. */
  duplicatePending: boolean;
  /**
   * `PAD_TOOL::m_previousHighContrastMode`: the high contrast flag as it was when the pad edit mode began, to put back when it ends; `null` while no pad is edited
   * (kicad-port/padEditMode.ts `enterContrast` / `exitContrast`).
   */
  padEditContrast: boolean | null;
  /**
   * The Text tool (`DRAWING_TOOL::PlaceText`, kicad-port/fpTextTool.ts): the text its first click made, while `DIALOG_TEXT_PROPERTIES` is open on it (`textDialog`), then
   * while it follows the cursor to the click that commits it (`carriedText`). The tool stays armed after each text.
   */
  textDialog: CmdText | null;
  carriedText: CmdText | null;
  /**
   * The Pad tool's carried pad (`doInteractiveItemPlacement` with IPO_SINGLE_CLICK, kicad-port/fpPadPlacer.ts) once R or F has turned or flipped it; `null` is the pad
   * made from the default pad as it is. A click places it and the default pad takes its settings.
   */
  carriedPad: LibraryPad | null;
  /**
   * `DIALOG_TEXT_PROPERTIES` on an item of the footprint (`FOOTPRINT_EDIT_FRAME::OnEditItemRequest`: `case PCB_FIELD_T: case PCB_TEXT_T: ShowTextPropertiesDialog`): the field or the
   * text it is open on, by id (`field:Reference`, a text's id); `null` when closed.
   */
  textEditId: string | null;
  /** `DIALOG_SHAPE_PROPERTIES` (`ShowGraphicItemPropertiesDialog`) on a graphic, by id. */
  shapeDialogId: string | null;
  /** The Properties dialog of an item of `eda_model::fp_items` (a dimension, a zone, a text box, a table, a barcode, an image, a group), by id and kind. */
  itemDialog: { id: string; kind: string } | null;
  /** `FOOTPRINT_EDITOR_SETTINGS::m_AngleSnapMode` (`pcbnew.EditorControl.lineModeFree` / `lineModeOrthonal` / `lineMode45`): the footprint editor's own line mode, 45 degrees by default. */
  lineMode: AngleSnapMode;
  /** The fp_items tools' state between clicks (`FpItemDraw`). */
  itemDraw: FpItemDraw | null;
  /** The item a tool made whose dialog is open on it (`FpPendingItem`). */
  pendingItem: FpPendingItem | null;
  /** `PCB_VIEWERS_SETTINGS_BASE::m_ViewersDisplay`: `m_DisplayPadFill` off (Sketch Pads, `pcbnew.Control.padDisplayMode`), `m_DisplayGraphicsFill` off (`graphicOutlines`), `m_DisplayTextFill` off (`textOutlines`). */
  sketchPads: boolean;
  sketchGraphics: boolean;
  sketchText: boolean;
  /**
   * The Appearance manager's switches (`APPEARANCE_CONTROLS` of the footprint editor, `pcbnew.Control.showLayersManager`): a layer, or an object row under `obj:<id>`, that is off,
   * as the board's `layerVisible` keys them. Absent = shown.
   */
  layerVisible: Record<string, boolean>;
  /** The Appearance manager's opacity sliders, by the same keys (0..1); absent = opaque. */
  layerOpacity: Record<string, number>;
  /** The Appearance manager is shown (the frame's right column). */
  appearanceShown: boolean;
  /** `HIGH_CONTRAST_MODE::HIDDEN` (the Appearance manager's Inactive layers "Hide"): with `highContrast`, what is not on the active layer is not drawn at all. */
  contrastHidden: boolean;
  toast: { message: string; kind: "error" | "info" } | null;
}

const initialState: FootprintEditorState = {
  name: null,
  footprint: null,
  error: null,
  view: { scale: 0, x: 0, y: 0 },
  viewInitialized: false,
  fitRequest: 0,
  selection: new Set(),
  activeTool: "select",
  drawState: null,
  movePreview: null,
  moveOriginNm: null,
  cursorNm: null,
  gridNm: 250_000, // 0.25mm -- a reasonable footprint-editor-scale default (KiCad's own fp-editor grid list starts finer than the board editor's)
  gridVisible: true,
  activeLayer: "F.SilkS",
  highContrast: false,
  padPropertiesId: null,
  footprintPropertiesOpen: false,
  renumberDialogOpen: false,
  defaultPad: DEFAULT_PAD_MASTER,
  enumerateParams: DEFAULT_ENUMERATE_PARAMS,
  enumerate: null,
  treeSelection: null,
  copiedFootprint: null,
  padTableOpen: false,
  defaultPadOpen: false,
  pushPadOpen: false,
  loadFromBoardOpen: false,
  createArrayOpen: false,
  linkedPart: null,
  lastPadNumber: null,
  duplicatePending: false,
  padEditContrast: null,
  textDialog: null,
  carriedText: null,
  carriedPad: null,
  textEditId: null,
  shapeDialogId: null,
  itemDialog: null,
  lineMode: "45",
  itemDraw: null,
  pendingItem: null,
  sketchPads: false,
  sketchGraphics: false,
  sketchText: false,
  layerVisible: {},
  layerOpacity: {},
  appearanceShown: true,
  contrastHidden: false,
  toast: null,
};

export type FpAction =
  | { type: "SET_NAME"; name: string | null }
  | { type: "FOOTPRINT_OK"; footprint: LibraryFootprint }
  | { type: "FOOTPRINT_ERR"; message: string }
  | { type: "SET_VIEW"; view: ViewTransform }
  | { type: "MARK_VIEW_INITIALIZED" }
  | { type: "REQUEST_FIT" }
  | { type: "SET_SELECTION"; refs: string[] }
  | { type: "CLEAR_SELECTION" }
  | { type: "ESCAPE" }
  | { type: "SET_ACTIVE_TOOL"; tool: FpToolId }
  | { type: "SET_DRAW_STATE"; draw: FpDrawState | null }
  | { type: "SET_MOVE_PREVIEW"; preview: FpMovePreview | null }
  | { type: "SET_MOVE_ORIGIN"; at: { x: number; y: number } | null }
  | { type: "SET_CURSOR"; at: { x: number; y: number } | null }
  | { type: "SET_GRID_NM"; um: number }
  | { type: "TOGGLE_GRID_VISIBLE" }
  | { type: "TOGGLE_HIGH_CONTRAST" }
  | { type: "SET_ACTIVE_LAYER"; layer: string }
  | { type: "SET_PAD_PROPERTIES_ID"; id: string | null }
  | { type: "SET_FOOTPRINT_PROPERTIES_OPEN"; open: boolean }
  | { type: "SET_RENUMBER_DIALOG_OPEN"; open: boolean }
  | { type: "SET_DEFAULT_PAD"; pad: PadSettings }
  | { type: "SET_ENUMERATE_PARAMS"; params: EnumerateParams }
  | { type: "SET_ENUMERATE"; state: EnumerateState | null }
  | { type: "SET_TREE_SELECTION"; name: string | null }
  | { type: "SET_COPIED_FOOTPRINT"; footprint: LibraryFootprint | null }
  | { type: "SET_PAD_TABLE_OPEN"; open: boolean }
  | { type: "SET_DEFAULT_PAD_OPEN"; open: boolean }
  | { type: "SET_PUSH_PAD_OPEN"; open: boolean }
  | { type: "SET_LOAD_FROM_BOARD_OPEN"; open: boolean }
  | { type: "SET_CREATE_ARRAY_OPEN"; open: boolean }
  | { type: "SET_LINKED_PART"; ref: string | null }
  | { type: "SET_LAST_PAD_NUMBER"; number: string | null }
  | { type: "SET_DUPLICATE_PENDING"; pending: boolean }
  /** The pad edit mode began (`PAD_TOOL::enterPadEditMode`): the active layer is the pad's and the contrast is dimmed. */
  | { type: "PAD_EDIT_ENTER"; layer: string }
  /** The pad edit mode ended (`ExitPadEditMode`). */
  | { type: "PAD_EDIT_EXIT" }
  | { type: "SET_TEXT_DIALOG"; text: CmdText | null }
  | { type: "SET_CARRIED_TEXT"; text: CmdText | null }
  | { type: "SET_CARRIED_PAD"; pad: LibraryPad | null }
  | { type: "SET_TEXT_EDIT_ID"; id: string | null }
  | { type: "SET_SHAPE_DIALOG_ID"; id: string | null }
  | { type: "SET_ITEM_DIALOG"; dialog: { id: string; kind: string } | null }
  | { type: "SET_ITEM_DRAW"; draw: FpItemDraw | null }
  | { type: "SET_PENDING_ITEM"; item: FpPendingItem | null }
  | { type: "SET_LINE_MODE"; mode: AngleSnapMode }
  | { type: "SET_DISPLAY"; patch: Partial<Pick<FootprintEditorState, "sketchPads" | "sketchGraphics" | "sketchText">> }
  | { type: "SET_LAYER_VISIBLE"; key: string; visible: boolean }
  | { type: "SET_LAYERS_VISIBLE"; visible: Record<string, boolean> }
  | { type: "SET_LAYER_OPACITY"; key: string; opacity: number }
  | { type: "SET_APPEARANCE_SHOWN"; shown: boolean }
  /** The Appearance manager's Inactive layers (`m_ContrastModeDisplay`): Normal, Dim or Hide. */
  | { type: "SET_CONTRAST_MODE"; mode: "normal" | "dimmed" | "hidden" }
  | { type: "TOAST"; message: string; kind: "error" | "info" }
  | { type: "TOAST_CLEAR" };

/** The items `after` has that `before` did not, at the top: a new group stands for its members (`placeBoardItems`: "We only need to add the items that aren't inside a
 *  group currently selected"). */
function topLevelNew(after: LibraryFootprint, before: Set<string>): string[] {
  const fresh = [...fpAllIds(after)].filter((id) => !before.has(id));
  const inNewGroup = new Set((after.items?.groups ?? []).filter((g) => fresh.includes(g.id)).flatMap((g) => g.member_ids));
  return fresh.filter((id) => !inNewGroup.has(id));
}

function reducer(state: FootprintEditorState, action: FpAction): FootprintEditorState {
  switch (action.type) {
    case "SET_NAME":
      if (action.name === state.name) return state;
      // What belongs to the editor session rather than to one footprint (the board's default pad, the Renumber Pads parameters, the library tree's
      // selection, a copied footprint) survives opening another footprint.
      return {
        ...initialState,
        name: action.name,
        gridNm: state.gridNm,
        gridVisible: state.gridVisible,
        highContrast: state.padEditContrast !== null ? exitContrast(state.padEditContrast, state.highContrast) : state.highContrast,
        defaultPad: state.defaultPad,
        enumerateParams: state.enumerateParams,
        treeSelection: state.treeSelection,
        copiedFootprint: state.copiedFootprint,
        lineMode: state.lineMode,
        sketchPads: state.sketchPads,
        sketchGraphics: state.sketchGraphics,
        sketchText: state.sketchText,
        layerVisible: state.layerVisible,
        layerOpacity: state.layerOpacity,
        appearanceShown: state.appearanceShown,
        contrastHidden: state.contrastHidden,
      };
    case "FOOTPRINT_OK":
      return { ...state, footprint: action.footprint, error: null };
    case "FOOTPRINT_ERR":
      return { ...state, error: action.message };
    case "SET_VIEW":
      return { ...state, view: action.view };
    case "MARK_VIEW_INITIALIZED":
      return { ...state, viewInitialized: true };
    case "REQUEST_FIT":
      return { ...state, fitRequest: state.fitRequest + 1 };
    case "SET_SELECTION":
      return { ...state, selection: new Set(action.refs) };
    case "CLEAR_SELECTION":
      return { ...state, selection: new Set(), activeTool: "select", drawState: null, movePreview: null, padPropertiesId: null, enumerate: null, textDialog: null, carriedText: null, carriedPad: null, itemDraw: null };
    case "ESCAPE": {
      // `PlaceText`: `IsCancelInteractive` with a text carried drops the text (`cleanup()`) and keeps the tool; without one it pops the tool.
      if (state.carriedText || state.textDialog) return { ...state, carriedText: null, textDialog: null };
      // The fp_items tools (`DrawDimension`, `DrawTable`, `PlaceReferenceImage`, `drawShape`, `DrawZone`): `IsCancelInteractive` with something in hand is `cleanup()`, the tool stays.
      if (FP_ITEM_TOOLS.has(state.activeTool) && (state.itemDraw || state.drawState)) return { ...state, itemDraw: null, drawState: null };
      const inProgress = state.activeTool !== "select" || state.drawState != null || state.movePreview != null;
      // `evt->IsCancelInteractive()` in EnumeratePads: `commit.Revert()` -- nothing was sent, so dropping the tool's state is the revert.
      if (inProgress) return { ...state, activeTool: "select", drawState: null, movePreview: null, enumerate: null, carriedPad: null };
      return { ...state, selection: new Set() };
    }
    case "SET_ACTIVE_TOOL":
      // Another tool (`evt->IsActivate()`): a carried text or pad is dropped (`cleanup()`).
      return { ...state, activeTool: action.tool, drawState: null, textDialog: null, carriedText: null, carriedPad: null, itemDraw: null };
    case "SET_TEXT_DIALOG":
      return { ...state, textDialog: action.text };
    case "SET_CARRIED_TEXT":
      return { ...state, carriedText: action.text };
    case "SET_CARRIED_PAD":
      return { ...state, carriedPad: action.pad };
    case "SET_DRAW_STATE":
      return { ...state, drawState: action.draw };
    case "SET_MOVE_PREVIEW":
      return { ...state, movePreview: action.preview };
    case "SET_MOVE_ORIGIN":
      return { ...state, moveOriginNm: action.at };
    case "SET_CURSOR":
      return { ...state, cursorNm: action.at };
    case "SET_GRID_NM":
      return { ...state, gridNm: action.um };
    case "TOGGLE_HIGH_CONTRAST":
      // `PCB_CONTROL::HighContrastMode`: NORMAL becomes DIMMED, anything else NORMAL
      return { ...state, highContrast: !state.highContrast, contrastHidden: false };
    case "TOGGLE_GRID_VISIBLE":
      return { ...state, gridVisible: !state.gridVisible };
    case "SET_ACTIVE_LAYER":
      return { ...state, activeLayer: action.layer };
    case "SET_PAD_PROPERTIES_ID":
      return { ...state, padPropertiesId: action.id };
    case "SET_FOOTPRINT_PROPERTIES_OPEN":
      return { ...state, footprintPropertiesOpen: action.open };
    case "SET_RENUMBER_DIALOG_OPEN":
      return { ...state, renumberDialogOpen: action.open };
    case "SET_DEFAULT_PAD":
      return { ...state, defaultPad: action.pad };
    case "SET_ENUMERATE_PARAMS":
      return { ...state, enumerateParams: action.params };
    case "SET_ENUMERATE":
      return { ...state, enumerate: action.state };
    case "SET_TREE_SELECTION":
      return { ...state, treeSelection: action.name };
    case "SET_COPIED_FOOTPRINT":
      return { ...state, copiedFootprint: action.footprint };
    case "SET_PAD_TABLE_OPEN":
      return { ...state, padTableOpen: action.open };
    case "SET_DEFAULT_PAD_OPEN":
      return { ...state, defaultPadOpen: action.open };
    case "SET_PUSH_PAD_OPEN":
      return { ...state, pushPadOpen: action.open };
    case "SET_LOAD_FROM_BOARD_OPEN":
      return { ...state, loadFromBoardOpen: action.open };
    case "SET_CREATE_ARRAY_OPEN":
      return { ...state, createArrayOpen: action.open };
    case "SET_LINKED_PART":
      return { ...state, linkedPart: action.ref };
    case "SET_LAST_PAD_NUMBER":
      return { ...state, lastPadNumber: action.number };
    case "SET_DUPLICATE_PENDING":
      return { ...state, duplicatePending: action.pending };
    case "PAD_EDIT_ENTER": {
      if (state.padEditContrast !== null) return state.activeLayer === action.layer ? state : { ...state, activeLayer: action.layer };
      const c = enterContrast(state.highContrast);
      return { ...state, activeLayer: action.layer, highContrast: c.highContrast, padEditContrast: c.remembered };
    }
    case "PAD_EDIT_EXIT":
      if (state.padEditContrast === null) return state;
      return { ...state, highContrast: exitContrast(state.padEditContrast, state.highContrast), padEditContrast: null };
    case "SET_TEXT_EDIT_ID":
      return { ...state, textEditId: action.id };
    case "SET_SHAPE_DIALOG_ID":
      return { ...state, shapeDialogId: action.id };
    case "SET_ITEM_DIALOG":
      return { ...state, itemDialog: action.dialog };
    case "SET_ITEM_DRAW":
      return { ...state, itemDraw: action.draw };
    case "SET_PENDING_ITEM":
      return { ...state, pendingItem: action.item };
    case "SET_LINE_MODE":
      return { ...state, lineMode: action.mode };
    case "SET_DISPLAY":
      return { ...state, ...action.patch };
    case "SET_LAYER_VISIBLE":
      return { ...state, layerVisible: { ...state.layerVisible, [action.key]: action.visible } };
    case "SET_LAYERS_VISIBLE":
      return { ...state, layerVisible: { ...state.layerVisible, ...action.visible } };
    case "SET_LAYER_OPACITY":
      return { ...state, layerOpacity: { ...state.layerOpacity, [action.key]: action.opacity } };
    case "SET_APPEARANCE_SHOWN":
      return { ...state, appearanceShown: action.shown };
    case "SET_CONTRAST_MODE":
      return { ...state, highContrast: action.mode !== "normal", contrastHidden: action.mode === "hidden" };
    case "TOAST":
      return { ...state, toast: { message: action.message, kind: action.kind } };
    case "TOAST_CLEAR":
      return { ...state, toast: null };
    default:
      return state;
  }
}

export interface FootprintEditorApi {
  /** The current state, read at call time (an action handler registered on another tab's render must not close over a stale one). */
  getState: () => FootprintEditorState;
  /** Open (or re-open -- a no-op server-side if already open) a footprint by name and start polling it. */
  openFootprint: (name: string) => Promise<void>;
  /**
   * `FOOTPRINT_EDIT_FRAME::LoadFootprintFromBoard` (Edit Footprint, Ctrl+E on the board; Load footprint from current PCB): a copy of the board footprint `reference`, linked to it
   * (`fp_editor open_from_board`, UI-LEFT F3), opened under its key. Save then updates that one footprint on the board.
   */
  openFromBoard: (reference: string) => Promise<void>;
  /** `pcbnew.ModuleEditor.newFootprint` (Ctrl+N): a new, empty SMD footprint named `Untitled` (made unique), opened in the editor. */
  newFootprint: () => Promise<void>;
  /**
   * `EDIT_TOOL::Duplicate` (Ctrl+D) / `pcbnew.InteractiveEdit.duplicateIncrementPads` (Ctrl+Shift+D, `increment`):
   * copy the selected pads/graphics/text in place as ONE undo step, select the copies and pick them up with Move
   * (Escape then reverts the duplicate). With `increment` each pad takes the next free pad number.
   */
  duplicateSelection: (increment: boolean) => Promise<void>;
  /**
   * Copy (Ctrl+C) / Cut (Ctrl+X) in the Footprint Editor (`EDIT_TOOL::copyToClipboard` / `cutToClipboard`, UI-LEFT-2 FF1): the selection as KiCad's clipboard footprint
   * (`CLIPBOARD_IO::SaveSelection`, `.kicad_mod` text measured from the cursor's grid point) on the system clipboard, where KiCad's Footprint Editor can paste it; Cut then
   * deletes it (one undo step).
   */
  copySelection: (cut: boolean) => Promise<boolean>;
  /**
   * Paste (Ctrl+V) in the Footprint Editor (`PCB_CONTROL::Paste` -> `pasteFootprintItemsToFootprintEditor`): KiCad's clipboard text (a footprint's, or a board's) as new
   * items with the clipboard's origin on the cursor, else the text as a text item on the active layer; the items are selected and ride the cursor until a click, the
   * paste and the drop one undo step (Escape leaves nothing).
   */
  pasteItems: () => Promise<void>;
  /** Escape while the Move tool carries what Duplicate, Paste or Import Graphics made: `commit.Revert()` -- the held step is cancelled, or the step undone. */
  cancelPickUp: () => Promise<void>;
  /**
   * The items `ids` just made, selected and carried by the Move tool until a click drops them (Escape undoes what made them): `doMoveSelection` after a Duplicate, and
   * the interactive placement of Import Graphics (`DRAWING_TOOL::PlaceImportedGraphics`, UI-LEFT F8).
   */
  pickUp: (ids: string[], message?: string) => void;
  /** Commit a finished move (a drag-drop or a Duplicate pick-up) of `refs` by `(dxNm, dyNm)` as ONE undo step -- each ref is looked up, so a mixed selection works. */
  commitMove: (refs: string[], dxNm: number, dyNm: number) => Promise<void>;
  /** Back to "nothing open" -- leaving the tab, or picking a different footprint before this one loaded. */
  closeFootprint: () => void;
  refresh: () => Promise<void>;
  /** Any Cmd this file doesn't have a named wrapper for. Returns whether the backend accepted it. */
  cmd: (c: Cmd) => Promise<boolean>;
  undo: () => Promise<void>;
  redo: () => Promise<void>;
  padById: (id: string) => LibraryPad | undefined;
  graphicById: (id: string) => CmdShape | undefined;
  textById: (id: string) => CmdText | undefined;
  addPad: (pad: LibraryPad) => Promise<void>;
  movePad: (id: string, x: Nm, y: Nm) => Promise<void>;
  rotatePad: (id: string, quarterTurns: number) => Promise<void>;
  deletePad: (id: string) => Promise<void>;
  editPad: (id: string, pad: LibraryPad) => Promise<boolean>;
  renumberPads: (start: number, prefix: string, step: number) => Promise<void>;
  pushPadProperties: (sourcePadId: string, filters: { shape: boolean; orientation: boolean; layers: boolean; type: boolean }) => Promise<void>;
  /** `PAD_TOOL::copyPadSettings` (`pcbnew.PadTool.CopyPadSettings`): `m_Pad_Master->ImportSettingsFrom( selPad )` -- the pad's settings become the default pad. */
  copyPadProperties: (id: string) => void;
  /** `PAD_TOOL::pastePadProperties` (`pcbnew.PadTool.ApplyPadSettings`): every pad of `ids` takes the default pad's settings (`ImportSettingsFrom( *masterPad )`), one undo step. */
  pastePadProperties: (ids: string[]) => Promise<void>;
  /** `PAD_TOOL::EnumeratePads`' commit (`finish`) or revert: the numbers the click session produced are sent as one `set_pad_numbers` (one undo step), or dropped. */
  finishEnumerate: (commit: boolean) => Promise<void>;
  /**
   * `PAD_TOOL::EditPad` (`pcbnew.PadTool.explodePad` and `recombinePad`, both Ctrl+E): with a pad being edited as shapes, recombine it; with none and a single pad selected,
   * explode it (selection cleared, the active layer the pad's). One undo step each. `false` when there was nothing to do.
   */
  editPadShapes: () => Promise<boolean>;
  /** Pick a tool the way the toolbar does: leaving the Renumber Pads tool commits what it numbered (`evt->IsActivate()` -> `commit.Push( "Renumber Pads" )`). */
  setTool: (tool: FpToolId) => Promise<void>;
  addGraphic: (shape: CmdShape) => Promise<void>;
  moveGraphic: (id: string, dx: Nm, dy: Nm) => Promise<void>;
  editGraphic: (id: string, layer: string, strokeWidth: Nm, filled: boolean) => Promise<void>;
  deleteGraphic: (id: string) => Promise<void>;
  /** Add a text and answer with the document's id for it (the text `DRAWING_TOOL::PlaceText` selects once it is placed), or `undefined` when the
   *  command was refused. The id is read back from the server's document: `runCmd`'s refresh dispatches it, but that state can land after this returns. */
  addText: (text: CmdText) => Promise<string | undefined>;
  moveText: (id: string, x: Nm, y: Nm) => Promise<void>;
  editText: (id: string, fields: Omit<CmdText, "id" | "at">) => Promise<void>;
  deleteText: (id: string) => Promise<void>;
  /** `EDIT_TOOL::Remove`: the given pads, graphics and texts in one commit -- one `batch`, one undo step. */
  deleteItems: (ids: readonly string[]) => Promise<void>;
  setAnchor: (at: PointXY) => Promise<void>;
  /** `EDIT_TOOL::Rotate` (R / Shift+R) of the selection, any kind of item, about its pivot (kicad-port/fpItems.ts), one undo step. `snap` puts a several-item pivot on the grid. */
  rotateSelection: (quarterTurnsCcw: number, snap?: Snap, stepDeg?: number) => Promise<void>;
  /** `EDIT_TOOL::Mirror` (`mirrorH` left-right, `mirrorV` top-bottom) of the selection, one undo step. */
  mirrorSelection: (direction: FlipDirectionWire, snap?: Snap) => Promise<void>;
  /** `PCB_GROUP_TOOL::Group` (Ctrl+G) / `GROUP_TOOL::Ungroup` (Ctrl+Shift+G) of the selection; the new group is selected. */
  groupSelection: () => Promise<void>;
  ungroupSelection: () => Promise<void>;
  /** `DIALOG_TEXT_PROPERTIES`' OK on a field (`fp_item edit_field`). */
  editField: (name: string, text: string, layout: import("../api/types").FieldLayoutCmd) => Promise<boolean>;
  /** `DIALOG_TEXT_PROPERTIES`' OK on a text of the footprint, its place included (`fp_item set_text`). */
  setText: (id: string, text: CmdText, look?: import("../api/fpEditorTypes").TextLook) => Promise<boolean>;
  /** `DIALOG_SHAPE_PROPERTIES`' OK on a graphic (`fp_item set_graphic`), with its line style and hatch (UI-LEFT F5). */
  setGraphic: (id: string, shape: CmdShape, look?: import("../api/fpEditorTypes").ShapeLook) => Promise<boolean>;
  /** Place an item of `eda_model::fp_items`' kinds (`fp_item add`) and answer with its id, or `undefined` when refused. */
  addItem: (item: FpItemWire) => Promise<string | undefined>;
  /** An item of `eda_model::fp_items`' kinds replaced by its dialog (`fp_item set`). */
  setItem: (id: string, item: FpItemWire) => Promise<boolean>;
  editProperties: (fields: FootprintPropertiesFields) => Promise<boolean>;
  updateOnBoard: () => Promise<void>;
  deleteFootprint: () => Promise<void>;
  /** `GET /api/footprint/export` (GAPS.md #8 step 6) as a browser download. */
  exportKicadMod: () => Promise<void>;
}

const FpStateContext = createContext<FootprintEditorState | null>(null);
const FpDispatchContext = createContext<React.Dispatch<FpAction> | null>(null);
const FpApiContext = createContext<FootprintEditorApi | null>(null);

export function FootprintEditorProvider({ children }: { children: React.ReactNode }) {
  const [state, dispatch] = useReducer(reducer, initialState);
  const stateRef = useRef(state);
  stateRef.current = state;

  const refresh = useCallback(async () => {
    const name = stateRef.current.name;
    if (!name) return;
    try {
      const footprint = await fetchFootprint(name);
      dispatch({ type: "FOOTPRINT_OK", footprint });
    } catch (e) {
      dispatch({ type: "FOOTPRINT_ERR", message: e instanceof Error ? e.message : String(e) });
    }
  }, []);

  // Poll while a footprint is open, same ~700ms cadence state/store.tsx's
  // own poll uses -- this editor has no separate /api/version-style cheap
  // check of its own, so (unlike that file) this just re-fetches the one
  // document directly on every tick rather than gating on a version diff;
  // the payload here is one footprint, not the whole board, so the extra
  // requests are cheap.
  useEffect(() => {
    if (!state.name) return;
    let stopped = false;
    const tick = async () => {
      if (!stopped) await refresh();
    };
    tick();
    const id = setInterval(tick, 700);
    return () => {
      stopped = true;
      clearInterval(id);
    };
  }, [state.name, refresh]);

  /** The carry the server holds open (`carry: "begin"`): Duplicate's or Paste's items and their drop are then one undo step (crates/cli/src/carry.rs). */
  const carryRef = useRef(false);
  /** This session's last Copy: Paste falls back to it where the browser will not hand the system clipboard over. */
  const clipboardRef = useRef<string | null>(null);
  /** The Move tool carries what Duplicate, Paste or Import Graphics just made (until the drop or Escape): Escape reverts it once. */
  const pendingRef = useRef(false);
  const runCmd = useCallback(
    async (cmd: Cmd, carry?: CarryStep) => {
      const reply = await postCmd(cmd, false, carry); // strict has no meaning here -- footprint-library edits never touch board placement/routing gates
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "error" });
      await refresh();
      return reply.ok;
    },
    [refresh]
  );

  const api: FootprintEditorApi = {
    getState: () => stateRef.current,
    openFootprint: async (name) => {
      dispatch({ type: "SET_NAME", name });
      // an installed KiCad footprint is copied into the project library first; when that fails, nothing is opened
      const problem = await ensureFootprintInProject(name);
      const notLoaded = (why: string) => {
        const message = `Footprint '${name}' could not be loaded: ${why}`;
        dispatch({ type: "FOOTPRINT_ERR", message });
        dispatch({ type: "TOAST", message, kind: "error" });
      };
      if (problem) return notLoaded(problem);
      // the verb copies a library's footprint in itself when the design has lost its copy since (`library_place::with_opened_footprint`) and refuses one
      // whose file cannot be read: that refusal is said too, and nothing is opened
      const opened = await postCmd({ op: "open_footprint_for_edit", name }, false);
      if (!opened.ok) return notLoaded(opened.message);
      const footprint = await fetchFootprint(name);
      dispatch({ type: "FOOTPRINT_OK", footprint });
    },
    openFromBoard: async (reference) => {
      const key = boardCopyKey(reference);
      const opened = await postCmd({ op: "fp_editor", verb: "open_from_board", reference }, false);
      if (!opened.ok) {
        dispatch({ type: "TOAST", message: opened.message, kind: "error" });
        return;
      }
      dispatch({ type: "SET_NAME", name: key });
      dispatch({ type: "SET_LINKED_PART", ref: reference });
      try {
        dispatch({ type: "FOOTPRINT_OK", footprint: await fetchFootprint(key) });
      } catch (e) {
        dispatch({ type: "FOOTPRINT_ERR", message: e instanceof Error ? e.message : String(e) });
      }
    },
    newFootprint: async () => {
      let existing: string[] = [];
      try {
        existing = (await fetchFootprintLibraryNames()).names;
      } catch {
        /* backend busy: Untitled is still very likely free; the verb refuses a duplicate name anyway */
      }
      // `FOOTPRINT_EDITOR_CONTROL::NewFootprint`: the footprint goes into the library of `GetTargetFPID()` (the tree's selection, else the loaded footprint's), named
      // `Untitled` made unique there (`CreateNewFootprint( wxEmptyString, libraryName )`: `FootprintExists`, the design's copies and the library's folder), and is
      // saved into it at once (`tryToSaveFootprintInLibrary`); a library that cannot be written leaves it in no library -- here the project library's -- with a
      // warning (UI-LEFT-2 LL2). The server gives it the attributes of the library's last footprint (`crate::library_buffer`).
      const st0 = stateRef.current;
      let lib = newFootprintLibrary(st0.treeSelection, getLibraryTree("footprint").selectedLibs, st0.name);
      let readOnly = false;
      let inFile: string[] = [];
      if (isTableLibrary(lib)) {
        const row = (await fetchWritableLibraries("footprint").catch(() => [])).find((l) => l.name === lib);
        if (row) {
          readOnly = !row.writable;
          inFile = await fetchLibraryItemsNow("footprint", lib);
        } else lib = "";
      } else lib = "";
      const item = lib ? uniqueNewFootprintName((n) => inFile.includes(n) || existing.includes(`${lib}:${n}`) || (readOnly && existing.includes(n))) : uniqueFootprintName(existing);
      const name = lib && !readOnly ? `${lib}:${item}` : item;
      // `CreateNewFootprint`'s texts come from Preferences > Footprint Editor > Footprint Defaults and Graphics Defaults (kicad-port/fpFieldDefaults.ts, UI-LEFT-2 FF7). The
      // backend makes KiCad's default ones; other settings are put on them in the same undo step (the carry: `new_footprint` begins it, the edits hold it, the end commits it).
      const items = getFpDefaultTextItems();
      const graphics = getFpGraphicsDefaults();
      const own = !isFactoryNewFootprint(items, graphics, item);
      const reply = await postCmd({ op: "new_footprint", name }, false, own ? "begin" : undefined);
      if (!reply.ok) {
        dispatch({ type: "TOAST", message: reply.message, kind: "error" });
        return;
      }
      if (own) {
        const t = newFootprintTexts(items, graphics, item);
        const made = await fetchFootprint(name).catch(() => null);
        const cmds: Cmd[] = [
          { op: "fp_item", verb: "edit_field", footprint: name, name: "Reference", text: t.reference.text, layout: t.reference.layout },
          { op: "fp_item", verb: "edit_field", footprint: name, name: "Value", text: t.value.text, layout: t.value.layout },
          ...(made?.texts ?? []).filter((x) => x.id).map((x): Cmd => ({ op: "delete_footprint_text", footprint: name, id: x.id! })),
          ...t.texts.map((text): Cmd => ({ op: "add_footprint_text", footprint: name, text })),
        ];
        for (const cmd of cmds) {
          const r = await postCmd(cmd, false, "hold");
          if (!r.ok) {
            dispatch({ type: "TOAST", message: r.message, kind: "error" });
            break;
          }
        }
        await postCarry("commit");
      }
      // `tryToSaveFootprintInLibrary`: into the library at once (`SaveFootprint`, `ClearModify`), or the read-only library's warning (`ShowInfoBarWarning`)
      if (lib && !readOnly) {
        const saved = await saveToLibrary({ kind: "footprint", id: name });
        if (!saved.ok) dispatch({ type: "TOAST", message: saved.error, kind: "error" });
      } else if (lib && readOnly) {
        dispatch({ type: "TOAST", message: newFootprintReadOnlyWarning(lib), kind: "error" });
      }
      dispatch({ type: "SET_NAME", name });
      dispatch({ type: "SET_TREE_SELECTION", name });
      dispatch({ type: "FOOTPRINT_OK", footprint: await fetchFootprint(name) });
    },
    duplicateSelection: async (increment) => {
      const st = stateRef.current;
      const fp = st.footprint;
      const name = st.name;
      if (!fp || !name) return;
      const ids = [...st.selection].filter((id) => !id.startsWith("field:"));
      if (ids.length === 0) return;
      // `EDIT_TOOL::Duplicate` in the footprint editor: every kind of item but a field, with its looks, a group with what is below it (`fp_item duplicate`, UI-LEFT-2
      // FF2); Duplicate and Increment gives the copied pads the next number (kicad-port/fpEditActions.ts `duplicatePads`: all of them the same one)
      const pads = fp.pads.filter((p) => p.id && fpLeaves(fp, ids).includes(p.id));
      const { lastPadNumber } = duplicatePads(fp.pads, pads, increment, st.lastPadNumber ?? FIRST_LAST_PAD_NUMBER);
      const before = new Set(fpAllIds(fp));
      const ok = await runCmd({ op: "fp_item", verb: "duplicate", footprint: name, ids, ...(increment && pads.length > 0 ? { pad_number: lastPadNumber } : {}) }, "begin");
      if (!ok) return;
      carryRef.current = true;
      // Read the document straight from the backend: the render that applies `runCmd`'s own refresh may not have happened yet.
      const after = await fetchFootprint(name);
      const created = topLevelNew(after, before);
      if (increment && pads.length > 0) dispatch({ type: "SET_LAST_PAD_NUMBER", number: lastPadNumber });
      dispatch({ type: "SET_SELECTION", refs: created });
      // `doMoveSelection( aEvent, &commit, true )`: the copies are glued to the cursor until a click drops them.
      const at = stateRef.current.cursorNm;
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
      dispatch({ type: "SET_MOVE_ORIGIN", at });
      dispatch({ type: "SET_DUPLICATE_PENDING", pending: true });
      pendingRef.current = true;
      dispatch({ type: "TOAST", message: `Duplicated ${created.length} item(s)`, kind: "info" });
    },
    copySelection: async (cut) => {
      const st = stateRef.current;
      const name = st.name;
      const ids = [...st.selection];
      if (!name || ids.length === 0) return false;
      // `refPoint = grid.BestDragOrigin( GetCursorPosition(), items )`: here the cursor on the grid (while the editor snaps to it)
      const c = st.cursorNm ?? { x: 0, y: 0 };
      const g = snapGridNm(st.gridNm) || 1;
      const reference = { x: Math.round(c.x / g) * g, y: Math.round(c.y / g) * g };
      const r = await fetch("/api/footprint/clipboard/copy", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ footprint: name, ids, reference }) });
      const reply = (await r.json()) as { text?: string; error?: string };
      if (!reply.text) {
        dispatch({ type: "TOAST", message: reply.error ?? "Copy failed", kind: "error" });
        return false;
      }
      clipboardRef.current = reply.text;
      await writeClipboardText(reply.text);
      dispatch({ type: "TOAST", message: "Selection copied", kind: "info" });
      // `cutToClipboard`: the copy, then `DeleteItems` of what was copied
      if (cut) {
        dispatch({ type: "CLEAR_SELECTION" });
        await runCmd({ op: "fp_item", verb: "delete", footprint: name, ids });
      }
      return true;
    },
    pasteItems: async () => {
      const st = stateRef.current;
      const fp = st.footprint;
      const name = st.name;
      if (!fp || !name) return;
      const system = await readClipboardText();
      const text = system != null && system !== "" ? system : clipboardRef.current;
      if (!text) return;
      const c = st.cursorNm ?? { x: 0, y: 0 };
      const g = snapGridNm(st.gridNm) || 1;
      const at = { x: Math.round(c.x / g) * g, y: Math.round(c.y / g) * g };
      // KiCad's text (`CLIPBOARD_IO::Parse`) goes in as items; any other text as a text item on the active layer ("If it wasn't content, then paste as a text object")
      let cmd: Cmd;
      if (fpClipboardKind(text) === "kicad") cmd = { op: "fp_item", verb: "paste", footprint: name, text, at };
      else {
        const plain = fpPlainTextPaste(text, at, st.activeLayer || "F.SilkS");
        if (!plain || (plain.longText && !window.confirm(LONG_TEXT_QUESTION))) return;
        cmd = { op: "add_footprint_text", footprint: name, text: plain.text };
      }
      const before = new Set(fpAllIds(fp));
      const ok = await runCmd(cmd, "begin");
      if (!ok) return;
      carryRef.current = true;
      const after = await fetchFootprint(name);
      const created = topLevelNew(after, before);
      if (created.length === 0) {
        carryRef.current = false;
        await postCarry("commit");
        return;
      }
      // `placeBoardItems( .., aAnchorAtOrigin = true )`: the clipboard's origin on the cursor, then `PCB_ACTIONS::move` with the commit
      dispatch({ type: "SET_SELECTION", refs: created });
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
      dispatch({ type: "SET_MOVE_ORIGIN", at });
      dispatch({ type: "SET_DUPLICATE_PENDING", pending: true });
      pendingRef.current = true;
    },
    cancelPickUp: async () => {
      if (!pendingRef.current) return;
      pendingRef.current = false;
      dispatch({ type: "SET_DUPLICATE_PENDING", pending: false });
      dispatch({ type: "CLEAR_SELECTION" });
      if (carryRef.current) {
        carryRef.current = false;
        const reply = await postCarry("cancel");
        if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "info" });
        await refresh();
        return;
      }
      await postUndo("footprint_editor");
      await refresh();
    },
    pickUp: (ids, message) => {
      dispatch({ type: "SET_SELECTION", refs: ids });
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
      dispatch({ type: "SET_MOVE_ORIGIN", at: stateRef.current.cursorNm });
      dispatch({ type: "SET_DUPLICATE_PENDING", pending: true });
      pendingRef.current = true;
      if (message) dispatch({ type: "TOAST", message, kind: "info" });
    },
    commitMove: async (refs, dxNm, dyNm) => {
      dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
      dispatch({ type: "SET_DUPLICATE_PENDING", pending: false });
      const name = stateRef.current.name;
      // the drop of what Duplicate or Paste made ends the held step: `commit.Push( _( "Duplicate" ) )` / "Paste", one undo step with the copies
      pendingRef.current = false;
      const carry = carryRef.current;
      carryRef.current = false;
      const cmd = name && (dxNm !== 0 || dyNm !== 0) ? fpMoveCmd(name, refs, dxNm, dyNm) : null;
      if (cmd) await runCmd(cmd, carry ? "commit" : undefined);
      else if (carry) {
        await postCarry("commit");
        await refresh();
      }
    },
    closeFootprint: () => dispatch({ type: "SET_NAME", name: null }),
    refresh,
    cmd: (c) => runCmd(c),
    undo: async () => {
      dispatch({ type: "CLEAR_SELECTION" });
      const reply = await postUndo("footprint_editor");
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "info" });
      await refresh();
    },
    redo: async () => {
      dispatch({ type: "CLEAR_SELECTION" });
      const reply = await postRedo("footprint_editor");
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "info" });
      await refresh();
    },
    padById: (id) => stateRef.current.footprint?.pads.find((p) => p.id === id),
    graphicById: (id) => stateRef.current.footprint?.graphics.find((s) => s.id === id),
    textById: (id) => stateRef.current.footprint?.texts.find((t) => t.id === id),
    addPad: async (pad) => {
      const name = stateRef.current.name;
      if (!name) return;
      await runCmd({ op: "add_pad", footprint: name, pad });
    },
    movePad: async (id, x, y) => {
      const name = stateRef.current.name;
      if (!name) return;
      await runCmd({ op: "move_pad", footprint: name, id, x, y });
    },
    rotatePad: async (id, quarterTurns) => {
      const name = stateRef.current.name;
      if (!name) return;
      await runCmd({ op: "rotate_pad", footprint: name, id, quarter_turns: ((quarterTurns % 4) + 4) % 4 });
    },
    deletePad: async (id) => {
      const name = stateRef.current.name;
      if (!name) return;
      dispatch({ type: "CLEAR_SELECTION" });
      await runCmd({ op: "delete_pad", footprint: name, id });
    },
    editPad: async (id, pad) => {
      const name = stateRef.current.name;
      if (!name) return false;
      return runCmd({ op: "edit_pad", footprint: name, id, pad });
    },
    renumberPads: async (start, prefix, step) => {
      const name = stateRef.current.name;
      if (!name) return;
      await runCmd({ op: "renumber_pads", footprint: name, start, prefix, step });
    },
    pushPadProperties: async (sourcePadId, filters) => {
      const name = stateRef.current.name;
      if (!name) return;
      await runCmd({ op: "push_pad_properties", footprint: name, source_pad_id: sourcePadId, filter_shape: filters.shape, filter_orientation: filters.orientation, filter_layers: filters.layers, filter_type: filters.type });
    },
    copyPadProperties: (id) => {
      const pad = stateRef.current.footprint?.pads.find((p) => p.id === id);
      if (!pad) return;
      dispatch({ type: "SET_DEFAULT_PAD", pad: settingsOf(pad) });
    },
    pastePadProperties: async (ids) => {
      const st = stateRef.current;
      const name = st.name;
      if (!name || !st.footprint) return;
      const cmds: Cmd[] = [];
      for (const id of ids) {
        const current = st.footprint.pads.find((p) => p.id === id);
        if (current) cmds.push({ op: "edit_pad", footprint: name, id, pad: importPadSettings(current, st.defaultPad) });
      }
      if (cmds.length === 0) return;
      await runCmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
    },
    finishEnumerate: async (commit) => {
      const st = stateRef.current;
      const session = st.enumerate;
      const name = st.name;
      dispatch({ type: "SET_ENUMERATE", state: null });
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      if (!commit || !session || !name) return;
      const numbers = enumerateCommit(session);
      if (numbers.length === 0) return;
      if (await runCmd({ op: "set_pad_numbers", footprint: name, numbers })) dispatch({ type: "SET_LAST_PAD_NUMBER", number: numbers[numbers.length - 1]![1] }); // `SetLastPadNumber( newNumber )`
    },
    editPadShapes: async () => {
      const st = stateRef.current;
      const name = st.name;
      if (!name) return false;
      const cmd = editPadCommand(name, st.footprint, st.selection);
      if (!cmd) return false;
      dispatch({ type: "CLEAR_SELECTION" }); // `ACTIONS::selectionClear`
      const ok = await runCmd(cmd);
      if (ok && cmd.op === "explode_pad") {
        const pad = st.footprint?.pads.find((p) => p.id === cmd.id);
        if (pad) dispatch({ type: "PAD_EDIT_ENTER", layer: padEditLayer(pad) }); // `frame()->SetActiveLayer( layer )`
      }
      return ok;
    },
    setTool: async (tool) => {
      if (stateRef.current.enumerate && tool !== "enumerate") await api.finishEnumerate(true);
      dispatch({ type: "SET_ACTIVE_TOOL", tool });
    },
    addGraphic: async (shape) => {
      const name = stateRef.current.name;
      if (!name) return;
      await runCmd({ op: "add_footprint_graphic", footprint: name, shape });
    },
    moveGraphic: async (id, dx, dy) => {
      const name = stateRef.current.name;
      if (!name) return;
      await runCmd({ op: "move_footprint_graphic", footprint: name, id, dx, dy });
    },
    editGraphic: async (id, layer, strokeWidth, filled) => {
      const name = stateRef.current.name;
      if (!name) return;
      await runCmd({ op: "edit_footprint_graphic", footprint: name, id, layer, stroke_width: strokeWidth, filled });
    },
    deleteGraphic: async (id) => {
      const name = stateRef.current.name;
      if (!name) return;
      dispatch({ type: "CLEAR_SELECTION" });
      await runCmd({ op: "delete_footprint_graphic", footprint: name, id });
    },
    addText: async (text) => {
      const name = stateRef.current.name;
      if (!name) return undefined;
      const before = new Set((stateRef.current.footprint?.texts ?? []).map((t) => t.id));
      if (!(await runCmd({ op: "add_footprint_text", footprint: name, text }))) return undefined;
      // Read the document for the new text's id: `runCmd` has refreshed the state, but React may not have committed it when this resolves.
      try {
        const footprint = await fetchFootprint(name);
        return footprint.texts.find((t) => t.id && !before.has(t.id))?.id;
      } catch {
        return undefined;
      }
    },
    moveText: async (id, x, y) => {
      const name = stateRef.current.name;
      if (!name) return;
      await runCmd({ op: "move_footprint_text", footprint: name, id, x, y });
    },
    editText: async (id, fields) => {
      const name = stateRef.current.name;
      if (!name) return;
      await runCmd({ op: "edit_footprint_text", footprint: name, id, ...fields });
    },
    deleteText: async (id) => {
      const name = stateRef.current.name;
      if (!name) return;
      dispatch({ type: "CLEAR_SELECTION" });
      await runCmd({ op: "delete_footprint_text", footprint: name, id });
    },
    deleteItems: async (ids) => {
      const st = stateRef.current;
      const name = st.name;
      const fp = st.footprint;
      if (!name || !fp) return;
      // `EDIT_TOOL::DeleteItems`: one `fp_item delete` takes every kind (a field is hidden, a group goes with its members).
      const known = fpAllIds(fp);
      const doomed = ids.filter((id) => known.has(id));
      dispatch({ type: "CLEAR_SELECTION" });
      if (doomed.length > 0) await runCmd({ op: "fp_item", verb: "delete", footprint: name, ids: doomed });
    },
    setAnchor: async (at) => {
      const name = stateRef.current.name;
      if (!name) return;
      await runCmd({ op: "set_footprint_anchor", name, at });
    },
    rotateSelection: async (quarterTurnsCcw, snap, stepDeg) => {
      const st = stateRef.current;
      if (!st.name || !st.footprint) return;
      const cmd = fpRotateCmd(st.footprint, st.name, [...st.selection], quarterTurnsCcw, snap, stepDeg);
      if (cmd) await runCmd(cmd);
    },
    mirrorSelection: async (direction, snap) => {
      const st = stateRef.current;
      if (!st.name || !st.footprint) return;
      const cmd = fpMirrorCmd(st.footprint, st.name, [...st.selection], direction, snap);
      if (cmd) await runCmd(cmd);
    },
    groupSelection: async () => {
      const st = stateRef.current;
      if (!st.name || !st.footprint) return;
      const cmd = fpGroupCmd(st.footprint, st.name, [...st.selection]);
      if (!cmd) return;
      const before = new Set((st.footprint.items?.groups ?? []).map((g) => g.id));
      if (!(await runCmd(cmd))) return;
      // `RunAction( ACTIONS::selectItem, group )`: the new group is the selection.
      const after = await fetchFootprint(st.name);
      const made = (after.items?.groups ?? []).find((g) => !before.has(g.id));
      dispatch({ type: "SET_SELECTION", refs: made ? [made.id] : [] });
    },
    ungroupSelection: async () => {
      const st = stateRef.current;
      if (!st.name || !st.footprint) return;
      const cmd = fpUngroupCmd(st.footprint, st.name, [...st.selection]);
      if (!cmd) return;
      // `GROUP_TOOL::Ungroup` selects the members it freed.
      const members = fpLeaves(st.footprint, [...st.selection]);
      if (await runCmd(cmd)) dispatch({ type: "SET_SELECTION", refs: members });
    },
    editField: async (fieldName, text, layout) => {
      const name = stateRef.current.name;
      if (!name) return false;
      return runCmd({ op: "fp_item", verb: "edit_field", footprint: name, name: fieldName, text, layout });
    },
    setText: async (id, text, look) => {
      const name = stateRef.current.name;
      if (!name) return false;
      return runCmd({ op: "fp_item", verb: "set_text", footprint: name, id, text, ...(look ? { look } : {}) });
    },
    setGraphic: async (id, shape, look) => {
      const name = stateRef.current.name;
      if (!name) return false;
      return runCmd({ op: "fp_item", verb: "set_graphic", footprint: name, id, shape, ...(look ? { look } : {}) });
    },
    addItem: async (item) => {
      const st = stateRef.current;
      if (!st.name) return undefined;
      const before = st.footprint ? fpAllIds(st.footprint) : new Set<string>();
      if (!(await runCmd({ op: "fp_item", verb: "add", footprint: st.name, item }))) return undefined;
      try {
        const after = await fetchFootprint(st.name);
        return [...fpAllIds(after)].find((id) => !before.has(id));
      } catch {
        return undefined;
      }
    },
    setItem: async (id, item) => {
      const name = stateRef.current.name;
      if (!name) return false;
      return runCmd({ op: "fp_item", verb: "set", footprint: name, id, item });
    },
    editProperties: async (fields) => {
      const name = stateRef.current.name;
      if (!name) return false;
      return runCmd({ op: "edit_footprint_properties", name, ...fields });
    },
    updateOnBoard: async () => {
      const name = stateRef.current.name;
      if (!name) return;
      const ok = await runCmd({ op: "update_footprint_on_board", name });
      dispatch({ type: "TOAST", message: ok ? `${name}: board instances now follow this library entry` : "update failed", kind: ok ? "info" : "error" });
    },
    deleteFootprint: async () => {
      const name = stateRef.current.name;
      if (!name) return;
      await postCmd({ op: "delete_library_footprint", name }, false);
      dispatch({ type: "SET_NAME", name: null });
    },
    exportKicadMod: async () => {
      const name = stateRef.current.name;
      if (!name) return;
      // `FOOTPRINT_EDIT_FRAME::SaveFootprint`: a pad being edited as shapes is recombined first.
      if (stateRef.current.footprint?.pad_edit) await runCmd({ op: "recombine_pad", footprint: name });
      try {
        await downloadFootprintKicadMod(name);
      } catch (e) {
        dispatch({ type: "TOAST", message: e instanceof Error ? e.message : String(e), kind: "error" });
      }
    },
  };

  return (
    <FpStateContext.Provider value={state}>
      <FpDispatchContext.Provider value={dispatch}>
        <FpApiContext.Provider value={api}>{children}</FpApiContext.Provider>
      </FpDispatchContext.Provider>
    </FpStateContext.Provider>
  );
}

export function useFpState(): FootprintEditorState {
  const ctx = useContext(FpStateContext);
  if (!ctx) throw new Error("useFpState must be used within FootprintEditorProvider");
  return ctx;
}

export function useFpDispatch(): React.Dispatch<FpAction> {
  const ctx = useContext(FpDispatchContext);
  if (!ctx) throw new Error("useFpDispatch must be used within FootprintEditorProvider");
  return ctx;
}

export function useFpApi(): FootprintEditorApi {
  const ctx = useContext(FpApiContext);
  if (!ctx) throw new Error("useFpApi must be used within FootprintEditorProvider");
  return ctx;
}
