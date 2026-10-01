// Hand-rolled state container (no state-management library, per the
// project's dependency limits): a useReducer store for pure UI state,
// plus a handful of async functions closed over `dispatch` for anything
// that talks to the backend. Board truth always comes back through the
// /api/version + /api/state poll -- these functions POST a command and
// then nudge the poller to refetch immediately, exactly like the old
// studio.html's `send()`.

import React, { createContext, useCallback, useContext, useEffect, useReducer, useRef } from "react";
import type { BoardState, Cmd, DrcReport, ErcReport, Part, Ratsnest, Schematic, SchematicSymbol, SchematicWire, Shape, Track, Um, Via, Zone, BoardText } from "../api/types";
import { fetchDrc, fetchErc, fetchRatsnest, fetchSchematic, fetchState, fetchVersion, postCmd, postRedo, postRoute, postUndo } from "../api/client";
import type { LengthUnit } from "./units";
import { STANDARD_LAYERS } from "../components/canvas/layers";
import { DEFAULT_SELECTION_FILTER, type SelectionFilter } from "../components/canvas/selectionCandidates";
import { allItemIds, collectClipboardContents, type ClipboardContents } from "../components/canvas/clipboard";

export type RightDockTab = "appearance" | "filter" | "activity";
/**
 * One window, three tabs -- unlike real KiCad, which is a separate
 * window per editor (pcbnew/eeschema/the 3D viewer). Each tab keeps that
 * editor's own toolbars/menus/panels (App.tsx), but selection and net
 * highlight are shared app-wide (this same state.selection/
 * netHighlight), so cross-probing between PCB and Schematic is just both
 * views reading the same fields, not a separate sync mechanism.
 */
export type EditorTab = "pcb" | "schematic" | "3d";

/**
 * A minimal active-tool state -- just enough to give the status bar's
 * tool-message field (eda_draw_frame.cpp field 6, DisplayToolMsg())
 * something real to show, and a proper home for Escape/M instead of
 * component-local state in Canvas.tsx. KiCad's own tool stack is far
 * deeper (a whole TOOL_MANAGER with push/pop tool states); this is only
 * as much of that idea as this app's two real modes need.
 */
export type ToolId = "select" | "move" | "route" | "via" | "zone" | "draw_segment" | "draw_arc" | "draw_rect" | "draw_circle" | "draw_polygon" | "text" | "wire";
export const TOOL_MESSAGES: Record<ToolId, string> = {
  select: "Select item(s)",
  move: "Move item(s)",
  route: "Route track: click to add a point, V for via, Enter/double-click to finish, Esc to cancel",
  via: "Click to place a via",
  zone: "Zone: click to add points, Enter/double-click to finish, Esc to cancel",
  draw_segment: "Line: click start, then end",
  draw_arc: "Arc: click start, mid, then end",
  draw_rect: "Rectangle: click one corner, then the opposite one",
  draw_circle: "Circle: click center, then a point on the edge",
  draw_polygon: "Polygon: click points, Enter/double-click to finish, Esc to cancel",
  text: "Click to place text",
  wire: "Wire: click to start/add a point (snaps to a pin when close), double-click or Enter to finish, Backspace to undo the last point, Esc to cancel",
};

/**
 * The interactive routing/zone/drawing tools' in-progress geometry
 * (Canvas.tsx). One shape covers all of them since they're all "click to
 * add a point, Enter/double-click to finish, Esc to cancel" -- the only
 * per-tool difference is how many points are needed and what finishing
 * does with them. Route commits incrementally (see useActionRunner.ts's
 * "V drops a via" handling) rather than only at the end, so `pts` there
 * is just the CURRENT segment's points since the last via/start.
 */
export type DrawState =
  | { kind: "route"; net: string; layer: string; width: Um; pts: [Um, Um][] }
  | { kind: "zone"; pts: [Um, Um][] }
  | { kind: "shape"; shapeKind: "segment" | "arc" | "rect" | "circle" | "polygon"; pts: [Um, Um][] }
  /** `W` (Schematic tab): sch_line_wire_bus_tool.cpp's in-progress wire polyline -- see SchematicView.tsx's own doc for what this session ported vs. left out (free-angle only, no 90/45 posture, no auto-junction placement needed since that's a rendering-only concept here). */
  | { kind: "wire"; pts: [Um, Um][] };

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
  /** Which kind of item `refs` names -- each commits through a different Cmd (parts: move_to per ref; via/shape/text: their own move_* by id; symbol: schematic move_symbol). Defaults to "part" (every pre-existing caller moves parts). */
  kind?: "part" | "via" | "shape" | "text" | "symbol";
  dxUm: number;
  dyUm: number;
  /**
   * edit_tool.cpp Rotate/Flip during an active Move: both act on the
   * live preview instead of committing immediately (updateModificationPoint's
   * `m_dragging && HasReferencePoint()` guard -- the item hasn't been
   * pushed to the board yet, so there's nothing to commit to). Only
   * meaningful for `kind === "part"` (the only kind with a real
   * rotate/flip Cmd); harmless and ignored for the others. Quarter turns
   * 0-3, same units as `Cmd::Rotate`.
   */
  rotateQuarterTurns?: number;
  flipped?: boolean;
}

