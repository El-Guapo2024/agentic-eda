// The HTTP side of the footprint assignment tools (crates/cli/src/fp_assign_api.rs): what Update PCB from Schematic lists, and, as the tools land, the exchange, update,
// compare and reannotate dialogs' reports.

/** One footprint whose symbol names another footprint than the board has (`GET /api/update_pcb`). */
export interface UpdatePcbChange {
  reference: string;
  /** The footprint the board has. */
  from: string;
  /** The footprint the symbol's Footprint field names. */
  to: string;
  /** `FOOTPRINT::IsLocked`: Update PCB from Schematic does not change a locked footprint. */
  locked: boolean;
  /** Some library (the tables', the Footprint Editor's or the board's own) has `to`. */
  found: boolean;
}

export interface UpdatePcbReport {
  ok: boolean;
  message?: string;
  changes: UpdatePcbChange[];
}

/** `GET /api/update_pcb`: the dry run of Update PCB from Schematic's footprint changes. */
export async function fetchUpdatePcb(): Promise<UpdatePcbReport> {
  const r = await fetch("/api/update_pcb");
  const j = (await r.json()) as Partial<UpdatePcbReport>;
  return { ok: j.ok ?? false, message: j.message, changes: j.changes ?? [] };
}
