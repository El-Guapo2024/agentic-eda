// Hand-rolled state container (no state-management library, per the
// project's dependency limits): a useReducer store for pure UI state,
// plus a handful of async functions closed over `dispatch` for anything
// that talks to the backend. Board truth always comes back through the
// /api/version + /api/state poll -- these functions POST a command and
// then nudge the poller to refetch immediately, exactly like the old
// studio.html's `send()`.

import React, { createContext, useCallback, useContext, useEffect, useReducer, useRef } from "react";
import type { BoardState, Part } from "../api/types";
import { fetchState, fetchVersion, postCmd, postRedo, postRoute, postUndo } from "../api/client";
import type { LengthUnit } from "./units";
import { STANDARD_LAYERS } from "../components/canvas/layers";

export type RightDockTab = "appearance" | "filter" | "activity";
export type EditorTab = "pcb" | "schematic";

export interface ViewTransform {
  /** Screen pixels per board µm. */
  scale: number;
  /** Screen-space translation. */
  x: number;
  y: number;
}

/** A part being dragged, previewed locally before `move_to` commits it on drop (see pcb_grid_helper-style snap in canvas/gridHelper.ts). */
export interface MovePreview {
  refs: string[];
  dxUm: number;
  dyUm: number;
}

export interface StudioState {
  board: BoardState | null;
  boardError: string | null;
  version: string | null;

  tab: EditorTab;
  rightDockTab: RightDockTab;

  selection: Set<string>;
  /** Refs to flash/outline because a problem in the panel references them. */
  hot: Set<string>;
  netHighlight: string | null;
  /** An unplaced part ref chosen from the panel, waiting for a canvas click to place it. */
  armed: string | null;
  movePreview: MovePreview | null;

  view: ViewTransform;
  viewInitialized: boolean;
  units: LengthUnit;
  polar: boolean;
  gridUm: number;
  gridVisible: boolean;
  fullscreenCrosshair: boolean;
  showRatsnest: boolean;
  /** Active layer for highlight/contrast (null = all layers equally weighted). */
  activeLayer: string | null;
  highContrast: boolean;
  layerVisible: Record<string, boolean>;
  layerOpacity: Record<string, number>;

  /**
   * pcbnew's selection filter lets a box-select skip whole item classes.
   * Only `footprints` has any effect right now -- tracks/vias aren't
   * individually selectable yet at all (see canvas/Canvas.tsx's
   * `partHit`), so those two toggles are here for the panel's shape but
   * are inert until track/via selection exists.
   */
  selectionFilter: { footprints: boolean; tracks: boolean; vias: boolean };

  /** Refuse a move/edit that adds gate failures. Not a KiCad feature -- see the task's Strict toggle. */
  strict: boolean;

  drcDialogOpen: boolean;
  toast: { message: string; kind: "error" | "info" } | null;

  /** Cursor position in board µm, for the status bar's X/Y/dx/dy/dist. */
  cursorUm: { x: number; y: number } | null;
  moveOriginUm: { x: number; y: number } | null;
}

const initialState: StudioState = {
  board: null,
  boardError: null,
  version: null,
  tab: "pcb",
  rightDockTab: "appearance",
  selection: new Set(),
  hot: new Set(),
  netHighlight: null,
  armed: null,
  movePreview: null,
  view: { scale: 0, x: 0, y: 0 },
  viewInitialized: false,
  units: "mm",
  polar: false,
  gridUm: 1000, // 1.0 mm; a placeholder until src/kicad/layers.json-adjacent grid defaults are extracted (KiCad's own default grid list is source-derived, see report)
  gridVisible: true,
  fullscreenCrosshair: false,
  showRatsnest: true,
  activeLayer: null,
  highContrast: false,
  // Copper layers (board.layers, e.g. "F.Cu") are added once the board
  // loads (see BOARD_OK below); the standard non-copper buckets have no
  // model data to wait for, so they're defaulted here.
  layerVisible: Object.fromEntries(STANDARD_LAYERS.map((l) => [l.key, true])),
  layerOpacity: Object.fromEntries(STANDARD_LAYERS.map((l) => [l.key, 1])),
  selectionFilter: { footprints: true, tracks: true, vias: true },
  strict: true,
  drcDialogOpen: false,
  toast: null,
  cursorUm: null,
  moveOriginUm: null,
};