/** The 3D viewer's own view-option toggles -- KiCad's 3D viewer has all of these (View menu / its own toolbar): hide silkscreen, hide solder mask, hide the rendered component models, flip to view the board from the other side, and an orthographic/perspective projection switch. */
export interface Viewer3DOptions {
  showComponents: boolean;
  showSilkscreen: boolean;
  showSolderMask: boolean;
  /** True = viewing the board flipped (as if turned over a horizontal hinge) so the bottom side reads right-way-up. A camera/board-orientation toggle, not a geometry edit. */
  flipped: boolean;
  /** True = an orthographic-looking projection (this app has no separate OrthographicCamera wiring -- Viewer3D approximates it by narrowing FOV and pulling the camera back, a well-known trick, not a real projection-matrix swap). */
  orthographic: boolean;
  /** True = show GET /api/board.glb's real KiCad-rendered board (real 3D models, kicad-cli's own colors/materials) in place of this app's own procedural scene. Viewer3D falls back to the procedural scene regardless of this flag when the GLB hasn't loaded (still fetching, or the board/kicad-cli export failed) -- there's nothing to show otherwise. Defaults on; the user can still turn it off to see the lighter procedural scene. */
  kicadModels: boolean;
}

/**
 * GET /api/board.glb's lifecycle for the version currently on screen --
 * see api/types.ts's `BoardGlbResult` and studio.rs's `GlbBuild` for the
 * backend side this mirrors. "idle" covers both "never asked" (toggle
 * off, or not on the 3D tab) and "not needed" (nothing changed since the
 * last "loaded"/"failed"); Viewer3D.tsx is the only writer.
 */
export type GlbStatus = "idle" | "pending" | "loaded" | "failed";

export const DEFAULT_VIEWER3D_OPTIONS: Viewer3DOptions = {
  showComponents: true,
  showSilkscreen: true,
  showSolderMask: true,
  flipped: false,
  orthographic: false,
  kicadModels: true,
};

export interface StudioState {
  board: BoardState | null;
  boardError: string | null;
  version: string | null;

  tab: EditorTab;
  rightDockTab: RightDockTab;
  viewer3d: Viewer3DOptions;
  /** See `GlbStatus`. Read by Viewer3D (the "loading models…" badge) and Viewer3DToolbar (the KiCad Models toggle's tooltip after a failure). */
  glbStatus: GlbStatus;
  glbError: string | null;

  selection: Set<string>;
  /** Refs to flash/outline because a problem in the panel references them. */
  hot: Set<string>;
  netHighlight: string | null;
  /** An unplaced part ref chosen from the panel, waiting for a canvas click to place it. */
  armed: string | null;
  movePreview: MovePreview | null;
  activeTool: ToolId;
  drawState: DrawState | null;
  /** A just-drawn zone outline waiting for its net/layer to be confirmed in ZoneDialog before `add_zone` commits it. */
  zonePending: [Um, Um][] | null;
  /** The text tool/E-to-edit dialog: "add" (fresh, at a clicked point) or "edit" (an existing text's id). */
  textDialog: { mode: "add"; at: [Um, Um] } | { mode: "edit"; id: string } | null;

  view: ViewTransform;
  viewInitialized: boolean;
  /** Independent pan/zoom for the schematic tab -- a different sheet, a different natural scale. */
  schematicView: ViewTransform;
  units: LengthUnit;
  polar: boolean;
  gridUm: number;
  gridVisible: boolean;
  fullscreenCrosshair: boolean;
  showRatsnest: boolean;
  /** pcbnew.Control.ratsnestLineMode ("Curved Ratsnest Lines"). */
  ratsnestCurved: boolean;
  /**
   * pcbnew.Control.padDisplayMode/trackDisplayMode/viaDisplayMode
   * ("Sketch Pads"/"Sketch Tracks"/"Sketch Vias"): outline-only instead
   * of filled. All client-side display options -- no board data changes.
   */
  sketchPads: boolean;
  sketchTracks: boolean;
  sketchVias: boolean;
  /** Active layer for highlight/contrast (null = all layers equally weighted). */
  activeLayer: string | null;
  highContrast: boolean;
  layerVisible: Record<string, boolean>;
  layerOpacity: Record<string, number>;

  /**
   * pcbnew's real Selection Filter panel (panel_selection_filter.cpp):
   * a click/box-select/select-all skips whole item classes that are
   * toggled off. See selectionCandidates.ts's `SelectionFilter` for which
   * of KiCad's real categories this app's simpler model has a selectable
   * equivalent for.
   */
  selectionFilter: SelectionFilter;

