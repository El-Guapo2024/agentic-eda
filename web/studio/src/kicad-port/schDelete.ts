// Delete for every selectable schematic item -- `SCH_EDIT_TOOL::DoDelete` (sch_edit_tool.cpp): the selection with
// every locked item removed first (`FilterSelectionForLockedItems`), then one delete verb per item. One batch, one
// undo step, like the single `SCH_COMMIT::Push`.
import type { Cmd } from "../api/types";
import { withoutLocked } from "./schLock";
import { parseFieldId } from "./schFieldEdit";
import { groupAndDescendants, groupLeaves, isGroup, ancestors } from "./groupTree";

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
  /** The tables, with the address of each cell (a cell id is `TABLE:ROW:COL`). */
  tables?: ReadonlyArray<{ id: string; cells: ReadonlyArray<{ id: string; row: number; col: number }> }>;
  sheets: ReadonlyArray<{ id: string; pins?: ReadonlyArray<{ id: string }> }>;
  /** The groups of the sheet: a deleted group takes everything below it with it. */
  groups?: ReadonlyArray<{ id: string; member_ids: readonly string[] }>;
}

/** The delete verbs for `ids` (locked items skipped; ids that are not on the sheet ignored). */
export function deleteCmds(sch: SchIds, ids: readonly string[], locked: ReadonlySet<string>): Cmd[] {
  const has = (list: ReadonlyArray<{ id: string }> | undefined, id: string) => (list ?? []).some((x) => x.id === id);
  const cmds: Cmd[] = [];
  const kept = withoutLocked(openGroups(sch.groups ?? [], ids, locked), locked);
  // the reference an owner key names (`R1#2` is a unit of `R1`)
  const refOf = (owner: string) => /^(.*)#(\d+)$/.exec(owner)?.[1] ?? owner;
  // `DoDelete`: a table is removed, but a table cell only has its text cleared; a locked table keeps both (a cell is as locked as its table), and a cell goes with its table when that is deleted too.
  const tables = sch.tables ?? [];
  const wanted = new Set(kept);
  const cleared = new Map<string, [number, number][]>();
  for (const id of kept) {
    if (tables.some((t) => t.id === id)) continue;
    const owner = tables.find((t) => t.cells.some((c) => c.id === id));
    const cell = owner?.cells.find((c) => c.id === id);
    if (!owner || !cell || wanted.has(owner.id) || locked.has(owner.id)) continue;
    cleared.set(owner.id, [...(cleared.get(owner.id) ?? []), [cell.row, cell.col]]);
  }
  for (const [tid, cells] of cleared) cmds.push({ op: "sch_table", verb: "edit_cells", id: tid, cells, edit: { text: "" } });
  for (const id of kept) {
    // `DoDelete` on a field hides it (`SetVisible( false )`) -- unless the item that has it goes too
    const field = parseFieldId(id);
    if (field) {
      const ownerGoes = kept.some((other) => other === refOf(field.owner) || other === field.owner);
      if (!ownerGoes && !locked.has(refOf(field.owner))) cmds.push({ op: "sch_edit", verb: "edit_field", id, visible: false });
      continue;
    }
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
    else if (has(tables, id)) cmds.push({ op: "sch_table", verb: "delete", id });
    else if (has(sch.sheets, id)) cmds.push({ op: "sch_edit", verb: "delete_sheet", id });
    else {
      // a pin of a sheet (`DoDelete` on `SCH_SHEET_PIN_T`: `sheet->RemovePin`): it goes with its sheet when that is deleted too, and a locked sheet keeps its pins
      const sheet = sch.sheets.find((s) => (s.pins ?? []).some((p) => p.id === id));
      if (sheet && !kept.includes(sheet.id) && !locked.has(sheet.id)) cmds.push({ op: "sch_edit", verb: "delete_sheet_pin", id });
    }
  }
  return cmds;
}

/**
 * `DoDelete`: "Groups need to delete their children" -- a group in `ids` is replaced by the items below it, nested groups opened. `FilterSelectionForLockedItems` first: a group
 * with a locked item anywhere below it, or itself locked or inside a locked group, is no selection and nothing of it goes.
 */
export function openGroups(groups: ReadonlyArray<{ id: string; member_ids: readonly string[] }>, ids: readonly string[], locked: ReadonlySet<string>): string[] {
  if (groups.length === 0) return [...ids];
  const lockedThrough = (id: string) => locked.has(id) || ancestors(groups, id).some((g) => locked.has(g.id));
  const out: string[] = [];
  for (const id of ids) {
    if (!isGroup(groups, id)) {
      if (!out.includes(id)) out.push(id);
      continue;
    }
    if (lockedThrough(id) || groupAndDescendants(groups, id).some((g) => locked.has(g)) || groupLeaves(groups, id).some(lockedThrough)) continue;
    for (const leaf of groupLeaves(groups, id)) if (!out.includes(leaf)) out.push(leaf);
  }
  return out;
}
