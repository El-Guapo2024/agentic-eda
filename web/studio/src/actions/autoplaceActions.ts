// Place > Auto-Place Footprints (menubar_pcb_editor.cpp: the submenu "Auto-Place Footprints" with "Place Off-Board Footprints" and "Place Selected Footprints";
// pcb_actions.cpp `autoplaceOffboardComponents`, `autoplaceSelectedComponents`, no hotkeys; pcbnew/autorouter/autoplace_tool.cpp at 8303b2ad). KiCad hands the
// footprints to `AR_AUTOPLACER`; the studio's server runs its own placer on them instead (`Cmd::Autoplace`, crates/ops/src/autoplace.rs, UI-LEFT P19), every other
// footprint fixed, judged by the placement gates even with Strict off, one undo step.
//
//   pcbnew.Autoplacer.autoplaceSelected   AUTOPLACE_TOOL::autoplaceSelected: the footprints of the selection (`item->Type() == PCB_FOOTPRINT_T`: a selected pad or field
//                                         is not its footprint); with none, nothing happens
//   pcbnew.Autoplacer.autoplaceOffboard   AUTOPLACE_TOOL::autoplaceOffboard: every footprint whose position is off the board (and the parts not on the board yet)
//
// The positions are whole steps of the grid a footprint is placed on (`GetGridSize( GRID_CONNECTABLE )`, the override when it is on). The studio has no
// "Override locks" setting, so a locked footprint is always left out (KiCad's default). KiCad's commit is empty when there is nothing to place, and nothing is
// said; the server's "nothing to place" refusal is not shown either. Its other refusals are: "Board edges must be defined on the Edge.Cuts layer.", the
// placer's (it could not place the footprints clear of the others on the board), and the gates' (the placement would add a failure, named).
import { postCmd } from "../api/client";
import type { BoardState, Cmd } from "../api/types";
import { itemKind } from "../kicad-port/pcbItems";
import { gridSizeFor } from "../kicad-port/gridOverrides";
import { getGridSettings } from "../state/gridSettings";
import { getGridOverrides } from "../state/gridOverrides";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";

/** The server's answers when there was nothing to place (crates/ops/src/autoplace.rs, `ops_autoplace_nothing`): KiCad says nothing then. */
export const NOTHING_TO_PLACE = ["There are no footprints off the board to place.", "There are no unlocked footprints selected to place."];

/** `for( EDA_ITEM* item : selection() ) if( item->Type() == PCB_FOOTPRINT_T )`: the selected footprints that are on the board, in selection order. */
export function selectedFootprints(board: BoardState, selection: Iterable<string>): string[] {
  const placed = new Set(board.parts.filter((p) => p.placed).map((p) => p.ref));
  return [...selection].filter((id) => itemKind(board, id) === "part" && placed.has(id));
}

export function registerAutoplaceActions(m: Map<string, (param?: unknown) => void>, ctx: SweepCtx): void {
  const { pcbOnly } = createSweepHelpers(ctx);
  m.set(
    "pcbnew.Autoplacer.autoplaceSelected",
    pcbOnly(() => {
      const board = ctx.state.board;
      if (!board) return;
      const footprints = selectedFootprints(board, ctx.state.selection);
      // `autoplace( footprints )` with an empty list: nothing to place, nothing happens
      if (footprints.length > 0) void autoplace(ctx, { footprints });
    })
  );
  m.set("pcbnew.Autoplacer.autoplaceOffboard", pcbOnly(() => void autoplace(ctx, { offboard: true })));
}

/** `AUTOPLACE_TOOL::autoplace`: one command, the reply shown unless there was nothing to place, the board read again. */
async function autoplace(ctx: SweepCtx, which: { footprints?: string[]; offboard?: boolean }): Promise<boolean> {
  const st = ctx.api.getState();
  const grid = gridSizeFor("connectable", st.gridNm, getGridSettings("pcb").grids, getGridOverrides("pcb"));
  const cmd: Cmd = { op: "autoplace", ...which, ...(grid > 0 ? { grid_nm: Math.round(grid) } : {}) };
  const reply = await postCmd(cmd, st.strict).catch((e: unknown) => ({ ok: false, message: e instanceof Error ? e.message : String(e) }));
  if (!reply.ok && !NOTHING_TO_PLACE.includes(reply.message)) ctx.dispatch({ type: "TOAST", message: reply.message, kind: "error" });
  await ctx.api.refresh();
  return reply.ok;
}