  /** Refuse a move/edit that adds gate failures. Not a KiCad feature -- see the task's Strict toggle. */
  strict: boolean;

  drcDialogOpen: boolean;
  hotkeysDialogOpen: boolean;
  footprintPropertiesOpen: boolean;
  /** E on a selected track/via/zone/shape: which one's read-only properties dialog is open (null = closed). Text has its own full-edit dialog (textDialog); a part has footprintPropertiesOpen. */
  itemPropertiesId: string | null;
  /** pcbnew.Control.showNetInspector ("Net Inspector") -- a basic net/pad-count list, not KiCad's full dockable inspector. */
  netInspectorOpen: boolean;
  toast: { message: string; kind: "error" | "info" } | null;

  /** Cursor position in board µm, for the status bar's X/Y/dx/dy/dist. */
  cursorUm: { x: number; y: number } | null;
  moveOriginUm: { x: number; y: number } | null;
  /**
   * base_screen.cpp's `m_LocalOrigin` (default (0,0), same as source) --
   * the status bar's dx/dy/dist is always relative to THIS point, set by
   * Space (common.Control.resetLocalCoords, pcb_base_frame.cpp's
   * UpdateStatusBar). Independent of moveOriginUm (the move tool's own
   * drag-grab anchor): KiCad's move tool never touches m_LocalOrigin.
   */
  localOriginUm: { x: number; y: number };
  /**
   * view_controls.cpp VC_SETTINGS::m_autoPanSettingEnabled -- edge
   * auto-pan while dragging/drawing near the canvas border. KiCad ships
   * with this OFF (input.auto_pan defaults false; Preferences > Mouse and
   * Touchpad turns it on) -- same default here, see kicad-port/
   * viewControls.ts.
   */
  autoPanEnabled: boolean;

  /**
   * GET /api/schematic (structured symbols/wires/labels, see studio.rs),
   * fetched only while `tab === "schematic"` -- read-only for now (no
   * schematic editing verbs exist yet).
   */
  schematic: Schematic | null;
  schematicError: string | null;

  /**
   * GET /api/ratsnest (crates/connectivity's real KiCad-matching
   * ratsnest, see api/client.ts's fetchRatsnest) -- fetched only while
   * `tab === "pcb"`, the only tab that ever draws it.
   */
  ratsnest: Ratsnest | null;

  /**
   * GET /api/drc (crates/drc's ported KiCad DRC engine, see
   * api/client.ts's fetchDrc) -- fetched only while `drcDialogOpen`,
   * the one place this app shows it (DrcDialog.tsx, and the PCB canvas's
   * violation markers while that dialog is up).
   */
  drc: DrcReport | null;
  /** Index into `drc.violations` the dialog's list has clicked, for the canvas's marker highlight and the "selects and zooms to it" behavior -- null selects nothing. */
  drcSelected: number | null;

  ercDialogOpen: boolean;
  /**
   * GET /api/erc (crates/kicad's `check_erc`, gap #4 -- see api/client.ts's
   * fetchErc) -- fetched only while `ercDialogOpen`, same reasoning as
   * `drc`/`drcDialogOpen` above.
   */
  erc: ErcReport | null;
  /** Index into `erc.violations` the dialog's list has clicked -- null selects nothing. */
  ercSelected: number | null;

  /** Cmd+C's clipboard (Cmd-ready IR shapes, see components/canvas/clipboard.ts) -- client-side only, holds full item data so Cmd+V still works after the original was deleted, or pasted more than once. Null = nothing copied yet this session. */
  clipboard: ClipboardContents | null;
  /** Shift+M "Move Exactly..." dialog -- open with the selection's own default anchor/bbox already resolved (components/MoveExactDialog.tsx computes the rest). Null = closed. */
  moveExactDialogOpen: boolean;
}

const initialState: StudioState = {
  board: null,
  boardError: null,
  version: null,
  tab: "pcb",
  rightDockTab: "appearance",
  viewer3d: DEFAULT_VIEWER3D_OPTIONS,
  glbStatus: "idle",
  glbError: null,
  selection: new Set(),
  hot: new Set(),
  netHighlight: null,
  armed: null,
  movePreview: null,
  activeTool: "select",
  drawState: null,
  zonePending: null,
  textDialog: null,
  view: { scale: 0, x: 0, y: 0 },
  viewInitialized: false,
  schematicView: { scale: 0, x: 0, y: 0 },
  units: "mm",
  polar: false,
  gridUm: 1000, // 1.0 mm; a placeholder until src/kicad/layers.json-adjacent grid defaults are extracted (KiCad's own default grid list is source-derived, see report)
  gridVisible: true,
  fullscreenCrosshair: false,
  showRatsnest: true,
  ratsnestCurved: false,
  sketchPads: false,
  sketchTracks: false,
  sketchVias: false,
  activeLayer: null,
  highContrast: false,
  // Copper layers (board.layers, e.g. "F.Cu") are added once the board
  // loads (see BOARD_OK below); the standard non-copper buckets have no
  // model data to wait for, so they're defaulted here.
  layerVisible: Object.fromEntries(STANDARD_LAYERS.map((l) => [l.key, true])),
  layerOpacity: Object.fromEntries(STANDARD_LAYERS.map((l) => [l.key, 1])),
  selectionFilter: DEFAULT_SELECTION_FILTER,
  strict: true,
  drcDialogOpen: false,
  hotkeysDialogOpen: false,
  footprintPropertiesOpen: false,
  itemPropertiesId: null,
  netInspectorOpen: false,
  toast: null,
  cursorUm: null,
  moveOriginUm: null,
  localOriginUm: { x: 0, y: 0 },
  autoPanEnabled: false,
  schematic: null,
  schematicError: null,
  ratsnest: null,
  drc: null,
  drcSelected: null,
  ercDialogOpen: false,
  erc: null,
  ercSelected: null,
  clipboard: null,
  moveExactDialogOpen: false,
};

