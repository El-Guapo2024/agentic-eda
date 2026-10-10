// Hand-rolled state container (no state-management library, per the
// project's dependency limits): a useReducer store for pure UI state,
// plus a handful of async functions closed over `dispatch` for anything
// that talks to the backend. Board truth always comes back through the
// /api/version + /api/state poll -- these functions POST a command and
// then nudge the poller to refetch immediately, exactly like the old
// studio.html's `send()`.

import React, { createContext, useCallback, useContext, useEffect, useReducer, useRef } from "react";
import type { BoardState, BoardText, Cmd, CmdDimension, CmdDimensionKind, Dimension, DrcReport, ErcReport, FillReport, Group, LabelScope, LintReport, Part, Ratsnest, RuleAreaFields, Schematic, SchematicSymbol, SchSearchData, SchematicText, SchematicWire, Shape, Track, TuneMode, Um, Via, ViaPreset, Zone, ZoneSettingsFields } from "../api/types";
import { defaultSearch } from "../kicad-port/schFind";
import { initialNavHistory, pushToHistory, type NavHistory } from "../kicad-port/navHistory";
import { revisionOf } from "../kicad-port/checkRevision";
import { patchDrcExcluded, patchDrcSeverity, patchErcSeverity } from "../kicad-port/rcItems";
import type { LineMode } from "../kicad-port/schLineMode";
import { fetchDrc, fetchErc, fetchFill, fetchLint, fetchRatsnest, fetchSchematic, fetchState, fetchVersion, fetchView, postClipboardCopy, postCmd, postRedo, postRoute, postUndo, postView, type SharedView } from "../api/client";
import { fitTransform } from "../kicad-port/view";
import type { LengthUnit } from "./units";
import { STANDARD_LAYERS } from "../components/canvas/layers";
import { DEFAULT_SELECTION_FILTER, type SelectionFilter } from "../components/canvas/selectionCandidates";
import { allItemIds, isKicadPcbText, newItemIds, type ClipboardContents } from "../components/canvas/clipboard";
import { subItemIds } from "../kicad-port/pcbItems";
import { planAlignSelection, planDistributeSelection } from "../kicad-port/alignDistribute";
import { editableSelection, planCarry, planFlip, planRotate, type TransformPlan } from "../kicad-port/pcbTransform";
import { snapPoint } from "../components/canvas/gridHelper";
import { readClipboardText, writeClipboardText } from "../api/libraryClient";
import { DEFAULT_PCB_PARITY, type PcbParityState } from "../kicad-port/pcbParityState";
import { substituteSelection } from "../kicad-port/groupTree";
import { DEFAULT_BOARD_CONTROL, withHighlight, type BoardControlState } from "../kicad-port/boardControlState";
import { DEFAULT_APPEARANCE, withObjectKeys, type AppearanceState } from "../kicad-port/appearance";
import { reduceView, type AppearanceOp, type ViewSlice } from "../kicad-port/appearanceOps";
import { applyAppearanceFile } from "../kicad-port/appearanceFile";
import { netsContext } from "../kicad-port/appearanceNets";
import { keepFilled } from "../kicad-port/boardControl";
import type { ArcGeom } from "../kicad-port/arcGeom";
import type { BezierGeom } from "../kicad-port/bezierGeom";
import { movableItem } from "../kicad-port/pcbEditActions";
import { repeatSource } from "../kicad-port/schRepeat";
import { onCurrentSheet } from "../kicad-port/schSheetCmd";
import { samePath } from "../kicad-port/sheetPages";
import { loadPreferences, savePreferences, type Preferences } from "../kicad-port/preferences";
import { DEFAULT_ROUTER_SETTINGS, type RouterSettings } from "../kicad-port/routerSettings";
import { keepOnSheet } from "../kicad-port/schSelectionPrune";
import { DEFAULT_SCH_SELECTION_FILTER, type SchSelectionFilter } from "../kicad-port/schSelectionFilter";
import type { SchToolDialog, SchTurn } from "../api/schEditTypes";
import { heldCmd, holdPoint, turnCmd } from "../kicad-port/schMove";
import type { ShapeEdit } from "../kicad-port/schShapeEdit";
import type { PolyGeom } from "../kicad-port/polygonGeom";
import type { BreakState } from "../kicad-port/schBreak";
import type { PinPlacement } from "../components/schematic/schPinTool";
import type { AlignEdge } from "../kicad-port/alignDistribute";

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
export type EditorTab = "pcb" | "schematic" | "footprint" | "symbol" | "3d";

/** `StudioState.schDialog`: which schematic tool dialog is open (null = none). "find"/"replace" are the same dialog (DIALOG_SCH_FIND) with the replace controls shown or not. */
export type SchDialog = "fields_table" | "find" | "replace" | "setup" | null;

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
  /** `6` (gap #7 task item 6, pcbnew.InteractiveRouter.DiffPair): route a differential pair -- see kicad-port/dpTool.ts's own header comment. */
  | "diffpair"
  | "via"
  | "zone"
  | "draw_segment"
  | "draw_arc"
  /** `pcbnew.InteractiveDrawing.bezier` (Ctrl+Shift+B): four clicks (start, control 1, end, control 2), chained -- see kicad-port/bezierGeom.ts. */
  | "draw_bezier"
  | "draw_rect"
  | "draw_circle"
  | "draw_polygon"
  | "text"
  | "wire"
  /** `B` (GAPS.md #20): same click-to-add-point/finish state machine as
   * "wire" (`DrawState`'s `"wire"` kind is shared by both -- see
   * SchematicView.tsx's own doc), just tagged `Cmd::AddWire { bus: true }`
   * at commit time instead of a plain wire. */
  | "bus"
  | "measure"
  /** Task item 7: two clicks (start, end) for whichever of the five
   * dimension kinds `state.nextDimensionKind` names -- see `DrawState`'s
   * own `"dimension"` kind. */
  | "dimension"
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
  /** GAPS.md #20: `eeschema.InteractiveDrawing.placeBusWireEntry` -- a
   * single click drops a fixed-size (±100mil, same as real KiCad's own
   * default) bus entry there, same one-click-commits-and-stays-armed shape
   * as `sch_no_connect` above (no orientation picker -- a documented
   * simplification; the IR itself places no restriction on `size`'s sign,
   * so an imported file's own entry in any of the 4 diagonal quadrants
   * still round-trips/renders correctly). */
  | "sch_bus_entry"
  /** `J` (eeschema.InteractiveDrawing.placeJunction): each click places an explicit junction where wires/pins meet -- see kicad-port/schJunction.ts. */
  | "sch_junction"
  /** `I` (eeschema.InteractiveDrawingLineWireBus.drawLines): the wire tool's click-to-add-point state machine, committing a graphic notes-layer line (`add_sch_line`) instead of a wire. */
  | "sch_line"
  /** `S` (eeschema.InteractiveDrawing.drawSheet): two clicks (opposite corners) size a hierarchical sheet, then its properties dialog names it. */
  | "sch_sheet"
  /** `A`: armed once SymbolChooserDialog confirms a choice -- see `state.armedSymbol`. */
  | "sch_place_symbol"
  /** `eeschema.EditorControl.highlightNetTool` (Highlight Nets): each click highlights the net under it, the tool stays armed -- see SchematicView.tsx. */
  | "sch_highlight_net"
  /** The schematic's shape tools (`SCH_DRAWING_TOOLS::DrawShape` / `DrawRuleArea` / `TwoClickPlace` for a directive label): kicad-port/schShapeEdit.ts, polygonGeom.ts. */
  | "sch_rect"
  | "sch_circle"
  | "sch_arc"
  | "sch_bezier"
  | "sch_textbox"
  | "sch_rule_area"
  | "sch_directive"
  /** Break / Slice (`SCH_MOVE_TOOL`'s `BREAK` / `SLICE` modes): the cut wire's new end follows the cursor until the click that drops it -- kicad-port/schBreak.ts. */
  | "sch_break"
  /** Place Pins from Sheet (`TwoClickPlace` with `placeSheetPin`): each click drops the next hierarchical label of the sheet's file as a pin on its border -- components/schematic/schPinTool.ts. */
  | "sch_sheet_pin"
  /** `common.Control.zoomTool` (Ctrl+F5, zoom_tool.cpp): drag a rectangle to zoom to it -- see components/ZoomAreaOverlay.tsx. */
  | "zoom_area"
  /** `pcbnew.EditorControl.drillOrigin` (Place > Drill/Place File Origin): one click sets the origin and the tool ends (`DrillOrigin`'s picker: "a one-shot"). */
  | "drill_origin"
  /** `pcbnew.Control.localRatsnestTool`: click a pad (or, failing that, a footprint) to show or hide its ratsnest lines; clicking off everything resets them; Esc leaves. */
  | "local_ratsnest";
export const TOOL_MESSAGES: Record<ToolId, string> = {
  select: "Select item(s)",
  move: "Move item(s)",
  drag: "Drag (keeps connections): click to drop, Esc to cancel",
  route: "Route track: click to add a point, V for via, Enter/double-click to finish, Esc to cancel",
  diffpair: "Route differential pair: click to add a point, Enter/double-click to finish, Esc to cancel",
  via: "Click to place a via",
  zone: "Zone: click to add points, Enter/double-click to finish, Esc to cancel",
  draw_segment: "Line: click start, then end",
  draw_arc: "Arc: click the centre, then the start point, then the end point (/ switches the direction)",
  draw_bezier: "Bezier: click the start, control 1, the end, then control 2 (the curve then continues from its end); double-click to finish, Esc to cancel",
  draw_rect: "Rectangle: click one corner, then the opposite one",
  draw_circle: "Circle: click center, then a point on the edge",
  draw_polygon: "Polygon: click points, Enter/double-click to finish, Esc to cancel",
  text: "Click to place text",
  wire: "Wire: click to start/add a point (snaps to a pin when close), double-click or Enter to finish, Backspace to undo the last point, Esc to cancel",
  bus: "Bus: click to start/add a point, double-click or Enter to finish, Backspace to undo the last point, Esc to cancel",
  measure: "Measure: click a start point, click again for the end point. Click anywhere to start a new measurement, Esc to clear",
  dimension: "Dimension: click the first feature point, then the second",
  sch_label_local: "Label: click where to place it",
  sch_label_global: "Global Label: click where to place it",
  sch_label_hier: "Hierarchical Label: click where to place it",
  sch_power: "Power Symbol: click a pin (snaps to the nearest one)",
  sch_text: "Text: click where to place it",
  sch_no_connect: "No Connect: click a pin to flag it unconnected",
  sch_bus_entry: "Bus Entry: click a point on a bus to tap a wire into it",
  sch_junction: "Junction: click where wires/pins meet (three or more directions) to join them, Esc to finish",
  sch_line: "Line: click to start/add a point, double-click or Enter to finish, Backspace to undo the last point, Esc to cancel",
  sch_sheet: "Hierarchical Sheet: click one corner, then the opposite one, then name the sheet. Esc to cancel",
  sch_place_symbol: "Place Symbol: click where to place it",
  sch_highlight_net: "Highlight Nets: click a wire, label or power symbol to highlight its net (click empty space to clear), Esc to leave",
  sch_rect: "Rectangle: click one corner, then the opposite one. Esc to cancel",
  sch_circle: "Circle: click the centre, then a point on the circle. Esc to cancel",
  sch_arc: "Arc: click the start, then the end. Esc to cancel",
  sch_bezier: "Bezier curve: click the start, the end, then control points 1 and 2. Esc to cancel",
  sch_textbox: "Text Box: click one corner, then the opposite one, then enter the text. Esc to cancel",
  sch_rule_area: "Rule Area: click the corners, double-click or press End to close it, Backspace to remove the last corner. Esc to cancel",
  sch_directive: "Directive Label: click where to place it",
  sch_break: "Break / Slice: move the new end of the wire, click to drop it, Esc to cancel",
  sch_sheet_pin: "Place Pins from Sheet: click over a sheet, then click along its border to drop each pin. Esc to cancel",
  zoom_area: "Zoom to Selection Area: drag a rectangle (left button zooms in, right button zooms out), Esc to cancel",
  drill_origin: "Drill/Place File Origin: click to place it, Esc to cancel",
  local_ratsnest: "Local Ratsnest: click a pad or footprint to show or hide its ratsnest, click empty space to reset, Esc to leave",
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
  /** `S` (`SCH_DRAWING_TOOLS::DrawSheet`): the first corner of the sheet being sized; the second click ends it (`sizeSheet`). */
  | { kind: "sheet"; start: [Um, Um] }
  /** The schematic shape being drawn (`DrawShape` / `DrawRuleArea`): `shape` for a rectangle, circle, arc, Bezier curve or text box, `poly` for a rule area; `brk` is a Break / Slice waiting for its drop. */
  | { kind: "sch_shape"; shape?: ShapeEdit; poly?: PolyGeom; brk?: BreakState; pin?: PinPlacement }
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
      /** `shove` mode only: other vias the live head would push aside. */
      displacedVias?: { source_via: string; x: Um; y: Um }[];
      /** Armed by the `V` hotkey: the next fix drops a via here and
       * continues on `pendingViaLayer`. */
      placingVia?: boolean;
      pendingViaLayer?: string;
    }
  /** `D` (gap #7 stage 5, `pcbnew.InteractiveRouter.Drag45Degree` ->
   * `eda_pns::dragger::Dragger`, driven through crates/cli/src/route_api.rs's
   * drag_{start,move,finish}): an in-progress drag of an existing track
   * segment/corner or via, keeping its connections -- see
   * kicad-port/dragTool.ts's own header comment for the full gesture and
   * `DragDrawState`, which this duplicates (dependency-free module,
   * manually kept in sync, same convention as the "route" variant above). */
  | {
      kind: "drag";
      dragKind: "corner" | "via";
      net: string | null;
      layer: string;
      width: Um;
      viaDiameter?: Um;
      /** The dragged item's live shape: the stretched line (corner) or
       * just the via's own new position, one point (via) -- see
       * `DragPreview`'s own doc comment. */
      pts: [Um, Um][];
      colliding?: boolean;
      /** `shove` mode only: other tracks/vias this drag would push aside. */
      displaced?: { layer: string; pts: [Um, Um][] }[];
      displacedVias?: { source_via: string; x: Um; y: Um }[];
      /** Via drag only: its own directly-attached tracks, already
       * stretched to follow it live, each with its own real width. */
      fanout?: { layer: string; width: Um; pts: [Um, Um][] }[];
    }
  /** `6` (gap #7 task item 6, pcbnew.InteractiveRouter.DiffPair ->
   * `eda_pns::diff_pair::DiffPairPlacer`): an in-progress differential-
   * pair route, both lines at once -- see kicad-port/dpTool.ts's own
   * header comment for the full gesture and `DpDrawState`, which this
   * duplicates (dependency-free module, manually kept in sync, same
   * convention as the "route"/"drag" variants above). */
  | {
      kind: "diffpair";
      netA: string | null;
      netB: string | null;
      layer: string;
      width: Um;
      ptsA: [Um, Um][];
      ptsB: [Um, Um][];
      colliding?: boolean;
      runsA?: { layer: string; pts: [Um, Um][] }[];
      runsB?: { layer: string; pts: [Um, Um][] }[];
      snappedEnd?: boolean;
    }
  | { kind: "zone"; pts: [Um, Um][] }
  | {
      kind: "shape";
      shapeKind: "segment" | "arc" | "rect" | "circle" | "polygon" | "bezier";
      pts: [Um, Um][];
      /** `shapeKind === "arc"` only: KIGFX::PREVIEW::ARC_GEOM_MANAGER's state (kicad-port/arcGeom.ts) -- centre, start, end angle, and the `/` posture (clockwise + lock). `pts` then holds just the clicks locked in so far. */
      arc?: ArcGeom;
      /** `shapeKind === "bezier"` only: KIGFX::PREVIEW::BEZIER_GEOM_MANAGER's state (kicad-port/bezierGeom.ts). */
      bezier?: BezierGeom;
    }
  /** Task item 7: two clicks (start, end) for any of the five dimension
   * kinds -- which one is read from `state.nextDimensionKind` when the
   * second click commits it (same one-shot-flag convention as zones' own
   * `nextZoneIsRuleArea`), not stored per-draw here. */
  | { kind: "dimension"; pts: [Um, Um][] }
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
  /** Which kind of item `refs` names -- each commits through a different Cmd (pcb: any mix of PCB items, one batch, see below; parts: move_to per ref; via/shape/text/dimension: their own move_* by id; sch_move / sch_drag: the schematic's `sch_move` verb for items of every kind, see kicad-port/schMove.ts). Defaults to "part" (every pre-existing caller moves parts). */
  kind?: "pcb" | "part" | "via" | "shape" | "text" | "dimension" | "sch_move" | "sch_drag";
  /** `sch_move` / `sch_drag`: the picked points of a wire (a click near a wire's end picks that end); a wire not named is picked whole. */
  vertices?: Record<string, number[]>;
  /** `sch_move` / `sch_drag`: R, Shift+R, X and Y pressed while the items are held, in order -- committed with the move as one step. */
  turns?: SchTurn[];
  /** `sch_move` / `sch_drag`: where the held items are held now -- the reference point they were picked up by (`BestDragOrigin`) plus how far they have gone, i.e. the snapped cursor; what R, Shift+R, X and Y turn them about. Absent: where the cursor went down plus the move. */
  holdUm?: [number, number];
  dxUm: number;
  dyUm: number;
  /**
   * `"pcb"`: any mix of footprints, tracks, vias, zones, graphics, text, dimensions and groups (the Move tool's own selection, locked items already
   * taken out). R and F turn and flip the whole selection about the point it was picked up at (`pivotUm` for a turn, `flipPivotUm` for a flip --
   * `EDIT_TOOL::Rotate` / `::Flip` act about the selection's reference point, which travels with the cursor), and the drop commits one batch
   * (kicad-port/pcbTransform.ts `planCarry`). The older kinds below move one kind each.
   */
  pivotUm?: [number, number];
  flipPivotUm?: [number, number];
  /**
   * edit_tool.cpp Rotate/Flip during an active Move: both act on the
   * live preview instead of committing immediately (updateModificationPoint's
   * `m_dragging && HasReferencePoint()` guard -- the item hasn't been
   * pushed to the board yet, so there's nothing to commit to). On the PCB
   * it is the number of R presses, counter-clockwise quarter turns 0-3
   * (Shift+R takes one away); the schematic's kinds (`sch_move`, `sch_drag`)
   * carry `turns` instead.
   */
  rotateQuarterTurns?: number;
  flipped?: boolean;
  /**
   * `P` (`EDIT_TOOL::PackAndMoveFootprints`): SpreadFootprints has already moved every selected footprint
   * to its packed place *before* the move tool picks the group up, so each ref carries its own extra shift
   * (um, from where it is on the board to its packed place) on top of the shared `dxUm`/`dyUm`. Only
   * meaningful for `kind === "part"`; absent for every ordinary move.
   */
  perRefOffsetUm?: Record<string, [number, number]>;
}

