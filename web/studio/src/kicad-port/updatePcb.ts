// Update PCB from Schematic (`common.Control.updatePcbFromSchematic`, F8; `DIALOG_UPDATE_PCB`, `SCH_EDITOR_CONTROL::UpdatePCB` / `PCB_EDIT_FRAME`'s own entry) and
// Update Schematic from PCB (`common.Control.updateSchematicFromPCB`, the back-annotation).
//
// KiCad keeps two documents and this action pushes the schematic's netlist onto the board: new footprints appear (unplaced, stacked at the origin), nets are reassigned,
// footprints with no symbol go. The studio has one design: every schematic edit re-derives the board's parts and nets in the same step (`reconcile_schematic`,
// crates/cli/src/board.rs), and the symbol's Footprint field is followed by the board when it is edited (`replaceFootprint`, crates/cli/src/library_place.rs). What can
// still differ is a symbol that names another footprint than the board has -- a schematic edited outside the studio, a project imported from KiCad -- and that is what F8
// lists (`BOARD_NETLIST_UPDATER`'s dry run: "Change %s footprint from '%s' to '%s'.") and applies, one exchange per footprint in one undo step; the back-annotation
// carries the board's footprint over to the symbol the same way round.

import type { Cmd } from "../api/types";
import type { UpdatePcbChange } from "../api/fpAssign";
import { REPLACE_OPTIONS, type ReportLine } from "./pcbExchange";

export interface UpdatePart {
  ref: string;
  placed: boolean;
}

/** The report line for F8: how the board stands with the schematic once the footprint changes are told. */
export function updatePcbMessage(parts: readonly UpdatePart[]): string {
  const unplaced = parts.filter((p) => !p.placed).map((p) => p.ref);
  const count = (n: number, noun: string) => `${n} ${noun}${n === 1 ? "" : "s"}`;
  const head = "PCB is up to date with the schematic (schematic edits reach the board as you make them).";
  if (parts.length === 0) return `${head} It has no footprints yet.`;
  if (unplaced.length === 0) return `${head} ${count(parts.length, "footprint")}, all placed.`;
  const shown = unplaced.slice(0, 8).join(", ") + (unplaced.length > 8 ? ", ..." : "");
  return `${head} ${count(parts.length, "footprint")}: ${parts.length - unplaced.length} placed, ${unplaced.length} still to place (${shown}) -- pick one in Properties to place it.`;
}

/** The footprint changes F8 can apply: not locked (`!m_overrideLocks`), and a footprint a library has. */
export function appliable(changes: readonly UpdatePcbChange[]): UpdatePcbChange[] {
  return changes.filter((c) => !c.locked && c.found);
}

/** The message window of the dialog, `replaceFootprint`'s texts: the dry run before the update (`done` off) and the report after it. */
export function changeLines(changes: readonly UpdatePcbChange[], done: boolean): ReportLine[] {
  const lines: ReportLine[] = [];
  for (const c of changes) {
    if (c.to === "") {
      lines.push({ severity: "error", text: `Cannot update ${c.reference} (no footprint assigned).` });
    } else if (!c.found) {
      lines.push({ severity: "error", text: `Cannot update ${c.reference} (footprint '${c.to}' not found).` });
    } else if (c.locked) {
      lines.push({ severity: "warning", text: done ? `Could not change ${c.reference} footprint from '${c.from}' to '${c.to}' (footprint is locked).` : `Cannot change ${c.reference} footprint from '${c.from}' to '${c.to}' (footprint is locked).` });
    } else {
      lines.push({ severity: "action", text: done ? `Changed ${c.reference} footprint from '${c.from}' to '${c.to}'.` : `Change ${c.reference} footprint from '${c.from}' to '${c.to}'.` });
    }
  }
  return lines;
}

/** F8's update: the exchange of each footprint the schematic names another one for, with every option on (`ExchangeFootprint( .., true )`), in one undo step. */
export function updateCommands(changes: readonly UpdatePcbChange[]): Cmd[] {
  return appliable(changes).map((c): Cmd => ({ op: "exchange_footprints", references: [c.reference], footprint: c.to, options: REPLACE_OPTIONS }));
}

/** Update Schematic from PCB (`BACK_ANNOTATE`, footprint assignments): the symbol of each footprint the board has another one for names the board's. Locked symbols are not a thing here. */
export function backAnnotateCommands(changes: readonly UpdatePcbChange[]): Cmd[] {
  return changes.filter((c) => c.from !== "").map((c): Cmd => ({ op: "edit_symbol_fields", id: c.reference, footprint: c.from }));
}

/** The lines of the back-annotation's report (`BACK_ANNOTATE::applyChangelist`: "Change %s footprint assignment from '%s' to '%s'."). */
export function backAnnotateLines(changes: readonly UpdatePcbChange[], done: boolean): ReportLine[] {
  return changes.filter((c) => c.from !== "").map((c): ReportLine => ({ severity: "action", text: `${done ? "Changed" : "Change"} ${c.reference} footprint assignment from '${c.to}' to '${c.from}'.` }));
}