export type Action =
  | { type: "BOARD_OK"; board: BoardState }
  | { type: "BOARD_ERR"; message: string }
  | { type: "VERSION"; version: string }
  | { type: "SET_TAB"; tab: EditorTab }
  | { type: "SET_RIGHT_DOCK_TAB"; tab: RightDockTab }
  | { type: "SET_VIEWER3D_OPTIONS"; options: Partial<Viewer3DOptions> }
  | { type: "SET_GLB_STATUS"; status: GlbStatus; error?: string }
  | { type: "SET_SELECTION"; refs: string[] }
  | { type: "TOGGLE_SELECTION"; ref: string }
  | { type: "CLEAR_SELECTION" }
  | { type: "ESCAPE" }
  | { type: "SET_HOT"; refs: string[] }
  | { type: "SET_NET_HIGHLIGHT"; net: string | null }
  | { type: "SET_ARMED"; ref: string | null }
  | { type: "SET_MOVE_PREVIEW"; preview: MovePreview | null }
  | { type: "SET_ACTIVE_TOOL"; tool: ToolId }
  | { type: "SET_VIEW"; view: ViewTransform }
  | { type: "SET_SCHEMATIC_VIEW"; view: ViewTransform }
  | { type: "MARK_VIEW_INITIALIZED" }
  | { type: "SET_UNITS"; units: LengthUnit }
  | { type: "TOGGLE_POLAR" }
  | { type: "SET_GRID_UM"; um: number }
  | { type: "TOGGLE_GRID_VISIBLE" }
  | { type: "TOGGLE_CROSSHAIR" }
  | { type: "SET_FULLSCREEN_CROSSHAIR"; value: boolean }
  | { type: "TOGGLE_RATSNEST" }
  | { type: "TOGGLE_RATSNEST_CURVED" }
  | { type: "TOGGLE_SKETCH_PADS" }
  | { type: "TOGGLE_SKETCH_TRACKS" }
  | { type: "TOGGLE_SKETCH_VIAS" }
  | { type: "SET_ACTIVE_LAYER"; layer: string | null }
  | { type: "TOGGLE_HIGH_CONTRAST" }
  | { type: "SET_LAYER_VISIBLE"; layer: string; visible: boolean }
  | { type: "SET_LAYER_OPACITY"; layer: string; opacity: number }
  | { type: "SET_SELECTION_FILTER"; filter: Partial<StudioState["selectionFilter"]> }
  | { type: "SET_STRICT"; strict: boolean }
  | { type: "SET_DRC_OPEN"; open: boolean }
  | { type: "SET_HOTKEYS_DIALOG_OPEN"; open: boolean }
  | { type: "SET_FOOTPRINT_PROPERTIES_OPEN"; open: boolean }
  | { type: "SET_ITEM_PROPERTIES_ID"; id: string | null }
  | { type: "SET_NET_INSPECTOR_OPEN"; open: boolean }
  | { type: "TOAST"; message: string; kind: "error" | "info" }
  | { type: "TOAST_CLEAR" }
  | { type: "SET_CURSOR"; at: { x: number; y: number } | null }
  | { type: "SET_MOVE_ORIGIN"; at: { x: number; y: number } | null }
  | { type: "SET_LOCAL_ORIGIN"; at: { x: number; y: number } }
  | { type: "TOGGLE_AUTO_PAN" }
  | { type: "SCHEMATIC_OK"; schematic: Schematic }
  | { type: "SCHEMATIC_ERR"; message: string }
  | { type: "RATSNEST_OK"; ratsnest: Ratsnest }
  | { type: "DRC_OK"; drc: DrcReport }
  | { type: "SET_DRC_SELECTED"; index: number | null }
  | { type: "SET_ERC_DIALOG_OPEN"; open: boolean }
  | { type: "ERC_OK"; erc: ErcReport }
  | { type: "SET_ERC_SELECTED"; index: number | null }
  | { type: "SET_DRAW_STATE"; draw: DrawState | null }
  | { type: "SET_ZONE_PENDING"; outline: [Um, Um][] | null }
  | { type: "SET_TEXT_DIALOG"; dialog: StudioState["textDialog"] }
  | { type: "SET_CLIPBOARD"; clipboard: ClipboardContents | null }
  | { type: "SET_MOVE_EXACT_DIALOG_OPEN"; open: boolean };

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
    case "SET_VIEWER3D_OPTIONS":
      return { ...state, viewer3d: { ...state.viewer3d, ...action.options } };
    case "SET_GLB_STATUS":
      return { ...state, glbStatus: action.status, glbError: action.error ?? null };
    case "SET_SELECTION":
      return { ...state, selection: new Set(action.refs), armed: null };
    case "TOGGLE_SELECTION": {
      const next = new Set(state.selection);
      if (next.has(action.ref)) next.delete(action.ref);
      else next.add(action.ref);
      return { ...state, selection: next };
    }
    case "CLEAR_SELECTION":
      // The universal "cancel and go back to Select" -- used by the box
      // select's own "start a fresh (non-additive) drag on empty space"
      // reset, and anywhere else that needs to drop whatever the route/
      // zone/drawing/text tools were in the middle of, not just a
      // footprint selection/move.
      return { ...state, selection: new Set(), armed: null, movePreview: null, activeTool: "select", drawState: null, zonePending: null, textDialog: null, itemPropertiesId: null };
    case "ESCAPE": {
      // pcb_selection_tool.cpp's IsCancel() handler, tiered exactly like
      // source: an in-progress tool (move/draw/armed-place) owns Escape
      // first and just reverts itself -- the SELECTION_TOOL's own
      // handler (selection non-empty -> clear it; otherwise -> clear net
      // highlight, m_ESCClearsNetHighlight default true) only runs once
      // nothing else is running. The earlier plain CLEAR_SELECTION this
      // replaced also wiped the selection on every Escape, even mid-move
      // -- wrong: EDIT_TOOL::doMoveSelection's own cancel just reverts
      // the move and leaves the pre-move selection exactly as it was.
      const inProgress = state.activeTool !== "select" || state.drawState != null || state.armed != null || state.movePreview != null;
      if (inProgress) {
        return { ...state, armed: null, movePreview: null, activeTool: "select", drawState: null, zonePending: null, textDialog: null };
      }
      if (state.selection.size > 0) {
        return { ...state, selection: new Set() };
      }
      // Idle, nothing selected: pcbnew_settings.cpp m_ESCClearsNetHighlight defaults true.
      return { ...state, netHighlight: null };
    }
    case "SET_HOT":
      return { ...state, hot: new Set(action.refs) };
    case "SET_NET_HIGHLIGHT":
      return { ...state, netHighlight: action.net };
    case "SET_ARMED":
      return { ...state, armed: action.ref, selection: new Set() };
    case "SET_MOVE_PREVIEW":
      return { ...state, movePreview: action.preview };
    case "SET_ACTIVE_TOOL":
      return { ...state, activeTool: action.tool };
    case "SET_VIEW":
      return { ...state, view: action.view };
    case "SET_SCHEMATIC_VIEW":
      return { ...state, schematicView: action.view };
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
    case "SET_FULLSCREEN_CROSSHAIR":
      return { ...state, fullscreenCrosshair: action.value };
    case "TOGGLE_RATSNEST":
      return { ...state, showRatsnest: !state.showRatsnest };
    case "TOGGLE_RATSNEST_CURVED":
      return { ...state, ratsnestCurved: !state.ratsnestCurved };
    case "TOGGLE_SKETCH_PADS":
      return { ...state, sketchPads: !state.sketchPads };
    case "TOGGLE_SKETCH_TRACKS":
      return { ...state, sketchTracks: !state.sketchTracks };
    case "TOGGLE_SKETCH_VIAS":
      return { ...state, sketchVias: !state.sketchVias };
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
    case "SET_HOTKEYS_DIALOG_OPEN":
      return { ...state, hotkeysDialogOpen: action.open };
    case "SET_FOOTPRINT_PROPERTIES_OPEN":
      return { ...state, footprintPropertiesOpen: action.open };
    case "SET_ITEM_PROPERTIES_ID":
      return { ...state, itemPropertiesId: action.id };
    case "SET_NET_INSPECTOR_OPEN":
      return { ...state, netInspectorOpen: action.open };
    case "TOAST":
      return { ...state, toast: { message: action.message, kind: action.kind } };
    case "TOAST_CLEAR":
      return { ...state, toast: null };
    case "SET_CURSOR":
      return { ...state, cursorUm: action.at };
    case "SET_MOVE_ORIGIN":
      return { ...state, moveOriginUm: action.at };
    case "SET_LOCAL_ORIGIN":
      return { ...state, localOriginUm: action.at };
    case "TOGGLE_AUTO_PAN":
      return { ...state, autoPanEnabled: !state.autoPanEnabled };
    case "SCHEMATIC_OK":
      return { ...state, schematic: action.schematic, schematicError: null };
    case "SCHEMATIC_ERR":
      return { ...state, schematicError: action.message };
    case "RATSNEST_OK":
      return { ...state, ratsnest: action.ratsnest };
    case "DRC_OK":
      // A fresh report invalidates any previous selection -- indices (and
      // the violations they pointed at) aren't stable across re-runs.
      return { ...state, drc: action.drc, drcSelected: null };
    case "SET_DRC_SELECTED":
      return { ...state, drcSelected: action.index };
    case "SET_ERC_DIALOG_OPEN":
      return { ...state, ercDialogOpen: action.open };
    case "ERC_OK":
      return { ...state, erc: action.erc, ercSelected: null };
    case "SET_ERC_SELECTED":
      return { ...state, ercSelected: action.index };
    case "SET_DRAW_STATE":
      return { ...state, drawState: action.draw };
    case "SET_ZONE_PENDING":
      return { ...state, zonePending: action.outline };
    case "SET_TEXT_DIALOG":
      return { ...state, textDialog: action.dialog };
    case "SET_CLIPBOARD":
      return { ...state, clipboard: action.clipboard };
    case "SET_MOVE_EXACT_DIALOG_OPEN":
      return { ...state, moveExactDialogOpen: action.open };
    default:
      return state;
  }
}