/**
 * The 3D viewer's own settings: the Appearance manager's rows (the layer tree) and the viewer's switches. A mirror of KiCad's 3D viewer settings
 * (EDA_3D_VIEWER_SETTINGS: the visible-layers bitset, the colours of the theme, `use_stackup_colors`, `use_board_editor_copper_colors`); the rows, their defaults and
 * the colours are kicad-port/appearance3d.ts's.
 */
export interface Viewer3DOptions {
  /** Row id (kicad-port/appearance3d.ts `ROWS`: `board`, `copper_top`, `th_models`, `user_3`, `zones`, ...) -> shown. A row that is not here has its KiCad default. */
  layers: Record<string, boolean>;
  /** Row id -> the colour the person set with its swatch, `#rrggbb[aa]`. A row that is not here has the theme's (or the stackup's) colour. */
  colors: Record<string, string>;
  /** `m_UseStackupColors` (default on in KiCad): the board body, silkscreen, solder mask and copper finish take the colours of Board Setup's physical stackup. */
  useStackupColors: boolean;
  /** `render.use_board_editor_copper_colors`: copper is drawn in the PCB editor's colours for F.Cu and B.Cu. */
  useEditorCopperColors: boolean;
  /** True = viewing the board flipped (as if turned over a horizontal hinge) so the bottom side reads right-way-up. A camera action (TrackballCamera.animateFlip(), see kicad-port/camera3d.ts), not a geometry edit -- this flag is read only to know how many times to call it (see Viewer3D.tsx's prevFlippedRef). */
  flipped: boolean;
  /** True = a real orthographic projection (kicad-port/camera3d.ts's TrackballCamera, a faithful CAMERA::ToggleProjection port -- see that file). */
  orthographic: boolean;
  /** True = "exact export": show GET /api/board.glb's whole board as kicad-cli renders it (every model through OpenCascade, seconds to minutes) in place of the live scene. The live scene is the default: it loads each part's KiCad 3D model by itself (components/viewer3d/modelCache.ts, GET /api/3dmodel) and has them in seconds. Viewer3D falls back to the live scene when the GLB hasn't loaded (still being built, or failed). */
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
  layers: {},
  colors: {},
  useStackupColors: true,
  useEditorCopperColors: false,
  flipped: false,
  orthographic: false,
  kicadModels: false,
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

/** `selectionBoundsCenter` extended with vias (each contributes its pad circle's box) -- the shared pivot of a mixed footprint + via rotate. */
export function selectionBoundsCenterWithVias(
  parts: ReadonlyArray<{ ref: string; placed: boolean; courtyard?: readonly [number, number, number, number] | null }>,
  vias: ReadonlyArray<{ id: string; x: number; y: number; d: number }>,
  refs: readonly string[],
  viaIds: readonly string[]
): { x: number; y: number } | null {
  let x0 = Infinity,
    y0 = Infinity,
    x1 = -Infinity,
    y1 = -Infinity;
  for (const ref of refs) {
    const p = parts.find((q) => q.ref === ref);
    if (!p?.placed || !p.courtyard) continue;
    x0 = Math.min(x0, p.courtyard[0]);
    y0 = Math.min(y0, p.courtyard[1]);
    x1 = Math.max(x1, p.courtyard[2]);
    y1 = Math.max(y1, p.courtyard[3]);
  }
  for (const id of viaIds) {
    const v = vias.find((q) => q.id === id);
    if (!v) continue;
    x0 = Math.min(x0, v.x - v.d / 2);
    y0 = Math.min(y0, v.y - v.d / 2);
    x1 = Math.max(x1, v.x + v.d / 2);
    y1 = Math.max(y1, v.y + v.d / 2);
  }
  return x0 <= x1 ? { x: (x0 + x1) / 2, y: (y0 + y1) / 2 } : null;
}

export const DEFAULT_RULE_AREA_SETTINGS: RuleAreaFields = {
  is_rule_area: false,
  keepout_tracks: false,
  keepout_vias: false,
  keepout_pads: false,
  keepout_copper_pour: false,
  keepout_footprints: false,
};

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
  smoothing: "none",
  corner_radius: 0,
};

export interface StudioState {
  board: BoardState | null;
  boardError: string | null;
  version: string | null;

  tab: EditorTab;
  /** GAPS.md #6: root-to-here `SheetInstance::id`s for whichever sheet the Hierarchy panel has navigated into on the Schematic tab -- `[]` is the root, same as every board before hierarchy support existed. See `StudioApi.navigateToSheet`. */
  currentSheetPath: string[];
  rightDockTab: RightDockTab;
  viewer3d: Viewer3DOptions;
  /** See `GlbStatus`. Read by Viewer3D (the "loading models…" badge) and Viewer3DToolbar (the KiCad Models toggle's tooltip after a failure). */
  glbStatus: GlbStatus;
  glbError: string | null;

  selection: Set<string>;
  /** `common.Interactive.groupEnter`/`groupLeave` (task item 5): the one
   * group, if any, currently "entered" -- while inside it, clicking one of
   * its own members selects that member alone instead of the whole group
   * (see `withGroupSubstitution` in this file). `null` outside any group. */
  enteredGroupId: string | null;
  /** Refs to flash/outline because a problem in the panel references them. */
  hot: Set<string>;
  netHighlight: string | null;
  /** eeschema wire/bus Line Mode (eeschema_settings.h LINE_MODE; `m_Drawing.line_mode`, default LINE_MODE_90) -- see kicad-port/schLineMode.ts. */
  schLineMode: LineMode;
  /** doDrawSegments' `static bool posture` (the `/` hotkey, SCH_ACTIONS::switchSegmentPosture). */
  schPosture: boolean;
  /** SCH_NAVIGATE_TOOL::m_navHistory/m_navIndex (Alt+Left/Alt+Right). */
  schNav: NavHistory;
  /** An unplaced part ref chosen from the panel, waiting for a canvas click to place it. */
  armed: string | null;
  /** A footprint of a library (`Lib:Name`) chosen in the Footprint Chooser (Place Footprint), waiting for a canvas click: it becomes a part of its own on the board (`place_footprint`). */
  armedFootprint: string | null;
  movePreview: MovePreview | null;
  activeTool: ToolId;
  drawState: DrawState | null;
  /** A just-drawn zone outline waiting for its settings to be confirmed in ZoneDialog before `add_zone` commits it. */
  zonePending: [Um, Um][] | null;
  /** A selected zone's id ("E", or double-click) -- opens ZoneDialog in edit mode (`edit_zone`) instead of add mode. Independent of `zonePending` (one or the other is ever set, never both). */
  zoneEditId: string | null;
  /** Task item 7: which of the five dimension kinds the next two-click
   * placement creates -- set by each of the five toolbar actions
   * (`pcbnew.InteractiveDrawing.alignedDimension` etc.) before arming
   * `activeTool: "dimension"`, same one-shot-flag convention as zones'
   * own `nextZoneIsRuleArea`. */
  nextDimensionKind: CmdDimensionKind["kind"];
  /** A dimension's id, just created (the draw tool opens this
   * immediately so height/leader-length/format fields can be set, since
   * there's no separate "pending outline" stage the way zones have) or
   * selected ("E", double-click) -- opens DimensionPropertiesDialog. */
  dimensionEditId: string | null;
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
   * `fractured` / `triangulated` are `SHOW_FRACTURE_BORDERS` /
   * `SHOW_TRIANGULATION` (`pcbnew.Control.zoneDisplayOutlines` /
   * `zoneDisplayTesselation`): the fill drawn with the edges of its
   * fractured ring, or with the triangles it is cut into.
   */
  zoneDisplayMode: "filled" | "outline" | "fractured" | "triangulated";
  /** Board Setup... (dialog_board_setup.cpp) -- net classes/track-via sizing/rules/etc, see BoardSetupDialog.tsx. */
  boardSetupDialogOpen: boolean;
  /** Which `BoardSetupDialog.tsx` page to land on next time it opens --
   * `pcbnew.GlobalEdit.editTeardrops` (task item 4) sets this to
   * `"teardrops"` before opening, so it lands straight on that page
   * instead of making the user click there themselves; `null` leaves
   * whatever page was last selected alone. Consumed once by the dialog's
   * own open effect, then reset. */
  boardSetupInitialPage: string | null;
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
  /** `S`: a sheet's two corners, waiting for SheetDialog to name it before `add_sheet` commits. */
  schSheetPending: { at: [Um, Um]; size: [Um, Um] } | null;
  /** `C` (`SCH_LINE_WIRE_BUS_TOOL::UnfoldBus`): the bus member chosen to break out of a bus -- the bus entry sits at `entryAt` and the wire being drawn from its far end takes a label of `net` when it finishes. */
  busUnfold: { net: string; entryAt: [Um, Um]; size: [Um, Um] } | null;
  /** `C`: the member nets of the bus under the cursor, waiting for BusUnfoldDialog to pick one (`BUS_UNFOLD_MENU`); `entryAt` is where the bus entry will root on the bus. */
  busUnfoldPicker: { bus: string; entryAt: [Um, Um]; members: string[] } | null;
  /** `F1`/Insert (`SCH_FRAME::m_items_to_repeat`): the Cmd(s) of the last thing placed, so "Repeat Last Item" can place a copy. */
  schRepeat: Cmd[];
  /** `P`: a just-clicked point (already pin-snapped, see SchematicView.tsx's `pinSnapPoints`) waiting for PowerSymbolDialog to confirm which rail. */
  schPowerPending: { at: [Um, Um] } | null;
  /** `T`: a just-clicked point waiting for SchTextDialog to confirm the content. */
  schTextPending: { at: [Um, Um] } | null;
  /** The dialog one of the schematic edit/drawing tools has open (text box text, directive label fields, ...): components/SchToolDialogs.tsx. */
  schToolDialog: SchToolDialog | null;
  /** `A`: SymbolChooserDialog's own open/closed flag. */
  symbolChooserOpen: boolean;
  /** `E`/`U`/`V`/`F` on a selected symbol: SymbolPropertiesDialog's own open/closed+focus state -- `field` picks which input autofocuses (`E` opens the same dialog with nothing singled out). */
  symbolProperties: { id: string; field: "reference" | "value" | "footprint" | "datasheet" | null } | null;
  /** `Ctrl+A`: AnnotateDialog's own open/closed flag (dialog_annotate.cpp's scope/order/reset options). */
  annotateDialogOpen: boolean;
  /** Which of the Symbol Fields Table / Find / Find and Replace / Schematic Setup dialogs is open (eeschema's editSymbolFields / find / findAndReplace / schematicSetup actions) -- one slot because they are never open together. */
  schDialog: SchDialog;
  /** Find / Replace's last search (`SCH_EDIT_FRAME::GetFindReplaceData`) plus its `m_afterItem` cursor (the key of the last visited match) -- shared by the dialog and F3 / Shift+F3 so Find Next works with the dialog closed. */
  schFind: { search: SchSearchData; cursor: string | null; status: string; /** `searchSelectedOnly`: restrict Find / Replace to the selection. */ selectedOnly: boolean; /** The Find dialog's "Backward" direction, which Replace and Find Next continues in. */ backward: boolean };
  /** Schematic tab's File > Plot... (`common.Control.plot`, DIALOG_PLOT_SCHEMATIC), see PlotSchematicDialog.tsx. */
  schPlotDialogOpen: boolean;
  /** Schematic tab's File > Export > Netlist... (`eeschema.EditorControl.exportNetlist`, DIALOG_EXPORT_NETLIST), see ExportNetlistDialog.tsx. */
  exportNetlistDialogOpen: boolean;
  /** `A`: the symbol SymbolChooserDialog confirmed, waiting for a canvas click to place it (`sch_place_symbol` tool) -- `referencePrefix` seeds `nextReference`'s own next-free-number placement (this app's own choice: a real id immediately, not a "U?" placeholder -- see `Cmd::AddSymbol`'s doc and PARITY-sch.md). `unit`: which unit of a multi-unit symbol to place (the chooser's own unit picker, shown when `SymbolLibraryEntry.unit_count > 1`; omitted/1 for a single-unit part). */
  armedSymbol: { libId: string; referencePrefix: string; unit?: number; /** Place Next Symbol Unit: the unit goes in under this existing reference (and value/footprint), once, instead of taking the next free reference. */ ref?: string; value?: string; footprint?: string } | null;
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
  /** The Schematic Editor's current grid (`GRID_SETTINGS::last_size_idx` of eeschema), um: one of its grid list (state/gridSettings.ts), 50 mil to start. */
  schGridUm: number;
  gridVisible: boolean;
  /** `MAGNETIC_SETTINGS::allLayers` (pcbnew_settings.cpp, default false): snap to items on every layer instead of the active layer only. Toggled by `common.Control.magneticSnapToggle` (Shift+S). UI state, not design data. */
  magneticAllLayers: boolean;
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
  /** The schematic's own (panel_sch_selection_filter.cpp): what a click/box/Select All on the sheet may pick (kicad-port/schSelectionFilter.ts). */
  schSelectionFilter: SchSelectionFilter;

