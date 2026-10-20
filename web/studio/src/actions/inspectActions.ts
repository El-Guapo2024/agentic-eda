// Inspect > Clearance Resolution and Constraints Resolution (`BOARD_INSPECTION_TOOL::InspectClearance` / `InspectConstraints`, pcbnew/tools/board_inspection_tool.cpp at 8303b2ad):
//
//   Clearance:   two items selected: the report. One selected: it is the first item and the second is picked ("Select second item for clearance resolution..."); none: both are
//                picked, the first one staying highlighted while the second is. The same item twice is "Select two different items for clearance resolution." in the info bar.
//   Constraints: one item selected: the report; none: it is picked ("Select item for constraints resolution..."); more: "Select a single item for a constraints resolution report."
//
// The report itself -- which rule wins and every rule that was considered, in KiCad's words -- is the server's (POST /api/inspect/*, crates/drc/src/inspect.rs); this only asks for it
// and opens the dialog (components/InspectReportDialog.tsx). Nothing here checks anything: DRC stays kicad-cli's.
import type { Dispatch } from "react";
import type { Action, StudioApi, StudioState } from "../state/store";
import type { SweepCtx } from "./pcbSweepKit";
import { inspectClearance as postClearance, inspectConstraints as postConstraints } from "../api/client";
import { itemKind } from "../kicad-port/pcbItems";
import { activeLayerName } from "../kicad-port/layerPresets";
import { CLEARANCE_KINDS, CONSTRAINT_KINDS, refineInspectionPick, type InspectPickContext } from "../kicad-port/inspectPick";
import { showInspectReport } from "../state/inspectReports";
import { pickItemAt } from "./pcbPicker";
import type { SelectableKind } from "../components/canvas/selectionCandidates";

type At = [number, number];

/** The items the inspections take (`clearanceTypes` / `constraintTypes`). */
function inspectable(state: StudioState, id: string, kinds: readonly SelectableKind[]): boolean {
  const board = state.board;
  if (!board) return false;
  const k = itemKind(board, id);
  if (!k) return false;
  const kind: SelectableKind | null = k === "dimension" || k === "group" ? null : (k as SelectableKind);
  return kind !== null && kinds.includes(kind);
}

function pickContext(state: StudioState): InspectPickContext {
  return { filter: state.selectionFilter, layerVisible: state.layerVisible, activeLayer: state.activeLayer, highContrast: state.highContrast, selection: state.selection, onePixelUm: 1 / state.view.scale, toleranceUm: Math.max(150, 6 / state.view.scale) };
}

interface Picked {
  id: string;
  at: At | undefined;
}

/** `pickItemForInspection`: a click on the canvas, the footprint it found given way to the pad or track under it. */
async function pick(api: StudioApi, prompt: string, kinds: readonly SelectableKind[]): Promise<Picked | null> {
  const hit = await pickItemAt(prompt);
  if (!hit) return null;
  const state = api.getState();
  const id = state.board ? refineInspectionPick(state.board, hit.id, hit.at, pickContext(state), kinds) : hit.id;
  return { id, at: [hit.at.x, hit.at.y] };
}

const error = (dispatch: Dispatch<Action>, message: string): void => dispatch({ type: "TOAST", message, kind: "error" });

/** `reportClearance`. */
export async function inspectClearance(api: StudioApi, dispatch: Dispatch<Action>): Promise<void> {
  const state = api.getState();
  if (!state.board) return;
  const selected = [...state.selection].filter((id) => inspectable(state, id, CLEARANCE_KINDS));
  let first: Picked | null = null;
  let second: Picked | null = null;
  if (state.selection.size === 2 && selected.length === 2) {
    first = { id: selected[0]!, at: undefined };
    second = { id: selected[1]!, at: undefined };
  } else {
    if (state.selection.size === 1 && selected.length === 1) first = { id: selected[0]!, at: undefined };
    const before = [...state.selection];
    if (!first) {
      first = await pick(api, "Select first item for clearance resolution...", CLEARANCE_KINDS);
      if (!first) return;
    }
    // Keep the first item highlighted while selecting the second.
    dispatch({ type: "SET_SELECTION", refs: [first.id], raw: true });
    second = await pick(api, "Select second item for clearance resolution...", CLEARANCE_KINDS);
    dispatch({ type: "SET_SELECTION", refs: before, raw: true });
    if (!second) return;
    if (first.id === second.id) return error(dispatch, "Select two different items for clearance resolution.");
  }
  const reply = await postClearance({ a: first.id, b: second.id, aAt: first.at, bAt: second.at, activeLayer: activeLayerName(state.activeLayer, "F.Cu"), units: state.units });
  if (!reply.ok) return error(dispatch, reply.message ?? "Cannot generate the clearance report.");
  showInspectReport("clearance", reply);
}

/** `InspectConstraints`. */
export async function inspectConstraints(api: StudioApi, dispatch: Dispatch<Action>): Promise<void> {
  const state = api.getState();
  if (!state.board) return;
  let item: Picked | null = null;
  if (state.selection.size === 1) {
    const id = [...state.selection][0]!;
    if (inspectable(state, id, CONSTRAINT_KINDS)) item = { id, at: undefined };
  } else if (state.selection.size === 0) {
    item = await pick(api, "Select item for constraints resolution...", CONSTRAINT_KINDS);
    if (!item) return;
  } else {
    return error(dispatch, "Select a single item for a constraints resolution report.");
  }
  if (!item) return;
  const reply = await postConstraints({ item: item.id, at: item.at, units: state.units });
  if (!reply.ok) return error(dispatch, reply.message ?? "Cannot generate the constraints report.");
  showInspectReport("constraints", reply);
}

/**
 * `InspectDRCError` for the cases that are a Clearance or a Constraints Resolution report (`BOARD_INSPECTION_TOOL::InspectDRCErrorMenuText`): the DRC dialog's
 * Inspect entry on a violation. The items are the violation's own (`GetMainItemID`, `GetAuxItemID`), not the selection's and not picked: a clearance report
 * wants both, a constraints report the first.
 */
export async function inspectViolation(api: StudioApi, dispatch: Dispatch<Action>, kind: "clearance" | "constraints", v: { items: ReadonlyArray<{ id: string | null; pos: [number, number] }> }): Promise<void> {
  const state = api.getState();
  const [a, b] = v.items;
  if (!a?.id) return error(dispatch, kind === "clearance" ? "Cannot generate the clearance report." : "Cannot generate the constraints report.");
  if (kind === "constraints") {
    const reply = await postConstraints({ item: a.id, at: a.pos, units: state.units });
    if (!reply.ok) return error(dispatch, reply.message ?? "Cannot generate the constraints report.");
    showInspectReport("constraints", reply);
    return;
  }
  if (!b?.id) return error(dispatch, "Cannot generate the clearance report.");
  const reply = await postClearance({ a: a.id, b: b.id, aAt: a.pos, bAt: b.pos, activeLayer: activeLayerName(state.activeLayer, "F.Cu"), units: state.units });
  if (!reply.ok) return error(dispatch, reply.message ?? "Cannot generate the clearance report.");
  showInspectReport("clearance", reply);
}

export function registerInspectActions(m: Map<string, (param?: unknown) => void>, ctx: SweepCtx): void {
  if (ctx.state.tab !== "pcb") return;
  const { api, dispatch } = ctx;
  m.set("pcbnew.InspectionTool.InspectClearance", () => void inspectClearance(api, dispatch));
  m.set("pcbnew.InspectionTool.InspectConstraints", () => void inspectConstraints(api, dispatch));
}
