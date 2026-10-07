// Delete for every selectable schematic item -- `SCH_EDIT_TOOL::DoDelete` (sch_edit_tool.cpp): the selection with
// every locked item removed first (`FilterSelectionForLockedItems`), then one delete verb per item. One batch, one
// undo step, like the single `SCH_COMMIT::Push`.
import type { Cmd } from "../api/types";
import { withoutLocked } from "./schLock";

/** The id lists of a schematic sheet -- structural, so this module needs no React or store types. */
export interface SchIds {
  symbols: ReadonlyArray<{ id: string }>;
  wires: ReadonlyArray<{ id: string }>;
  labels: ReadonlyArray<{ id: string }>;
  texts: ReadonlyArray<{ id: string }>;
  power_symbols: ReadonlyArray<{ id: string }>;
  no_connects: ReadonlyArray<{ id: string }>;
  bus_entries: ReadonlyArray<{ id: string }>;
  junctions?: ReadonlyArray<{ id: string }>;
  lines?: ReadonlyArray<{ id: string }>;
  graphics?: ReadonlyArray<{ id: string }>;
  sheets: ReadonlyArray<{ id: string }>;
}

/** The delete verbs for `ids` (locked items skipped; ids that are not on the sheet ignored). */
export function deleteCmds(sch: SchIds, ids: readonly string[], locked: ReadonlySet<string>): Cmd[] {
  const has = (list: ReadonlyArray<{ id: string }> | undefined, id: string) => (list ?? []).some((x) => x.id === id);
  const cmds: Cmd[] = [];
  for (const id of withoutLocked(ids, locked)) {
    if (has(sch.symbols, id)) cmds.push({ op: "delete_symbol", id });
    else if (has(sch.wires, id)) cmds.push({ op: "delete_wire", id });
    else if (has(sch.labels, id)) cmds.push({ op: "delete_label", id });
    else if (has(sch.texts, id)) cmds.push({ op: "delete_sch_text", id });
    else if (has(sch.power_symbols, id)) cmds.push({ op: "delete_power_symbol", id });
    else if (has(sch.no_connects, id)) cmds.push({ op: "delete_no_connect", id });
    else if (has(sch.bus_entries, id)) cmds.push({ op: "delete_bus_entry", id });
    else if (has(sch.junctions, id)) cmds.push({ op: "delete_junction", id });
    else if (has(sch.lines, id)) cmds.push({ op: "delete_sch_line", id });
    else if (has(sch.graphics, id)) cmds.push({ op: "sch_edit", verb: "delete_graphic", id });
    else if (has(sch.sheets, id)) cmds.push({ op: "sch_edit", verb: "delete_sheet", id });
  }
  return cmds;
}