  /** Refuse a move/edit that adds gate failures. Not a KiCad feature -- see the task's Strict toggle. */
  strict: boolean;

  drcDialogOpen: boolean;
  hotkeysDialogOpen: boolean;
  footprintPropertiesOpen: boolean;
  /** E on a selected track/via/zone/shape: which one's read-only properties dialog is open (null = closed). Text has its own full-edit dialog (textDialog); a part has footprintPropertiesOpen. */
  itemPropertiesId: string | null;
  /** E / a double-click on a pad of the board: the id (`REF.NUM[#k]`) of the pad whose Pad Properties dialog is open (null = closed) -- components/BoardPadPropertiesDialog.tsx. */
  boardPadPropertiesId: string | null;
  /** pcbnew.Control.showNetInspector ("Net Inspector") -- a basic net/pad-count list, not KiCad's full dockable inspector. */
  netInspectorOpen: boolean;
  toast: { message: string; kind: "error" | "info" } | null;

  /** Cursor position in board µm, for the status bar's X/Y/dx/dy/dist. */
  cursorUm: { x: number; y: number } | null;
  moveOriginUm: { x: number; y: number } | null;
  /**
   * Which points of a wire a click picked (`SCH_SELECTION_TOOL::selectPoint`: a click near a wire's end picks that end only, so `G` or a drag
   * stretches it; anywhere else the segment's two ends). Only meaningful while the selection is exactly that wire (`pickedVertices`).
   */
  schWirePick: { id: string; vertices: number[] } | null;
  /**
   * base_screen.cpp's `m_LocalOrigin` (default (0,0), same as source) --
   * the status bar's dx/dy/dist is always relative to THIS point, set by
   * Space (common.Control.resetLocalCoords, pcb_base_frame.cpp's
   * UpdateStatusBar). Independent of moveOriginUm (the move tool's own
   * drag-grab anchor): KiCad's move tool never touches m_LocalOrigin.
   */
  localOriginUm: { x: number; y: number };
  /**
   * Preferences > Mouse and Touchpad (`common.SuiteControl.openPreferences`, kicad-port/preferences.ts): the wheel
   * gestures, the zoom speed/acceleration and `prefs.autoPan` -- view_controls.cpp VC_SETTINGS::m_autoPanSettingEnabled,
   * edge auto-pan while dragging/drawing near the canvas border. KiCad ships with this OFF (input.auto_pan defaults
   * false) -- same default here. Saved in the browser's local storage (see StudioProvider).
   */
  prefs: Preferences;
  preferencesDialogOpen: boolean;

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
   * GET /api/drc: `kicad-cli pcb drc` on the current design (see
   * api/client.ts's fetchDrc). It takes seconds, so it runs on demand --
   * when the dialog opens on a board it has not judged yet, and on "Run DRC"
   * -- never on every change; `drcRunning` is what the dialog and status
   * show meanwhile, and nothing else waits: the board stays editable during
   * a run. The PCB canvas draws its markers (DrcDialog.tsx owns the list) and
   * keeps drawing the last report after the dialog closes, like KiCad's own
   * markers until the next run -- dimmed once the report is out of date.
   */
  drc: DrcReport | null;
  drcRunning: boolean;
  drcError: string | null;
  /** The board revision (the `version` stamp) the current `drc` report was computed on -- the server's own (`drc.revision`); when it differs from `version`, the board has moved on and the report is out of date (kicad-port/checkRevision.ts). */
  drcVersion: string | null;
  /** "Refill all zones before performing DRC" (`--refill-zones`). Off by default: kicad-cli 10.99 skips its courtyard checks on a run that refills. */
  drcRefillZones: boolean;
  /** "Test for parity between PCB and schematic" (`--schematic-parity`): the next run also compares the board with the schematic (the Schematic Parity tab). */
  drcParity: boolean;
  /** Index into `drc.violations` the dialog's list has clicked, for the canvas's marker highlight and the "selects and zooms to it" behavior -- null selects nothing. */
  drcSelected: number | null;
  /** Same, for the dialog's Lint tab (`lint.pcb.violations`). */
  drcLintSelected: number | null;

  ercDialogOpen: boolean;
  /** GET /api/erc: `kicad-cli sch erc`, on demand like `drc` above. */
  erc: ErcReport | null;
  ercRunning: boolean;
  ercError: string | null;
  /** Same as `drcVersion`, for `erc`. */
  ercVersion: string | null;
  /** Index into `erc.violations` the dialog's list has clicked -- null selects nothing. */
  ercSelected: number | null;
  /** Same, for the dialog's Lint tab (`lint.schematic.violations`). */
  ercLintSelected: number | null;

  /**
   * GET /api/lint: our own checks, the ones KiCad does not have (crates/lint).
   * In-process and cheap, so it is refetched on every board change while a
   * DRC or ERC dialog is open and shown in those dialogs' Lint tabs (and as
   * markers while they are open) -- never mixed into kicad-cli's lists.
   */
  lint: LintReport | null;

  /** Cmd+C's clipboard (Cmd-ready IR shapes, see components/canvas/clipboard.ts) -- client-side only, holds full item data so Cmd+V still works after the original was deleted, or pasted more than once. Null = nothing copied yet this session. */
  clipboard: ClipboardContents | null;
  /** Shift+M "Move Exactly..." dialog -- open with the selection's own default anchor/bbox already resolved (components/MoveExactDialog.tsx computes the rest). Null = closed. */
  moveExactDialogOpen: boolean;
  /** `Ctrl+<` "Interactive Router Settings..." (dialog_pns_settings.cpp) -- components/RouterSettingsDialog.tsx. */
  routerSettingsDialogOpen: boolean;
  /** `7` (pcbnew.LengthTuner.TuneSingleTrack) / `8` (TuneDiffPair) / `9` (TuneDiffPairSkew) -- components/LengthTuningDialog.tsx. */
  lengthTuningDialogOpen: boolean;
  /** Which of the three tuners the dialog is running. */
  lengthTuningMode: TuneMode;
  /** The "pcbnew parity" action batch's own state (move-individually queue, zone cutout/similar mode, parity dialogs, angle snap, ...) -- see kicad-port/pcbParityState.ts. Patched by the single `PCBX` action. */
  pcbx: PcbParityState;
  /** The board-control actions' state (sketch modes, ratsnest/highlight sets, partial zone fill, board flip, their dialogs) -- see kicad-port/boardControlState.ts. Patched by the single `BCX` action. */
  bcx: BoardControlState;
  /**
   * The Appearance panel's own settings (kicad-port/appearance.ts): object visibility and opacity, the inactive-layer and net colour modes, net and net
   * class colours, the saved layer presets and viewports. Saved per project in appearance.json (kicad-port/appearanceFile.ts), never in the design.
   * Object visibility also rides in `layerVisible` under `obj:<id>` keys, so every picker that gets that record honours it.
   */
  appearance: AppearanceState;
  /**
   * `pcbnew.InteractiveDrawing.ruleArea` vs `.zone` (task item 3): both
   * arm the same outline-drawing tool (`activeTool === "zone"`); this is
   * the one bit that tells `ZoneDialog.tsx` which hotkey/menu entry armed
   * it, so a fresh outline's dialog opens with "Rule area" pre-checked
   * when it was the dedicated rule-area entry. Irrelevant once
   * `state.zonePending` is set (the dialog reads it exactly once, when a
   * new outline first arrives) and reset whenever either tool is
   * (re-)armed.
   */
  nextZoneIsRuleArea: boolean;
  /** `pcbnew.GlobalEdit.cleanupTracksAndVias` -- components/CleanupTracksDialog.tsx. */
  cleanupTracksDialogOpen: boolean;
  /** `pcbnew.GlobalEdit.editTracksAndVias` -- components/GlobalEditTracksAndViasDialog.tsx. */
  editTracksAndViasDialogOpen: boolean;
  /** `pcbnew.InspectionTool.ShowBoardStatistics` -- components/BoardStatisticsDialog.tsx. */
  boardStatisticsDialogOpen: boolean;
  /** `pcbnew.GlobalEdit.swapLayers` -- components/SwapLayersDialog.tsx. */
  swapLayersDialogOpen: boolean;
  /** `pcbnew.GlobalEdit.editTextAndGraphics` -- components/GlobalEditTextAndGraphicsDialog.tsx. */
  editTextAndGraphicsDialogOpen: boolean;
  /** `pcbnew.Array.createArray` (Ctrl+T, task item 6) -- components/CreateArrayDialog.tsx. */
  createArrayDialogOpen: boolean;
  /**
   * `eda_pns::RoutingSettings`, the subset of `PNS::ROUTING_SETTINGS` this app's router implements
   * (kicad-port/routerSettings.ts; `Ctrl+<` edits them, components/RouterSettingsDialog.tsx). Every field is read by the
   * router. Sent with `X`/`D`'s own session start (`routeStart`/`routeDragStart`) -- this app starts a fresh backend session
   * for every route or drag -- and, when the dialog is closed with OK while one is running, to that session
   * (`routeSetSettings`), so a change applies from the next move as upstream's does.
   */
  routerSettings: RouterSettings;
}

/** The browser's local storage, or null where it is absent or reading it throws (private windows, blocked site data, node tests). */
function browserStorage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

const initialState: StudioState = {
  board: null,
  boardError: null,
  version: null,
  tab: "pcb",
  currentSheetPath: [],
  rightDockTab: "appearance",
  viewer3d: DEFAULT_VIEWER3D_OPTIONS,
  glbStatus: "idle",
  glbError: null,
  selection: new Set(),
  enteredGroupId: null,
  hot: new Set(),
  netHighlight: null,
  schLineMode: 1,
  schPosture: false,
  schNav: initialNavHistory(),
  armed: null,
  armedFootprint: null,
  movePreview: null,
  activeTool: "select",
  drawState: null,
  zonePending: null,
  zoneEditId: null,
  nextDimensionKind: "aligned",
  dimensionEditId: null,
  zoneFill: null,
  zoneDisplayMode: "filled",
  boardSetupDialogOpen: false,
  boardSetupInitialPage: null,
  plotDialogOpen: false,
  generateDrillDialogOpen: false,
  footprintPositionDialogOpen: false,
  currentTrackWidthUm: null,
  currentViaPreset: null,
  textDialog: null,
  schLabelPending: null,
  schSheetPending: null,
  busUnfold: null,
  busUnfoldPicker: null,
  schRepeat: [],
  schPowerPending: null,
  schTextPending: null,
  schToolDialog: null,
  symbolChooserOpen: false,
  armedSymbol: null,
  symbolProperties: null,
  annotateDialogOpen: false,
  schDialog: null,
  schFind: { search: defaultSearch(), cursor: null, status: "", selectedOnly: false, backward: false },
  schPlotDialogOpen: false,
  exportNetlistDialogOpen: false,
  lastLabelText: "",
  lastPowerLibId: "power:GND",
  view: { scale: 0, x: 0, y: 0 },
  viewInitialized: false,
  schematicView: { scale: 0, x: 0, y: 0 },
  units: "mm",
  polar: false,
  gridUm: 1000, // 1.0 mm; a placeholder until src/kicad/layers.json-adjacent grid defaults are extracted (KiCad's own default grid list is source-derived, see report)
  schGridUm: 1270, // eeschema's default grid: 50 mil
  gridVisible: true,
  magneticAllLayers: false,
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
  layerVisible: withObjectKeys(Object.fromEntries(STANDARD_LAYERS.map((l) => [l.key, true])), DEFAULT_APPEARANCE),
  layerOpacity: Object.fromEntries(STANDARD_LAYERS.map((l) => [l.key, 1])),
  selectionFilter: DEFAULT_SELECTION_FILTER,
  schSelectionFilter: DEFAULT_SCH_SELECTION_FILTER,
  strict: true,
  drcDialogOpen: false,
  hotkeysDialogOpen: false,
  footprintPropertiesOpen: false,
  itemPropertiesId: null,
  boardPadPropertiesId: null,
  netInspectorOpen: false,
  toast: null,
  cursorUm: null,
  moveOriginUm: null,
  schWirePick: null,
  localOriginUm: { x: 0, y: 0 },
  prefs: loadPreferences(browserStorage()),
  preferencesDialogOpen: false,
  schematic: null,
  schematicError: null,
  ratsnest: null,
  drc: null,
  drcRunning: false,
  drcError: null,
  drcVersion: null,
  drcRefillZones: false,
  drcParity: false,
  drcSelected: null,
  drcLintSelected: null,
  ercDialogOpen: false,
  erc: null,
  ercRunning: false,
  ercError: null,
  ercVersion: null,
  ercSelected: null,
  ercLintSelected: null,
  lint: null,
  clipboard: null,
  moveExactDialogOpen: false,
  routerSettingsDialogOpen: false,
  lengthTuningDialogOpen: false,
  lengthTuningMode: "single",
  pcbx: DEFAULT_PCB_PARITY,
  bcx: DEFAULT_BOARD_CONTROL,
  appearance: DEFAULT_APPEARANCE,
  nextZoneIsRuleArea: false,
  cleanupTracksDialogOpen: false,
  editTracksAndViasDialogOpen: false,
  boardStatisticsDialogOpen: false,
  swapLayersDialogOpen: false,
  editTextAndGraphicsDialogOpen: false,
  createArrayDialogOpen: false,
  // `RoutingSettings::default()`'s own real defaults (crates/pns/src/settings.rs) -- Walkaround, RemoveLoops on, matching KiCad's own out-of-the-box router.
  routerSettings: DEFAULT_ROUTER_SETTINGS,
};

export type Action =
  | { type: "BOARD_OK"; board: BoardState }
  | { type: "BOARD_ERR"; message: string }
  | { type: "VERSION"; version: string }
  | { type: "SET_TAB"; tab: EditorTab }
  | { type: "SET_SHEET_PATH"; path: string[]; /** false = do not push onto the Back/Forward history (Back/Forward themselves). Default true. */ record?: boolean }
  | { type: "SET_SCH_NAV"; nav: NavHistory }
  | { type: "SET_SCH_LINE_MODE"; mode: LineMode }
  | { type: "TOGGLE_SCH_POSTURE" }
  | { type: "SET_RIGHT_DOCK_TAB"; tab: RightDockTab }
  | { type: "SET_VIEWER3D_OPTIONS"; options: Partial<Viewer3DOptions> }
  | { type: "SET_GLB_STATUS"; status: GlbStatus; error?: string }
  /** `raw`: the items as they are, with no group standing in for its members (`select( item )` of a group that was just made or left). */
  | { type: "SET_SELECTION"; refs: string[]; raw?: boolean }
  | { type: "SET_ENTERED_GROUP"; id: string | null }
  /** `PCB_SELECTION_TOOL::EnterGroup`: the group becomes the one worked in, and its members are selected. */
  | { type: "ENTER_GROUP"; id: string }
  | { type: "TOGGLE_SELECTION"; ref: string }
  | { type: "CLEAR_SELECTION" }
  | { type: "ESCAPE" }
  | { type: "SET_HOT"; refs: string[] }
  | { type: "SET_NET_HIGHLIGHT"; net: string | null }
  | { type: "SET_ARMED"; ref: string | null }
  | { type: "SET_ARMED_FOOTPRINT"; name: string | null }
  | { type: "SET_MOVE_PREVIEW"; preview: MovePreview | null }
  | { type: "SET_ACTIVE_TOOL"; tool: ToolId }
  | { type: "SET_VIEW"; view: ViewTransform }
  | { type: "SET_SCHEMATIC_VIEW"; view: ViewTransform }
  | { type: "MARK_VIEW_INITIALIZED" }
  | { type: "SET_UNITS"; units: LengthUnit }
  | { type: "TOGGLE_POLAR" }
  | { type: "SET_GRID_UM"; um: number }
  | { type: "SET_SCH_GRID_UM"; um: number }
  | { type: "TOGGLE_GRID_VISIBLE" }
  | { type: "SET_MAGNETIC_ALL_LAYERS"; value: boolean }
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
  | { type: "SET_SCH_SELECTION_FILTER"; filter: SchSelectionFilter }
  | { type: "SET_STRICT"; strict: boolean }
  | { type: "SET_DRC_OPEN"; open: boolean }
  | { type: "SET_HOTKEYS_DIALOG_OPEN"; open: boolean }
  | { type: "SET_FOOTPRINT_PROPERTIES_OPEN"; open: boolean }
  | { type: "SET_ITEM_PROPERTIES_ID"; id: string | null }
  | { type: "SET_BOARD_PAD_PROPERTIES_ID"; id: string | null }
  | { type: "SET_NET_INSPECTOR_OPEN"; open: boolean }
  | { type: "TOAST"; message: string; kind: "error" | "info" }
  | { type: "TOAST_CLEAR" }
  | { type: "SET_CURSOR"; at: { x: number; y: number } | null }
  | { type: "SET_MOVE_ORIGIN"; at: { x: number; y: number } | null }
  | { type: "SET_SCH_WIRE_PICK"; pick: { id: string; vertices: number[] } | null }
  | { type: "SET_LOCAL_ORIGIN"; at: { x: number; y: number } }
  | { type: "TOGGLE_AUTO_PAN" }
  | { type: "SET_PREFERENCES"; prefs: Preferences }
  | { type: "SET_PREFERENCES_DIALOG_OPEN"; open: boolean }
  | { type: "SCHEMATIC_OK"; schematic: Schematic }
  | { type: "SCHEMATIC_ERR"; message: string }
  | { type: "RATSNEST_OK"; ratsnest: Ratsnest }
  | { type: "DRC_RUNNING" }
  | { type: "DRC_OK"; drc: DrcReport; /** The revision the report was computed on (kicad-port/checkRevision.ts's `revisionOf`). */ version: string | null }
  | { type: "DRC_ERR"; message: string }
  /** `BOARD::DeleteMARKERs` (Global Deletions > Delete Markers): the last report and its markers go. */
  | { type: "DRC_CLEAR" }
  | { type: "SET_DRC_REFILL"; refill: boolean }
  | { type: "SET_DRC_PARITY"; parity: boolean }
  /** Waive or restore DRC violations in the report on screen without running kicad-cli again (the exclusion itself is a persisted `Cmd`); `version` as for `ERC_MARK_EXCLUDED`. */
  | { type: "DRC_PATCH_EXCLUDED"; keys: ReadonlyArray<{ check: string; items: string[] }>; excluded: boolean; comment: string; version: string | null }
  /** A check's severity changed (`set_rule_severities`): the report on screen takes it (`patchDrcSeverity`) -- an ignored check leaves the list and joins the ignored ones. */
  | { type: "DRC_PATCH_SEVERITY"; check: string; severity: "error" | "warning" | "ignore"; description: string; version: string | null }
  /** The same for the schematic (`set_erc_severities`). */
  | { type: "ERC_PATCH_SEVERITY"; check: string; severity: "error" | "warning" | "ignore"; description: string; version: string | null }
  | { type: "SET_DRC_SELECTED"; index: number | null }
  | { type: "SET_DRC_LINT_SELECTED"; index: number | null }
  | { type: "SET_ERC_DIALOG_OPEN"; open: boolean }
  | { type: "ERC_RUNNING" }
  | { type: "ERC_OK"; erc: ErcReport; /** The revision the report was computed on. */ version: string | null }
  | { type: "ERC_ERR"; message: string }
  /** Exclude / un-exclude one finding in the report on screen without re-running kicad-cli (the exclusion itself is a persisted `Cmd`); `version` is the revision the patched report is now valid for (checkRevision.ts's `revisionAfterOwnEdit`): the board's after that Cmd when the report was current, so it is not called out of date by its own exclusion -- the report's own when it already was out of date. */
  | { type: "ERC_MARK_EXCLUDED"; check: string; location: string; excluded: boolean; version: string | null }
  | { type: "SET_ERC_SELECTED"; index: number | null }
  | { type: "SET_ERC_LINT_SELECTED"; index: number | null }
  | { type: "LINT_OK"; lint: LintReport }
  | { type: "SET_DRAW_STATE"; draw: DrawState | null }
  | { type: "SET_ZONE_PENDING"; outline: [Um, Um][] | null }
  | { type: "SET_ZONE_EDIT_ID"; id: string | null }
  | { type: "SET_NEXT_DIMENSION_KIND"; kind: CmdDimensionKind["kind"] }
  | { type: "SET_DIMENSION_EDIT_ID"; id: string | null }
  | { type: "FILL_OK"; fill: FillReport }
  | { type: "CLEAR_ZONE_FILL" }
  | { type: "SET_ZONE_DISPLAY_MODE"; mode: "filled" | "outline" | "fractured" | "triangulated" }
  | { type: "SET_BOARD_SETUP_DIALOG_OPEN"; open: boolean }
  | { type: "SET_BOARD_SETUP_INITIAL_PAGE"; page: string | null }
  | { type: "SET_PLOT_DIALOG_OPEN"; open: boolean }
  | { type: "SET_GENERATE_DRILL_DIALOG_OPEN"; open: boolean }
  | { type: "SET_FOOTPRINT_POSITION_DIALOG_OPEN"; open: boolean }
  | { type: "SET_CURRENT_TRACK_WIDTH"; widthUm: Um }
  | { type: "SET_CURRENT_VIA_PRESET"; preset: ViaPreset }
  | { type: "SET_TEXT_DIALOG"; dialog: StudioState["textDialog"] }
  | { type: "SET_SCH_LABEL_PENDING"; pending: StudioState["schLabelPending"] }
  | { type: "SET_SCH_SHEET_PENDING"; pending: StudioState["schSheetPending"] }
  | { type: "SET_BUS_UNFOLD"; unfold: StudioState["busUnfold"] }
  | { type: "SET_BUS_UNFOLD_PICKER"; picker: StudioState["busUnfoldPicker"] }
  | { type: "SET_SCH_REPEAT"; cmds: Cmd[] }
  | { type: "SET_SCH_POWER_PENDING"; pending: StudioState["schPowerPending"] }
  | { type: "SET_SCH_TEXT_PENDING"; pending: StudioState["schTextPending"] }
  | { type: "SET_SCH_TOOL_DIALOG"; dialog: SchToolDialog | null }
  | { type: "SET_LAST_LABEL_TEXT"; text: string }
  | { type: "SET_LAST_POWER_LIB_ID"; libId: string }
  | { type: "SET_SYMBOL_CHOOSER_OPEN"; open: boolean }
  | { type: "SET_ARMED_SYMBOL"; symbol: StudioState["armedSymbol"] }
  | { type: "SET_SYMBOL_PROPERTIES"; value: StudioState["symbolProperties"] }
  | { type: "SET_ANNOTATE_DIALOG_OPEN"; open: boolean }
  | { type: "SET_SCH_DIALOG"; dialog: SchDialog }
  | { type: "SET_SCH_FIND"; find: Partial<StudioState["schFind"]> }
  | { type: "SET_SCH_PLOT_DIALOG_OPEN"; open: boolean }
  | { type: "SET_EXPORT_NETLIST_DIALOG_OPEN"; open: boolean }
  | { type: "SET_CLIPBOARD"; clipboard: ClipboardContents | null }
  | { type: "SET_MOVE_EXACT_DIALOG_OPEN"; open: boolean }
  | { type: "SET_ROUTER_SETTINGS_DIALOG_OPEN"; open: boolean }
  | { type: "SET_ROUTER_SETTINGS"; settings: StudioState["routerSettings"] }
  | { type: "SET_LENGTH_TUNING_DIALOG_OPEN"; open: boolean; mode?: TuneMode }
  | { type: "PCBX"; patch: Partial<PcbParityState> }
  | { type: "BCX"; patch: Partial<BoardControlState> }
  /** An edit made in the Appearance panel (kicad-port/appearanceOps.ts), or by a script through `studio.Appearance.op`. */
  | { type: "APPEARANCE"; op: AppearanceOp }
  /** The project's saved appearance (appearance.json) laid over the state once it has been read. */
  | { type: "APPEARANCE_LOAD"; file: unknown }
  /** `highlightNetSelection`: several nets highlighted at once (the first is `netHighlight`, the rest `bcx.netHighlightMore`); empty clears. */
  | { type: "SET_NET_HIGHLIGHT_SET"; nets: string[] }
  | { type: "SET_NEXT_ZONE_IS_RULE_AREA"; value: boolean }
  | { type: "SET_CLEANUP_TRACKS_DIALOG_OPEN"; open: boolean }
  | { type: "SET_EDIT_TRACKS_AND_VIAS_DIALOG_OPEN"; open: boolean }
  | { type: "SET_BOARD_STATISTICS_DIALOG_OPEN"; open: boolean }
  | { type: "SET_SWAP_LAYERS_DIALOG_OPEN"; open: boolean }
  | { type: "SET_EDIT_TEXT_AND_GRAPHICS_DIALOG_OPEN"; open: boolean }
  | { type: "SET_CREATE_ARRAY_DIALOG_OPEN"; open: boolean };

/**
 * `pcb_selection_tool.cpp`'s "clicking a group member selects the group" rule (`FilterCollectorForHierarchy`): any ref that is a member of a group
 * becomes the outermost group that holds it inside `enteredGroupId` -- the group being worked in, whose own members (and the groups directly in it)
 * can be picked one by one (`common.Interactive.groupEnter`, `StudioState.enteredGroupId`). A ref naming a group directly, or not in any group, passes
 * through. The tree is kicad-port/groupTree.ts.
 */
export function withGroupSubstitution(refs: string[], groups: Group[] | undefined, enteredGroupId: string | null): string[] {
  if (!groups || groups.length === 0) return refs;
  return substituteSelection(groups, refs, enteredGroupId).ids;
}

/** The fields of the state the Appearance panel edits (kicad-port/appearanceOps.ts `ViewSlice`), `bcx`'s three lifted out. */
function viewSliceOf(state: StudioState): ViewSlice {
  return {
    appearance: state.appearance,
    layerVisible: state.layerVisible,
    layerOpacity: state.layerOpacity,
    activeLayer: state.activeLayer,
    highContrast: state.highContrast,
    showRatsnest: state.showRatsnest,
    gridVisible: state.gridVisible,
    boardFlipped: state.bcx.boardFlipped,
    ratsnestMode: state.bcx.ratsnestMode,
    hiddenNets: state.bcx.hiddenRatsnestNets,
  };
}

/** The state with an edited slice put back. Turning the global ratsnest on or off forgets the Local Ratsnest tool's pads (`SetElementVisibility( LAYER_RATSNEST )`). */
function withViewSlice(state: StudioState, slice: ViewSlice): StudioState {
  const ratsnestChanged = slice.showRatsnest !== state.showRatsnest;
  return {
    ...state,
    appearance: slice.appearance,
    layerVisible: slice.layerVisible,
    layerOpacity: slice.layerOpacity,
    activeLayer: slice.activeLayer,
    highContrast: slice.highContrast,
    showRatsnest: slice.showRatsnest,
    gridVisible: slice.gridVisible,
    bcx: { ...state.bcx, boardFlipped: slice.boardFlipped, ratsnestMode: slice.ratsnestMode, hiddenRatsnestNets: slice.hiddenNets, localRatsnestPads: ratsnestChanged ? [] : state.bcx.localRatsnestPads },
  };
}

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
      // Drop selection/hot refs for parts that no longer exist (ripped, renamed) and for items that were deleted or re-created under a
      // new id -- but keep a selected track/via/zone/shape/text/group that is still there (every refresh used to drop all of those).
      const refs = new Set(action.board.parts.map((p) => p.ref));
      // On the schematic tab the selection also holds the sheet's wires, labels, shapes... which are no parts: SCHEMATIC_OK prunes those against the sheet.
      const live = allItemIds(action.board);
      for (const g of action.board.drawings?.groups ?? []) live.add(g.id);
      // A pad and a footprint's field are items too (`REF.NUMBER`, `REF:Name`): they stay selected while their footprint has them, hidden or not.
      for (const id of subItemIds(action.board)) live.add(id);
      const selection = state.tab === "schematic" ? state.selection : new Set([...state.selection].filter((r) => refs.has(r) || live.has(r)));
      const hot = new Set([...state.hot].filter((r) => refs.has(r)));
      // A group that was dissolved or emptied is no longer the one worked in (`EDIT_TOOL::DeleteItems`: "If the entered group has been emptied then leave it").
      const enteredGroupId = state.enteredGroupId != null && (action.board.drawings?.groups ?? []).some((g) => g.id === state.enteredGroupId) ? state.enteredGroupId : null;
      return { ...state, board: action.board, boardError: null, layerVisible, layerOpacity, selection, hot, enteredGroupId };
    }
    case "BOARD_ERR":
      return { ...state, boardError: action.message };
    case "VERSION":
      return { ...state, version: action.version };
    case "SET_TAB":
      return { ...state, tab: action.tab };
    case "SET_SHEET_PATH":
      return { ...state, currentSheetPath: action.path, schNav: action.record === false ? state.schNav : pushToHistory(state.schNav, action.path) };
    case "SET_SCH_NAV":
      return { ...state, schNav: action.nav };
    case "SET_SCH_LINE_MODE":
      return { ...state, schLineMode: action.mode };
    case "TOGGLE_SCH_POSTURE":
      return { ...state, schPosture: !state.schPosture };
    case "SET_RIGHT_DOCK_TAB":
      return { ...state, rightDockTab: action.tab };
    case "SET_VIEWER3D_OPTIONS":
      return { ...state, viewer3d: { ...state.viewer3d, ...action.options } };
    case "SET_GLB_STATUS":
      return { ...state, glbStatus: action.status, glbError: action.error ?? null };
    case "SET_SELECTION": {
      const groups = state.board?.drawings?.groups;
      if (action.raw || !groups || groups.length === 0) return { ...state, selection: new Set(action.refs), armed: null, armedFootprint: null };
      // Selecting something outside the entered group leaves it (`PCB_SELECTION_TOOL::select`: `ExitGroup()`).
      const { ids, exits } = substituteSelection(groups, action.refs, state.enteredGroupId);
      return { ...state, selection: new Set(ids), armed: null, armedFootprint: null, enteredGroupId: exits ? null : state.enteredGroupId };
    }
    case "SET_ENTERED_GROUP":
      return { ...state, enteredGroupId: action.id };
    case "ENTER_GROUP": {
      // `PCB_SELECTION_TOOL::EnterGroup`: only a group can be entered; the selection becomes its members -- each as it is, a group inside it too.
      const group = state.board?.drawings?.groups.find((g) => g.id === action.id);
      if (!group) return state;
      return { ...state, enteredGroupId: group.id, selection: new Set(group.member_ids), armed: null };
    }
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
        armedFootprint: null,
        movePreview: null,
        activeTool: "select",
        drawState: null,
        zonePending: null,
        zoneEditId: null,
        dimensionEditId: null,
        textDialog: null,
        itemPropertiesId: null,
        boardPadPropertiesId: null,
        schLabelPending: null,
        schSheetPending: null,
        busUnfold: null,
        busUnfoldPicker: null,
        schPowerPending: null,
        schTextPending: null,
        armedSymbol: null,
        symbolProperties: null,
        pcbx: { ...state.pcbx, moveQueue: [], movingIndividually: false, zoneDrawMode: null },
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
      const inProgress = state.activeTool !== "select" || state.drawState != null || state.armed != null || state.armedFootprint != null || state.movePreview != null;
      if (inProgress) {
        return {
          ...state,
          armed: null,
          armedFootprint: null,
          movePreview: null,
          activeTool: "select",
          drawState: null,
          zonePending: null,
          zoneEditId: null,
          dimensionEditId: null,
          textDialog: null,
          schLabelPending: null,
          schSheetPending: null,
          busUnfold: null,
          busUnfoldPicker: null,
          schPowerPending: null,
          schTextPending: null,
          armedSymbol: null,
          symbolProperties: null,
          pcbx: { ...state.pcbx, moveQueue: [], movingIndividually: false, zoneDrawMode: null },
          // `LocalRatsnestTool`'s finalize handler: leaving the picker with Esc puts every pad's local ratsnest flag back to the global state.
          bcx: state.activeTool === "local_ratsnest" ? { ...state.bcx, localRatsnestPads: [] } : state.bcx,
        };
      }
      if (state.selection.size > 0) {
        return { ...state, selection: new Set() };
      }
      // Nothing selected but still inside a group: the selection tool's next `else if` tier, `ExitGroup()` -- which leaves the group and does not select
      // it (`ExitGroup( bool aSelectGroup = false )`; only the Leave Group action, `ExitGroup( true )`, does).
      if (state.enteredGroupId != null) {
        return { ...state, enteredGroupId: null };
      }
      // Idle, nothing selected, no entered group: pcbnew_settings.cpp m_ESCClearsNetHighlight defaults true.
      return { ...state, netHighlight: null, bcx: withHighlight(state.bcx, state.netHighlight, []) };
    }
    case "SET_HOT":
      return { ...state, hot: new Set(action.refs) };
    case "SET_NET_HIGHLIGHT":
      return { ...state, netHighlight: action.net, bcx: withHighlight(state.bcx, state.netHighlight, action.net ? [action.net] : []) };
    case "SET_NET_HIGHLIGHT_SET":
      return { ...state, netHighlight: action.nets[0] ?? null, bcx: withHighlight(state.bcx, state.netHighlight, action.nets) };
    case "SET_ARMED":
      return { ...state, armed: action.ref, armedFootprint: null, selection: new Set() };
    case "SET_ARMED_FOOTPRINT":
      return { ...state, armedFootprint: action.name, armed: null, selection: new Set() };
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
    case "SET_SCH_GRID_UM":
      return { ...state, schGridUm: action.um };
    case "TOGGLE_GRID_VISIBLE":
      return { ...state, gridVisible: !state.gridVisible };
    case "SET_MAGNETIC_ALL_LAYERS":
      return { ...state, magneticAllLayers: action.value };
    case "TOGGLE_CROSSHAIR":
      return { ...state, fullscreenCrosshair: !state.fullscreenCrosshair };
    case "SET_FULLSCREEN_CROSSHAIR":
      return { ...state, fullscreenCrosshair: action.value };
    case "TOGGLE_RATSNEST":
      // `SetElementVisibility( LAYER_RATSNEST )` sets every pad's local-ratsnest flag to the new global state: the Local Ratsnest tool's clicks are forgotten.
      return { ...state, showRatsnest: !state.showRatsnest, bcx: { ...state.bcx, localRatsnestPads: [] } };
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
      // `PCB_CONTROL::HighContrastMode`: NORMAL <-> DIMMED (from HIDDEN it goes back to NORMAL).
      return { ...state, highContrast: !state.highContrast, appearance: state.appearance.contrastHidden ? { ...state.appearance, contrastHidden: false } : state.appearance };
    case "SET_LAYER_VISIBLE":
      return { ...state, layerVisible: { ...state.layerVisible, [action.layer]: action.visible } };
    case "SET_LAYER_OPACITY":
      return { ...state, layerOpacity: { ...state.layerOpacity, [action.layer]: action.opacity } };
    case "SET_SELECTION_FILTER":
      return { ...state, selectionFilter: { ...state.selectionFilter, ...action.filter } };
    case "SET_SCH_SELECTION_FILTER":
      return { ...state, schSelectionFilter: action.filter };
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
    case "SET_BOARD_PAD_PROPERTIES_ID":
      return { ...state, boardPadPropertiesId: action.id };
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
    case "SET_SCH_WIRE_PICK":
      return { ...state, schWirePick: action.pick };
    case "SET_LOCAL_ORIGIN":
      return { ...state, localOriginUm: action.at };
    case "TOGGLE_AUTO_PAN":
      return { ...state, prefs: { ...state.prefs, autoPan: !state.prefs.autoPan } };
    case "SET_PREFERENCES":
      return { ...state, prefs: action.prefs };
    case "SET_PREFERENCES_DIALOG_OPEN":
      return { ...state, preferencesDialogOpen: action.open };
    case "SCHEMATIC_OK":
      return { ...state, schematic: action.schematic, schematicError: null, selection: state.tab === "schematic" ? keepOnSheet(state.selection, action.schematic) : state.selection };
    case "SCHEMATIC_ERR":
      return { ...state, schematicError: action.message };
    case "RATSNEST_OK":
      return { ...state, ratsnest: action.ratsnest };
    case "DRC_RUNNING":
      return { ...state, drcRunning: true, drcError: null };
    case "DRC_OK":
      // A fresh report invalidates any previous selection -- indices (and
      // the violations they pointed at) aren't stable across re-runs.
      return { ...state, drc: action.drc, drcRunning: false, drcError: null, drcVersion: action.version, drcSelected: null };
    case "DRC_ERR":
      return { ...state, drcRunning: false, drcError: action.message };
    case "DRC_CLEAR":
      return { ...state, drc: null, drcVersion: null, drcSelected: null };
    case "SET_DRC_REFILL":
      return { ...state, drcRefillZones: action.refill };
    case "SET_DRC_PARITY":
      return { ...state, drcParity: action.parity };
    case "DRC_PATCH_EXCLUDED":
      if (!state.drc) return state;
      return { ...state, drc: patchDrcExcluded(state.drc, action.keys, action.excluded, action.comment), drcVersion: action.version };
    case "DRC_PATCH_SEVERITY":
      if (!state.drc) return state;
      return { ...state, drc: patchDrcSeverity(state.drc, action.check, action.severity, action.description), drcVersion: action.version, drcSelected: null };
    case "ERC_PATCH_SEVERITY":
      if (!state.erc) return state;
      return { ...state, erc: patchErcSeverity(state.erc, action.check, action.severity, action.description), ercVersion: action.version, ercSelected: null };
    case "SET_DRC_SELECTED":
      return { ...state, drcSelected: action.index, drcLintSelected: action.index === null ? state.drcLintSelected : null };
    case "SET_DRC_LINT_SELECTED":
      return { ...state, drcLintSelected: action.index, drcSelected: action.index === null ? state.drcSelected : null };
    case "SET_ERC_DIALOG_OPEN":
      return { ...state, ercDialogOpen: action.open };
    case "ERC_RUNNING":
      return { ...state, ercRunning: true, ercError: null };
    case "ERC_OK":
      return { ...state, erc: action.erc, ercRunning: false, ercError: null, ercVersion: action.version, ercSelected: null };
    case "ERC_ERR":
      return { ...state, ercRunning: false, ercError: action.message };
    case "ERC_MARK_EXCLUDED": {
      if (!state.erc) return state;
      const counts = { ...state.erc.counts };
      const violations = state.erc.violations.map((v) => {
        if (v.check !== action.check || v.location !== action.location) return v;
        if (action.excluded && v.severity !== "excluded") counts[v.check] = Math.max(0, (counts[v.check] ?? 0) - 1);
        if (!action.excluded && v.severity === "excluded") counts[v.check] = (counts[v.check] ?? 0) + 1;
        return { ...v, severity: action.excluded ? ("excluded" as const) : (v.base_severity ?? ("error" as const)) };
      });
      return { ...state, erc: { ...state.erc, violations, counts }, ercVersion: action.version };
    }
    case "SET_ERC_SELECTED":
      return { ...state, ercSelected: action.index, ercLintSelected: action.index === null ? state.ercLintSelected : null };
    case "SET_ERC_LINT_SELECTED":
      return { ...state, ercLintSelected: action.index, ercSelected: action.index === null ? state.ercSelected : null };
    case "LINT_OK":
      return { ...state, lint: action.lint };
    case "SET_DRAW_STATE":
      return { ...state, drawState: action.draw };
    case "SET_ZONE_PENDING":
      return { ...state, zonePending: action.outline };
    case "SET_ZONE_EDIT_ID":
      return { ...state, zoneEditId: action.id };
    case "SET_NEXT_DIMENSION_KIND":
      return { ...state, nextDimensionKind: action.kind };
    case "SET_DIMENSION_EDIT_ID":
      return { ...state, dimensionEditId: action.id };
    case "FILL_OK":
      // Only the zones that are filled show a fill: all of them after Fill All, the named ones after a draft fill of a selection.
      return { ...state, zoneFill: keepFilled(action.fill, state.bcx.zoneFilled) };
    case "CLEAR_ZONE_FILL":
      // zone_filler_tool.cpp ZoneUnfillAll: discards the computed fill;
      // the canvas falls back to outline-only (painter.ts), same as a
      // zone that has never been filled at all.
      return { ...state, zoneFill: null, bcx: { ...state.bcx, zoneFilled: null } };
    case "SET_ZONE_DISPLAY_MODE":
      return { ...state, zoneDisplayMode: action.mode };
    case "SET_BOARD_SETUP_DIALOG_OPEN":
      return { ...state, boardSetupDialogOpen: action.open };
    case "SET_BOARD_SETUP_INITIAL_PAGE":
      return { ...state, boardSetupInitialPage: action.page };
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
    case "SET_SCH_SHEET_PENDING":
      return { ...state, schSheetPending: action.pending };
    case "SET_BUS_UNFOLD":
      return { ...state, busUnfold: action.unfold };
    case "SET_BUS_UNFOLD_PICKER":
      return { ...state, busUnfoldPicker: action.picker };
    case "SET_SCH_REPEAT":
      return { ...state, schRepeat: action.cmds };
    case "SET_SCH_LABEL_PENDING":
      return { ...state, schLabelPending: action.pending };
    case "SET_SCH_POWER_PENDING":
      return { ...state, schPowerPending: action.pending };
    case "SET_SCH_TEXT_PENDING":
      return { ...state, schTextPending: action.pending };
    case "SET_SCH_TOOL_DIALOG":
      return { ...state, schToolDialog: action.dialog };
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
    case "SET_SCH_DIALOG":
      return { ...state, schDialog: action.dialog };
    case "SET_SCH_FIND":
      return { ...state, schFind: { ...state.schFind, ...action.find } };
    case "SET_SCH_PLOT_DIALOG_OPEN":
      return { ...state, schPlotDialogOpen: action.open };
    case "SET_EXPORT_NETLIST_DIALOG_OPEN":
      return { ...state, exportNetlistDialogOpen: action.open };
    case "SET_CLIPBOARD":
      return { ...state, clipboard: action.clipboard };
    case "SET_MOVE_EXACT_DIALOG_OPEN":
      return { ...state, moveExactDialogOpen: action.open };
    case "SET_ROUTER_SETTINGS_DIALOG_OPEN":
      return { ...state, routerSettingsDialogOpen: action.open };
    case "SET_ROUTER_SETTINGS":
      return { ...state, routerSettings: action.settings };
    case "SET_LENGTH_TUNING_DIALOG_OPEN":
      return { ...state, lengthTuningDialogOpen: action.open, lengthTuningMode: action.mode ?? state.lengthTuningMode };
    case "PCBX":
      return { ...state, pcbx: { ...state.pcbx, ...action.patch } };
    case "BCX":
      return { ...state, bcx: { ...state.bcx, ...action.patch } };
    case "APPEARANCE":
      return withViewSlice(state, reduceView(viewSliceOf(state), action.op, netsContext(state.board)));
    case "APPEARANCE_LOAD":
      return withViewSlice(state, applyAppearanceFile(viewSliceOf(state), action.file, { copper: state.board?.layers ?? [] }));
    case "SET_NEXT_ZONE_IS_RULE_AREA":
      // Arming a plain zone/rule-area draw also drops any pending cutout/similar mode (pcbx.zoneDrawMode).
      return { ...state, nextZoneIsRuleArea: action.value, pcbx: { ...state.pcbx, zoneDrawMode: null } };
    case "SET_CLEANUP_TRACKS_DIALOG_OPEN":
      return { ...state, cleanupTracksDialogOpen: action.open };
    case "SET_EDIT_TRACKS_AND_VIAS_DIALOG_OPEN":
      return { ...state, editTracksAndViasDialogOpen: action.open };
    case "SET_BOARD_STATISTICS_DIALOG_OPEN":
      return { ...state, boardStatisticsDialogOpen: action.open };
    case "SET_SWAP_LAYERS_DIALOG_OPEN":
      return { ...state, swapLayersDialogOpen: action.open };
    case "SET_EDIT_TEXT_AND_GRAPHICS_DIALOG_OPEN":
      return { ...state, editTextAndGraphicsDialogOpen: action.open };
    case "SET_CREATE_ARRAY_DIALOG_OPEN":
      return { ...state, createArrayDialogOpen: action.open };
    default:
      return state;
  }
}