type Action =
  | { type: "BOARD_OK"; board: BoardState }
  | { type: "BOARD_ERR"; message: string }
  | { type: "VERSION"; version: string }
  | { type: "SET_TAB"; tab: EditorTab }
  | { type: "SET_RIGHT_DOCK_TAB"; tab: RightDockTab }
  | { type: "SET_SELECTION"; refs: string[] }
  | { type: "TOGGLE_SELECTION"; ref: string }
  | { type: "CLEAR_SELECTION" }
  | { type: "SET_HOT"; refs: string[] }
  | { type: "SET_NET_HIGHLIGHT"; net: string | null }
  | { type: "SET_ARMED"; ref: string | null }
  | { type: "SET_MOVE_PREVIEW"; preview: MovePreview | null }
  | { type: "SET_VIEW"; view: ViewTransform }
  | { type: "MARK_VIEW_INITIALIZED" }
  | { type: "SET_UNITS"; units: LengthUnit }
  | { type: "TOGGLE_POLAR" }
  | { type: "SET_GRID_UM"; um: number }
  | { type: "TOGGLE_GRID_VISIBLE" }
  | { type: "TOGGLE_CROSSHAIR" }
  | { type: "TOGGLE_RATSNEST" }
  | { type: "SET_ACTIVE_LAYER"; layer: string | null }
  | { type: "TOGGLE_HIGH_CONTRAST" }
  | { type: "SET_LAYER_VISIBLE"; layer: string; visible: boolean }
  | { type: "SET_LAYER_OPACITY"; layer: string; opacity: number }
  | { type: "SET_SELECTION_FILTER"; filter: Partial<StudioState["selectionFilter"]> }
  | { type: "SET_STRICT"; strict: boolean }
  | { type: "SET_DRC_OPEN"; open: boolean }
  | { type: "TOAST"; message: string; kind: "error" | "info" }
  | { type: "TOAST_CLEAR" }
  | { type: "SET_CURSOR"; at: { x: number; y: number } | null }
  | { type: "SET_MOVE_ORIGIN"; at: { x: number; y: number } | null };

function reducer(state: StudioState, action: Action): StudioState {
  switch (action.type) {
    case "BOARD_OK": {
      // Layers can appear only after routing exists etc.; default any new
      // one to visible/opaque without clobbering a choice already made.
      const layerVisible = { ...state.layerVisible };
      const layerOpacity = { ...state.layerOpacity };
      for (const l of action.board.layers) {
        if (!(l in layerVisible)) layerVisible[l] = true;
        if (!(l in layerOpacity)) layerOpacity[l] = 1;
      }
      // Drop selection/hot refs for parts that no longer exist (ripped, renamed).
      const refs = new Set(action.board.parts.map((p) => p.ref));
      const selection = new Set([...state.selection].filter((r) => refs.has(r)));
      const hot = new Set([...state.hot].filter((r) => refs.has(r)));
      return { ...state, board: action.board, boardError: null, layerVisible, layerOpacity, selection, hot };
    }
    case "BOARD_ERR":
      return { ...state, boardError: action.message };
    case "VERSION":
      return { ...state, version: action.version };
    case "SET_TAB":
      return { ...state, tab: action.tab };
    case "SET_RIGHT_DOCK_TAB":
      return { ...state, rightDockTab: action.tab };
    case "SET_SELECTION":
      return { ...state, selection: new Set(action.refs), armed: null };
    case "TOGGLE_SELECTION": {
      const next = new Set(state.selection);
      if (next.has(action.ref)) next.delete(action.ref);
      else next.add(action.ref);
      return { ...state, selection: next };
    }
    case "CLEAR_SELECTION":
      return { ...state, selection: new Set(), armed: null, movePreview: null };
    case "SET_HOT":
      return { ...state, hot: new Set(action.refs) };
    case "SET_NET_HIGHLIGHT":
      return { ...state, netHighlight: action.net };
    case "SET_ARMED":
      return { ...state, armed: action.ref, selection: new Set() };
    case "SET_MOVE_PREVIEW":
      return { ...state, movePreview: action.preview };
    case "SET_VIEW":
      return { ...state, view: action.view };
    case "MARK_VIEW_INITIALIZED":
      return { ...state, viewInitialized: true };
    case "SET_UNITS":
      return { ...state, units: action.units };
    case "TOGGLE_POLAR":
      return { ...state, polar: !state.polar };
    case "SET_GRID_UM":
      return { ...state, gridUm: action.um };
    case "TOGGLE_GRID_VISIBLE":
      return { ...state, gridVisible: !state.gridVisible };
    case "TOGGLE_CROSSHAIR":
      return { ...state, fullscreenCrosshair: !state.fullscreenCrosshair };
    case "TOGGLE_RATSNEST":
      return { ...state, showRatsnest: !state.showRatsnest };
    case "SET_ACTIVE_LAYER":
      return { ...state, activeLayer: action.layer };
    case "TOGGLE_HIGH_CONTRAST":
      return { ...state, highContrast: !state.highContrast };
    case "SET_LAYER_VISIBLE":
      return { ...state, layerVisible: { ...state.layerVisible, [action.layer]: action.visible } };
    case "SET_LAYER_OPACITY":
      return { ...state, layerOpacity: { ...state.layerOpacity, [action.layer]: action.opacity } };
    case "SET_SELECTION_FILTER":
      return { ...state, selectionFilter: { ...state.selectionFilter, ...action.filter } };
    case "SET_STRICT":
      return { ...state, strict: action.strict };
    case "SET_DRC_OPEN":
      return { ...state, drcDialogOpen: action.open };
    case "TOAST":
      return { ...state, toast: { message: action.message, kind: action.kind } };
    case "TOAST_CLEAR":
      return { ...state, toast: null };
    case "SET_CURSOR":
      return { ...state, cursorUm: action.at };
    case "SET_MOVE_ORIGIN":
      return { ...state, moveOriginUm: action.at };
    default:
      return state;
  }
}

