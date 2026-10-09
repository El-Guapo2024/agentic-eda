// The right-click menu of the board canvas, built from the studio's state: which tool owns the click (the router, a drawing tool, a picker, the selection tool),
// what the selection is, and the lists the Zoom, Grid and Track/Via Width submenus show. The entries and their conditions are kicad-port/pcbContextMenu.ts's.
import type { MenuNode } from "../../kicad/types";
import type { BoardState } from "../../api/types";
import type { StudioState } from "../../state/store";
import { picker } from "../../actions/pcbPicker";
import { getGridSettings } from "../../state/gridSettings";
import { formatLength, type LengthUnit } from "../../state/units";
import { scaleToZoomFactor, zoomListFor } from "../../kicad-port/zoomFit";
import { activeEditPoint, canRemoveCorner } from "../../kicad-port/pcbPointEdit";
import { handleToleranceUm, pointItemOf, pointsOfItem, ringOf } from "../../actions/pcbPointEditSweep";
import { boardHasItems, summarizePcbSelection } from "../../kicad-port/pcbSelectionSummary";
import {
  defaultMenuContext,
  drawingMenu,
  pcbSelectionMenu,
  pickerMenu,
  routerMenu,
  type DrawingMode,
  type PcbMenuContext,
  type RouterMenuContext,
} from "../../kicad-port/pcbContextMenu";
import { routeQueueActive } from "./routeQueue";

/** `DRAWING_TOOL::m_mode` of the tools that are the board canvas's own drawing tools. */
const DRAWING_MODES: Partial<Record<string, DrawingMode>> = {
  draw_segment: "line",
  draw_arc: "arc",
  draw_bezier: "bezier",
  draw_rect: "rectangle",
  draw_circle: "circle",
  draw_polygon: "polygon",
  text: "text",
  dimension: "dimension",
  via: "via",
  zone: "zone",
};

/** The tools that are `PICKER_TOOL` sessions in KiCad: a click answers a question, and the right click only offers Cancel and the standard submenus. */
const PICKER_TOOLS = new Set<string>(["local_ratsnest", "drill_origin", "measure"]);

const otherUnit = (unit: LengthUnit): LengthUnit => (unit === "mm" ? "mil" : "mm");

/** `ZOOM_MENU::update`: every zoom of the editor's list as "Zoom: 1.00", the one nearest the current zoom checked. */
function zoomEntries(state: StudioState): PcbMenuContext["zoom"] {
  const zoom = scaleToZoomFactor(state.view.scale || 1);
  return zoomListFor("pcb").map((factor) => ({ label: `Zoom: ${factor.toFixed(2)}`, checked: Math.abs(factor - zoom) / zoom < 0.1 }));
}

/** `GRID_MENU::update` and `BuildChoiceList`: each grid in the display unit and, in brackets, the other system's; the current one checked. */
function gridEntries(state: StudioState): PcbMenuContext["grid"] {
  return getGridSettings("pcb").grids.map((um) => ({ label: `${formatLength(um, state.units)} (${formatLength(um, otherUnit(state.units))})`, checked: um === state.gridUm }));
}

/** The point editor's handle under the pointer (`PCB_POINT_EDITOR::HasCorner / HasMidpoint / CanRemoveCorner`). */
function handleUnder(state: StudioState, board: BoardState, refs: readonly string[], at: readonly [number, number]): PcbMenuContext["handle"] {
  const item = refs.length === 1 ? pointItemOf(board, new Set(refs)) : null;
  const active = item ? activeEditPoint(pointsOfItem(item), at, handleToleranceUm(state.view.scale)) : null;
  const ring = item ? ringOf(item) : null;
  return { corner: active?.kind === "corner", midpoint: active?.kind === "midpoint", canRemoveCorner: !!ring && active?.kind === "corner" && active.id.startsWith("v") && canRemoveCorner(ring) };
}

/** What the board editor's tools are doing, for the conditions of the menus. */
export function menuContext(state: StudioState, board: BoardState, refs: readonly string[], at: readonly [number, number]): PcbMenuContext {
  const moving = state.activeTool === "move" || state.activeTool === "drag" || state.movePreview != null;
  return {
    ...defaultMenuContext(),
    toolActive: state.activeTool !== "select" || state.drawState != null || state.armed != null || state.movePreview != null || picker.session() != null,
    moving,
    movingIndividually: state.pcbx.movingIndividually,
    inGroup: state.enteredGroupId != null,
    boardHasItems: boardHasItems(board),
    haveHighlight: state.netHighlight != null || state.bcx.netHighlightMore.length > 0,
    handle: handleUnder(state, board, refs, at),
    zoom: zoomEntries(state),
    grid: gridEntries(state),
  };
}

