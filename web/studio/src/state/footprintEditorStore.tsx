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
import type { Cmd, CmdShape, CmdText, FootprintPropertiesFields, LibraryFootprint, LibraryPad, PointXY, Um } from "../api/types";
import { fetchFootprint, postCmd, postRedo, postUndo } from "../api/client";
import type { ViewTransform } from "./store";

export type FpToolId = "select" | "move" | "pad" | "draw_segment" | "draw_arc" | "draw_rect" | "draw_circle" | "text";

export const FP_TOOL_MESSAGES: Record<FpToolId, string> = {
  select: "Select item(s)",
  move: "Move item(s)",
  pad: "Pad: click to place, number auto-increments",
  draw_segment: "Line: click start, then end",
  draw_arc: "Arc: click start, mid, then end",
  draw_rect: "Rectangle: click one corner, then the opposite one",
  draw_circle: "Circle: click center, then a point on the edge",
  text: "Click to place text",
};

/** Same "click to add a point(s), commit on the last one" shape the PCB tab's own `DrawState` uses, narrowed to what the footprint editor's graphics tools need (no route/zone/measure concept here). */
export type FpDrawState = { kind: "shape"; shapeKind: "segment" | "arc" | "rect" | "circle"; pts: [Um, Um][] };

export interface FpMovePreview {
  refs: string[];
  kind: "pad" | "graphic" | "text";
  dxUm: number;
  dyUm: number;
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
  selection: Set<string>;
  activeTool: FpToolId;
  drawState: FpDrawState | null;
  movePreview: FpMovePreview | null;
  moveOriginUm: { x: number; y: number } | null;
  cursorUm: { x: number; y: number } | null;
  gridUm: number;
  gridVisible: boolean;
  /** Which of F.SilkS/F.Fab/F.CrtYd a graphics/text tool draws on. */
  activeLayer: string;
  /** "E" on a selected pad, or double-click -- which pad's Pad Properties dialog is open. */
  padPropertiesId: string | null;
  footprintPropertiesOpen: boolean;
  renumberDialogOpen: boolean;
  /** `pad_tool.cpp`'s Copy Pad Properties (Cmd+C on a selected pad) -- client-side only, same spirit as the PCB tab's own `state.clipboard`. Paste (`pastePadProperties`) applies it to every pad in the current selection via `edit_pad`. */
  copiedPadProps: Partial<LibraryPad> | null;
  toast: { message: string; kind: "error" | "info" } | null;
}

const initialState: FootprintEditorState = {
  name: null,
  footprint: null,
  error: null,
  view: { scale: 0, x: 0, y: 0 },
  viewInitialized: false,
  selection: new Set(),
  activeTool: "select",
  drawState: null,
  movePreview: null,
  moveOriginUm: null,
  cursorUm: null,
  gridUm: 250, // 0.25mm -- a reasonable footprint-editor-scale default (KiCad's own fp-editor grid list starts finer than the board editor's)
  gridVisible: true,
  activeLayer: "F.SilkS",
  padPropertiesId: null,
  footprintPropertiesOpen: false,
  renumberDialogOpen: false,
  copiedPadProps: null,
  toast: null,
};

export type FpAction =
  | { type: "SET_NAME"; name: string | null }
  | { type: "FOOTPRINT_OK"; footprint: LibraryFootprint }
  | { type: "FOOTPRINT_ERR"; message: string }
  | { type: "SET_VIEW"; view: ViewTransform }
  | { type: "MARK_VIEW_INITIALIZED" }
  | { type: "SET_SELECTION"; refs: string[] }
  | { type: "CLEAR_SELECTION" }
  | { type: "ESCAPE" }
  | { type: "SET_ACTIVE_TOOL"; tool: FpToolId }
  | { type: "SET_DRAW_STATE"; draw: FpDrawState | null }
  | { type: "SET_MOVE_PREVIEW"; preview: FpMovePreview | null }
  | { type: "SET_MOVE_ORIGIN"; at: { x: number; y: number } | null }
  | { type: "SET_CURSOR"; at: { x: number; y: number } | null }
  | { type: "SET_GRID_UM"; um: number }
  | { type: "TOGGLE_GRID_VISIBLE" }
  | { type: "SET_ACTIVE_LAYER"; layer: string }
  | { type: "SET_PAD_PROPERTIES_ID"; id: string | null }
  | { type: "SET_FOOTPRINT_PROPERTIES_OPEN"; open: boolean }
  | { type: "SET_RENUMBER_DIALOG_OPEN"; open: boolean }
  | { type: "SET_COPIED_PAD_PROPS"; props: Partial<LibraryPad> | null }
  | { type: "TOAST"; message: string; kind: "error" | "info" }
  | { type: "TOAST_CLEAR" };