export interface StudioApi {
  /** Force an immediate /api/state refetch (after a command, or on demand). */
  refresh: () => Promise<void>;
  rotateSelection: (quarterTurns: number) => Promise<void>;
  ripSelection: () => Promise<void>;
  /** Commit a completed drag: each ref moves by (dxUm, dyUm) from its current position. */
  commitMove: (refs: string[], dxUm: number, dyUm: number) => Promise<void>;
  placeArmedAt: (xUm: number, yUm: number) => Promise<void>;
  route: () => Promise<void>;
  undo: () => Promise<void>;
  redo: () => Promise<void>;
  partByRef: (ref: string) => Part | undefined;
}

const StudioStateContext = createContext<StudioState | null>(null);
const StudioDispatchContext = createContext<React.Dispatch<Action> | null>(null);
const StudioApiContext = createContext<StudioApi | null>(null);

export function StudioProvider({ children }: { children: React.ReactNode }) {
  const [state, dispatch] = useReducer(reducer, initialState);
  const stateRef = useRef(state);
  stateRef.current = state;

  const refresh = useCallback(async () => {
    try {
      const board = await fetchState();
      dispatch({ type: "BOARD_OK", board });
    } catch (e) {
      dispatch({ type: "BOARD_ERR", message: e instanceof Error ? e.message : String(e) });
    }
  }, []);

  // Poll /api/version (cheap) and only refetch the full /api/state when it
  // changes -- mirrors the old studio.html poll loop so CLI edits and
  // other browser tabs show up here within ~1s without hammering the
  // single-threaded backend.
  useEffect(() => {
    let stopped = false;
    let lastVersion: string | null = null;
    const tick = async () => {
      try {
        const v = await fetchVersion();
        if (stopped) return;
        if (v !== lastVersion) {
          lastVersion = v;
          dispatch({ type: "VERSION", version: v });
          await refresh();
        }
      } catch {
        // backend restarting or unreachable; try again next tick
      }
    };
    tick();
    const id = setInterval(tick, 700);
    return () => {
      stopped = true;
      clearInterval(id);
    };
  }, [refresh]);

  const runCmd = useCallback(
    async (cmd: Parameters<typeof postCmd>[0]) => {
      const reply = await postCmd(cmd, stateRef.current.strict);
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "error" });
      await refresh();
      return reply.ok;
    },
    [refresh]
  );

  const api: StudioApi = {
    refresh,
    partByRef: (ref) => stateRef.current.board?.parts.find((p) => p.ref === ref),
    rotateSelection: async (quarterTurns) => {
      for (const ref of stateRef.current.selection) {
        const p = api.partByRef(ref);
        if (p?.placed) await runCmd({ op: "rotate", part: ref, quarter_turns: ((quarterTurns % 4) + 4) % 4 });
      }
    },
    ripSelection: async () => {
      const refs = [...stateRef.current.selection];
      dispatch({ type: "CLEAR_SELECTION" });
      for (const ref of refs) await runCmd({ op: "rip", part: ref });
    },
    commitMove: async (refs, dxUm, dyUm) => {
      dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
      for (const ref of refs) {
        const p = api.partByRef(ref);
        if (p?.placed && p.at) await runCmd({ op: "move_to", part: ref, x: p.at[0] + dxUm, y: p.at[1] + dyUm });
      }
    },
    placeArmedAt: async (xUm, yUm) => {
      const ref = stateRef.current.armed;
      if (!ref) return;
      dispatch({ type: "SET_ARMED", ref: null });
      const ok = await runCmd({ op: "place_at", part: ref, x: xUm, y: yUm });
      if (ok) dispatch({ type: "SET_SELECTION", refs: [ref] });
    },
    route: async () => {
      const reply = await postRoute();
      dispatch({ type: "TOAST", message: reply.message, kind: reply.ok ? "info" : "error" });
      await refresh();
    },
    undo: async () => {
      dispatch({ type: "CLEAR_SELECTION" });
      const reply = await postUndo();
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "info" });
      await refresh();
    },
    redo: async () => {
      dispatch({ type: "CLEAR_SELECTION" });
      const reply = await postRedo();
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "info" });
      await refresh();
    },
  };

  return (
    <StudioStateContext.Provider value={state}>
      <StudioDispatchContext.Provider value={dispatch}>
        <StudioApiContext.Provider value={api}>{children}</StudioApiContext.Provider>
      </StudioDispatchContext.Provider>
    </StudioStateContext.Provider>
  );
}

export function useStudioState(): StudioState {
  const ctx = useContext(StudioStateContext);
  if (!ctx) throw new Error("useStudioState must be used within StudioProvider");
  return ctx;
}

export function useStudioDispatch(): React.Dispatch<Action> {
  const ctx = useContext(StudioDispatchContext);
  if (!ctx) throw new Error("useStudioDispatch must be used within StudioProvider");
  return ctx;
}

export function useStudioApi(): StudioApi {
  const ctx = useContext(StudioApiContext);
  if (!ctx) throw new Error("useStudioApi must be used within StudioProvider");
  return ctx;
}
