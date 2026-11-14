// The Simulation Model dialog's reads (`crates/cli/src/sim_model_api.rs`): the symbol's simulation fields and pins, a SPICE library's models, and the
// library files of the board folder. Kept apart from `client.ts` so the dialog's additions never grow that shared file.
import type { LibraryModelInfo, SymbolPin } from "../kicad-port/sim/simModel";
import simModelsUrl from "../kicad/sim_models.json?url";
import { decodeTables, type SimModelTables, type SimModelsJson } from "../kicad-port/sim/simModelBuiltin";

export interface SimModelSymbolReply {
  ok: boolean;
  message?: string;
  ref: string;
  value: string;
  imported: boolean;
  /** The fields the dialog starts from: the instance's over the library symbol's simulation fields. */
  fields: Record<string, string>;
  /** The fields that are the instance's own. */
  own: string[];
  pins: SymbolPin[];
  /** Each text that holds a variable, as the symbol shows it (`SCH_FIELD::GetShownText`): the fields', the Value's, "${SIM.PARAMS}" and the
   * texts the request named. */
  shown?: Record<string, string>;
}

export interface SimLibraryReply {
  ok: boolean;
  /** The path as `Sim.Library` stores it (relative to the board folder when it is inside it). */
  path: string;
  models: LibraryModelInfo[];
  /** The info bar's lines (KiCad's words). */
  messages: string[];
  /** An IBIS file: its models are components. */
  ibis?: boolean;
}

async function post<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  return (await r.json()) as T;
}

/** POST /api/sim/model/symbol; `texts` are more texts to resolve as the symbol's (Symbol Properties' rows, not yet applied). */
export function fetchSimModelSymbol(ref: string, texts: string[] = []): Promise<SimModelSymbolReply> {
  return post("/api/sim/model/symbol", { ref, texts });
}

/** POST /api/sim/model/library. */
export function fetchSimLibrary(path: string): Promise<SimLibraryReply> {
  return post("/api/sim/model/library", { path });
}

/** GET /api/sim/model/inductors: the K device's l1 / l2 choices, the circuit's inductors read from the design (the schematic in memory, UI-LEFT-3 M3-1). */
export async function fetchSimInductors(): Promise<string[]> {
  const r = await fetch("/api/sim/model/inductors", { cache: "no-store" });
  if (!r.ok) throw new Error(`/api/sim/model/inductors: HTTP ${r.status}`);
  const j = (await r.json()) as { ok: boolean; inductors?: string[]; message?: string };
  if (!j.ok) throw new Error(j.message ?? "no inductors");
  return j.inductors ?? [];
}

/** GET /api/sim/model/files. */
export async function fetchSimLibraryFiles(): Promise<string[]> {
  const r = await fetch("/api/sim/model/files", { cache: "no-store" });
  const j = (await r.json()) as { files?: string[] };
  return j.files ?? [];
}

let tablesPromise: Promise<SimModelTables> | null = null;
/** The simulation model tables (src/kicad/sim_models.json, 0.8 MB, copied from the C++ by tools/extract-sim-models.js): fetched once, the first
 *  time the Simulation Model dialog or the tuner's Save needs them. */
export function loadSimModelTables(): Promise<SimModelTables> {
  tablesPromise ??= fetch(simModelsUrl)
    .then((r) => r.json() as Promise<SimModelsJson>)
    .then(decodeTables)
    .catch((e: unknown) => {
      tablesPromise = null;
      throw e;
    });
  return tablesPromise;
}