function reducer(state: FootprintEditorState, action: FpAction): FootprintEditorState {
  switch (action.type) {
    case "SET_NAME":
      if (action.name === state.name) return state;
      return { ...initialState, name: action.name, gridUm: state.gridUm, gridVisible: state.gridVisible };
    case "FOOTPRINT_OK":
      return { ...state, footprint: action.footprint, error: null };
    case "FOOTPRINT_ERR":
      return { ...state, error: action.message };
    case "SET_VIEW":
      return { ...state, view: action.view };
    case "MARK_VIEW_INITIALIZED":
      return { ...state, viewInitialized: true };
    case "SET_SELECTION":
      return { ...state, selection: new Set(action.refs) };
    case "CLEAR_SELECTION":
      return { ...state, selection: new Set(), activeTool: "select", drawState: null, movePreview: null, padPropertiesId: null };
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
    case "SET_ACTIVE_LAYER":
      return { ...state, activeLayer: action.layer };
    case "SET_PAD_PROPERTIES_ID":
      return { ...state, padPropertiesId: action.id };
    case "SET_FOOTPRINT_PROPERTIES_OPEN":
      return { ...state, footprintPropertiesOpen: action.open };
    case "SET_RENUMBER_DIALOG_OPEN":
      return { ...state, renumberDialogOpen: action.open };
    case "SET_COPIED_PAD_PROPS":
      return { ...state, copiedPadProps: action.props };
    case "TOAST":
      return { ...state, toast: { message: action.message, kind: action.kind } };
    case "TOAST_CLEAR":
      return { ...state, toast: null };
    default:
      return state;
  }
}

export interface FootprintEditorApi {
  /** Open (or re-open -- a no-op server-side if already open) a footprint by name and start polling it. */
  openFootprint: (name: string) => Promise<void>;
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
  movePad: (id: string, x: Um, y: Um) => Promise<void>;
  rotatePad: (id: string, quarterTurns: number) => Promise<void>;
  deletePad: (id: string) => Promise<void>;
  editPad: (id: string, pad: LibraryPad) => Promise<boolean>;
  renumberPads: (start: number, prefix: string, step: number) => Promise<void>;
  pushPadProperties: (sourcePadId: string, filters: { shape: boolean; orientation: boolean; layers: boolean; type: boolean }) => Promise<void>;
  /** `pad_tool.cpp`'s Copy Pad Properties -- stores the pad's full field set (minus id/number/position) client-side. */
  copyPadProperties: (id: string) => void;
  /** Paste Pad Properties -- applies the copied fields to every pad in `ids` via `edit_pad`. */
  pastePadProperties: (ids: string[]) => Promise<void>;
  addGraphic: (shape: CmdShape) => Promise<void>;
  moveGraphic: (id: string, dx: Um, dy: Um) => Promise<void>;
  editGraphic: (id: string, layer: string, strokeWidth: Um, filled: boolean) => Promise<void>;
  deleteGraphic: (id: string) => Promise<void>;
  addText: (text: CmdText) => Promise<void>;
  moveText: (id: string, x: Um, y: Um) => Promise<void>;
  editText: (id: string, fields: Omit<CmdText, "id" | "at">) => Promise<void>;
  deleteText: (id: string) => Promise<void>;
  setAnchor: (at: PointXY) => Promise<void>;
  editProperties: (fields: FootprintPropertiesFields) => Promise<boolean>;
  updateOnBoard: () => Promise<void>;
  deleteFootprint: () => Promise<void>;
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

  const runCmd = useCallback(
    async (cmd: Cmd) => {
      const reply = await postCmd(cmd, false); // strict has no meaning here -- footprint-library edits never touch board placement/routing gates
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "error" });
      await refresh();
      return reply.ok;
    },
    [refresh]
  );

  const api: FootprintEditorApi = {
    openFootprint: async (name) => {
      dispatch({ type: "SET_NAME", name });
      await postCmd({ op: "open_footprint_for_edit", name }, false);
      const footprint = await fetchFootprint(name);
      dispatch({ type: "FOOTPRINT_OK", footprint });
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
      // Mirrors pad_tool.cpp's ImportSettingsFrom scope: shape/size/drill/
      // layers/overrides, never number/position/rotation -- see
      // pastePadProperties's own doc.
      const props: Partial<LibraryPad> = {
        offset: pad.offset,
        size: pad.size,
        shape: pad.shape,
        kind: pad.kind,
        drill: pad.drill,
        drill_slot: pad.drill_slot,
        roundrect_ratio: pad.roundrect_ratio,
        trapezoid_delta: pad.trapezoid_delta,
        chamfer_ratio: pad.chamfer_ratio,
        chamfer_corners: pad.chamfer_corners,
        layers: pad.layers,
        clearance_override: pad.clearance_override,
        thermal_gap_override: pad.thermal_gap_override,
        thermal_spoke_width_override: pad.thermal_spoke_width_override,
      };
      dispatch({ type: "SET_COPIED_PAD_PROPS", props });
    },
    pastePadProperties: async (ids) => {
      const name = stateRef.current.name;
      const copied = stateRef.current.copiedPadProps;
      if (!name || !copied) return;
      for (const id of ids) {
        const current = stateRef.current.footprint?.pads.find((p) => p.id === id);
        if (!current) continue;
        await runCmd({ op: "edit_pad", footprint: name, id, pad: { ...current, ...copied } });
      }
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
      if (!name) return;
      await runCmd({ op: "add_footprint_text", footprint: name, text });
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
    setAnchor: async (at) => {
      const name = stateRef.current.name;
      if (!name) return;
      await runCmd({ op: "set_footprint_anchor", name, at });
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