/** `TRACK_WIDTH_MENU::update`: "Use Starting Track Width", "Use Net Class Values", "Use Custom Values...", the track widths, the via sizes -- the current one checked. */
export function trackViaWidthMenu(state: StudioState, board: BoardState): MenuNode[] {
  const rules = board.board_rules;
  const widths = [rules?.track_width ?? 250, ...(board.routing?.track_width_presets ?? [])];
  const vias = [{ diameter: rules?.via_diameter ?? 600, drill: rules?.via_drill ?? 300 }, ...(board.routing?.via_presets ?? [])];
  const auto = state.bcx.autoTrackWidth;
  const useIndex = !auto;
  const width = state.currentTrackWidthUm;
  const via = state.currentViaPreset;
  const mm = (um: number) => formatLength(um, state.units);
  const out: MenuNode[] = [
    { type: "item", action: "pcbnew.EditorControl.autoTrackWidth", label: "Use Starting Track Width", checked: auto },
    { type: "item", action: "studio.Router.useNetclassSizes", label: "Use Net Class Values", checked: useIndex && width == null && via == null },
    { type: "item", action: "pcbnew.InteractiveRouter.CustomTrackViaSize", label: "Use Custom Values...", checked: false },
    { type: "separator" },
  ];
  widths.forEach((w, i) => {
    const current = width == null ? i === 0 : width === w;
    out.push({ type: "item", action: "studio.Router.setTrackWidth", label: i === 0 ? "Track netclass width" : `Track ${mm(w)}`, arg: i, checked: useIndex && current });
  });
  out.push({ type: "separator" });
  vias.forEach((v, i) => {
    const current = via == null ? i === 0 : via.diameter === v.diameter && via.drill === v.drill;
    out.push({ type: "item", action: "studio.Router.setViaSize", label: i === 0 ? "Via netclass values" : v.drill > 0 ? `Via ${mm(v.diameter)}, hole ${mm(v.drill)}` : `Via ${mm(v.diameter)}`, arg: i, checked: useIndex && current });
  });
  return out;
}

/** `DIFF_PAIR_MENU::update`: "Use Net Class Values" and "Use Custom Values..." (this studio keeps no list of pair sizes besides the custom one). */
export function diffPairMenu(state: StudioState): MenuNode[] {
  const custom = state.pcbx.customDiffPair != null;
  return [
    { type: "item", action: "studio.Router.useNetclassDiffPair", label: "Use Net Class Values", checked: !custom },
    { type: "item", action: "pcbnew.InteractiveRouter.DiffPairDialog", label: "Use Custom Values...", checked: custom },
  ];
}

function routerContext(state: StudioState, board: BoardState): RouterMenuContext {
  const draw = state.drawState;
  const routing = draw?.kind === "route" || draw?.kind === "diffpair";
  const net = draw?.kind === "route" ? draw.net : null;
  return {
    routing,
    inRouteSelected: routeQueueActive(),
    // `hasOtherEnd`: the net being routed still has ratsnest lines, so something is left unconnected to finish to.
    hasOtherEnd: net != null && (state.ratsnest?.edges ?? []).some((e) => e.net === net),
    hasSelection: state.selection.size > 0,
    haveHighlight: state.netHighlight != null || state.bcx.netHighlightMore.length > 0,
    diffPair: state.activeTool === "diffpair" || draw?.kind === "diffpair",
    // The studio's router always routes 45-degree mitered corners (`PNS::ROUTING_SETTINGS`' default corner mode).
    cornerMode: "45",
    trackViaMenu: trackViaWidthMenu(state, board),
    diffPairMenu: diffPairMenu(state),
    zoom: zoomEntries(state),
    grid: gridEntries(state),
  };
}

/**
 * The menu a right click opens at `at` (board um) with `refs` selected: the router's while the router is armed or routing, a drawing tool's while one is
 * drawing, a picker's while a pick is running, else the selection tool's -- which a running Move shows too, with Cancel on top.
 */
export function buildPcbMenu(state: StudioState, board: BoardState, refs: readonly string[], at: readonly [number, number]): MenuNode[] {
  const tool = state.activeTool;
  const draw = state.drawState;
  if (tool === "route" || tool === "diffpair" || draw?.kind === "route" || draw?.kind === "diffpair") return routerMenu(routerContext(state, board));
  const mode = DRAWING_MODES[tool];
  const ctx = menuContext(state, board, refs, at);
  if (mode) return drawingMenu(mode, ctx);
  if (picker.session() != null || PICKER_TOOLS.has(tool)) return pickerMenu(ctx);
  return pcbSelectionMenu(summarizePcbSelection(board, refs), ctx);
}
