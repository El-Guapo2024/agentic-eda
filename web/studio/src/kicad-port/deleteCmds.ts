// The edit that deletes what the board's interactive delete tool picked. KiCad hands the one item it found to the editor's own
// `DeleteItems` (`EDIT_TOOL::DeleteItems( items, false )`); this is that deletion as `/api/cmd` verbs, in one batch so a click is
// one undo step. (The schematic's is kicad-port/schDelete.ts, `SCH_EDIT_TOOL::DoDelete`.)
//
//   pcbnew/tools/pcb_control.cpp    PCB_CONTROL::InteractiveDelete
//   pcbnew/tools/edit_tool.cpp      EDIT_TOOL::DeleteItems: a locked item is skipped; deleting a group deletes its members
import type { BoardState, Cmd } from "../api/types";
import { expandGroups } from "./groupTree";
import { isLocked } from "./pcbTransform";
import { extraDeleteCmd, extraItems } from "./pcbExtraItems";
import { cellById } from "./tables";
import { editFieldCmd, fieldById } from "./fpFields";

/** The tuning patterns (`PCB_GENERATOR`s) among `ids`: deleting one puts the track back as it was before it was tuned, which the tuner's API does (`/api/tune/remove`), not a verb. */
export function tuningPatternIds(board: BoardState | null | undefined, ids: readonly string[]): string[] {
  const patterns = board?.drawings?.tuning_patterns ?? [];
  return ids.filter((id) => patterns.some((p) => p.id === id));
}

/**
 * The verbs that delete `ids` from the board: tracks, vias, zones, graphics, text and dimensions by their verb, a placed footprint
 * by ripping it up (the board's footprints come from the schematic, so deleting one unplaces it), a group as all its members -- the groups it holds opened.
 * A locked item is skipped (`FilterCollectorForLockedItems`), and so is a group with a locked item anywhere below it or a locked group above it;
 * `skippedLocked` says how many of `ids` were. The backend takes a deleted item out of its groups, and a group left without members goes too.
 */
export function boardDeleteCmds(board: BoardState, ids: readonly string[]): { cmds: Cmd[]; skippedLocked: number; message?: string } {
  const groups = board.drawings?.groups ?? [];
  let skippedLocked = 0;
  const free: string[] = [];
  const patterns = new Set(tuningPatternIds(board, ids));
  for (const id of ids) {
    // a tuning pattern is not deleted with its tracks: it is removed (`PCB_TUNING_PATTERN::Remove`, the track put back) -- see [`tuningPatternIds`]
    if (patterns.has(id)) continue;
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
  const extras = new Map(extraItems(board).map((i) => [i.id, i]));
  const tables = new Set((board.drawings?.tables ?? []).map((t) => t.id));
  const cmds: Cmd[] = [];
  // `EDIT_TOOL::DeleteItems`: a table is removed, but a table cell only has its text cleared ("Clear contents of table cell"); a locked table keeps both.
  const cleared = new Map<string, [number, number][]>();
  for (const id of wanted) {
    const cell = tables.has(id) ? undefined : cellById(board, id);
    if (!cell) continue;
    if (wanted.has(cell.table.id)) continue;
    if (isLocked(board, cell.table.id)) {
      skippedLocked++;
      continue;
    }
    cleared.set(cell.table.id, [...(cleared.get(cell.table.id) ?? []), [cell.cell.row, cell.cell.col]]);
  }
  for (const [tid, cells] of cleared) cmds.push({ op: "table", verb: "edit_cells", id: tid, cells, edit: { text: "" } });
  // `case PCB_FIELD_T:` a field is not deleted: a shown one is hidden ("Hide Field" / "Hide Fields"); for one already hidden, when nothing else was
  // deleted, "Use the Footprint Properties dialog to remove fields." (A field of a footprint that is itself deleted goes with it.)
  let alreadyHidden = 0;
  let deleted = 0;
  for (const id of wanted) {
    const f = fieldById(board, id);
    if (!f || wanted.has(f.part.ref)) continue;
    if (f.field.visible) cmds.push(editFieldCmd(f.part, f.field, { visible: false }));
    else alreadyHidden++;
  }
  for (const id of wanted) {
    const known = tables.has(id) || tracks.has(id) || vias.has(id) || zones.has(id) || shapes.has(id) || texts.has(id) || dimensions.has(id) || extras.has(id) || placed.has(id);
    if (!known) continue;
    if (tables.has(id)) cmds.push({ op: "table", verb: "delete", id });
    else if (tracks.has(id)) cmds.push({ op: "delete_track", id });
    else if (vias.has(id)) cmds.push({ op: "delete_via", id });
    else if (zones.has(id)) cmds.push({ op: "delete_zone", id });
    else if (shapes.has(id)) cmds.push({ op: "delete_shape", id });
    else if (texts.has(id)) cmds.push({ op: "delete_text", id });
    else if (dimensions.has(id)) cmds.push({ op: "delete_dimension", id });
    else if (extras.has(id)) cmds.push(extraDeleteCmd(extras.get(id)!));
    else cmds.push({ op: "rip", part: id });
    deleted++;
  }
  const message = deleted === 0 && cleared.size === 0 && cmds.length === 0 && alreadyHidden > 0 ? "Use the Footprint Properties dialog to remove fields." : undefined;
  return { cmds, skippedLocked, ...(message ? { message } : {}) };
}
