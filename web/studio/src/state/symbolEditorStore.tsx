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
// **down**) -- `gridUm`/`cursorUm`/`moveOriginUm`/`SymMovePreview`'s own
// `dxUm`/`dyUm` -- exactly like the Footprint Editor's own store, so
// `SymbolEditorCanvas.tsx` can reuse `kicad-port/view.ts`/`gridHelper.ts`
// and `components/schematic/transform.ts`'s `resolvePin`/`resolveLibPoint`
// (which already do the mm->µm, Y-up->Y-down conversion -- the same
// pipeline a placed instance's own rendering uses) unchanged, converting
// back to mm only at the `Cmd` boundary (`api.movePin`/`addPin`/etc. all
// take plain mm numbers, matching the wire format directly).
import React, { createContext, useCallback, useContext, useEffect, useReducer, useRef } from "react";
import type { Cmd, LibraryFill, LibrarySymbol, LibrarySymbolGraphic, LibrarySymbolPin, PushPinField, SymbolPropertiesFields } from "../api/types";
import { downloadSymbolKicadSym, fetchLibrarySymbol, postCmd, postRedo, postUndo } from "../api/client";
import type { ViewTransform } from "./store";

export type SymToolId = "select" | "move" | "pin" | "draw_segment" | "draw_arc" | "draw_rect" | "draw_circle" | "draw_polygon" | "text";

export const SYM_TOOL_MESSAGES: Record<SymToolId, string> = {
  select: "Select item(s)",
  move: "Move item(s)",
  pin: "Pin: click to place, number auto-increments",
  draw_segment: "Line: click start, then end",
  draw_arc: "Arc: click start, mid, then end",
  draw_rect: "Rectangle: click one corner, then the opposite one",
  draw_circle: "Circle: click center, then a point on the edge",
  draw_polygon: "Polygon: click points, Enter/double-click to finish, Esc to cancel",
  text: "Click to place text",
};

/** Same "click to add point(s), commit on the last one" shape the Footprint Editor's own `FpDrawState` uses. Points are internal-space µm (see this file's own unit-convention doc). */
export type SymDrawState = { kind: "shape"; shapeKind: "segment" | "arc" | "rect" | "circle" | "polygon"; pts: [number, number][] };

export interface SymMovePreview {
  refs: string[];
  kind: "pin" | "graphic";
  dxUm: number;
  dyUm: number;
}

export interface SymbolEditorState {
  /** The symbol currently open, or `null` before anything has been opened this session. */
  libId: string | null;
  /** The last successful GET /api/symbol?lib_id=... for `libId` -- raw mm/Y-up wire shape, untouched. */
  symbol: LibrarySymbol | null;
  error: string | null;
  view: ViewTransform;
  viewInitialized: boolean;
  selection: Set<string>;
  activeTool: SymToolId;
  drawState: SymDrawState | null;
  movePreview: SymMovePreview | null;
  moveOriginUm: { x: number; y: number } | null;
  cursorUm: { x: number; y: number } | null;
  gridUm: number;
  gridVisible: boolean;
  /** Which unit (1-based; 0 = "common to all units") new pins/graphics are placed on. */
  activeUnit: number;
  /** Which body style (1 normal, 2 alternate/DeMorgan) new pins/graphics are placed on -- only selectable once `symbol.has_alternate_body_style`. */
  activeBodyStyle: number;
  /** "E" on a selected pin, or double-click -- which pin's Pin Properties dialog is open. */
  pinPropertiesId: string | null;
  pinTableOpen: boolean;
  propertiesOpen: boolean;
  toast: { message: string; kind: "error" | "info" } | null;
}

const initialState: SymbolEditorState = {
  libId: null,
  symbol: null,
  error: null,
  view: { scale: 0, x: 0, y: 0 },
  viewInitialized: false,
  selection: new Set(),
  activeTool: "select",
  drawState: null,
  movePreview: null,
  moveOriginUm: null,
  cursorUm: null,
  gridUm: 1270, // 1.27mm (50 mil) -- KiCad's own Symbol Editor default grid
  gridVisible: true,
  activeUnit: 1,
  activeBodyStyle: 1,
  pinPropertiesId: null,
  pinTableOpen: false,
  propertiesOpen: false,
  toast: null,
};

export type SymAction =
  | { type: "SET_LIB_ID"; libId: string | null }
  | { type: "SYMBOL_OK"; symbol: LibrarySymbol }
  | { type: "SYMBOL_ERR"; message: string }
  | { type: "SET_VIEW"; view: ViewTransform }
  | { type: "MARK_VIEW_INITIALIZED" }
  | { type: "SET_SELECTION"; refs: string[] }
  | { type: "CLEAR_SELECTION" }
  | { type: "ESCAPE" }
  | { type: "SET_ACTIVE_TOOL"; tool: SymToolId }
  | { type: "SET_DRAW_STATE"; draw: SymDrawState | null }
  | { type: "SET_MOVE_PREVIEW"; preview: SymMovePreview | null }
  | { type: "SET_MOVE_ORIGIN"; at: { x: number; y: number } | null }
  | { type: "SET_CURSOR"; at: { x: number; y: number } | null }
  | { type: "SET_GRID_UM"; um: number }
  | { type: "TOGGLE_GRID_VISIBLE" }
  | { type: "SET_ACTIVE_UNIT"; unit: number }
  | { type: "SET_ACTIVE_BODY_STYLE"; style: number }
  | { type: "SET_PIN_PROPERTIES_ID"; id: string | null }
  | { type: "SET_PIN_TABLE_OPEN"; open: boolean }
  | { type: "SET_PROPERTIES_OPEN"; open: boolean }
  | { type: "TOAST"; message: string; kind: "error" | "info" }
  | { type: "TOAST_CLEAR" };

