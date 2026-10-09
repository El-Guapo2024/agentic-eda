// The edit that deletes what the board's interactive delete tool picked. KiCad hands the one item it found to the editor's own
// `DeleteItems` (`EDIT_TOOL::DeleteItems( items, false )`); this is that deletion as `/api/cmd` verbs, in one batch so a click is
// one undo step. (The schematic's is kicad-port/schDelete.ts, `SCH_EDIT_TOOL::DoDelete`.)
//
//   pcbnew/tools/pcb_control.cpp    PCB_CONTROL::InteractiveDelete
//   pcbnew/tools/edit_tool.cpp      EDIT_TOOL::DeleteItems: a locked item is skipped; deleting a group deletes its members
import type { BoardState, Cmd } from "../api/types";
import { expandGroups } from "./groupTree";
import { isLocked } from "./pcbTransform";

/**
 * The verbs that delete `ids` from the board: tracks, vias, zones, graphics, text and dimensions by their verb, a placed footprint
 * by ripping it up (the board's footprints come from the schematic, so deleting one unplaces it), a group as all its members -- the groups it holds opened.
 * A locked item is skipped (`FilterCollectorForLockedItems`), and so is a group with a locked item anywhere below it or a locked group above it;
 * `skippedLocked` says how many of `ids` were. The backend takes a deleted item out of its groups, and a group left without members goes too.
 */
export function boardDeleteCmds(board: BoardState, ids: readonly string[]): { cmds: Cmd[]; skippedLocked: number } {
  const groups = board.drawings?.groups ?? [];
  let skippedLocked = 0;
  const free: string[] = [];
  for (const id of ids) {
    if (isLocked(board, id)) skippedLocked++;
    else free.push(id);
  }
  const wanted = new Set<string>(expandGroups(groups, free));
  const tracks = new Set((board.routing?.tracks ?? []).map((t) => t.id));
  const vias = new Set((board.routing?.vias ?? []).map((v) => v.id));
  const zones = new Set((board.routing?.zones ?? []).map((z) => z.id));
  const shapes = new Set((board.drawings?.shapes ?? []).map((s) => s.id));
  const texts = new Set((board.drawings?.texts ?? []).map((t) => t.id));
  const dimensions = new Set((board.drawings?.dimensions ?? []).map((d) => d.id));
  const placed = new Set(board.parts.filter((p) => p.placed).map((p) => p.ref));
  const cmds: Cmd[] = [];
  for (const id of wanted) {
    const known = tracks.has(id) || vias.has(id) || zones.has(id) || shapes.has(id) || texts.has(id) || dimensions.has(id) || placed.has(id);
    if (!known) continue;
    if (tracks.has(id)) cmds.push({ op: "delete_track", id });
    else if (vias.has(id)) cmds.push({ op: "delete_via", id });
    else if (zones.has(id)) cmds.push({ op: "delete_zone", id });
    else if (shapes.has(id)) cmds.push({ op: "delete_shape", id });
    else if (texts.has(id)) cmds.push({ op: "delete_text", id });
    else if (dimensions.has(id)) cmds.push({ op: "delete_dimension", id });
    else cmds.push({ op: "rip", part: id });
  }
  return { cmds, skippedLocked };
}
