// The Symbol Editor tab's own state (eeschema's Symbol Editor). A library
// symbol is a genuinely separate document from the schematic (its own
// undo domain -- "symbol_editor", see api/client.ts's postUndo/postRedo --
// its own selection, its own pan/zoom), so this is a small, self-contained
// useReducer store of exactly the same shape `state/footprintEditorStore
// .tsx` uses for a library footprint, rather than growing store.tsx with a
// second, unrelated document to track -- see that file's own doc comment
// for the full reasoning, which applies here unchanged.
//
// Unit convention: `design.symbol_library`'s own wire format is
// millimetres, +y **up** (KiCad's own library convention -- crates/model/
// src/symbol.rs's doc) -- `state.symbol` holds that raw shape untouched,
// same "no second hand-built shape" rule `LibrarySymbol`'s own doc in
// api/types.ts states. Everything *interactive* in this file instead
// works in this app's shared internal space (integer-scaled µm, +y
// **down**) -- `gridNm`/`cursorNm`/`moveOriginNm`/`SymMovePreview`'s own
// `dxNm`/`dyNm` -- exactly like the Footprint Editor's own store, so
// `SymbolEditorCanvas.tsx` can reuse `kicad-port/view.ts`/`gridHelper.ts`
// and `components/schematic/transform.ts`'s `resolvePin`/`resolveLibPoint`
// (which already do the mm->µm, Y-up->Y-down conversion -- the same
// pipeline a placed instance's own rendering uses) unchanged, converting
// back to mm only at the `Cmd` boundary (`api.movePin`/`addPin`/etc. all
// take plain mm numbers, matching the wire format directly).
import React, { createContext, useCallback, useContext, useEffect, useReducer, useRef } from "react";
import type { Cmd, LibraryFill, LibrarySymbol, LibrarySymbolGraphic, LibrarySymbolPin, PushPinField, SymbolPropertiesFields } from "../api/types";
import { downloadSymbolKicadSym, downloadSymbolLibraryKicadSym, fetchSymbolForEditor, postCmd, postRedo, postUndo } from "../api/client";
import { ensureSymbolInProject } from "../api/libraryClient";
import { joinLibName } from "../kicad-port/libraryNames";
import { PROJECT_SYMBOL_LIB } from "../kicad-port/symEditActions";
import { allFields, wireField } from "../kicad-port/symFields";
import { movedRows } from "../kicad-port/symFieldLayout";
import { defaultSyncMode, imagePinsFor, linkedPinsToMove, planSyncedEdit, synchronizePins } from "../kicad-port/symPinSync";
import { DEFAULT_PIN_TEMPLATE, samePinTemplate, type PinTemplate } from "../kicad-port/symPinTemplate";
import { deletePlan, duplicateItems, pastedItems, placedPinWithId, repeatPin, rotatePlan } from "../kicad-port/symEditItems";
import { pinOccupiedMessage, pinPlacementOccupant } from "../kicad-port/pinPlacement";
import { askConfirm } from "../components/library/libraryDialogs";
import { INITIAL_LAST_TEXT_BOX, type LastTextBox, type LibTextBox } from "../kicad-port/symTextBox";
import type { ViewTransform } from "./store";

export type SymToolId = "select" | "move" | "pin" | "draw_segment" | "draw_lines" | "draw_arc" | "draw_rect" | "draw_circle" | "draw_polygon" | "draw_textbox" | "text" | "anchor";

export const SYM_TOOL_MESSAGES: Record<SymToolId, string> = {
  select: "Select item(s)",
  move: "Move item(s)",
  pin: "Pin: click to place, number auto-increments",
  draw_segment: "Line: click start, then end",
  draw_lines: "Lines: click each point of the connected graphic lines; double-click or Enter to finish, Esc to cancel",
  draw_arc: "Arc: click start, mid, then end",
  draw_rect: "Rectangle: click one corner, then the opposite one",
  draw_circle: "Circle: click center, then a point on the edge",
  draw_polygon: "Polygon: click points, Enter/double-click to finish (the shape closes), Esc to cancel",
  draw_textbox: "Text box: click one corner, then the opposite one, then enter the text",
  text: "Text: click where it goes, then type it",
  anchor: "Move the symbol anchor: click the point that becomes the symbol's origin",
};

/** Same "click to add point(s), commit on the last one" shape the Footprint Editor's own `FpDrawState` uses. Points are internal-space µm (see this file's own unit-convention doc). `lines` is the open multi-point polyline (`drawSymbolLines`), `polygon` the closed one. */
export type SymDrawState = { kind: "shape"; shapeKind: "segment" | "lines" | "arc" | "rect" | "circle" | "polygon" | "textbox"; pts: [number, number][] };

/** `DIALOG_SAVE_SYMBOL_AS` (Save As / Save Copy As): which of the two it is. */
export interface SaveAsRequest {
  /** `saveSymbolAs` (`aOpenCopy`) opens the copy afterwards; `saveSymbolCopyAs` leaves the editor where it was. */
  openCopy: boolean;
  /** The symbol being saved. */
  libId: string;
}

/** A `.kicad_sym` file read for Import Symbol: the symbols in it, for the selection dialog (`DIALOG_IMPORT_SYMBOL_SELECT`). */
export interface ImportRequest {
  fileName: string;
  symbols: LibrarySymbol[];
  warnings: string[];
  /** The library the symbols land in (the tree selection's library). */
  lib: string;
}

export interface SymMovePreview {
  refs: string[];
  kind: "pin" | "graphic" | "field";
  dxNm: number;
  dyNm: number;
}