export interface StudioApi {
  /** Force an immediate /api/state refetch (after a command, or on demand). */
  refresh: () => Promise<void>;
  /** Run kicad-cli's DRC on the current design (seconds; `state.drcRunning` meanwhile). A run already in flight is not doubled. */
  runDrc: () => Promise<void>;
  /** Run kicad-cli's ERC on the current schematic -- same contract as `runDrc`. */
  runErc: () => Promise<void>;
  /** The store's latest state -- unlike the `useStudioState()` snapshot a handler closed over, it includes what an `await`ed refetch just loaded. */
  getState: () => StudioState;
  /** R/Shift+R (edit_tool.cpp Rotate): `refs` defaults to the selection; the caller passes RequestSelection's hover fallback. Footprints and vias rotate; several items share one pivot and commit as ONE undo step. */
  rotateSelection: (quarterTurns: number, refs?: string[]) => Promise<void>;
  ripSelection: () => Promise<void>;
  /** F (edit_tool.cpp Flip): footprints, one undo step for the whole selection. */
  flipSelection: (refs?: string[]) => Promise<void>;
  /** Several Cmds as ONE undo step (`Cmd::Batch`): all-or-nothing. A single Cmd is sent as itself. Returns whether the backend accepted it. */
  cmdBatch: (cmds: Cmd[]) => Promise<boolean>;
  /** pcbnew.AlignAndDistribute.align* (no default hotkey in source either -- reached from the right-click menu, Canvas.tsx's onContextMenu). Placed footprints only -- see kicad-port/alignDistribute.ts's own scope note. A no-op under 2 placed parts, same floor source's menu-visibility condition enforces. */
  alignSelection: (edge: AlignEdge) => Promise<void>;
  /** pcbnew.AlignAndDistribute.distribute* -- same placed-footprints-only scope. A no-op under 3 placed parts. */
  distributeSelection: (axis: "x" | "y", mode: "gaps" | "centers") => Promise<void>;
  /** Commit a completed drag: each ref moves by (dxUm, dyUm) from its current position, then (parts only) applies any rotate/flip accumulated during the move (MovePreview.rotateQuarterTurns/flipped -- edit_tool.cpp composes Move+Rotate+Flip as one undo step; this app commits them as sequential Cmds since each is independent of the others' position/orientation fields). `kind` picks which Cmd the move itself becomes (default "part"). */
  commitMove: (refs: string[], dxUm: number, dyUm: number, kind?: MovePreview["kind"], rotateQuarterTurns?: number, flipped?: boolean, perRefOffsetUm?: MovePreview["perRefOffsetUm"]) => Promise<void>;
  placeArmedAt: (xUm: number, yUm: number) => Promise<void>;
  /** Place Footprint's click: the footprint of a library armed in the Footprint Chooser becomes a part of its own at this point (`place_footprint`, one undo step). */
  placeLibraryFootprintAt: (xUm: number, yUm: number) => Promise<void>;
  /** GAPS.md #6: the Hierarchy panel's own "enter sheet"/"leave sheet"/jump-to-breadcrumb -- sets `state.currentSheetPath` and immediately refetches the schematic for it (the version-gated poll loop alone wouldn't notice a pure navigation with no backend mutation behind it). `[]` is the root. */
  navigateToSheet: (path: string[], record?: boolean) => Promise<void>;
  route: () => Promise<void>;
  undo: () => Promise<void>;
  redo: () => Promise<void>;
  /** Escape while a Duplicate's or a Paste's new items are carried: takes them away again (one undo step), provided nothing else was edited since. True when it did. */
  revertCarriedPlacement: () => Promise<boolean>;
  partByRef: (ref: string) => Part | undefined;
  trackById: (id: string) => Track | undefined;
  viaById: (id: string) => Via | undefined;
  zoneById: (id: string) => Zone | undefined;
  shapeById: (id: string) => Shape | undefined;
  textById: (id: string) => BoardText | undefined;
  /** Task item 5. */
  groupById: (id: string) => Group | undefined;
  /** Task item 7. */
  dimensionById: (id: string) => Dimension | undefined;
  /** `add_dimension`, returning the new dimension's own assigned id (or
   * `null` on failure) -- the draw tool needs it immediately, to open
   * DimensionPropertiesDialog on the thing it just created, the same
   * before/after-diff pattern `duplicateSelection`/`groupSelection`
   * already use for exactly this reason. */
  addDimension: (dimension: CmdDimension) => Promise<string | null>;
  symbolById: (id: string) => SchematicSymbol | undefined;
  wireById: (id: string) => SchematicWire | undefined;
  schTextById: (id: string) => SchematicText | undefined;
  /** R/Shift+R on the Schematic tab: rotate a symbol in place (quarterTurns: 1 = CCW/'R', 3 = CW/Shift+R, matching sch_edit_tool.cpp's own default). */
  rotateSymbol: (id: string, quarterTurns: number) => Promise<void>;
  /** sch_edit_tool.cpp Rotate/Mirror over a selection of any kind of schematic item: one item turns about its own anchor, several about the (half-grid) centre of the selection (`sch_move` rotate / mirror). One undo step. */
  transformSchItems: (ids: string[], turn: SchTurn, vertices?: Record<string, number[]>) => Promise<void>;
  /** Drop a held schematic selection: `state.movePreview` (kind `sch_move` / `sch_drag`) as one `sch_move` command, turns made while it was held included. */
  commitSchHeld: (preview: MovePreview) => Promise<void>;
  /** X on the Schematic tab ("Mirror Horizontally"). */
  mirrorSymbol: (id: string) => Promise<void>;
  /** Y on the Schematic tab ("Mirror Vertically") -- mutually exclusive with `mirrorSymbol` on the backend (Cmd::MirrorSymbolVertical's own doc). */
  mirrorSymbolVertical: (id: string) => Promise<void>;
  /** Del on the Schematic tab: removes the symbol instance; any wire landed on its pins is left dangling, same as real eeschema. */
  deleteSymbol: (id: string) => Promise<void>;
  /** Any other Cmd this file doesn't have a named wrapper for (the delete_ ops, set_track_width, edit_text, ...) -- returns whether the backend accepted it, same as every named wrapper's underlying runCmd. */
  cmd: (c: Cmd) => Promise<boolean>;
  /** `moveIndividually` hand-off to the next queued item (state.pcbx.moveQueue) -- after a drop commits, or for `skip` (Tab) without committing. */
  advanceMoveQueue: () => void;
  /**
   * Cmd+D on the current selection: footprints (as new parts), tracks, vias, zones, graphics, text, dimensions and groups -- see `Cmd::Duplicate`'s
   * own doc comment. Duplicates in place, then selects the new copies and arms the Move tool on them at the current cursor, same as
   * EDIT_TOOL::Duplicate handing straight off to doMoveSelection in source.
   */
  duplicateSelection: () => Promise<void>;
  /** Ctrl+G: group the current selection (2+ items), then select the new group as a unit. */
  groupSelection: () => Promise<void>;
  /** Ctrl+Shift+G: dissolve every group named in the current selection, then select their former members. */
  ungroupSelection: () => Promise<void>;
  /** Cmd+C: the selection as KiCad's clipboard text, on the system clipboard and in `state.clipboard`. `refs` (default: the selection) lets Cut/Copy honour RequestSelection's hover fallback; `reference` is the point Copy with Reference Point picked. */
  copySelection: (refs?: readonly string[], reference?: { x: number; y: number }) => Promise<void>;
  /** Cmd+V: put what is on the clipboard on the board (KiCad's text, else this session's copy, else plain text as a text item), then select it and arm Move on it, same as duplicateSelection. */
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
  addZone: (net: string, layer: string, outline: [Um, Um][], settings: ZoneSettingsFields & RuleAreaFields) => Promise<void>;
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
  // Preferences persist in the browser (the studio's equivalent of KiCad's common.json) -- UI settings, not design data.
  useEffect(() => {
    savePreferences(browserStorage(), state.prefs);
  }, [state.prefs]);
  const stateRef = useRef(state);
  stateRef.current = state;
  /**
   * `EDIT_TOOL::Duplicate` and `PCB_CONTROL::Paste` hand their new items to the Move tool inside the one commit the move finishes: cancelling the move
   * reverts it (`commit.Revert()`), so Escape leaves nothing behind. Here the duplicate or the paste is a step of its own, so a cancel undoes exactly that
   * step: this notes which one (the board's version right after it) while the new items are being carried.
   */
  const carriedPlacementRef = useRef<{ version: string } | null>(null);
  useEffect(() => {
    if (state.activeTool !== "move") carriedPlacementRef.current = null;
  }, [state.activeTool]);

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
      const asked = stateRef.current.currentSheetPath;
      const schematic = await fetchSchematic(asked);
      // The server shows the nearest sheet that still exists when the path has gone stale (an undo took the sheets away, say): follow it,
      // so the next edit is not addressed to a sheet that is no longer there. Only while nobody navigated meanwhile.
      const shown = (schematic.sheet_path ?? []).map((c) => c.id);
      if (asked.length > 0 && !samePath(shown, asked) && samePath(stateRef.current.currentSheetPath, asked)) dispatch({ type: "SET_SHEET_PATH", path: shown, record: false });
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

