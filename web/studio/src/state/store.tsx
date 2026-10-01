// Hand-rolled state container (no state-management library, per the
// project's dependency limits): a useReducer store for pure UI state,
// plus a handful of async functions closed over `dispatch` for anything
// that talks to the backend. Board truth always comes back through the
// /api/version + /api/state poll -- these functions POST a command and
// then nudge the poller to refetch immediately, exactly like the old
// studio.html's `send()`.

import React, { createContext, useCallback, useContext, useEffect, useReducer, useRef } from "react";
import type { BoardState, BoardText, Cmd, DrcReport, ErcReport, FillReport, LabelScope, Part, Ratsnest, Schematic, SchematicSymbol, SchematicText, SchematicWire, Shape, Track, Um, Via, ViaPreset, Zone, ZoneSettingsFields } from "../api/types";
import { fetchDrc, fetchErc, fetchFill, fetchRatsnest, fetchSchematic, fetchState, fetchVersion, postCmd, postRedo, postRoute, postUndo } from "../api/client";
import type { LengthUnit } from "./units";
import { STANDARD_LAYERS } from "../components/canvas/layers";
import { DEFAULT_SELECTION_FILTER, type SelectionFilter } from "../components/canvas/selectionCandidates";
import { allItemIds, collectClipboardContents, type ClipboardContents } from "../components/canvas/clipboard";
import { alignAxis, alignDeltas, getDeltasForDistributeByGaps, getDeltasForDistributeByPoints, type AlignEdge, type Box } from "../kicad-port/alignDistribute";

export type RightDockTab = "appearance" | "filter" | "activity";
/**
 * One window, four tabs -- unlike real KiCad, which is a separate window
 * per editor (pcbnew/eeschema/the footprint editor/the 3D viewer). Each
 * tab keeps that editor's own toolbars/menus/panels (App.tsx), but
 * selection and net highlight are shared app-wide (this same
 * state.selection/netHighlight) for PCB/Schematic, so cross-probing
 * between them is just both views reading the same fields, not a
 * separate sync mechanism. The Footprint Editor tab (GAPS.md #8) is the
 * one exception: it is a genuinely separate document (a footprint
 * definition, not the board), so it keeps its own selection/view/undo
 * domain entirely in `state/footprintEditorStore.tsx` rather than reusing
 * this file's -- see that file's own header comment.
 */
export type EditorTab = "pcb" | "schematic" | "footprint" | "3d";

/**
 * A minimal active-tool state -- just enough to give the status bar's
 * tool-message field (eda_draw_frame.cpp field 6, DisplayToolMsg())
 * something real to show, and a proper home for Escape/M instead of
 * component-local state in Canvas.tsx. KiCad's own tool stack is far
 * deeper (a whole TOOL_MANAGER with push/pop tool states); this is only
 * as much of that idea as this app's two real modes need.
 */
export type ToolId =
  | "select"
  | "move"
  | "drag"
  | "route"
  | "via"
  | "zone"
  | "draw_segment"
  | "draw_arc"
  | "draw_rect"
  | "draw_circle"
  | "draw_polygon"
  | "text"
  | "wire"
  | "measure"
  // ---------------------------------------------------- eeschema placement
  // `L`/Ctrl+`L`/`H`/`P`/`T`/`Q` (sch_drawing_tools.cpp TwoClickPlace/
  // SingleClickPlace) -- SchematicView.tsx's own doc has the full
  // click-then-small-dialog flow these arm (a deliberate, documented
  // simplification of source's "dialog pops up immediately, item then
  // follows the cursor" order -- see PARITY-sch.md).
  | "sch_label_local"
  | "sch_label_global"
  | "sch_label_hier"
  | "sch_power"
  | "sch_text"
  | "sch_no_connect"
  /** `A`: armed once SymbolChooserDialog confirms a choice -- see `state.armedSymbol`. */
  | "sch_place_symbol";
