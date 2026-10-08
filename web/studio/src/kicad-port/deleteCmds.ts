// The edits that delete what the shared tools picked. KiCad's interactive delete tool hands the one item it found to the
// editor's own `DeleteItems` (`EDIT_TOOL::DeleteItems( items, false )` in pcbnew, the `doDelete` action in eeschema); these
// are those deletions as `/api/cmd` verbs, in one batch so a click is one undo step.
//
//   pcbnew/tools/pcb_control.cpp    PCB_CONTROL::InteractiveDelete
//   eeschema/tools/sch_tool_base.h  SCH_TOOL_BASE::InteractiveDelete
//   pcbnew/tools/edit_tool.cpp      EDIT_TOOL::DeleteItems: a locked item is skipped; deleting a group deletes its members
import type { BoardState, Cmd, Schematic } from "../api/types";

/**
 * The verbs that delete `ids` from the board: tracks, vias, zones, graphics, text and dimensions by their verb, a placed footprint
 * by ripping it up (the board's footprints come from the schematic, so deleting one unplaces it), a group as all its members.
 * Locked items are skipped (`FilterCollectorForLockedItems`); `skippedLocked` says how many were.
 */
export function boardDeleteCmds(board: BoardState, ids: readonly string[]): { cmds: Cmd[]; skippedLocked: number } {
  const locked = new Set(board.locked ?? []);
  const groups = new Map((board.drawings?.groups ?? []).map((g) => [g.id, g.member_ids]));
  const wanted = new Set<string>();
  for (const id of ids) {
    const members = groups.get(id);
    if (members) for (const m of members) wanted.add(m);
    else wanted.add(id);
  }
  const tracks = new Set((board.routing?.tracks ?? []).map((t) => t.id));
  const vias = new Set((board.routing?.vias ?? []).map((v) => v.id));
  const zones = new Set((board.routing?.zones ?? []).map((z) => z.id));
  const shapes = new Set((board.drawings?.shapes ?? []).map((s) => s.id));
  const texts = new Set((board.drawings?.texts ?? []).map((t) => t.id));
  const dimensions = new Set((board.drawings?.dimensions ?? []).map((d) => d.id));
  const placed = new Set(board.parts.filter((p) => p.placed).map((p) => p.ref));
  const cmds: Cmd[] = [];
  let skippedLocked = 0;
  for (const id of wanted) {
    const known = tracks.has(id) || vias.has(id) || zones.has(id) || shapes.has(id) || texts.has(id) || dimensions.has(id) || placed.has(id);
    if (!known) continue;
    if (locked.has(id)) {
      skippedLocked++;
      continue;
    }
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

/** The verbs that delete `ids` from the schematic sheet: symbols, wires and buses, junctions, graphic lines, labels, text, power symbols, no-connects and bus entries. */
export function schematicDeleteCmds(sch: Schematic, ids: readonly string[]): Cmd[] {
  const wanted = new Set(ids);
  const cmds: Cmd[] = [];
  for (const s of sch.symbols) if (wanted.has(s.id)) cmds.push({ op: "delete_symbol", id: s.id });
  for (const w of sch.wires) if (w.id && wanted.has(w.id)) cmds.push({ op: "delete_wire", id: w.id });
  for (const j of sch.junctions ?? []) if (j.id && wanted.has(j.id)) cmds.push({ op: "delete_junction", id: j.id });
  for (const l of sch.lines ?? []) if (l.id && wanted.has(l.id)) cmds.push({ op: "delete_sch_line", id: l.id });
  for (const l of sch.labels) if (wanted.has(l.id)) cmds.push({ op: "delete_label", id: l.id });
  for (const t of sch.texts) if (wanted.has(t.id)) cmds.push({ op: "delete_sch_text", id: t.id });
  for (const p of sch.power_symbols) if (wanted.has(p.id)) cmds.push({ op: "delete_power_symbol", id: p.id });
  for (const n of sch.no_connects) if (wanted.has(n.id)) cmds.push({ op: "delete_no_connect", id: n.id });
  for (const e of sch.bus_entries ?? []) if (e.id && wanted.has(e.id)) cmds.push({ op: "delete_bus_entry", id: e.id });
  return cmds;
}
