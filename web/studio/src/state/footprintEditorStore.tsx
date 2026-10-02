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
import { downloadFootprintKicadMod, fetchFootprint, fetchFootprintLibraryNames, postCmd, postRedo, postUndo } from "../api/client";
import { duplicatePads, uniqueFootprintName } from "../kicad-port/fpEditActions";
import { highestPadNumber } from "../kicad-port/padNumbering";
import type { ViewTransform } from "./store";
import type { ArcGeom } from "../kicad-port/arcGeom";
import type { BezierGeom } from "../kicad-port/bezierGeom";

export type FpToolId = "select" | "move" | "pad" | "draw_segment" | "draw_arc" | "draw_bezier" | "draw_rect" | "draw_circle" | "draw_polygon" | "text" | "anchor";

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
};

/** Same "click to add a point(s), commit on the last one" shape the PCB tab's own `DrawState` uses, narrowed to what the footprint editor's graphics tools need (no route/zone/measure concept here). `arc`/`bezier`: the construction managers' state, see the PCB tab's `DrawState`. */
export type FpDrawState = { kind: "shape"; shapeKind: "segment" | "arc" | "rect" | "circle" | "polygon" | "bezier"; pts: [Um, Um][]; arc?: ArcGeom; bezier?: BezierGeom };

export interface FpMovePreview {
  refs: string[];
  /** The kind of the item a drag started on. Optional because a Duplicate pick-up carries a mixed selection (pads + graphics + text); the commit looks each ref up either way. */
  kind?: "pad" | "graphic" | "text";
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
  /** `PAD_TOOL::m_lastPadNumber`: the number the last placed (or duplicated-with-increment) pad took, which `Duplicate and Increment` continues from. `null` until one is set -- then the highest-numbered pad stands in (padNumbering.ts's `nextPadNumber`). */
  lastPadNumber: string | null;
  /** `EDIT_TOOL::Duplicate` hands the copies straight to `doMoveSelection` and, if that is cancelled, `commit.Revert()`s them: true while the freshly duplicated items are glued to the cursor, so Escape undoes the duplicate and a drop keeps it. */
  duplicatePending: boolean;
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
  lastPadNumber: null,
  duplicatePending: false,
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
  | { type: "SET_LAST_PAD_NUMBER"; number: string | null }
  | { type: "SET_DUPLICATE_PENDING"; pending: boolean }
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
    case "SET_LAST_PAD_NUMBER":
      return { ...state, lastPadNumber: action.number };
    case "SET_DUPLICATE_PENDING":
      return { ...state, duplicatePending: action.pending };
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
  /** `pcbnew.ModuleEditor.newFootprint` (Ctrl+N): a new, empty SMD footprint named `Untitled` (made unique), opened in the editor. */
  newFootprint: () => Promise<void>;
  /**
   * `EDIT_TOOL::Duplicate` (Ctrl+D) / `pcbnew.InteractiveEdit.duplicateIncrementPads` (Ctrl+Shift+D, `increment`):
   * copy the selected pads/graphics/text in place as ONE undo step, select the copies and pick them up with Move
   * (Escape then reverts the duplicate). With `increment` each pad takes the next free pad number.
   */
  duplicateSelection: (increment: boolean) => Promise<void>;
  /** Commit a finished move (a drag-drop or a Duplicate pick-up) of `refs` by `(dxUm, dyUm)` as ONE undo step -- each ref is looked up, so a mixed selection works. */
  commitMove: (refs: string[], dxUm: number, dyUm: number) => Promise<void>;
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
    getState: () => stateRef.current,
    openFootprint: async (name) => {
      dispatch({ type: "SET_NAME", name });
      await postCmd({ op: "open_footprint_for_edit", name }, false);
      const footprint = await fetchFootprint(name);
      dispatch({ type: "FOOTPRINT_OK", footprint });
    },
    newFootprint: async () => {
      let existing: string[] = [];
      try {
        existing = (await fetchFootprintLibraryNames()).names;
      } catch {
        /* backend busy: Untitled is still very likely free; the verb refuses a duplicate name anyway */
      }
      const name = uniqueFootprintName(existing);
      const reply = await postCmd({ op: "new_footprint", name }, false);
      if (!reply.ok) {
        dispatch({ type: "TOAST", message: reply.message, kind: "error" });
        return;
      }
      dispatch({ type: "SET_NAME", name });
      dispatch({ type: "FOOTPRINT_OK", footprint: await fetchFootprint(name) });
    },
    duplicateSelection: async (increment) => {
      const st = stateRef.current;
      const fp = st.footprint;
      const name = st.name;
      if (!fp || !name) return;
      const ids = st.selection;
      const pads = fp.pads.filter((p) => p.id && ids.has(p.id));
      const graphics = fp.graphics.filter((g) => g.id && ids.has(g.id));
      const texts = fp.texts.filter((t) => t.id && ids.has(t.id));
      if (pads.length + graphics.length + texts.length === 0) return;
      const { pads: newPads, lastPadNumber } = duplicatePads(fp.pads, pads, increment, st.lastPadNumber ?? highestPadNumber(fp.pads));
      const before = new Set([...fp.pads.map((p) => p.id), ...fp.graphics.map((g) => g.id), ...fp.texts.map((t) => t.id)]);
      const cmds: Cmd[] = [
        ...newPads.map((pad): Cmd => ({ op: "add_pad", footprint: name, pad })),
        ...graphics.map((g): Cmd => ({ op: "add_footprint_graphic", footprint: name, shape: { ...g, id: undefined } })),
        ...texts.map((t): Cmd => ({ op: "add_footprint_text", footprint: name, text: { ...t, id: undefined } })),
      ];
      const ok = await runCmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
      if (!ok) return;
      // Read the document straight from the backend: the render that applies `runCmd`'s own refresh may not have happened yet.
      const after = await fetchFootprint(name);
      const created = [...after.pads.map((p) => p.id), ...after.graphics.map((g) => g.id), ...after.texts.map((t) => t.id)].filter((id): id is string => !!id && !before.has(id));
      if (increment) dispatch({ type: "SET_LAST_PAD_NUMBER", number: lastPadNumber });
      dispatch({ type: "SET_SELECTION", refs: created });
      // `doMoveSelection( aEvent, &commit, true )`: the copies are glued to the cursor until a click drops them.
      const at = stateRef.current.cursorUm;
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
      dispatch({ type: "SET_MOVE_ORIGIN", at });
      dispatch({ type: "SET_DUPLICATE_PENDING", pending: true });
      dispatch({ type: "TOAST", message: `Duplicated ${created.length} item(s)`, kind: "info" });
    },
    commitMove: async (refs, dxUm, dyUm) => {
      dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
      dispatch({ type: "SET_DUPLICATE_PENDING", pending: false });
      const name = stateRef.current.name;
      if (!name || (dxUm === 0 && dyUm === 0)) return;
      const cmds: Cmd[] = [];
      for (const id of refs) {
        const pad = api.padById(id);
        if (pad) {
          cmds.push({ op: "move_pad", footprint: name, id, x: pad.at.x + dxUm, y: pad.at.y + dyUm });
          continue;
        }
        if (api.graphicById(id)) {
          cmds.push({ op: "move_footprint_graphic", footprint: name, id, dx: dxUm, dy: dyUm });
          continue;
        }
        const text = api.textById(id);
        if (text) cmds.push({ op: "move_footprint_text", footprint: name, id, x: text.at.x + dxUm, y: text.at.y + dyUm });
      }
      if (cmds.length === 0) return;
      await runCmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds }); // one undo step, like one BOARD_COMMIT::Push
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
    exportKicadMod: async () => {
      const name = stateRef.current.name;
      if (!name) return;
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