export interface SymbolEditorState {
  /** The symbol currently open, or `null` before anything has been opened this session. */
  libId: string | null;
  /** The last successful GET /api/symbol?lib_id=... for `libId` -- raw mm/Y-up wire shape, untouched. For a derived symbol it is the flat symbol (everything it inherits): what is drawn. */
  symbol: LibrarySymbol | null;
  /** A derived symbol's own entry (`extends`, its own fields); `null` for a symbol that is not derived (`symbol` is then the entry itself). */
  derivedOwn: LibrarySymbol | null;
  error: string | null;
  view: ViewTransform;
  viewInitialized: boolean;
  /** Bumped by Zoom to Fit (`REQUEST_FIT`): the canvas fits the view to the symbol again, whether or not the view was moved by hand since. */
  fitRequest: number;
  selection: Set<string>;
  activeTool: SymToolId;
  drawState: SymDrawState | null;
  movePreview: SymMovePreview | null;
  moveOriginNm: { x: number; y: number } | null;
  cursorNm: { x: number; y: number } | null;
  gridNm: number;
  gridVisible: boolean;
  /** Which unit (1-based; 0 = "common to all units") new pins/graphics are placed on. */
  activeUnit: number;
  /** Which body style (1 normal, 2 alternate/DeMorgan) new pins/graphics are placed on -- only selectable once `symbol.has_alternate_body_style`. */
  activeBodyStyle: number;
  /** "E" on a selected pin, or double-click -- which pin's Pin Properties dialog is open. */
  pinPropertiesId: string | null;
  pinTableOpen: boolean;
  propertiesOpen: boolean;
  /** The field the Symbol Properties dialog opens on (a double click on a field of the canvas): its row is the grid's selected one. */
  propertiesField: string | null;
  /** The symbols the library tree has selected (`GetSelectedLibIds()`); empty = none (the open symbol is the target, `GetTargetLibId`). */
  treeSelection: string[];
  /** `m_ShowPinsElectricalType`: draw each pin's electrical type beside it. On to begin with, as `show_pin_electrical_type` is in `SYMBOL_EDITOR_SETTINGS`. */
  showElectricalTypes: boolean;
  /** `m_ShowPinNumbers`: force the pin numbers on even where the symbol hides them. Off to begin with (`SCH_RENDER_SETTINGS`). */
  showPinNumbers: boolean;
  /** `m_ShowHiddenPins`: draw the pins marked hidden. On to begin with (`show_hidden_lib_pins`). */
  showHiddenPins: boolean;
  /** `m_ShowHiddenFields`: draw the fields that are not shown, in the hidden-items colour. On to begin with (`SYMBOL_EDITOR_SETTINGS::m_ShowHiddenFields`). */
  showHiddenFields: boolean;
  /** `m_ShowPinAltIcons`: the icon beside the name of a pin that has alternate functions. On to begin with (`SYMBOL_EDITOR_SETTINGS::m_ShowPinAltIcons`). */
  showPinAltIcons: boolean;
  /** `SYMBOL_EDIT_FRAME::m_SyncPinEdit`: Synchronized Pins Mode. On by default for a multi-unit symbol. */
  syncPins: boolean;
  /** `SYMBOL_EDITOR_DRAWING_TOOLS::m_drawSpecificUnit`: a new graphic or text is on the unit being edited (else on unit 0, common to all units). Set to `UnitsLocked()` when a
   * symbol is loaded and when Symbol Properties is closed with OK ("if units are locked, graphic items are specific to each unit"), and to "not Common to all units" by Shape
   * Properties' OK. */
  drawSpecificUnit: boolean;
  /** `m_drawSpecificBodyStyle`: a new item is on the body style being edited (else common to both); on to begin with, set by Shape Properties' OK. */
  drawSpecificBodyStyle: boolean;
  /** The symbols Copy / Cut put on the clipboard: kept beside the clipboard text so Paste of our own copy loses nothing (and works where the browser denies clipboard access). */
  copied: { text: string; symbols: LibrarySymbol[] } | null;
  saveAs: SaveAsRequest | null;
  importRequest: ImportRequest | null;
  fieldsTableOpen: boolean;
  /** Which symbols the fields table lists (`DIALOG_LIB_FIELDS_TABLE::SCOPE`): the library's, or the ones related to the target (its root and everything derived from it). */
  fieldsTableScope: "library" | "related";
  /** `deriveFromExistingSymbol`: the Derive from Existing Symbol dialog (`DIALOG_LIB_NEW_SYMBOL` with a parent) is open on this symbol. */
  deriveRequest: { parent: string } | null;
  /** `DIALOG_LIB_NEW_SYMBOL` (Ctrl+N, `CreateNewSymbol` with no parent): open, asking for the new symbol's name and first properties. */
  newSymbolOpen: boolean;
  /** `SYMBOL_EDITOR_PIN_TOOL::CreatePin`: the pin the Pin tool has just dropped, whose properties Pin Properties asks for before it is added
   * (`CreatePin`'s `EditPinProperties`). OK adds it, Cancel drops it and places nothing. */
  pendingPin: LibrarySymbolPin | null;
  /** `g_lastPin`: the pin placed last (by the Pin tool or by Repeat Pin), for the symbol it was placed in -- what Repeat Pin (Insert) repeats. KiCad keeps its UUID
   * (`g_lastPin = pin->m_Uuid`): the ids are the server's, so `PIN_PLACED` (sent after the add and its refetch) takes the id of the pin of the symbol that is the
   * pin as placed, and Repeat finds it by that id, as it is now (moved, renumbered or renamed since). */
  lastPlacedPin: { libId: string | null; pin: LibrarySymbolPin } | null;
  /** `SYMBOL_EDITOR_EDIT_TOOL::Copy`'s clipboard of items (pins and graphics), for Paste. Kept in the editor session, not on the system clipboard. */
  copiedItems: { pins: LibrarySymbolPin[]; graphics: LibrarySymbolGraphic[] } | null;
  /** `g_LastPin*`: the look the next pin starts with -- the last pin placed or edited (see `kicad-port/symPinTemplate.ts`). */
  pinTemplate: PinTemplate;
  /** `updateSymbolFields`: the Update Symbol Fields dialog (`DIALOG_UPDATE_SYMBOL_FIELDS`) is open on the symbol being edited. */
  updateFieldsOpen: boolean;
  /** `placeSymbolText` / the Text tool: where the click landed, in internal µm; the Text Properties dialog asks for the text. */
  textDialog: { atNm: [number, number] } | null;
  /** The text the dialog was OK'd with, following the cursor until the click that places it (`TwoClickPlace`'s `item` while `IS_MOVING`). */
  pendingText: { text: string; sizeMm: number; angleDeg: number } | null;
  /** Text Box Properties (`DIALOG_TEXT_PROPERTIES` of a `SCH_TEXTBOX`): `id` is the box being edited, `null` for the one the Text Box tool has just drawn and the dialog is about to add. */
  textBoxDialog: { id: string | null; box: LibTextBox } | null;
  /** Shape Properties (`DIALOG_SHAPE_PROPERTIES`): the shape it is open on (double click, `E`, the context menu). */
  shapeDialogId: string | null;
  /** What the Text Box tool carries from one box to the next (`m_lastTextBold`, `m_lastTextJust`, `m_lastFillStyle`, `m_lastStroke`...). */
  lastTextBox: LastTextBox;
  /** `m_lastTextAngle`: the text angle the next Text dialog starts with. */
  lastTextAngle: number;
  toast: { message: string; kind: "error" | "info" } | null;
}