function reducer(state: SymbolEditorState, action: SymAction): SymbolEditorState {
  switch (action.type) {
    case "SET_LIB_ID":
      if (action.libId === state.libId) return state;
      return { ...initialState, libId: action.libId, gridUm: state.gridUm, gridVisible: state.gridVisible };
    case "SYMBOL_OK":
      return { ...state, symbol: action.symbol, error: null };
    case "SYMBOL_ERR":
      return { ...state, error: action.message };
    case "SET_VIEW":
      return { ...state, view: action.view };
    case "MARK_VIEW_INITIALIZED":
      return { ...state, viewInitialized: true };
    case "SET_SELECTION":
      return { ...state, selection: new Set(action.refs) };
    case "CLEAR_SELECTION":
      return { ...state, selection: new Set(), activeTool: "select", drawState: null, movePreview: null, pinPropertiesId: null };
    case "ESCAPE": {
      const inProgress = state.activeTool !== "select" || state.drawState != null || state.movePreview != null;
      if (inProgress) return { ...state, activeTool: "select", drawState: null, movePreview: null };
      return { ...state, selection: new Set() };
    }
    case "SET_ACTIVE_TOOL":
      return { ...state, activeTool: action.tool, drawState: null };
    case "SET_DRAW_STATE":
      return { ...state, drawState: action.draw };
    case "SET_MOVE_PREVIEW":
      return { ...state, movePreview: action.preview };
    case "SET_MOVE_ORIGIN":
      return { ...state, moveOriginUm: action.at };
    case "SET_CURSOR":
      return { ...state, cursorUm: action.at };
    case "SET_GRID_UM":
      return { ...state, gridUm: action.um };
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
      return { ...state, propertiesOpen: action.open };
    case "TOAST":
      return { ...state, toast: { message: action.message, kind: action.kind } };
    case "TOAST_CLEAR":
      return { ...state, toast: null };
    default:
      return state;
  }
}

export interface SymbolEditorApi {
  /** Open (or re-open -- a no-op server-side if already open) a symbol by lib_id and start polling it. */
  openSymbol: (libId: string) => Promise<void>;
  closeSymbol: () => void;
  refresh: () => Promise<void>;
  /** Any Cmd this file doesn't have a named wrapper for. Returns whether the backend accepted it. */
  cmd: (c: Cmd) => Promise<boolean>;
  undo: () => Promise<void>;
  redo: () => Promise<void>;
  pinById: (id: string) => LibrarySymbolPin | undefined;
  graphicById: (id: string) => LibrarySymbolGraphic | undefined;
  /** `x`/`y` and every other geometry argument below is plain mm, matching the wire format directly -- callers (the canvas) convert from internal µm themselves, see this file's own unit-convention doc. */
  addPin: (pin: LibrarySymbolPin) => Promise<void>;
  movePin: (id: string, xMm: number, yMm: number) => Promise<void>;
  deletePin: (id: string) => Promise<void>;
  editPin: (id: string, pin: LibrarySymbolPin) => Promise<boolean>;
  pushPinProperty: (sourcePinId: string, field: PushPinField) => Promise<void>;
  addGraphic: (graphic: LibrarySymbolGraphic) => Promise<void>;
  moveGraphic: (id: string, dxMm: number, dyMm: number) => Promise<void>;
  editGraphic: (id: string, strokeMm: number, fill: LibraryFill) => Promise<void>;
  deleteGraphic: (id: string) => Promise<void>;
  editText: (id: string, text: string, angleDeg: number, sizeMm: number) => Promise<void>;
  editProperties: (fields: SymbolPropertiesFields) => Promise<boolean>;
  updateOnBoard: () => Promise<void>;
  deleteSymbol: () => Promise<void>;
  exportKicadSym: () => Promise<void>;
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
      const symbol = await fetchLibrarySymbol(libId);
      dispatch({ type: "SYMBOL_OK", symbol });
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
    openSymbol: async (libId) => {
      dispatch({ type: "SET_LIB_ID", libId });
      await postCmd({ op: "open_symbol_for_edit", lib_id: libId }, false);
      const symbol = await fetchLibrarySymbol(libId);
      dispatch({ type: "SYMBOL_OK", symbol });
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
      const libId = stateRef.current.libId;
      if (!libId) return;
      await runCmd({ op: "add_symbol_pin", lib_id: libId, pin });
    },
    movePin: async (id, xMm, yMm) => {
      const libId = stateRef.current.libId;
      if (!libId) return;
      await runCmd({ op: "move_symbol_pin", lib_id: libId, id, x: xMm, y: yMm });
    },
    deletePin: async (id) => {
      const libId = stateRef.current.libId;
      if (!libId) return;
      dispatch({ type: "CLEAR_SELECTION" });
      await runCmd({ op: "delete_symbol_pin", lib_id: libId, id });
    },
    editPin: async (id, pin) => {
      const libId = stateRef.current.libId;
      if (!libId) return false;
      return runCmd({ op: "edit_symbol_pin", lib_id: libId, id, pin });
    },
    pushPinProperty: async (sourcePinId, field) => {
      const libId = stateRef.current.libId;
      if (!libId) return;
      await runCmd({ op: "push_pin_property", lib_id: libId, source_pin_id: sourcePinId, field });
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
    editGraphic: async (id, strokeMm, fill) => {
      const libId = stateRef.current.libId;
      if (!libId) return;
      await runCmd({ op: "edit_symbol_graphic", lib_id: libId, id, stroke_mm: strokeMm, fill });
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