export interface StudioApi {
  /** Force an immediate /api/state refetch (after a command, or on demand). */
  refresh: () => Promise<void>;
  rotateSelection: (quarterTurns: number) => Promise<void>;
  ripSelection: () => Promise<void>;
  flipSelection: () => Promise<void>;
  /** Commit a completed drag: each ref moves by (dxUm, dyUm) from its current position, then (parts only) applies any rotate/flip accumulated during the move (MovePreview.rotateQuarterTurns/flipped -- edit_tool.cpp composes Move+Rotate+Flip as one undo step; this app commits them as sequential Cmds since each is independent of the others' position/orientation fields). `kind` picks which Cmd the move itself becomes (default "part"). */
  commitMove: (refs: string[], dxUm: number, dyUm: number, kind?: MovePreview["kind"], rotateQuarterTurns?: number, flipped?: boolean) => Promise<void>;
  placeArmedAt: (xUm: number, yUm: number) => Promise<void>;
  route: () => Promise<void>;
  undo: () => Promise<void>;
  redo: () => Promise<void>;
  partByRef: (ref: string) => Part | undefined;
  trackById: (id: string) => Track | undefined;
  viaById: (id: string) => Via | undefined;
  zoneById: (id: string) => Zone | undefined;
  shapeById: (id: string) => Shape | undefined;
  textById: (id: string) => BoardText | undefined;
  symbolById: (id: string) => SchematicSymbol | undefined;
  wireById: (id: string) => SchematicWire | undefined;
  /** R/Shift+R on the Schematic tab: rotate a symbol in place (quarterTurns: 1 = CCW/'R', 3 = CW/Shift+R, matching sch_edit_tool.cpp's own default). */
  rotateSymbol: (id: string, quarterTurns: number) => Promise<void>;
  /** X on the Schematic tab ("Mirror Horizontally") -- see `Cmd::MirrorSymbol`'s own doc on why this is the one axis wired. */
  mirrorSymbol: (id: string) => Promise<void>;
  /** Del on the Schematic tab: removes the symbol instance; any wire landed on its pins is left dangling, same as real eeschema. */
  deleteSymbol: (id: string) => Promise<void>;
  /** Any other Cmd this file doesn't have a named wrapper for (the delete_ ops, set_track_width, edit_text, ...) -- returns whether the backend accepted it, same as every named wrapper's underlying runCmd. */
  cmd: (c: Cmd) => Promise<boolean>;
  /**
   * Cmd+D on the current selection's tracks/vias/zones/shapes/text
   * (footprints excluded -- see `Cmd::Duplicate`'s own doc comment).
   * Duplicates in place, then selects the new copies and arms the Move
   * tool on them at the current cursor, same as EDIT_TOOL::Duplicate
   * handing straight off to doMoveSelection in source.
   */
  duplicateSelection: () => Promise<void>;
  /** Cmd+C: snapshot the current selection's tracks/vias/zones/shapes/text into the clipboard (state.clipboard). A no-op if none of the selection is copyable. */
  copySelection: () => void;
  /** Cmd+V: insert fresh copies of whatever's in the clipboard, then select and arm Move on them, same as duplicateSelection. */
  pasteClipboard: () => Promise<void>;
  /** Shift+M "Move Exactly..." dialog's OK action. */
  moveExact: (parts: string[], dx: number, dy: number, rotateMillideg: number, pivot: { x: number; y: number } | null) => Promise<boolean>;
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