/** The unit and body style a new graphic or text goes on (`SYMBOL_EDITOR_DRAWING_TOOLS`: `if( m_drawSpecificUnit ) item->SetUnit( m_frame->GetUnit() )`, else it stays on 0, common to all). */
export function drawTarget(state: Pick<SymbolEditorState, "drawSpecificUnit" | "drawSpecificBodyStyle" | "activeUnit" | "activeBodyStyle">): { unit: number; bodyStyle: number } {
  return { unit: state.drawSpecificUnit ? state.activeUnit : 0, bodyStyle: state.drawSpecificBodyStyle ? state.activeBodyStyle : 0 };
}

const initialState: SymbolEditorState = {
  libId: null,
  symbol: null,
  derivedOwn: null,
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
  gridNm: 1_270_000, // 1.27mm (50 mil) -- KiCad's own Symbol Editor default grid
  gridVisible: true,
  activeUnit: 1,
  activeBodyStyle: 1,
  pinPropertiesId: null,
  pinTableOpen: false,
  propertiesOpen: false,
  propertiesField: null,
  treeSelection: [],
  showElectricalTypes: true,
  showPinNumbers: false,
  showHiddenPins: true,
  showHiddenFields: true,
  showPinAltIcons: true,
  syncPins: false,
  drawSpecificUnit: false,
  drawSpecificBodyStyle: true,
  copied: null,
  saveAs: null,
  importRequest: null,
  fieldsTableOpen: false,
  fieldsTableScope: "library",
  deriveRequest: null,
  newSymbolOpen: false,
  pendingPin: null,
  lastPlacedPin: null,
  copiedItems: null,
  pinTemplate: DEFAULT_PIN_TEMPLATE,
  updateFieldsOpen: false,
  textDialog: null,
  pendingText: null,
  textBoxDialog: null,
  shapeDialogId: null,
  lastTextBox: INITIAL_LAST_TEXT_BOX,
  lastTextAngle: 0,
  toast: null,
};

export type SymAction =
  | { type: "SET_LIB_ID"; libId: string | null }
  | { type: "SYMBOL_OK"; symbol: LibrarySymbol; own?: LibrarySymbol | null }
  | { type: "SYMBOL_ERR"; message: string }
  | { type: "SET_VIEW"; view: ViewTransform }
  | { type: "MARK_VIEW_INITIALIZED" }
  | { type: "REQUEST_FIT" }
  | { type: "SET_SELECTION"; refs: string[] }
  | { type: "CLEAR_SELECTION" }
  | { type: "ESCAPE" }
  | { type: "SET_ACTIVE_TOOL"; tool: SymToolId }
  | { type: "SET_DRAW_STATE"; draw: SymDrawState | null }
  | { type: "SET_MOVE_PREVIEW"; preview: SymMovePreview | null }
  | { type: "SET_MOVE_ORIGIN"; at: { x: number; y: number } | null }
  | { type: "SET_CURSOR"; at: { x: number; y: number } | null }
  | { type: "SET_GRID_NM"; um: number }
  | { type: "TOGGLE_GRID_VISIBLE" }
  | { type: "SET_ACTIVE_UNIT"; unit: number }
  | { type: "SET_ACTIVE_BODY_STYLE"; style: number }
  | { type: "SET_PIN_PROPERTIES_ID"; id: string | null }
  | { type: "SET_PIN_TABLE_OPEN"; open: boolean }
  | { type: "SET_PROPERTIES_OPEN"; open: boolean; field?: string }
  | { type: "SET_TREE_SELECTION"; names: string[] }
  | { type: "TOGGLE_ELECTRICAL_TYPES" }
  | { type: "TOGGLE_PIN_NUMBERS" }
  | { type: "TOGGLE_HIDDEN_PINS" }
  | { type: "TOGGLE_HIDDEN_FIELDS" }
  | { type: "TOGGLE_PIN_ALT_ICONS" }
  | { type: "SET_SYNC_PINS"; on: boolean }
  | { type: "SET_DRAW_SPECIFIC"; unit?: boolean; bodyStyle?: boolean }
  | { type: "SET_COPIED"; copied: { text: string; symbols: LibrarySymbol[] } | null }
  | { type: "SET_SAVE_AS"; request: SaveAsRequest | null }
  | { type: "SET_IMPORT_REQUEST"; request: ImportRequest | null }
  | { type: "SET_FIELDS_TABLE_OPEN"; open: boolean; scope?: "library" | "related" }
  | { type: "SET_DERIVE_REQUEST"; parent: string | null }
  | { type: "SET_NEW_SYMBOL_OPEN"; open: boolean }
  | { type: "SET_PENDING_PIN"; pin: LibrarySymbolPin | null }
  /** The pending pin has been added: its number becomes the next click's starting point (`g_lastPin`, `PIN_PLACED` only on success). */
  | { type: "PIN_PLACED"; pin: LibrarySymbolPin }
  | { type: "SET_COPIED_ITEMS"; items: { pins: LibrarySymbolPin[]; graphics: LibrarySymbolGraphic[] } | null }
  /** `g_LastPin*`: the look the next pin starts with. */
  | { type: "SET_PIN_TEMPLATE"; pin: PinTemplate }
  | { type: "SET_UPDATE_FIELDS_OPEN"; open: boolean }
  | { type: "SET_TEXT_DIALOG"; at: [number, number] | null }
  | { type: "SET_PENDING_TEXT"; pending: { text: string; sizeMm: number; angleDeg: number } | null }
  /** Opens Text Box Properties on a box (`id` set: an existing one), or closes it. A box the tool drew (`id` null) is added by the dialog's OK, which also makes its looks the next box's. */
  | { type: "SET_TEXT_BOX_DIALOG"; dialog: { id: string | null; box: LibTextBox } | null }
  /** `m_lastTextBold`, `m_lastTextItalic`, `m_lastTextAngle`, `m_lastTextJust`, `m_lastFillStyle`, `m_lastStroke`: the looks of the box the tool has just added, for the next. */
  | { type: "SET_LAST_TEXT_BOX"; last: LastTextBox }
  | { type: "SET_SHAPE_DIALOG_ID"; id: string | null }
  | { type: "TOAST"; message: string; kind: "error" | "info" }
  | { type: "TOAST_CLEAR" };