export const TOOL_MESSAGES: Record<ToolId, string> = {
  select: "Select item(s)",
  move: "Move item(s)",
  drag: "Drag item(s) (keeps wire connections)",
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
  measure: "Measure: click a start point, click again for the end point. Click anywhere to start a new measurement, Esc to clear",
  sch_label_local: "Label: click where to place it",
  sch_label_global: "Global Label: click where to place it",
  sch_label_hier: "Hierarchical Label: click where to place it",
  sch_power: "Power Symbol: click a pin (snaps to the nearest one)",
  sch_text: "Text: click where to place it",
  sch_no_connect: "No Connect: click a pin to flag it unconnected",
  sch_place_symbol: "Place Symbol: click where to place it",
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
  | {
      kind: "route";
      net: string;
      layer: string;
      width: Um;
      /** The live head preview the backend's `eda_pns::Router` resolved
       * (walkaround/shove/mark-obstacles already applied) -- draw this
       * directly, same convention `drawInProgress` already used for the
       * old client-only posture45 rubber-band. */
      pts: [Um, Um][];
      /** Whether `pts` (or `via`) currently collides -- only meaningful in
       * `mark_obstacles` mode, where a violation is shown, not resolved. */
      colliding?: boolean;
      /** Already-fixed runs from earlier in this session (before a via/
       * layer switch put the head on a different layer). */
      runs?: { layer: string; pts: [Um, Um][] }[];
      via?: { x: Um; y: Um; diameter: Um; drill: Um } | null;
      snappedEnd?: [Um, Um] | null;
      /** `shove` mode only: other tracks the live head would push aside. */
      displaced?: { layer: string; pts: [Um, Um][] }[];
      /** Armed by the `V` hotkey: the next fix drops a via here and
       * continues on `pendingViaLayer`. */
      placingVia?: boolean;
      pendingViaLayer?: string;
    }
  | { kind: "zone"; pts: [Um, Um][] }
  | { kind: "shape"; shapeKind: "segment" | "arc" | "rect" | "circle" | "polygon"; pts: [Um, Um][] }
  /** `W` (Schematic tab): sch_line_wire_bus_tool.cpp's in-progress wire polyline -- see SchematicView.tsx's own doc for what this session ported vs. left out (free-angle only, no 90/45 posture, no auto-junction placement needed since that's a rendering-only concept here). */
  | { kind: "wire"; pts: [Um, Um][] }
  /** `Ctrl+Shift+M` (common.Interactive.measureTool, pcb_viewer_tools.cpp): a client-side-only ruler, never committed to the backend. 1 point = still dragging the end (rubber-banded to the cursor); 2 = a finished measurement that stays on screen (not cleared) until Esc or a fresh click starts the next one. */
  | { kind: "measure"; pts: [Um, Um][] };

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
  /** Which kind of item `refs` names -- each commits through a different Cmd (parts: move_to per ref; via/shape/text: their own move_* by id; symbol: schematic move_symbol; symbol_drag: schematic drag_symbol, see state.dragAttach). Defaults to "part" (every pre-existing caller moves parts). */
  kind?: "part" | "via" | "shape" | "text" | "symbol" | "symbol_drag";
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

/**
 * The 3D viewer's own view-option toggles -- a local-UI-state mirror of
 * KiCad's Preferences > 3D Viewer > Appearance per-layer visibility
 * bitset (BOARD_ADAPTER::GetVisibleLayers/SetVisibleLayers; see
 * PARITY-3d.md for the full list and which of its ~27 flags this app
 * does/doesn't have a data model for).
 */
export interface Viewer3DOptions {
  /** Master "show any part body" switch -- real KiCad has no single flag like this (show_footprints_normal/virtual/insert/etc are independent); kept as this app's own pre-existing umbrella on top of the new showTHT/showSMD split below, both of which must also be true for a given part's body to actually show. */
  showComponents: boolean;
  /** render.show_footprints_normal's rough equivalent -- a part counts as "TH" here if it has at least one through-hole pad (`pad.th`). */
  showTHT: boolean;
  /** The SMD counterpart: a part with no through-hole pads at all. */
  showSMD: boolean;
  showSilkscreen: boolean;
  showSolderMask: boolean;
  /** render.show_solderpaste -- new in this port (phase 3); see scene.ts's addSolderPaste. */
  showSolderPaste: boolean;
  /** render.show_board_body -- the dielectric slab itself (and, following it, both solder-mask layers, which have nothing to tint without it). Independent of showSilkscreen/showComponents, same as source. */
  showBoardBody: boolean;
  /** render.opengl_show_model_bbox, default false in source too -- a wireframe box per placed part, sized to its courtyard footprint and body height. */
  showBoundingBoxes: boolean;
  /** True = viewing the board flipped (as if turned over a horizontal hinge) so the bottom side reads right-way-up. A camera action (TrackballCamera.flip(), see kicad-port/camera3d.ts), not a geometry edit -- this flag is read only to know how many times to call it (see Viewer3D.tsx's prevFlippedRef). */
  flipped: boolean;
  /** True = a real orthographic projection (kicad-port/camera3d.ts's TrackballCamera, a faithful CAMERA::ToggleProjection port -- see that file). */
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
  showTHT: true,
  showSMD: true,
  showSilkscreen: true,
  showSolderMask: true,
  showSolderPaste: true,
  showBoardBody: true,
  showBoundingBoxes: false,
  flipped: false,
  orthographic: false,
  kicadModels: true,
};

/**
 * `ZONE_SETTINGS::ZONE_SETTINGS()`'s hardcoded defaults (`pcbnew/
 * zone_settings.cpp`), exactly mirroring `crates/model/src/ir.rs` `Zone`'s
 * own `impl Default` (same field values, same source comments there) --
 * what a brand-new zone gets from `add_zone` before `ZoneDialog.tsx`'s own
 * settings panel (or `api.addZone`'s unchanged-settings check) touches
 * anything.
 */
/**
 * `PCB_SELECTION::GetCenter()`'s real meaning: the center of the union
 * bounding box of every selected item, not an average of their individual
 * centers. Shared by `MoveExactDialog.tsx` (its own "selection center"
 * pivot option) and this file's `rotateSelection`/`flipSelection` (the
 * same shared-pivot rule `edit_tool.cpp`'s `updateModificationPoint` uses
 * for a plain multi-item R/Shift+R/F, not just the Move Exactly dialog).
 * Placed footprints only -- see `alignDistribute.ts`'s identical scope
 * note for why this app's Rotate/Flip/Align/Distribute family stops at
 * footprints rather than every selectable kind.
 */
export function selectionBoundsCenter(
  parts: ReadonlyArray<{ ref: string; placed: boolean; courtyard?: readonly [number, number, number, number] | null }>,
  refs: readonly string[]
): { x: number; y: number } | null {
  let x0 = Infinity,
    y0 = Infinity,
    x1 = -Infinity,
    y1 = -Infinity;
  let any = false;
  for (const ref of refs) {
    const p = parts.find((q) => q.ref === ref);
    if (!p?.placed || !p.courtyard) continue;
    any = true;
    x0 = Math.min(x0, p.courtyard[0]);
    y0 = Math.min(y0, p.courtyard[1]);
    x1 = Math.max(x1, p.courtyard[2]);
    y1 = Math.max(y1, p.courtyard[3]);
  }
  return any ? { x: (x0 + x1) / 2, y: (y0 + y1) / 2 } : null;
}

export const DEFAULT_ZONE_SETTINGS: ZoneSettingsFields = {
  clearance: 500,
  min_thickness: 250,
  thermal_gap: 500,
  thermal_spoke_width: 500,
  pad_connection: "Thermal",
  priority: 0,
  island_removal_mode: "Always",
  min_island_area: 10_000_000,
  fill_mode: "Polygons",
  hatch_thickness: 1000,
  hatch_gap: 1500,
  hatch_orientation_mdeg: 0,
  hatch_smoothing_level: 0,
  hatch_smoothing_value: 0.1,
  hatch_hole_min_area: 0.15,
  hatch_border_algorithm: 1,
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
  /** A just-drawn zone outline waiting for its settings to be confirmed in ZoneDialog before `add_zone` commits it. */
  zonePending: [Um, Um][] | null;
  /** A selected zone's id ("E", or double-click) -- opens ZoneDialog in edit mode (`edit_zone`) instead of add mode. Independent of `zonePending` (one or the other is ever set, never both). */
  zoneEditId: string | null;
  /**
   * `pcbnew.ZoneFiller.zoneFillAll`/`zoneUnfillAll` (B/Ctrl+B,
   * zone_filler_tool.cpp): the last GET /api/fill this session asked
   * for, or null if zones have never been filled (or were just
   * unfilled) -- the "outline only, nothing computed" state every zone
   * starts in, same as a freshly drawn KiCad zone. Kept fresh by the
   * same version-change poll `ratsnest`/`drc` already use, gated on
   * this being non-null (see StudioProvider's poll loop) -- unlike real
   * KiCad, there is no "stale fill" hatch state to show, since
   * `/api/fill` is cheap to recompute and never cached either side.
   */
  zoneFill: FillReport | null;
  /**
   * `pcbnew.Control.zoneDisplayEnable`/`zoneDisplayDisable`/
   * `zoneDisplayToggle` (`ZONE_DISPLAY_MODE`): how a zone that DOES have
   * fill data (`zoneFill` above) paints -- solid copper, or just its
   * outline. A zone with no fill data yet always shows its outline
   * regardless of this mode, same as source (nothing to fill with).
   * KiCad's other two modes (fracture-borders/triangulation) are
   * developer debug views, not ported -- see PARITY-pcb.md.
   */
  zoneDisplayMode: "filled" | "outline";
  /** Board Setup... (dialog_board_setup.cpp) -- net classes/track-via sizing/rules/etc, see BoardSetupDialog.tsx. */
  boardSetupDialogOpen: boolean;
  /** File > Fabrication Outputs > Gerbers... (dialog_plot.cpp), see PlotDialog.tsx. */
  plotDialogOpen: boolean;
  /** File > Fabrication Outputs > Drill Files... (dialog_gendrill.cpp), see GenerateDrillDialog.tsx. */
  generateDrillDialogOpen: boolean;
  /** File > Fabrication Outputs > Component Placement... (dialog_gen_footprint_position.cpp), see FootprintPositionDialog.tsx. */
  footprintPositionDialogOpen: boolean;
  /**
   * `pcbnew.EditorControl.trackWidthInc`/`trackWidthDec` (W/Shift+W):
   * the board's own default (`board_rules.track_width`) plus
   * `routing.track_width_presets`' index the cycle is currently on, as
   * an actual width rather than an index so it survives the preset list
   * changing underneath it -- null until the first W/Shift+W press
   * (every route/new-track client falls back to `board_rules.track_width`
   * until then, same value this would resolve to anyway).
   */
  currentTrackWidthUm: Um | null;
  /** Same idea as `currentTrackWidthUm`, for `pcbnew.EditorControl.viaSizeInc`/`viaSizeDec` cycling `board_rules.via_diameter`/`via_drill` plus `routing.via_presets`. */
  currentViaPreset: ViaPreset | null;
  /** The text tool/E-to-edit dialog: "add" (fresh, at a clicked point) or "edit" (an existing text's id). */
  textDialog: { mode: "add"; at: [Um, Um] } | { mode: "edit"; id: string } | null;
  /** `L`/Ctrl+`L`/`H`: a just-clicked point waiting for LabelDialog to confirm its text (and, for global/hierarchical, its shape) before `add_label` commits -- same "draw/click first, dialog last" shape as `zonePending`. */
  schLabelPending: { at: [Um, Um]; scope: LabelScope } | null;
  /** `P`: a just-clicked point (already pin-snapped, see SchematicView.tsx's `pinSnapPoints`) waiting for PowerSymbolDialog to confirm which rail. */
  schPowerPending: { at: [Um, Um] } | null;
  /** `T`: a just-clicked point waiting for SchTextDialog to confirm the content. */
  schTextPending: { at: [Um, Um] } | null;
  /** `A`: SymbolChooserDialog's own open/closed flag. */
  symbolChooserOpen: boolean;
  /** `E`/`U`/`V`/`F` on a selected symbol: SymbolPropertiesDialog's own open/closed+focus state -- `field` picks which input autofocuses (`E` opens the same dialog with nothing singled out). */
  symbolProperties: { id: string; field: "reference" | "value" | "footprint" | "datasheet" | null } | null;
  /** `Ctrl+A`: AnnotateDialog's own open/closed flag (dialog_annotate.cpp's scope/order/reset options). */
  annotateDialogOpen: boolean;
  /** `A`: the symbol SymbolChooserDialog confirmed, waiting for a canvas click to place it (`sch_place_symbol` tool) -- `referencePrefix` seeds `nextReference`'s own next-free-number placement (this app's own choice: a real id immediately, not a "U?" placeholder -- see `Cmd::AddSymbol`'s doc and PARITY-sch.md). `unit`: which unit of a multi-unit symbol to place (the chooser's own unit picker, shown when `SymbolLibraryEntry.unit_count > 1`; omitted/1 for a single-unit part). */
  armedSymbol: { libId: string; referencePrefix: string; unit?: number } | null;
  /** `createNewLabel`'s own "last text used" (`m_lastTextOrientation`-style session memory, see `incrementLabelText`) -- seeds the next LabelDialog with an auto-incremented suggestion instead of starting blank every time, so placing a same-shaped bus of labels (DATA0, DATA1, DATA2...) doesn't mean re-typing the whole name each click. */
  lastLabelText: string;
  /** `P`'s own last-chosen rail (e.g. "power:GND") -- seeds PowerSymbolDialog so placing several of the same rail in a row (common -- a row of decoupling caps all going to GND) only needs one pick. */
  lastPowerLibId: string;

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
   * `G`'s own attachment data: which `(wire index, point index)` pairs
   * (into `schematic.wires[i].pts`) are glued to which selected symbol's
   * pin, resolved once when a drag starts -- see
   * `wireAttachment.ts::computeDragAttachment`'s own doc for why this is
   * frozen at drag-start rather than recomputed live (same reasoning
   * `moveOriginUm` itself is only ever set, never explicitly cleared: the
   * next drag always overwrites it, and it's only ever read while a
   * "symbol_drag" move/preview is actually active). Read by
   * SchematicView.tsx (the live rubber-band preview) and `commitMove`'s
   * own "symbol_drag" branch (the real `Cmd::DragSymbol` call).
   */
  dragAttach: Record<string, [number, number][]> | null;
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
  zoneEditId: null,
  zoneFill: null,
  zoneDisplayMode: "filled",
  boardSetupDialogOpen: false,
  plotDialogOpen: false,
  generateDrillDialogOpen: false,
  footprintPositionDialogOpen: false,
  currentTrackWidthUm: null,
  currentViaPreset: null,
  textDialog: null,
  schLabelPending: null,
  schPowerPending: null,
  schTextPending: null,
  symbolChooserOpen: false,
  armedSymbol: null,
  symbolProperties: null,
  annotateDialogOpen: false,
  lastLabelText: "",
  lastPowerLibId: "power:GND",
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
  dragAttach: null,
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
  | { type: "SET_DRAG_ATTACH"; attach: Record<string, [number, number][]> | null }
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
  | { type: "SET_ZONE_EDIT_ID"; id: string | null }
  | { type: "FILL_OK"; fill: FillReport }
  | { type: "CLEAR_ZONE_FILL" }
  | { type: "SET_ZONE_DISPLAY_MODE"; mode: "filled" | "outline" }
  | { type: "SET_BOARD_SETUP_DIALOG_OPEN"; open: boolean }
  | { type: "SET_PLOT_DIALOG_OPEN"; open: boolean }
  | { type: "SET_GENERATE_DRILL_DIALOG_OPEN"; open: boolean }
  | { type: "SET_FOOTPRINT_POSITION_DIALOG_OPEN"; open: boolean }
  | { type: "SET_CURRENT_TRACK_WIDTH"; widthUm: Um }
  | { type: "SET_CURRENT_VIA_PRESET"; preset: ViaPreset }
  | { type: "SET_TEXT_DIALOG"; dialog: StudioState["textDialog"] }
  | { type: "SET_SCH_LABEL_PENDING"; pending: StudioState["schLabelPending"] }
  | { type: "SET_SCH_POWER_PENDING"; pending: StudioState["schPowerPending"] }
  | { type: "SET_SCH_TEXT_PENDING"; pending: StudioState["schTextPending"] }
  | { type: "SET_LAST_LABEL_TEXT"; text: string }
  | { type: "SET_LAST_POWER_LIB_ID"; libId: string }
  | { type: "SET_SYMBOL_CHOOSER_OPEN"; open: boolean }
  | { type: "SET_ARMED_SYMBOL"; symbol: StudioState["armedSymbol"] }
  | { type: "SET_SYMBOL_PROPERTIES"; value: StudioState["symbolProperties"] }
  | { type: "SET_ANNOTATE_DIALOG_OPEN"; open: boolean }
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
      return {
        ...state,
        selection: new Set(),
        armed: null,
        movePreview: null,
        activeTool: "select",
        drawState: null,
        zonePending: null,
        zoneEditId: null,
        textDialog: null,
        itemPropertiesId: null,
        schLabelPending: null,
        schPowerPending: null,
        schTextPending: null,
        armedSymbol: null,
        symbolProperties: null,
      };
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
        return {
          ...state,
          armed: null,
          movePreview: null,
          activeTool: "select",
          drawState: null,
          zonePending: null,
          zoneEditId: null,
          textDialog: null,
          schLabelPending: null,
          schPowerPending: null,
          schTextPending: null,
          armedSymbol: null,
          symbolProperties: null,
        };
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
    case "SET_DRAG_ATTACH":
      return { ...state, dragAttach: action.attach };
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
    case "SET_ZONE_EDIT_ID":
      return { ...state, zoneEditId: action.id };
    case "FILL_OK":
      return { ...state, zoneFill: action.fill };
    case "CLEAR_ZONE_FILL":
      // zone_filler_tool.cpp ZoneUnfillAll: discards the computed fill;
      // the canvas falls back to outline-only (painter.ts), same as a
      // zone that has never been filled at all.
      return { ...state, zoneFill: null };
    case "SET_ZONE_DISPLAY_MODE":
      return { ...state, zoneDisplayMode: action.mode };
    case "SET_BOARD_SETUP_DIALOG_OPEN":
      return { ...state, boardSetupDialogOpen: action.open };
    case "SET_PLOT_DIALOG_OPEN":
      return { ...state, plotDialogOpen: action.open };
    case "SET_GENERATE_DRILL_DIALOG_OPEN":
      return { ...state, generateDrillDialogOpen: action.open };
    case "SET_FOOTPRINT_POSITION_DIALOG_OPEN":
      return { ...state, footprintPositionDialogOpen: action.open };
    case "SET_CURRENT_TRACK_WIDTH":
      return { ...state, currentTrackWidthUm: action.widthUm };
    case "SET_CURRENT_VIA_PRESET":
      return { ...state, currentViaPreset: action.preset };
    case "SET_TEXT_DIALOG":
      return { ...state, textDialog: action.dialog };
    case "SET_SCH_LABEL_PENDING":
      return { ...state, schLabelPending: action.pending };
    case "SET_SCH_POWER_PENDING":
      return { ...state, schPowerPending: action.pending };
    case "SET_SCH_TEXT_PENDING":
      return { ...state, schTextPending: action.pending };
    case "SET_LAST_LABEL_TEXT":
      return { ...state, lastLabelText: action.text };
    case "SET_LAST_POWER_LIB_ID":
      return { ...state, lastPowerLibId: action.libId };
    case "SET_SYMBOL_CHOOSER_OPEN":
      return { ...state, symbolChooserOpen: action.open };
    case "SET_ARMED_SYMBOL":
      return { ...state, armedSymbol: action.symbol };
    case "SET_SYMBOL_PROPERTIES":
      return { ...state, symbolProperties: action.value };
    case "SET_ANNOTATE_DIALOG_OPEN":
      return { ...state, annotateDialogOpen: action.open };
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
  /** pcbnew.AlignAndDistribute.align* (no default hotkey in source either -- reached from the right-click menu, Canvas.tsx's onContextMenu). Placed footprints only -- see kicad-port/alignDistribute.ts's own scope note. A no-op under 2 placed parts, same floor source's menu-visibility condition enforces. */
  alignSelection: (edge: AlignEdge) => Promise<void>;
  /** pcbnew.AlignAndDistribute.distribute* -- same placed-footprints-only scope. A no-op under 3 placed parts. */
  distributeSelection: (axis: "x" | "y", mode: "gaps" | "centers") => Promise<void>;
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
  schTextById: (id: string) => SchematicText | undefined;
  /** R/Shift+R on the Schematic tab: rotate a symbol in place (quarterTurns: 1 = CCW/'R', 3 = CW/Shift+R, matching sch_edit_tool.cpp's own default). */
  rotateSymbol: (id: string, quarterTurns: number) => Promise<void>;
  /** X on the Schematic tab ("Mirror Horizontally"). */
  mirrorSymbol: (id: string) => Promise<void>;
  /** Y on the Schematic tab ("Mirror Vertically") -- mutually exclusive with `mirrorSymbol` on the backend (Cmd::MirrorSymbolVertical's own doc). */
  mirrorSymbolVertical: (id: string) => Promise<void>;
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
  /**
   * ZoneDialog's "Add Zone": `add_zone` (net/layer/outline only, same as
   * `Cmd::AddZone`'s own shape -- see that type's doc on why it stays
   * that way) then, if the dialog's settings aren't every one of
   * `Zone::default()`'s own values, an immediate follow-up `edit_zone` on
   * the fresh zone's id (found the same before/after-id-diff way
   * `duplicateSelection`/`pasteClipboard` already do, since the reply has
   * no structured "here's what I made" field). Two Cmds, not one atomic
   * `Cmd::AddZone` with inline settings, so every existing caller of that
   * Cmd (the CLI, this app's own tests) keeps its exact three-field shape.
   */
  addZone: (net: string, layer: string, outline: [Um, Um][], settings: ZoneSettingsFields) => Promise<void>;
  /** B ("Fill All Zones"): GET /api/fill now, and keep it live-updated (state.zoneFill) until `unfillZones`. */
  fillZones: () => Promise<void>;
  /** Ctrl+B ("Unfill All Zones"): back to outline-only, same as a zone that was never filled. */
  unfillZones: () => void;
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

  const refreshFill = useCallback(async () => {
    try {
      const fill = await fetchFill();
      dispatch({ type: "FILL_OK", fill });
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
    let lastFillFetch: string | null = null;
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
        // Gated on "has been filled at least once this session" (not a
        // dialog -- there isn't one, B/Ctrl+B just toggle a canvas
        // rendering mode) -- same reasoning as DRC/ERC above: computing
        // fills nobody is looking at would be pure waste, but once shown
        // they should track the board live, same as KiCad's own
        // auto-refill-on-edit behavior (ZONE_FILLER_TOOL::ZoneFillDirty).
        if (stateRef.current.zoneFill != null && lastFillFetch !== v) {
          lastFillFetch = v;
          await refreshFill();
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
  }, [refresh, refreshSchematic, refreshRatsnest, refreshDrc, refreshErc, refreshFill]);

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
    schTextById: (id) => stateRef.current.schematic?.texts.find((t) => t.id === id),
    rotateSymbol: async (id, quarterTurns) => {
      await runCmd({ op: "rotate_symbol", id, quarter_turns: ((quarterTurns % 4) + 4) % 4 });
    },
    mirrorSymbol: async (id) => {
      await runCmd({ op: "mirror_symbol", id });
    },
    mirrorSymbolVertical: async (id) => {
      await runCmd({ op: "mirror_symbol_vertical", id });
    },
    deleteSymbol: async (id) => {
      dispatch({ type: "CLEAR_SELECTION" });
      await runCmd({ op: "delete_symbol", id });
    },
    cmd: (c) => runCmd(c),
    // edit_tool.cpp's real Rotate: a single selected item spins about its
    // own anchor (dx=dy=0, no pivot -- Cmd::MoveExact's own pivot:None
    // branch lands on exactly the same pose Cmd::Rotate's simpler
    // part+quarter_turns shape would); 2+ items share ONE pivot --
    // updateModificationPoint's `aSelection.GetCenter()`, the selection's
    // union-bounding-box center -- and commit as ONE MoveExact, matching
    // source's own single BOARD_COMMIT::Push() for the whole group
    // (previously: one independent Cmd::Rotate per part, each spinning in
    // place about its own anchor -- see PARITY-pcb.md's "Edit tool"
    // section for why that was a documented simplification, not the real
    // behavior).
    rotateSelection: async (quarterTurns) => {
      const refs = [...stateRef.current.selection].filter((r) => api.partByRef(r)?.placed);
      if (refs.length === 0) return;
      const rotateMillideg = (((quarterTurns % 4) + 4) % 4) * 90_000;
      const pivot = refs.length > 1 ? selectionBoundsCenter(stateRef.current.board?.parts ?? [], refs) : null;
      await runCmd({ op: "move_exact", parts: refs, dx: 0, dy: 0, rotate_millideg: rotateMillideg, pivot: pivot ? { x: pivot.x, y: pivot.y } : null });
    },
    ripSelection: async () => {
      const refs = [...stateRef.current.selection];
      dispatch({ type: "CLEAR_SELECTION" });
      for (const ref of refs) await runCmd({ op: "rip", part: ref });
    },
    // edit_tool.cpp's real Flip: a single item flips about its own anchor
    // (no position change, only `side` toggles -- Cmd::Flip's existing
    // shape already does exactly this); 2+ items share the selection's
    // bounding-box center as the mirror line's X, same `updateModification
    // Point`/`GetCenter()` rule Rotate uses above. Cmd::Flip has no pivot
    // concept of its own (it only ever toggles `side`), so a group flip is
    // composed client-side as "move to the mirrored X, then flip" per
    // part, the same two-Cmd composition `commitMove` already uses for a
    // single dragged-and-flipped part.
    flipSelection: async () => {
      const refs = [...stateRef.current.selection].filter((r) => api.partByRef(r)?.placed);
      if (refs.length === 0) return;
      const center = refs.length > 1 ? selectionBoundsCenter(stateRef.current.board?.parts ?? [], refs) : null;
      for (const ref of refs) {
        if (center) {
          const p = api.partByRef(ref)!;
          const [x, y] = p.at!;
          await runCmd({ op: "move_to", part: ref, x: 2 * center.x - x, y });
        }
        await runCmd({ op: "flip", part: ref });
      }
    },
    alignSelection: async (edge) => {
      const refs = [...stateRef.current.selection].filter((r) => {
        const p = api.partByRef(r);
        return p?.placed && p.courtyard && p.at;
      });
      if (refs.length < 2) return;
      const boxes: Box[] = refs.map((r) => api.partByRef(r)!.courtyard!);
      const deltas = alignDeltas(boxes, edge);
      const axis = alignAxis(edge);
      for (let i = 0; i < refs.length; i++) {
        const d = deltas[i]!;
        if (d === 0) continue;
        const p = api.partByRef(refs[i]!)!;
        const [x, y] = p.at!;
        await runCmd({ op: "move_to", part: refs[i]!, x: axis === "x" ? x + d : x, y: axis === "y" ? y + d : y });
      }
    },
    distributeSelection: async (axis, mode) => {
      const refs = [...stateRef.current.selection].filter((r) => {
        const p = api.partByRef(r);
        return p?.placed && p.courtyard && p.at;
      });
      if (refs.length < 3) return;
      // align_distribute_tool.cpp's doDistributeGaps/doDistributeCenters:
      // sort by start (gaps) or by center (centers) along the chosen axis
      // before computing deltas -- the end caps of THIS sort never move.
      const key = (box: Box): number => {
        const lo = axis === "x" ? box[0] : box[1];
        const hi = axis === "x" ? box[2] : box[3];
        return mode === "gaps" ? lo : (lo + hi) / 2;
      };
      const sorted = refs.map((r) => ({ r, box: api.partByRef(r)!.courtyard! })).sort((a, b) => key(a.box) - key(b.box));
      const deltas =
        mode === "gaps"
          ? getDeltasForDistributeByGaps(sorted.map(({ box }) => (axis === "x" ? [box[0], box[2]] : [box[1], box[3]]) as [number, number]))
          : getDeltasForDistributeByPoints(sorted.map(({ box }) => key(box)));
      for (let i = 0; i < sorted.length; i++) {
        const d = deltas[i]!;
        if (d === 0) continue;
        const p = api.partByRef(sorted[i]!.r)!;
        const [x, y] = p.at!;
        await runCmd({ op: "move_to", part: sorted[i]!.r, x: axis === "x" ? x + d : x, y: axis === "y" ? y + d : y });
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
        } else if (kind === "symbol_drag") {
          // `G`: same as "symbol" above, except the wire endpoints
          // `state.dragAttach` resolved for this ref at drag-start move
          // along with it (sch_move_tool.cpp's rubber-band) -- see
          // `Cmd::DragSymbol`'s own doc. A ref with no recorded attachment
          // (shouldn't happen -- computeDragAttachment covers every
          // dragged symbol -- but cheap to default safely) just drags with
          // nothing glued to it, same as a plain move.
          const s = api.symbolById(ref);
          if (s) {
            const attached = stateRef.current.dragAttach?.[ref] ?? [];
            await runCmd({ op: "drag_symbol", id: ref, x: s.at[0] + dxUm, y: s.at[1] + dyUm, attached_wire_endpoints: attached });
          }
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
    addZone: async (net, layer, outline, settings) => {
      const board = stateRef.current.board;
      if (!board) return;
      const before = allItemIds(board);
      const ok = await runCmd({ op: "add_zone", net, layer, outline: outline.map(([x, y]) => ({ x, y })) });
      if (!ok) return;
      const after = stateRef.current.board;
      if (!after) return;
      const newId = [...allItemIds(after)].find((id) => !before.has(id));
      if (!newId) return;
      // Default-valued settings need no follow-up at all -- `add_zone`
      // already landed on exactly that.
      const unchanged = (Object.keys(DEFAULT_ZONE_SETTINGS) as (keyof ZoneSettingsFields)[]).every((k) => settings[k] === DEFAULT_ZONE_SETTINGS[k]);
      if (!unchanged) await runCmd({ op: "edit_zone", id: newId, net, layer, ...settings });
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
    fillZones: refreshFill,
    unfillZones: () => dispatch({ type: "CLEAR_ZONE_FILL" }),
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
