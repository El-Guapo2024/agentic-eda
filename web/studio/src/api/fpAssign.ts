// The HTTP side of the footprint assignment tools (crates/cli/src/fp_assign_api.rs): what Update PCB from Schematic lists, and, as the tools land, the exchange, update,
// compare and reannotate dialogs' reports.

import type { Shape } from "./types";

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

/** One footprint of `POST /api/exchange_plan`: where it stands before an exchange. */
export interface ExchangePlanEntry {
  reference: string;
  /** The footprint it is on now. */
  from: string;
  /** The footprint it is to be put on (an update: the same). */
  to: string;
  /** The footprint is found: in a library of the tables, in the Footprint Editor's, or already known to the board. */
  found: boolean;
  /** An update: the board's footprint is not the library's copy (`FootprintNeedsUpdate`); a change is always a difference. */
  differs?: boolean;
  /** An update: how it differs. */
  lines?: string[];
  error?: string;
}

/**
 * `POST /api/exchange_plan`: for each of `references` (what the dialog's choice names), whether the footprint is found -- `footprint` is the one they are changed to, or
 * `null` for an update from each one's own library copy -- and, for an update, whether the board's differs from the library's.
 */
export async function postExchangePlan(references: readonly string[], footprint: string | null): Promise<ExchangePlanEntry[]> {
  const r = await fetch("/api/exchange_plan", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ references, footprint }) });
  const j = (await r.json()) as { ok?: boolean; entries?: ExchangePlanEntry[]; message?: string };
  if (!j.ok) throw new Error(j.message ?? "The exchange could not be planned.");
  return j.entries ?? [];
}

/** A footprint drawn for the Compare dialog, in the footprint's own frame, micrometres. */
export interface FootprintDrawing {
  pads: { num: string; x: number; y: number; w: number; h: number; shape: string; rot: number; ratio: number | null; drill: number | null; slot: [number, number] | null; th: boolean }[];
  /** The graphics, as board shapes. */
  shapes: Shape[];
  courtyard: [number, number][][];
}

/** `GET /api/footprint_diff`: `DiffFootprint`'s summary and both drawings, or why the library copy is not there. */
export interface FootprintDiff {
  ok: boolean;
  message?: string;
  reference: string;
  library: string;
  item: string;
  found: boolean;
  reason?: string;
  lines?: string[];
  board?: FootprintDrawing;
  library_copy?: FootprintDrawing;
}

export async function fetchFootprintDiff(reference: string): Promise<FootprintDiff> {
  const r = await fetch(`/api/footprint_diff?ref=${encodeURIComponent(reference)}`);
  return (await r.json()) as FootprintDiff;
}