  const refreshSchematic = useCallback(async () => {
    try {
      const schematic = await fetchSchematic();
      dispatch({ type: "SCHEMATIC_OK", schematic });
    } catch (e) {
      dispatch({ type: "SCHEMATIC_ERR", message: e instanceof Error ? e.message : String(e) });
    }
  }, []);

  const refreshRatsnest = useCallback(async () => {
    try {
      const ratsnest = await fetchRatsnest();
      dispatch({ type: "RATSNEST_OK", ratsnest });
    } catch {
      // backend restarting, board mid-edit, etc -- keep showing the last
      // good ratsnest rather than clearing it; try again next tick.
    }
  }, []);

  const refreshDrc = useCallback(async () => {
    try {
      const drc = await fetchDrc();
      dispatch({ type: "DRC_OK", drc });
    } catch {
      // same reasoning as refreshRatsnest -- keep the last good report.
    }
  }, []);

  const refreshErc = useCallback(async () => {
    try {
      const erc = await fetchErc();
      dispatch({ type: "ERC_OK", erc });
    } catch {
      // same reasoning as refreshDrc -- keep the last good report.
    }
  }, []);

  // Poll /api/version (cheap) and only refetch the full /api/state when it
  // changes -- mirrors the old studio.html poll loop so CLI edits and
  // other browser tabs show up here within ~1s without hammering the
  // single-threaded backend. The schematic and ratsnest are each read-only
  // and only ever shown on their own tab, so they piggyback on the same
  // version check rather than running their own poll: fetched once on
  // switching to that tab, and again whenever the board changes while
  // it's showing.
  useEffect(() => {
    let stopped = false;
    let lastVersion: string | null = null;
    let lastSchematicFetch: string | null = null;
    let lastRatsnestFetch: string | null = null;
    let lastDrcFetch: string | null = null;
    let lastErcFetch: string | null = null;
    const tick = async () => {
      try {
        const v = await fetchVersion();
        if (stopped) return;
        if (v !== lastVersion) {
          lastVersion = v;
          dispatch({ type: "VERSION", version: v });
          await refresh();
        }
        if (stateRef.current.tab === "schematic" && lastSchematicFetch !== v) {
          lastSchematicFetch = v;
          await refreshSchematic();
        }
        if (stateRef.current.tab === "pcb" && lastRatsnestFetch !== v) {
          lastRatsnestFetch = v;
          await refreshRatsnest();
        }
        // Gated on the dialog being open, not a tab -- DRC has no tab of
        // its own (it overlays whichever tab is showing), and running
        // the DRC engine on every tick regardless of whether anyone's
        // looking at it would be pure waste.
        if (stateRef.current.drcDialogOpen && lastDrcFetch !== v) {
          lastDrcFetch = v;
          await refreshDrc();
        }
        // Same reasoning as DRC above -- ERC overlays whichever tab is
        // showing (normally the Schematic one, but nothing stops running
        // it from the PCB tab), gated on the dialog, not a tab.
        if (stateRef.current.ercDialogOpen && lastErcFetch !== v) {
          lastErcFetch = v;
          await refreshErc();
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
  }, [refresh, refreshSchematic, refreshRatsnest, refreshDrc, refreshErc]);

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
    trackById: (id) => stateRef.current.board?.routing?.tracks.find((t) => t.id === id),
    viaById: (id) => stateRef.current.board?.routing?.vias.find((v) => v.id === id),
    zoneById: (id) => stateRef.current.board?.routing?.zones.find((z) => z.id === id),
    shapeById: (id) => stateRef.current.board?.drawings?.shapes.find((s) => s.id === id),
    textById: (id) => stateRef.current.board?.drawings?.texts.find((t) => t.id === id),
    symbolById: (id) => stateRef.current.schematic?.symbols.find((s) => s.id === id),
    wireById: (id) => stateRef.current.schematic?.wires.find((w) => w.id === id),
    rotateSymbol: async (id, quarterTurns) => {
      await runCmd({ op: "rotate_symbol", id, quarter_turns: ((quarterTurns % 4) + 4) % 4 });
    },
    mirrorSymbol: async (id) => {
      await runCmd({ op: "mirror_symbol", id });
    },
    deleteSymbol: async (id) => {
      dispatch({ type: "CLEAR_SELECTION" });
      await runCmd({ op: "delete_symbol", id });
    },
    cmd: (c) => runCmd(c),
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
    flipSelection: async () => {
      for (const ref of stateRef.current.selection) {
        const p = api.partByRef(ref);
        if (p?.placed) await runCmd({ op: "flip", part: ref });
      }
    },
    commitMove: async (refs, dxUm, dyUm, kind = "part", rotateQuarterTurns, flipped) => {
      dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
      for (const ref of refs) {
        if (kind === "part") {
          const p = api.partByRef(ref);
          if (p?.placed && p.at) await runCmd({ op: "move_to", part: ref, x: p.at[0] + dxUm, y: p.at[1] + dyUm });
          // Rotating/flipping about the part's own (already-moved) anchor
          // is exactly what the backend's Rotate/Flip ops do regardless of
          // when they're called, so applying them after the move lands on
          // the same final pose as KiCad's live in-place spin during a
          // single-item drag -- see MovePreview's own doc comment.
          if (rotateQuarterTurns) await runCmd({ op: "rotate", part: ref, quarter_turns: ((rotateQuarterTurns % 4) + 4) % 4 });
          if (flipped) await runCmd({ op: "flip", part: ref });
        } else if (kind === "via") {
          const v = api.viaById(ref);
          if (v) await runCmd({ op: "move_via", id: ref, x: v.x + dxUm, y: v.y + dyUm });
        } else if (kind === "shape") {
          await runCmd({ op: "move_shape", id: ref, dx: dxUm, dy: dyUm });
        } else if (kind === "text") {
          const t = api.textById(ref);
          if (t) await runCmd({ op: "move_text", id: ref, x: t.x + dxUm, y: t.y + dyUm });
        } else if (kind === "symbol") {
          const s = api.symbolById(ref);
          if (s) await runCmd({ op: "move_symbol", id: ref, x: s.at[0] + dxUm, y: s.at[1] + dyUm });
          // sch_edit_tool.cpp's R/Shift+R-during-move branch (see
          // useActionRunner.ts's tryTransformDuringMove): applied after
          // the move, same reasoning commitMove's own doc comment gives
          // for parts -- rotating about the symbol's own (already-moved)
          // anchor lands on the same final pose as a live in-place spin.
          if (rotateQuarterTurns) await runCmd({ op: "rotate_symbol", id: ref, quarter_turns: ((rotateQuarterTurns % 4) + 4) % 4 });
        }
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
    // Scoped to the tab that asked (GAPS.md #15): Ctrl+Z while looking at
    // the Schematic tab reverts that tab's own last edit (or is a no-op
    // once its stack is empty), never the PCB tab's, and vice versa -- see
    // `postUndo`/`board::undo`'s own doc for the backend half of this.
    undo: async () => {
      dispatch({ type: "CLEAR_SELECTION" });
      const reply = await postUndo(stateRef.current.tab === "schematic" ? "schematic" : "pcb");
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "info" });
      await refresh();
    },
    redo: async () => {
      dispatch({ type: "CLEAR_SELECTION" });
      const reply = await postRedo(stateRef.current.tab === "schematic" ? "schematic" : "pcb");
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "info" });
      await refresh();
    },
    // EDIT_TOOL::Duplicate hands straight off to doMoveSelection in
    // source -- the duplicate appears glued to the cursor until the user
    // clicks to drop it (or Escape, which now -- see the "ESCAPE" reducer
    // case -- cancels the in-progress move without touching the
    // selection, so the fresh duplicate stays selected right where it
    // was created rather than being un-done; a real cancel-removes-the-
    // duplicate would need this move to carry its own undo token, which
    // the backend's plain undo-stack doesn't expose per-ref).
    duplicateSelection: async () => {
      const board = stateRef.current.board;
      if (!board) return;
      const ids = [...stateRef.current.selection].filter((id) => api.trackById(id) || api.viaById(id) || api.zoneById(id) || api.shapeById(id) || api.textById(id));
      if (ids.length === 0) return;
      const before = allItemIds(board);
      const ok = await runCmd({ op: "duplicate", ids });
      if (!ok) return;
      const after = stateRef.current.board;
      if (!after) return;
      const newIds = [...allItemIds(after)].filter((id) => !before.has(id));
      if (newIds.length === 0) return;
      dispatch({ type: "SET_SELECTION", refs: newIds });
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
      dispatch({ type: "SET_MOVE_ORIGIN", at: stateRef.current.cursorUm });
    },
    copySelection: () => {
      const board = stateRef.current.board;
      if (!board) return;
      const clipboard = collectClipboardContents(board, stateRef.current.selection);
      if (clipboard) dispatch({ type: "SET_CLIPBOARD", clipboard });
    },
    pasteClipboard: async () => {
      const clip = stateRef.current.clipboard;
      const board = stateRef.current.board;
      if (!clip || !board) return;
      const before = allItemIds(board);
      const ok = await runCmd({ op: "paste_items", tracks: clip.tracks, vias: clip.vias, zones: clip.zones, shapes: clip.shapes, texts: clip.texts });
      if (!ok) return;
      const after = stateRef.current.board;
      if (!after) return;
      const newIds = [...allItemIds(after)].filter((id) => !before.has(id));
      if (newIds.length === 0) return;
      dispatch({ type: "SET_SELECTION", refs: newIds });
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
      dispatch({ type: "SET_MOVE_ORIGIN", at: stateRef.current.cursorUm });
    },
    moveExact: async (parts, dx, dy, rotateMillideg, pivot) => {
      return runCmd({ op: "move_exact", parts, dx, dy, rotate_millideg: rotateMillideg, pivot: pivot ? { x: pivot.x, y: pivot.y } : null });
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
