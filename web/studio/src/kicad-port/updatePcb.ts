// Update PCB from Schematic (`common.Control.updatePcbFromSchematic`, F8; `DIALOG_UPDATE_PCB`,
// `SCH_EDITOR_CONTROL::UpdatePCB` / `PCB_EDIT_FRAME`'s own entry).
//
// KiCad keeps two documents and this action pushes the schematic's netlist onto the board: new footprints
// appear (unplaced, stacked at the origin), nets are reassigned, footprints with no symbol go. The studio has
// one design: every schematic edit re-derives the board's parts and nets in the same step
// (`reconcile_schematic`, crates/cli/src/board.rs), so the board is never out of date and there is no separate
// update step to run. F8 therefore runs the dialog's report -- what an update would leave you with -- over the
// live design: how many footprints the board holds and which of them still wait to be placed (the ones an
// update would have just added at the origin).

export interface UpdatePart {
  ref: string;
  placed: boolean;
}

/** The report line for F8. */
export function updatePcbMessage(parts: readonly UpdatePart[]): string {
  const unplaced = parts.filter((p) => !p.placed).map((p) => p.ref);
  const count = (n: number, noun: string) => `${n} ${noun}${n === 1 ? "" : "s"}`;
  const head = "PCB is up to date with the schematic (schematic edits reach the board as you make them).";
  if (parts.length === 0) return `${head} It has no footprints yet.`;
  if (unplaced.length === 0) return `${head} ${count(parts.length, "footprint")}, all placed.`;
  const shown = unplaced.slice(0, 8).join(", ") + (unplaced.length > 8 ? ", ..." : "");
  return `${head} ${count(parts.length, "footprint")}: ${parts.length - unplaced.length} placed, ${unplaced.length} still to place (${shown}) -- pick one in Properties to place it.`;
}