function reducer(state: SymbolEditorState, action: SymAction): SymbolEditorState {
  switch (action.type) {
    case "SET_LIB_ID":
      if (action.libId === state.libId) return state;
      // The view options, the library tree's selection and the clipboard belong to the editor session, not to one symbol.
      return {
        ...initialState,
        libId: action.libId,
        gridNm: state.gridNm,
        gridVisible: state.gridVisible,
        treeSelection: state.treeSelection,
        showElectricalTypes: state.showElectricalTypes,
        showPinNumbers: state.showPinNumbers,
        showHiddenPins: state.showHiddenPins,
        showHiddenFields: state.showHiddenFields,
        showPinAltIcons: state.showPinAltIcons,
        copied: state.copied,
        copiedItems: state.copiedItems,
        lastTextAngle: state.lastTextAngle,
        lastTextBox: state.lastTextBox,
        // `g_LastPin*` is static in KiCad: the look carries from one symbol to the next.
        pinTemplate: state.pinTemplate,
      };
    case "SYMBOL_OK":
      // `m_SyncPinEdit = IsMultiUnit() && !UnitsLocked()` each time a symbol is loaded: on for a multi-unit symbol whose units are interchangeable.
      return {
        ...state,
        symbol: action.symbol,
        derivedOwn: action.own ?? null,
        error: null,
        syncPins: state.symbol ? state.syncPins : defaultSyncMode(action.symbol.unit_count, !!action.symbol.units_locked),
        // `SetDrawSpecificUnit( symbol->UnitsLocked() )` when a symbol is loaded
        drawSpecificUnit: state.symbol ? state.drawSpecificUnit : !!action.symbol.units_locked,
      };
    case "SYMBOL_ERR":
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
      return { ...state, selection: new Set(), activeTool: "select", drawState: null, movePreview: null, pinPropertiesId: null };
    case "ESCAPE": {
      // `IsCancelInteractive()` in the drawing tools: with a shape (or the text to place) in hand, Escape drops that and the tool stays armed;
      // only without one does it leave the tool.
      if (state.drawState != null || state.pendingText != null) return { ...state, drawState: null, pendingText: null, textDialog: null };
      const inProgress = state.activeTool !== "select" || state.movePreview != null;
      if (inProgress) return { ...state, activeTool: "select", drawState: null, movePreview: null };
      return { ...state, selection: new Set() };
    }
    case "SET_ACTIVE_TOOL":
      return { ...state, activeTool: action.tool, drawState: null, pendingText: null, textDialog: null };
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
    case "TOGGLE_GRID_VISIBLE":
      return { ...state, gridVisible: !state.gridVisible };
    case "SET_ACTIVE_UNIT":
      return { ...state, activeUnit: action.unit };
    case "SET_ACTIVE_BODY_STYLE":
      return { ...state, activeBodyStyle: action.style };
    case "SET_PIN_PROPERTIES_ID":
      return { ...state, pinPropertiesId: action.id };
    case "SET_PIN_TABLE_OPEN":
      return { ...state, pinTableOpen: action.open };
    case "SET_PROPERTIES_OPEN":
      return { ...state, propertiesOpen: action.open, propertiesField: action.open ? (action.field ?? null) : null };
    case "SET_TREE_SELECTION":
      return { ...state, treeSelection: action.names };
    case "TOGGLE_ELECTRICAL_TYPES":
      return { ...state, showElectricalTypes: !state.showElectricalTypes };
    case "TOGGLE_PIN_NUMBERS":
      return { ...state, showPinNumbers: !state.showPinNumbers };
    case "TOGGLE_HIDDEN_PINS":
      return { ...state, showHiddenPins: !state.showHiddenPins };
    case "TOGGLE_HIDDEN_FIELDS":
      return { ...state, showHiddenFields: !state.showHiddenFields };
    case "TOGGLE_PIN_ALT_ICONS":
      return { ...state, showPinAltIcons: !state.showPinAltIcons };
    case "SET_SYNC_PINS":
      return { ...state, syncPins: action.on };
    case "SET_DRAW_SPECIFIC":
      return { ...state, drawSpecificUnit: action.unit ?? state.drawSpecificUnit, drawSpecificBodyStyle: action.bodyStyle ?? state.drawSpecificBodyStyle };
    case "SET_COPIED":
      return { ...state, copied: action.copied };
    case "SET_SAVE_AS":
      return { ...state, saveAs: action.request };
    case "SET_IMPORT_REQUEST":
      return { ...state, importRequest: action.request };
    case "SET_FIELDS_TABLE_OPEN":
      return { ...state, fieldsTableOpen: action.open, fieldsTableScope: action.scope ?? state.fieldsTableScope };
    case "SET_DERIVE_REQUEST":
      return { ...state, deriveRequest: action.parent ? { parent: action.parent } : null };
    case "SET_NEW_SYMBOL_OPEN":
      return { ...state, newSymbolOpen: action.open };
    case "SET_PENDING_PIN":
      return { ...state, pendingPin: action.pin };
    case "PIN_PLACED":
      return { ...state, lastPlacedPin: { libId: state.libId, pin: placedPinWithId(action.pin, state.symbol?.pins ?? []) } };
    case "SET_COPIED_ITEMS":
      return { ...state, copiedItems: action.items };
    case "SET_PIN_TEMPLATE":
      // The poll re-reads the selected pin every ~700ms: a template it already has must not re-render the editor.
      return samePinTemplate(state.pinTemplate, action.pin) ? state : { ...state, pinTemplate: action.pin };
    case "SET_UPDATE_FIELDS_OPEN":
      return { ...state, updateFieldsOpen: action.open };
    case "SET_TEXT_DIALOG":
      return { ...state, textDialog: action.at ? { atNm: action.at } : null };
    case "SET_TEXT_BOX_DIALOG":
      return { ...state, textBoxDialog: action.dialog, drawState: null };
    case "SET_LAST_TEXT_BOX":
      return { ...state, lastTextBox: action.last };
    case "SET_SHAPE_DIALOG_ID":
      return { ...state, shapeDialogId: action.id };
    case "SET_PENDING_TEXT":
      return { ...state, pendingText: action.pending, textDialog: null, lastTextAngle: action.pending ? action.pending.angleDeg : state.lastTextAngle };
    case "TOAST":
      return { ...state, toast: { message: action.message, kind: action.kind } };
    case "TOAST_CLEAR":
      return { ...state, toast: null };
    default:
      return state;
  }
}