  // kicad-cli runs take seconds, so a run is started on demand (the dialogs'
  // Run buttons, and a dialog opening on a board it has not judged yet) and a
  // second request while one is in flight is dropped. The server keeps
  // answering edits and the version poll meanwhile, so the board stays
  // editable during a run: the report says which revision it was computed on
  // (`revision`), and the dialogs and markers show it out of date when the
  // board has moved on since (kicad-port/checkRevision.ts).
  const drcInFlight = useRef(false);
  const runDrc = useCallback(async () => {
    if (drcInFlight.current) return;
    drcInFlight.current = true;
    const askedAt = stateRef.current.version;
    dispatch({ type: "DRC_RUNNING" });
    try {
      const drc = await fetchDrc(stateRef.current.drcRefillZones, stateRef.current.drcParity);
      dispatch({ type: "DRC_OK", drc, version: revisionOf(drc, askedAt) });
    } catch (e) {
      dispatch({ type: "DRC_ERR", message: e instanceof Error ? e.message : String(e) });
    } finally {
      drcInFlight.current = false;
    }
  }, []);

  const ercInFlight = useRef(false);
  const runErc = useCallback(async () => {
    if (ercInFlight.current) return;
    ercInFlight.current = true;
    const askedAt = stateRef.current.version;
    dispatch({ type: "ERC_RUNNING" });
    try {
      const erc = await fetchErc();
      dispatch({ type: "ERC_OK", erc, version: revisionOf(erc, askedAt) });
    } catch (e) {
      dispatch({ type: "ERC_ERR", message: e instanceof Error ? e.message : String(e) });
    } finally {
      ercInFlight.current = false;
    }
  }, []);

