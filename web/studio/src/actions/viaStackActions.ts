// The board editor's actions for KiCad 10's microvia stack (KiCad 25ec2335; UI-LEFT-3 X3-1). Called from `registerPcbItemActions`.
//
//   pcbnew.InteractiveDrawing.viaStack   DRAWING_TOOL::PlaceMicroviaStack ("Place Microvia Stack"): the selection cleared, the toolbar's preset placed (the Microvia Stack
//                                        dialog first when the board has none), the span checked, then the loop on the canvas (components/canvas/viaStackPlace.ts)
//   pcbnew.Generator.makeViaStack        GENERATOR_TOOL::MakeViaStackFromSelection ("Create Stack from Selection"): the selected vias and traces made one stack, which is
//                                        then selected; refused with the info bar's warning
import type { Dispatch } from "react";
import type { Action } from "../state/store";
import type { BoardState } from "../api/types";
import type { BoardViaStack } from "../api/pcbItemTypes";
import { copperOf, isSpanValid, makeStackIds, MAKE_STACK_REFUSED, newViaStack, placementPitch, placementPreset, STACK_LAYERS_MISSING, withPreset } from "../kicad-port/viaStack";
import { currentViaSize } from "../kicad-port/routerVia";
import { getViaStackTool, openViaStackDialog, setViaStackPlacement } from "../state/viaStackTool";
import { newPlacement } from "../components/canvas/viaStackPlace";
import { fetchState } from "../api/client";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";

/**
 * The loop of Place Microvia Stack on `settings` (what the dialog or the preset gave): refused with the info bar's error when the board has not the span, a staggered stack
 * without a pitch given twice its via, then the tool in force.
 */
export function startViaStackPlacement(settings: BoardViaStack, board: BoardState, currentVia: number, dispatch: Dispatch<Action>, seed?: Parameters<typeof newPlacement>[2]): void {
  const copper = copperOf(board);
  if (!isSpanValid(copper, settings.start_layer, settings.end_layer)) {
    dispatch({ type: "SHOW_PCB_INFOBAR", message: STACK_LAYERS_MISSING, kind: "error", ms: 8000, close: true });
    setViaStackPlacement(null);
    return;
  }
  const s = { ...settings, pitch_nm: placementPitch(settings, currentVia) };
  setViaStackPlacement(newPlacement(s, copper, seed));
  dispatch({ type: "SET_ACTIVE_TOOL", tool: "via_stack" });
}

export function registerViaStackActions(m: Map<string, (param?: unknown) => void>, ctx: SweepCtx): void {
  const { pcbOnly } = createSweepHelpers(ctx);
  const { api, dispatch } = ctx;
  m.set(
    "pcbnew.InteractiveDrawing.viaStack",
    pcbOnly(() => {
      const st = api.getState();
      const board = st.board;
      if (!board) return;
      // `m_toolMgr->RunAction( ACTIONS::selectionClear )`
      dispatch({ type: "SET_SELECTION", refs: [] });
      const via = currentViaSize(board, st.currentViaIndex, st.pcbx.useCustomTrackVia, st.pcbx.customTrackVia);
      const preset = placementPreset(board.drawings?.via_stack_presets, getViaStackTool().index);
      const at = st.cursorNm ?? { x: 0, y: 0 };
      // "Only fall back to the properties dialog when no presets are defined yet, so there is still a way to place and define one."
      if (!preset) return openViaStackDialog(null, newViaStack(at));
      startViaStackPlacement(withPreset(newViaStack(at), preset), board, via.diameter, dispatch);
    })
  );
  m.set(
    "pcbnew.Generator.makeViaStack",
    pcbOnly(async () => {
      const board = api.getState().board;
      if (!board) return;
      const ids = makeStackIds(board, ctx.requestSelection());
      const before = new Set((board.drawings?.via_stacks ?? []).map((s) => s.id));
      if (ids.length < 2) return void dispatch({ type: "SHOW_PCB_INFOBAR", message: MAKE_STACK_REFUSED, kind: "warning", ms: 8000, close: true });
      dispatch({ type: "SET_SELECTION", refs: [] });
      if (!(await api.cmdBatch([{ op: "pcb_item", verb: "make_via_stack", ids }]))) return;
      // `selTool->AddItemToSel( stack )`
      const fresh = await fetchState().catch(() => null);
      const made = (fresh?.drawings?.via_stacks ?? []).find((s) => !before.has(s.id));
      if (made) dispatch({ type: "SET_SELECTION", refs: [made.id] });
    })
  );
}