/**
 * `DIALOG_LIB_NEW_SYMBOL` as `CreateNewSymbol` asks it with no parent (`NewSymbolDialog.tsx`): the name and the properties the model
 * has. "Pin name inside" is already folded into `pinNameOffsetMm` (`CreateSymbol` keeps the offset when it is inside -- 20 mils for one
 * of 0 -- and writes 0 when it is outside), and a parent symbol is the Derive dialog's question, so neither is a field here.
 */
export interface NewSymbolRequest {
  /** `GetName()`'s form: trimmed, spaces to underscores, LIB_ID specials escaped -- `newSymbolNameCheck` makes it. */
  name: string;
  /** "Default reference designator" (KiCad's "U"). */
  reference: string;
  /** "Number of units per package" (1..64). */
  unitCount: number;
  /** "All units are interchangeable" (`units_locked` is the opposite, and only for a symbol of several units). KiCad's dialog starts it
   * unchecked, so a multi-unit symbol's units are locked unless the person checks it. */
  unitsInterchangeable: boolean;
  /** "Create symbol with alternate body style (De Morgan)". */
  alternateBodyStyle: boolean;
  /** "Create symbol as power symbol" (it also excludes the symbol from the BOM and the board). */
  power: boolean;
  excludeFromBom: boolean;
  excludeFromBoard: boolean;
  /** "Pin name position offset", in mm (KiCad's dialog shows mils; the studio shows the user's unit), as `CreateSymbol` settles it from
   * "Pin name inside": the offset when inside, 20 mils for an inside offset of 0, and 0 when outside. */
  pinNameOffsetMm: number;
  /** "Show pin number text" / "Show pin name text" (`pin_numbers_hidden`/`pin_names_hidden` are the opposites). */
  showPinNumbers: boolean;
  showPinNames: boolean;
}

export interface SymbolEditorApi {
  /** The live state, read at call time (a hotkey handler's closure may be a render behind). */
  getState: () => SymbolEditorState;
  /** Open (or re-open -- a no-op server-side if already open) a symbol by lib_id and start polling it. */
  openSymbol: (libId: string) => Promise<void>;
  /** `SYMBOL_EDIT_FRAME::CreateNewSymbol`'s `LIB_SYMBOL_LIBRARY_MANAGER::CreateNewSymbol`: a new, empty symbol in the project library,
   * one undo step (the `new_symbol` verb and its first properties), opened in the editor. Returns whether the verb accepted it (a name
   * the library has is refused; the dialog has already checked, the verb is the authority). */
  newSymbol: (request: NewSymbolRequest) => Promise<boolean>;
  closeSymbol: () => void;
  refresh: () => Promise<void>;
  /** Any Cmd this file doesn't have a named wrapper for. Returns whether the backend accepted it. */
  cmd: (c: Cmd) => Promise<boolean>;
  undo: () => Promise<void>;
  redo: () => Promise<void>;
  pinById: (id: string) => LibrarySymbolPin | undefined;
  graphicById: (id: string) => LibrarySymbolGraphic | undefined;
  /** `x`/`y` and every other geometry argument below is plain mm, matching the wire format directly -- callers (the canvas) convert from internal µm themselves, see this file's own unit-convention doc. */
  addPin: (pin: LibrarySymbolPin) => Promise<boolean>;
  movePin: (id: string, xMm: number, yMm: number) => Promise<void>;
  /** Several pins moved to absolute positions as ONE undo step; in Synchronized Pins Mode the matching pins of the other units move with them. */
  movePins: (moves: { id: string; x: number; y: number }[]) => Promise<void>;
  deletePin: (id: string) => Promise<void>;
  /** `SYMBOL_EDITOR_EDIT_TOOL::DoDelete`: the given pins and graphics, with the twins of the pins in Synchronized Pins Mode, as ONE undo step (kicad-port/symEditItems.ts). */
  deleteItems: (ids: readonly string[]) => Promise<void>;
  /** `SYMBOL_EDITOR_EDIT_TOOL::Rotate`: the selected pins a quarter turn about one point, one undo step (`rotatePlan`). */
  rotateSelection: (ccw: boolean) => Promise<void>;
  /** `SYMBOL_EDITOR_DRAWING_TOOLS::RepeatDrawItem` -> `RepeatPin` (Insert): the last placed pin again, one step on, numbered and named one up. */
  repeatLastPin: () => Promise<void>;
  /** `SYMBOL_EDITOR_EDIT_TOOL::Duplicate` (Ctrl+D): the selected pins and graphics copied in place, numbered `GetMaxPinNumber() + 1`. */
  duplicateSelection: () => Promise<void>;
  /** `SYMBOL_EDITOR_EDIT_TOOL::Copy` / `Cut` (Ctrl+C / Ctrl+X): the selected pins and graphics onto the editor's clipboard (Cut then deletes them). */
  copySelection: (cut: boolean) => Promise<void>;
  /** `SYMBOL_EDITOR_EDIT_TOOL::Paste` (Ctrl+V): the clipboard's items on the unit and body style being edited. */
  pasteItems: () => Promise<void>;
  editPin: (id: string, pin: LibrarySymbolPin) => Promise<boolean>;
  pushPinProperty: (sourcePinId: string, field: PushPinField) => Promise<void>;
  addGraphic: (graphic: LibrarySymbolGraphic) => Promise<void>;
  moveGraphic: (id: string, dxMm: number, dyMm: number) => Promise<void>;
  /** A drag of fields on the canvas (`SCH_FIELD`s moved by the move tool): the named fields of the symbol move by the offset, one undo step. A field the symbol inherits becomes its own. */
  moveFields: (names: string[], dxMm: number, dyMm: number) => Promise<void>;
  editGraphic: (id: string, strokeMm: number, fill: LibraryFill, scope?: { unit?: number; bodyStyle?: number }) => Promise<boolean>;
  /** A text box's dialog: the whole graphic as it was left (`replace_symbol_graphic`). */
  replaceGraphic: (id: string, graphic: LibrarySymbolGraphic) => Promise<boolean>;
  deleteGraphic: (id: string) => Promise<void>;
  editText: (id: string, text: string, angleDeg: number, sizeMm: number) => Promise<void>;
  editProperties: (fields: SymbolPropertiesFields) => Promise<boolean>;
  updateOnBoard: () => Promise<void>;
  deleteSymbol: () => Promise<void>;
  exportKicadSym: () => Promise<void>;
  /** `eeschema.SymbolLibraryControl.saveLibraryAs` (Ctrl+Shift+S): the whole project symbol library as one `.kicad_sym` download. */
  exportLibraryKicadSym: () => Promise<void>;
}