  const refreshLint = useCallback(async () => {
    try {
      dispatch({ type: "LINT_OK", lint: await fetchLint() });
    } catch {
      // backend restarting, board mid-edit -- keep the last good result.
    }
  }, []);

  const refreshFill = useCallback(async () => {
    try {
      // The triangulation display needs each island's holes (`?polys=1`); every other mode draws the fractured ring it already has.
      const fill = await fetchFill(stateRef.current.zoneDisplayMode === "triangulated");
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
  // Shared view state (view_api.rs): apply a view revision someone else
  // (an agent, the CLI) wrote -- tab, selection, a one-shot zoom-to -- and
  // push our own tab/selection/camera back, debounced, as `ui`. Our own
  // revisions are never re-applied, so panning never fights itself.
  const lastViewRev = useRef(0);
  // What the server's view last held for each field, as far as this page
  // knows (applied from someone else, or pushed by us). Only fields that
  // differ are pushed, so a camera move never overwrites a selection an
  // agent just set (and vice versa).
  const synced = useRef<{ tab?: string; selection?: string; camera?: string }>({});
  const applyRemoteView = useCallback((v: SharedView) => {
    if (v.rev <= lastViewRev.current) return;
    lastViewRev.current = v.rev;
    if (v.by === "ui" || v.by == null) return;
    synced.current = { tab: v.tab, selection: JSON.stringify(v.selection ?? []), camera: synced.current.camera };
    const tabs: EditorTab[] = ["pcb", "schematic", "footprint", "symbol", "3d"];
    if (tabs.includes(v.tab as EditorTab) && v.tab !== stateRef.current.tab) dispatch({ type: "SET_TAB", tab: v.tab as EditorTab });
    dispatch({ type: "SET_SELECTION", refs: v.selection ?? [] });
    const rect = document.querySelector(".pcb-canvas-container")?.getBoundingClientRect();
    if (!rect) return;
    if (v.zoom_to && v.zoom_to.length) {
      let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
      for (const ref of v.zoom_to) {
        const p = stateRef.current.board?.parts.find((q) => q.ref === ref);
        if (!p?.placed || !p.courtyard) continue;
        x0 = Math.min(x0, p.courtyard[0]);
        y0 = Math.min(y0, p.courtyard[1]);
        x1 = Math.max(x1, p.courtyard[2]);
        y1 = Math.max(y1, p.courtyard[3]);
      }
      if (x0 < x1) dispatch({ type: "SET_VIEW", view: fitTransform({ minX: x0, minY: y0, maxX: x1, maxY: y1 }, rect.width, rect.height, 80) });
    } else if (v.center && v.scale) {
      dispatch({ type: "SET_VIEW", view: { scale: v.scale, x: rect.width / 2 - v.center[0] * v.scale, y: rect.height / 2 - v.center[1] * v.scale } });
    }
  }, []);

  useEffect(() => {
    const t = setTimeout(() => {
      const s = stateRef.current;
      const rect = document.querySelector(".pcb-canvas-container")?.getBoundingClientRect();
      const camera = rect && s.view.scale > 0 ? { center: [(rect.width / 2 - s.view.x) / s.view.scale, (rect.height / 2 - s.view.y) / s.view.scale] as [number, number], scale: s.view.scale } : null;
      const selection = [...s.selection];
      const patch: Parameters<typeof postView>[0] = {};
      if (synced.current.tab !== s.tab) patch.tab = s.tab;
      if (synced.current.selection !== JSON.stringify(selection)) patch.selection = selection;
      if (camera && synced.current.camera !== JSON.stringify(camera)) Object.assign(patch, camera);
      if (Object.keys(patch).length === 0) return;
      synced.current = { tab: s.tab, selection: JSON.stringify(selection), camera: camera ? JSON.stringify(camera) : synced.current.camera };
      postView({ ...patch, base_rev: lastViewRev.current })
        .then((v) => {
          // A newer view from someone else won (our update was stale): apply it.
          if (v.by !== "ui") applyRemoteView(v);
          else if (v.rev > lastViewRev.current) lastViewRev.current = v.rev;
        })
        .catch(() => {});
    }, 400);
    return () => clearTimeout(t);
  }, [state.tab, state.selection, state.view]);

  useEffect(() => {
    let stopped = false;
    let lastVersion: string | null = null;
    let lastSchematicFetch: string | null = null;
    let lastRatsnestFetch: string | null = null;
    let lastLintFetch: string | null = null;
    let lastFillFetch: string | null = null;
    const tick = async () => {
      try {
        fetchView().then(applyRemoteView).catch(() => {});
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
        // kicad-cli's DRC and ERC are not polled: they take seconds, so they
        // run on demand (runDrc/runErc). Our own lint checks are cheap and
        // in-process, and are shown by those two dialogs, so they follow the
        // board while either is open.
        if ((stateRef.current.drcDialogOpen || stateRef.current.ercDialogOpen) && lastLintFetch !== v) {
          lastLintFetch = v;
          await refreshLint();
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
  }, [refresh, refreshSchematic, refreshRatsnest, refreshLint, refreshFill, applyRemoteView]);

  const runCmd = useCallback(
    async (cmd: Parameters<typeof postCmd>[0]) => {
      // On a nested sheet every schematic edit is addressed to that sheet (`Cmd::OnSheet`), as KiCad edits whichever sheet is open.
      const reply = await postCmd(onCurrentSheet(cmd, stateRef.current.tab === "schematic" ? stateRef.current.currentSheetPath : []), stateRef.current.strict);
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "error" });
      // `SCH_EDIT_FRAME::SaveCopyForRepeatItem`: a placement on the schematic becomes what "Repeat Last Item" repeats.
      if (reply.ok && stateRef.current.tab === "schematic") {
        const source = repeatSource(cmd);
        if (source) dispatch({ type: "SET_SCH_REPEAT", cmds: source });
      }
      await refresh();
      // The schematic tab draws `state.schematic`, which the version poll refetches up to a poll later: an edit made there (the Properties grid shows what the board
      // answered) wants the new sheet at once.
      if (stateRef.current.tab === "schematic") await refreshSchematic();
      return reply.ok;
    },
    [refresh, refreshSchematic]
  );

  /** One undo step for N Cmds (`Cmd::Batch`); a lone Cmd is sent as itself. */
  const runBatch = async (cmds: Cmd[]): Promise<boolean> => {
    if (cmds.length === 0) return true;
    return runCmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
  };

  /** The grid point nearest a reference point (`PCB_GRID_HELPER::BestSnapAnchor`, its grid half): what a multi-item Rotate and Move snap their reference to. */
  const gridSnap = () => (p: readonly [number, number]): [number, number] => snapPoint(p[0], p[1], stateRef.current.gridUm);

  /** Sends a transform plan (kicad-port/pcbTransform.ts) as one undo step; says so when the locks left nothing to do (`ReportFilteredLockedItems`). */
  const runPlan = async (plan: TransformPlan): Promise<boolean> => {
    if (plan.cmds.length === 0) {
      if (plan.lockedOut) dispatch({ type: "TOAST", message: "Selection contains locked items.", kind: "info" });
      return false;
    }
    return runBatch(plan.cmds);
  };

  /** Where a Paste lands the clipboard's origin: the cursor's grid point, else the middle of the board. */
  const pasteAt = (): { x: number; y: number } => {
    const st = stateRef.current;
    if (st.cursorUm) {
      const [x, y] = snapPoint(st.cursorUm.x, st.cursorUm.y, st.gridUm);
      return { x, y };
    }
    const o = st.board?.outline;
    if (o && o.length > 0) return { x: Math.round((Math.min(...o.map((p) => p[0])) + Math.max(...o.map((p) => p[0]))) / 2), y: Math.round((Math.min(...o.map((p) => p[1])) + Math.max(...o.map((p) => p[1]))) / 2) };
    return { x: 0, y: 0 };
  };

  /**
   * `EDIT_TOOL::Move`'s moveIndividually hand-off ("if( ++itemIdx <
   * orig_items.size() ) { ...Pick up new item }"): once the dropped item has
   * committed, glue the next queued one to the cursor -- selection becomes
   * just that item, and the move origin is ITS anchor so the anchor lands
   * on the cursor (`nextItem->Move( cursor - nextItem->GetPosition() )`).
   * Also what `pcbnew.InteractiveEdit.skip` (Tab) calls, minus the commit.
   * No-op when the queue is empty (every ordinary move).
   */
  const advanceMoveQueue = (queue: readonly string[] = stateRef.current.pcbx.moveQueue) => {
    const board = stateRef.current.board;
    if (queue.length === 0 || !board) {
      // Last item dropped/skipped: Move Individually is over.
      if (stateRef.current.pcbx.movingIndividually) {
        dispatch({ type: "PCBX", patch: { movingIndividually: false } });
        dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      }
      return;
    }
    const [next, ...rest] = queue;
    const item = movableItem(board, next!);
    dispatch({ type: "PCBX", patch: { moveQueue: rest } });
    if (!item) {
      // Vanished or unmovable since the queue was built: skip straight on.
      advanceMoveQueue(rest);
      return;
    }
    dispatch({ type: "SET_SELECTION", refs: [next!] });
    dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
    dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
    dispatch({ type: "SET_MOVE_ORIGIN", at: { x: item.at[0], y: item.at[1] } });
  };

  const api: StudioApi = {
    refresh,
    runDrc,
    runErc,
    getState: () => stateRef.current,
    partByRef: (ref) => stateRef.current.board?.parts.find((p) => p.ref === ref),
    trackById: (id) => stateRef.current.board?.routing?.tracks.find((t) => t.id === id),
    viaById: (id) => stateRef.current.board?.routing?.vias.find((v) => v.id === id),
    zoneById: (id) => stateRef.current.board?.routing?.zones.find((z) => z.id === id),
    groupById: (id) => stateRef.current.board?.drawings?.groups.find((g) => g.id === id),
    dimensionById: (id) => stateRef.current.board?.drawings?.dimensions.find((d) => d.id === id),
    addDimension: async (dimension) => {
      const board = stateRef.current.board;
      if (!board) return null;
      const before = allItemIds(board);
      const ok = await runCmd({ op: "add_dimension", dimension });
      if (!ok) return null;
      const after = stateRef.current.board;
      if (!after) return null;
      return [...allItemIds(after)].find((id) => !before.has(id)) ?? null;
    },
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
    transformSchItems: async (ids, turn, vertices) => {
      if (ids.length === 0) return;
      await runCmd(turnCmd(ids, turn, vertices));
    },
    commitSchHeld: async (preview) => {
      dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
      const origin = stateRef.current.moveOriginUm;
      const cmd = heldCmd({
        mode: preview.kind === "sch_drag" ? "drag" : "move",
        ids: preview.refs,
        vertices: preview.vertices,
        dxUm: preview.dxUm,
        dyUm: preview.dyUm,
        turns: preview.turns,
        holdUm: preview.holdUm ?? holdPoint(origin, preview.dxUm, preview.dyUm),
      });
      if (cmd) await runCmd(cmd);
    },
    cmd: (c) => runCmd(c),
    advanceMoveQueue: () => advanceMoveQueue(),
    // edit_tool.cpp's Rotate over any selection (kicad-port/pcbTransform.ts `planRotate`): one item turns about its own position, several about the
    // centre of their box snapped to the grid, a lone rectangle or polygon about its centre; locked items stay. One `rotate_items`, so one undo
    // step, as one BOARD_COMMIT::Push. R is counter-clockwise: the callers pass 1 for R and 3 for Shift+R.
    rotateSelection: async (quarterTurns, explicit) => {
      const st = stateRef.current;
      if (!st.board) return;
      const q = ((quarterTurns % 4) + 4) % 4;
      await runPlan(planRotate(st.board, explicit ?? [...st.selection], q === 3 ? -1 : q, gridSnap()));
    },
    cmdBatch: (cmds) => runBatch(cmds),
    ripSelection: async () => {
      const refs = [...stateRef.current.selection];
      dispatch({ type: "CLEAR_SELECTION" });
      await runBatch(refs.map((ref): Cmd => ({ op: "rip", part: ref })));
    },
    // edit_tool.cpp's Flip ("Change Side / Flip", F) over any selection (`planFlip`): mirrored about the centre of the selection's box (a lone item
    // about its own position), each item put on the other side of the board -- a footprint's side, a track's, zone's, graphic's, text's and
    // dimension's layer. One `flip_items`, one undo step.
    flipSelection: async (explicit) => {
      const st = stateRef.current;
      if (!st.board) return;
      await runPlan(planFlip(st.board, explicit ?? [...st.selection]));
    },
    // align_distribute_tool.cpp (kicad-port/alignDistribute.ts): every kind of item, a locked item is the target and never moves, the cursor can
    // pick the target; one move per item, one undo step.
    alignSelection: async (edge) => {
      const st = stateRef.current;
      if (!st.board) return;
      const cursor: [number, number] | null = st.cursorUm ? [st.cursorUm.x, st.cursorUm.y] : null;
      const cmds = planAlignSelection(st.board, [...st.selection], edge, cursor);
      if (cmds.length > 0) await runBatch(cmds);
    },
    distributeSelection: async (axis, mode) => {
      const st = stateRef.current;
      if (!st.board) return;
      const cmds = planDistributeSelection(st.board, [...st.selection], axis, mode);
      if (cmds.length > 0) await runBatch(cmds);
    },
    commitMove: async (refs, dxUm, dyUm, kind = "part", rotateQuarterTurns, flipped, perRefOffsetUm) => {
      dispatch({ type: "SET_MOVE_PREVIEW", preview: null });
      carriedPlacementRef.current = null; // dropped: the duplicate or paste stands
      // Any mix of PCB items: turned and flipped about where it was picked up, then moved, as one batch (`planCarry`).
      if (kind === "pcb") {
        const board = stateRef.current.board;
        if (board) await runPlan(planCarry(board, refs, dxUm, dyUm, rotateQuarterTurns ?? 0, flipped ?? false, gridSnap()));
        advanceMoveQueue();
        return;
      }
      const cmds: Cmd[] = [];
      for (const ref of refs) {
        if (kind === "part") {
          const p = api.partByRef(ref);
          // Pack and Move: this ref's own SpreadFootprints shift rides along with the shared drag delta.
          const own = perRefOffsetUm?.[ref];
          if (p?.placed && p.at) cmds.push({ op: "move_to", part: ref, x: p.at[0] + dxUm + (own?.[0] ?? 0), y: p.at[1] + dyUm + (own?.[1] ?? 0) });
          // Rotating/flipping about the part's own (already-moved) anchor
          // is exactly what the backend's Rotate/Flip ops do regardless of
          // when they're called, so applying them after the move lands on
          // the same final pose as KiCad's live in-place spin during a
          // single-item drag -- see MovePreview's own doc comment.
          // (R presses count counter-clockwise; `Cmd::Rotate` turns clockwise.)
          if (rotateQuarterTurns) cmds.push({ op: "rotate", part: ref, quarter_turns: (4 - (((rotateQuarterTurns % 4) + 4) % 4)) % 4 });
          if (flipped) cmds.push({ op: "flip", part: ref });
        } else if (kind === "via") {
          const v = api.viaById(ref);
          if (v) cmds.push({ op: "move_via", id: ref, x: v.x + dxUm, y: v.y + dyUm });
        } else if (kind === "shape") {
          cmds.push({ op: "move_shape", id: ref, dx: dxUm, dy: dyUm });
        } else if (kind === "text") {
          const t = api.textById(ref);
          if (t) cmds.push({ op: "move_text", id: ref, x: t.x + dxUm, y: t.y + dyUm });
        } else if (kind === "dimension") {
          cmds.push({ op: "move_dimension", id: ref, dx: dxUm, dy: dyUm });
        }
      }
      // One undo step for the whole drop (BOARD_COMMIT::Push once).
      await runBatch(cmds);
      advanceMoveQueue();
    },
    placeLibraryFootprintAt: async (xUm, yUm) => {
      const name = stateRef.current.armedFootprint;
      if (!name) return;
      // `BOARD_EDITOR_CONTROL::PlaceFootprint` commits on the click and goes back to asking for the next footprint; here the tool is done after one.
      dispatch({ type: "SET_ARMED_FOOTPRINT", name: null });
      await runCmd({ op: "place_footprint", footprint: name, at: { x: xUm, y: yUm } });
    },
    placeArmedAt: async (xUm, yUm) => {
      const ref = stateRef.current.armed;
      if (!ref) return;
      dispatch({ type: "SET_ARMED", ref: null });
      const ok = await runCmd({ op: "place_at", part: ref, x: xUm, y: yUm });
      if (ok) dispatch({ type: "SET_SELECTION", refs: [ref] });
    },
    navigateToSheet: async (path, record = true) => {
      dispatch({ type: "SET_SHEET_PATH", path, record });
      // Clearing the selection/net-highlight on navigation matches real
      // eeschema's own `SCH_SHEET_PATH::UpdateAllScreenReferences`-adjacent
      // behavior (switching sheets repaints the view fresh) and avoids a
      // stale ref from the old sheet lingering selected/hot on the new one.
      dispatch({ type: "SET_SELECTION", refs: [] });
      try {
        const schematic = await fetchSchematic(path);
        dispatch({ type: "SCHEMATIC_OK", schematic });
      } catch (e) {
        dispatch({ type: "SCHEMATIC_ERR", message: e instanceof Error ? e.message : String(e) });
      }
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
    revertCarriedPlacement: async () => {
      const token = carriedPlacementRef.current;
      carriedPlacementRef.current = null;
      if (!token || (await fetchVersion().catch(() => null)) !== token.version) return false;
      const reply = await postUndo("pcb");
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "info" });
      await refresh();
      return reply.ok;
    },
    redo: async () => {
      dispatch({ type: "CLEAR_SELECTION" });
      const reply = await postRedo(stateRef.current.tab === "schematic" ? "schematic" : "pcb");
      if (!reply.ok) dispatch({ type: "TOAST", message: reply.message, kind: "info" });
      await refresh();
    },
    // EDIT_TOOL::Duplicate hands straight off to doMoveSelection in
    // source -- the duplicate appears glued to the cursor until the user
    // clicks to drop it, or cancels with Escape, which takes the duplicate
    // away again (`commit.Revert()`; here `revertCarriedPlacement`, one undo
    // step for the step the duplicate was).
    duplicateSelection: async () => {
      const board = stateRef.current.board;
      if (!board) return;
      // `Duplicate` filters markers, groups and free pads only: a locked item is copied too (the copy is not locked), a pad stands for its footprint.
      const { ids } = editableSelection(board, [...stateRef.current.selection], { respectLocks: false });
      if (ids.length === 0) return;
      const before = allItemIds(board);
      const ok = await runCmd({ op: "duplicate", ids });
      if (!ok) return;
      // The board as the backend has it now: `stateRef` only catches up on the next render, so the copies would not be in it yet
      // and the Move tool would never pick them up.
      const after = await fetchState().catch(() => null);
      if (!after) return;
      const newIds = newItemIds(before, after);
      if (newIds.length === 0) return;
      const version = await fetchVersion().catch(() => null);
      carriedPlacementRef.current = version ? { version } : null;
      dispatch({ type: "SET_SELECTION", refs: newIds });
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
      dispatch({ type: "SET_MOVE_ORIGIN", at: stateRef.current.cursorUm });
    },
    // `common.Interactive.group`/`ungroup` (task item 5). `state.selection`
    // already holds whatever `withGroupSubstitution` resolved a click to
    // (a group's own id for any of its members), so a multi-select here is
    // already "the right set of things to act on" with no extra lookup.
    groupSelection: async () => {
      const board = stateRef.current.board;
      if (!board) return;
      const ids = [...stateRef.current.selection];
      if (ids.length < 2) return;
      const before = new Set((board.drawings?.groups ?? []).map((g) => g.id));
      const ok = await runCmd({ op: "group", ids });
      if (!ok) return;
      // The board as the backend has it now (`stateRef` only catches up on the next render): the new group is what is selected afterwards
      // (`RunAction( ACTIONS::selectItem, group )`), as it is -- not as the group above it, when it was made inside an entered group.
      const after = await fetchState().catch(() => null);
      const newGroupId = (after?.drawings?.groups ?? []).map((g) => g.id).find((id) => !before.has(id));
      if (newGroupId) dispatch({ type: "SET_SELECTION", refs: [newGroupId], raw: true });
    },
    ungroupSelection: async () => {
      const groupIds = [...stateRef.current.selection].filter((id) => api.groupById(id));
      if (groupIds.length === 0) return;
      // Capture members before they're released, so the resulting
      // selection is "whatever was inside" -- source's own Ungroup
      // selects the released members, not nothing.
      const members = groupIds.flatMap((id) => api.groupById(id)?.member_ids ?? []);
      const ok = await runCmd({ op: "ungroup", ids: groupIds });
      if (!ok) return;
      dispatch({ type: "SET_SELECTION", refs: members, raw: true });
    },
    addZone: async (net, layer, outline, settings) => {
      // The board as the backend has it now (several zones can be added one after the other, e.g. "Create Zone from Selection").
      const board = await fetchState().catch(() => null);
      if (!board) return;
      const before = allItemIds(board);
      const ok = await runCmd({ op: "add_zone", net, layer, outline: outline.map(([x, y]) => ({ x, y })) });
      if (!ok) return;
      // Read the board from the backend: `stateRef` only catches up on the next render, which is after this continuation, so the new
      // zone would not be in it yet and its settings (a rule area's flags, say) would never be applied.
      const after = await fetchState().catch(() => null);
      if (!after) return;
      const newId = [...allItemIds(after)].find((id) => !before.has(id));
      if (!newId) return;
      // Default-valued settings need no follow-up at all -- `add_zone`
      // already landed on exactly that.
      const defaults = { ...DEFAULT_ZONE_SETTINGS, ...DEFAULT_RULE_AREA_SETTINGS };
      const unchanged = (Object.keys(defaults) as (keyof typeof defaults)[]).every((k) => settings[k] === defaults[k]);
      if (!unchanged) await runCmd({ op: "edit_zone", id: newId, net, layer, ...settings });
    },
    // edit_tool.cpp copyToClipboard: the selection (a pad stands for its footprint, a locked item is copied) as KiCad's own clipboard text, measured
    // from the cursor's grid point or the point Copy with Reference picked -- on the system clipboard, where KiCad can paste it, and here.
    copySelection: async (refs, reference) => {
      const st = stateRef.current;
      if (!st.board) return;
      const { ids } = editableSelection(st.board, refs ? [...refs] : [...st.selection], { respectLocks: false });
      if (ids.length === 0) return;
      const at = reference ?? (st.cursorUm ? (([x, y]) => ({ x, y }))(snapPoint(st.cursorUm.x, st.cursorUm.y, st.gridUm)) : null);
      const reply = await postClipboardCopy(ids, at);
      if (!reply.ok) {
        dispatch({ type: "TOAST", message: reply.message, kind: "error" });
        return;
      }
      dispatch({ type: "SET_CLIPBOARD", clipboard: at ? { text: reply.text, reference: at } : { text: reply.text } });
      await writeClipboardText(reply.text);
    },
    // pcb_control.cpp Paste: whatever KiCad text is on the system clipboard (a copy made in KiCad, or on another board), else this session's own copy
    // where the browser will not hand the system clipboard over; text that is not KiCad's is pasted as a text item on the active layer ("If it wasn't
    // content, then paste as a text object"). The items land with the clipboard's origin on the cursor and are selected, the Move tool armed on them.
    pasteClipboard: async () => {
      const st = stateRef.current;
      const board = st.board;
      if (!board) return;
      const system = await readClipboardText();
      const at = pasteAt();
      let cmd: Cmd;
      if (system != null && isKicadPcbText(system)) {
        cmd = { op: "paste_clipboard", text: system, at };
      } else if (system != null && system.trim() !== "") {
        cmd = { op: "add_text", text: { content: system.trim(), at, angle: 0, layer: st.activeLayer ?? "F.SilkS", size_um: 1000, stroke_width: 150, justify: "center", mirror: false } };
      } else if (st.clipboard) {
        cmd = { op: "paste_clipboard", text: st.clipboard.text, at };
      } else {
        return;
      }
      const before = allItemIds(board);
      const ok = await runCmd(cmd);
      if (!ok) return;
      // As in `duplicateSelection`: read the board from the backend, `stateRef` is still the one from before the paste.
      const after = await fetchState().catch(() => null);
      if (!after) return;
      const newIds = newItemIds(before, after);
      if (newIds.length === 0) return;
      const version = await fetchVersion().catch(() => null);
      carriedPlacementRef.current = version ? { version } : null;
      dispatch({ type: "SET_SELECTION", refs: newIds });
      dispatch({ type: "SET_ACTIVE_TOOL", tool: "move" });
      dispatch({ type: "SET_MOVE_ORIGIN", at });
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