const SymStateContext = createContext<SymbolEditorState | null>(null);
const SymDispatchContext = createContext<React.Dispatch<SymAction> | null>(null);
const SymApiContext = createContext<SymbolEditorApi | null>(null);

export function SymbolEditorProvider({ children }: { children: React.ReactNode }) {
  const [state, dispatch] = useReducer(reducer, initialState);
  const stateRef = useRef(state);
  stateRef.current = state;

  const refresh = useCallback(async () => {
    const libId = stateRef.current.libId;
    if (!libId) return;
    try {
      const { symbol, own } = await fetchSymbolForEditor(libId);
      dispatch({ type: "SYMBOL_OK", symbol, own });
    } catch (e) {
      dispatch({ type: "SYMBOL_ERR", message: e instanceof Error ? e.message : String(e) });
    }
  }, []);

  // Poll while a symbol is open, same ~700ms cadence the Footprint
  // Editor's own poll uses.
  useEffect(() => {
    if (!state.libId) return;
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
  }, [state.libId, refresh]);

  const runCmd = useCallback(
    async (cmd: Cmd) => {
      const reply = await postCmd(cmd, false); // strict has no meaning here -- symbol-library edits never touch board placement/routing gates
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "error" });
      await refresh();
      return reply.ok;
    },
    [refresh]
  );

  const api: SymbolEditorApi = {
    getState: () => stateRef.current,
    openSymbol: async (libId) => {
      dispatch({ type: "SET_LIB_ID", libId });
      await ensureSymbolInProject(libId); // an installed KiCad symbol is copied into the project library first, or the verb below would open a blank one
      await postCmd({ op: "open_symbol_for_edit", lib_id: libId }, false);
      const { symbol, own } = await fetchSymbolForEditor(libId);
      dispatch({ type: "SYMBOL_OK", symbol, own });
    },
    newSymbol: async (request) => {
      const libId = joinLibName(PROJECT_SYMBOL_LIB, request.name);
      // `CreateNewSymbol` makes the symbol and then sets what the dialog asked (`NEW_SYMBOL_PROPERTIES`) as one edit: the batch is one
      // undo step, and a name the verb refuses rolls the whole thing back.
      const reply = await postCmd(
        {
          op: "batch",
          cmds: [
            { op: "new_symbol", lib_id: libId },
            {
              op: "edit_symbol_properties",
              lib_id: libId,
              reference_prefix: request.reference,
              description: "",
              keywords: "",
              datasheet: "",
              power: request.power,
              in_bom: !request.excludeFromBom,
              on_board: !request.excludeFromBoard,
              pin_numbers_hidden: !request.showPinNumbers,
              pin_names_hidden: !request.showPinNames,
              pin_name_offset_mm: request.pinNameOffsetMm,
              unit_count: request.unitCount,
              has_alternate_body_style: request.alternateBodyStyle,
              footprint_filters: [],
              // "All units are interchangeable" unchecked; only ever set for a symbol of several units
              units_locked: request.unitCount > 1 && !request.unitsInterchangeable,
            },
          ],
        },
        false
      );
      if (!reply.ok) {
        dispatch({ type: "TOAST", message: reply.message, kind: "error" });
        return false;
      }
      dispatch({ type: "SET_LIB_ID", libId });
      const { symbol, own } = await fetchSymbolForEditor(libId);
      dispatch({ type: "SYMBOL_OK", symbol, own });
      return true;
    },
    closeSymbol: () => dispatch({ type: "SET_LIB_ID", libId: null }),
    refresh,
    cmd: (c) => runCmd(c),
    undo: async () => {
      dispatch({ type: "CLEAR_SELECTION" });
      const reply = await postUndo("symbol_editor");
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "info" });
      await refresh();
    },
    redo: async () => {
      dispatch({ type: "CLEAR_SELECTION" });
      const reply = await postRedo("symbol_editor");
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "info" });
      await refresh();
    },
    pinById: (id) => stateRef.current.symbol?.pins.find((p) => p.id === id),
    graphicById: (id) => stateRef.current.symbol?.graphics.find((g) => g.id === id),
    addPin: async (pin) => {
      const st = stateRef.current;
      const libId = st.libId;
      if (!libId) return false;
      // `PlacePin` -> `CreateImagePins`: in Synchronized Pins Mode a pin placed on one unit gets its image in every other unit.
      const images = st.symbol && synchronizePins(st.syncPins, st.symbol.unit_count, !!st.symbol.units_locked) ? imagePinsFor(pin, st.symbol.unit_count) : [];
      const cmds: Cmd[] = [pin, ...images].map((p): Cmd => ({ op: "add_symbol_pin", lib_id: libId, pin: p }));
      return runCmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
    },
    movePin: async (id, xMm, yMm) => {
      const libId = stateRef.current.libId;
      if (!libId) return;
      await runCmd({ op: "move_symbol_pin", lib_id: libId, id, x: xMm, y: yMm });
    },
    movePins: async (moves) => {
      const st = stateRef.current;
      const libId = st.libId;
      const sym = st.symbol;
      if (!libId || !sym || moves.length === 0) return;
      // `SYMBOL_EDITOR_MOVE_TOOL::Main`: "Pick up any synchronized pins" -- the matching pin of every other unit moves with the pin.
      const sync = synchronizePins(st.syncPins, sym.unit_count, !!sym.units_locked);
      const target = new Map<string, { x: number; y: number }>(moves.map((m) => [m.id, { x: m.x, y: m.y }]));
      if (sync) {
        for (const m of moves) {
          const pin = sym.pins.find((p) => p.id === m.id);
          if (!pin) continue;
          for (const other of linkedPinsToMove(pin, sym.pins, sym.unit_count)) if (other.id && !target.has(other.id)) target.set(other.id, { x: m.x, y: m.y });
        }
      }
      const cmds: Cmd[] = [...target.entries()].map(([id, at]): Cmd => ({ op: "move_symbol_pin", lib_id: libId, id, x: at.x, y: at.y }));
      await runCmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
    },
    deletePin: async (id) => {
      const libId = stateRef.current.libId;
      if (!libId) return;
      dispatch({ type: "CLEAR_SELECTION" });
      await runCmd({ op: "delete_symbol_pin", lib_id: libId, id });
    },
    deleteItems: async (ids) => {
      const st = stateRef.current;
      const libId = st.libId;
      const sym = st.symbol;
      if (!libId || !sym || ids.length === 0) return;
      const plan = deletePlan(ids, sym.pins, sym.graphics, synchronizePins(st.syncPins, sym.unit_count, !!sym.units_locked), sym.unit_count);
      const cmds: Cmd[] = [...plan.pins.map((id): Cmd => ({ op: "delete_symbol_pin", lib_id: libId, id })), ...plan.graphics.map((id): Cmd => ({ op: "delete_symbol_graphic", lib_id: libId, id }))];
      dispatch({ type: "CLEAR_SELECTION" });
      if (cmds.length > 0) await runCmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
    },
    rotateSelection: async (ccw) => {
      const st = stateRef.current;
      const libId = st.libId;
      const sym = st.symbol;
      if (!libId || !sym || st.selection.size === 0) return;
      const ids = [...st.selection];
      const pins = ids.map((id) => sym.pins.find((p) => p.id === id)).filter((p): p is LibrarySymbolPin => !!p);
      const turned = rotatePlan(pins, ids.length, ccw, st.gridNm / 1e6);
      const cmds: Cmd[] = turned.map((p): Cmd => ({ op: "edit_symbol_pin", lib_id: libId, id: p.id!, pin: p }));
      if (cmds.length > 0) await runCmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
    },
    repeatLastPin: async () => {
      const st = stateRef.current;
      const libId = st.libId;
      const sym = st.symbol;
      const last = st.lastPlacedPin;
      if (!libId || !sym || !last || last.libId !== libId) return;
      // `RepeatDrawItem` finds `g_lastPin` in the symbol by UUID (`test->m_Uuid == g_lastPin`); a pin deleted since is not repeated.
      const source = last.pin.id ? sym.pins.find((p) => p.id === last.pin.id) : undefined;
      if (!source) return;
      const pin = repeatPin(source);
      // `PlacePin`: the occupied-spot question in sync mode, then the images in the other units -- as the Pin tool's placement does.
      const syncMode = synchronizePins(st.syncPins, sym.unit_count, !!sym.units_locked);
      const occupant = pinPlacementOccupant(pin, sym.pins, syncMode);
      if (occupant && !(await askConfirm({ title: "Confirmation", message: pinOccupiedMessage(occupant.unit), okLabel: "Place Pin Anyway" }))) return;
      const images = syncMode ? imagePinsFor(pin, sym.unit_count) : [];
      const cmds: Cmd[] = [pin, ...images].map((p): Cmd => ({ op: "add_symbol_pin", lib_id: libId, pin: p }));
      dispatch({ type: "CLEAR_SELECTION" });
      if (await runCmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds })) {
        dispatch({ type: "PIN_PLACED", pin });
        // `PlacePin`: a new pin that was not pasted leaves its orientation, type and shape for the next one (`g_LastPinOrient`, `g_LastPinType`,
        // `g_LastPinShape`); Repeat's pin is `IS_NEW`.
        dispatch({ type: "SET_PIN_TEMPLATE", pin: { ...st.pinTemplate, angle_deg: pin.angle_deg, electrical_type: pin.electrical_type, shape: pin.shape } });
      }
    },
    duplicateSelection: async () => {
      const st = stateRef.current;
      const libId = st.libId;
      const sym = st.symbol;
      if (!libId || !sym || st.selection.size === 0) return;
      const d = duplicateItems([...st.selection], sym.pins, sym.graphics);
      const cmds: Cmd[] = [...d.pins.map((pin): Cmd => ({ op: "add_symbol_pin", lib_id: libId, pin })), ...d.graphics.map((graphic): Cmd => ({ op: "add_symbol_graphic", lib_id: libId, graphic }))];
      dispatch({ type: "CLEAR_SELECTION" });
      if (cmds.length > 0) await runCmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
    },
    copySelection: async (cut) => {
      const st = stateRef.current;
      const sym = st.symbol;
      if (!sym || st.selection.size === 0) return;
      const ids = [...st.selection];
      const pins = sym.pins.filter((p) => p.id && ids.includes(p.id));
      const graphics = sym.graphics.filter((g) => g.id && ids.includes(g.id));
      if (pins.length + graphics.length === 0) return;
      dispatch({ type: "SET_COPIED_ITEMS", items: { pins, graphics } });
      if (cut) await api.deleteItems(ids);
    },
    pasteItems: async () => {
      const st = stateRef.current;
      const libId = st.libId;
      const sym = st.symbol;
      // `Paste`: nothing on a derived symbol (`symbol->IsDerived()`)
      if (!libId || !sym || !st.copiedItems || st.derivedOwn) return;
      const items = pastedItems(st.copiedItems, st.activeUnit, st.activeBodyStyle);
      const cmds: Cmd[] = [...items.pins.map((pin): Cmd => ({ op: "add_symbol_pin", lib_id: libId, pin })), ...items.graphics.map((graphic): Cmd => ({ op: "add_symbol_graphic", lib_id: libId, graphic }))];
      dispatch({ type: "CLEAR_SELECTION" });
      if (cmds.length > 0) await runCmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
    },
    editPin: async (id, pin) => {
      const st = stateRef.current;
      const libId = st.libId;
      if (!libId) return false;
      const sym = st.symbol;
      const original = sym?.pins.find((p) => p.id === id);
      const cmds: Cmd[] = [{ op: "edit_symbol_pin", lib_id: libId, id, pin }];
      // `EditPinProperties`: with synchronized pins the matching pin of every other unit follows the edit (and a pin made common to all units
      // makes its twins redundant).
      if (sym && original && synchronizePins(st.syncPins, sym.unit_count, !!sym.units_locked)) {
        const plan = planSyncedEdit(original, { ...pin, id }, sym.pins, sym.unit_count);
        for (const other of plan.updates) if (other.id) cmds.push({ op: "edit_symbol_pin", lib_id: libId, id: other.id, pin: other });
        for (const rid of plan.removeIds) cmds.push({ op: "delete_symbol_pin", lib_id: libId, id: rid });
      }
      return runCmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
    },
    pushPinProperty: async (sourcePinId, field) => {
      const st = stateRef.current;
      const libId = st.libId;
      if (!libId) return;
      await runCmd({ op: "push_pin_property", lib_id: libId, source_pin_id: sourcePinId, field, body_style: st.activeBodyStyle });
    },
    addGraphic: async (graphic) => {
      const libId = stateRef.current.libId;
      if (!libId) return;
      await runCmd({ op: "add_symbol_graphic", lib_id: libId, graphic });
    },
    moveGraphic: async (id, dxMm, dyMm) => {
      const libId = stateRef.current.libId;
      if (!libId) return;
      await runCmd({ op: "move_symbol_graphic", lib_id: libId, id, dx_mm: dxMm, dy_mm: dyMm });
    },
    moveFields: async (names, dxMm, dyMm) => {
      const st = stateRef.current;
      const libId = st.libId;
      const flat = st.symbol;
      if (!libId || !flat || names.length === 0) return;
      // the symbol's own fields (a derived symbol keeps its own list), with an inherited user field that is moved joining them as the symbol's own copy
      const own = allFields(st.derivedOwn ?? flat);
      const moving = new Set(names);
      for (const f of allFields(flat)) {
        if (!moving.has(f.name)) continue;
        const i = own.findIndex((o) => o.name === f.name);
        // a field the symbol inherits (a parent's user field, or a mandatory one it leaves empty) is what is drawn, so it is what moves: it becomes the symbol's own
        if (i < 0) own.push(f);
        else if (own[i]!.text === "" && f.text !== "") own[i] = f;
      }
      const fields = movedRows(own, moving, dxMm, dyMm).map(wireField);
      await runCmd({ op: "edit_library_symbol_fields", lib_id: libId, fields });
    },
    editGraphic: async (id, strokeMm, fill, scope) => {
      const libId = stateRef.current.libId;
      if (!libId) return false;
      return runCmd({ op: "edit_symbol_graphic", lib_id: libId, id, stroke_mm: strokeMm, fill, ...(scope?.unit !== undefined ? { unit: scope.unit } : {}), ...(scope?.bodyStyle !== undefined ? { body_style: scope.bodyStyle } : {}) });
    },
    replaceGraphic: async (id, graphic) => {
      const libId = stateRef.current.libId;
      if (!libId) return false;
      return runCmd({ op: "replace_symbol_graphic", lib_id: libId, id, graphic });
    },
    deleteGraphic: async (id) => {
      const libId = stateRef.current.libId;
      if (!libId) return;
      dispatch({ type: "CLEAR_SELECTION" });
      await runCmd({ op: "delete_symbol_graphic", lib_id: libId, id });
    },
    editText: async (id, text, angleDeg, sizeMm) => {
      const libId = stateRef.current.libId;
      if (!libId) return;
      await runCmd({ op: "edit_symbol_text", lib_id: libId, id, text, angle_deg: angleDeg, size_mm: sizeMm });
    },
    editProperties: async (fields) => {
      const libId = stateRef.current.libId;
      if (!libId) return false;
      return runCmd({ op: "edit_symbol_properties", lib_id: libId, ...fields });
    },
    updateOnBoard: async () => {
      const libId = stateRef.current.libId;
      if (!libId) return;
      const ok = await runCmd({ op: "update_symbol_on_board", lib_id: libId });
      dispatch({ type: "TOAST", message: ok ? `${libId}: placed instances now follow this library entry` : "update failed", kind: ok ? "info" : "error" });
    },
    deleteSymbol: async () => {
      const libId = stateRef.current.libId;
      if (!libId) return;
      await postCmd({ op: "delete_library_symbol", lib_id: libId }, false);
      dispatch({ type: "SET_LIB_ID", libId: null });
    },
    exportKicadSym: async () => {
      const libId = stateRef.current.libId;
      if (!libId) return;
      try {
        await downloadSymbolKicadSym(libId);
      } catch (e) {
        dispatch({ type: "TOAST", message: e instanceof Error ? e.message : String(e), kind: "error" });
      }
    },
    exportLibraryKicadSym: async () => {
      try {
        await downloadSymbolLibraryKicadSym();
      } catch (e) {
        dispatch({ type: "TOAST", message: e instanceof Error ? e.message : String(e), kind: "error" });
      }
    },
  };

  return (
    <SymStateContext.Provider value={state}>
      <SymDispatchContext.Provider value={dispatch}>
        <SymApiContext.Provider value={api}>{children}</SymApiContext.Provider>
      </SymDispatchContext.Provider>
    </SymStateContext.Provider>
  );
}

export function useSymState(): SymbolEditorState {
  const ctx = useContext(SymStateContext);
  if (!ctx) throw new Error("useSymState must be used within SymbolEditorProvider");
  return ctx;
}

export function useSymDispatch(): React.Dispatch<SymAction> {
  const ctx = useContext(SymDispatchContext);
  if (!ctx) throw new Error("useSymDispatch must be used within SymbolEditorProvider");
  return ctx;
}

export function useSymApi(): SymbolEditorApi {
  const ctx = useContext(SymApiContext);
  if (!ctx) throw new Error("useSymApi must be used within SymbolEditorProvider");
  return ctx;
}
