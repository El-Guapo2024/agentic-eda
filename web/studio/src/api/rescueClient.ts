// The server's half of Rescue Symbols and Remap Legacy Library Symbols (crates/cli/src/rescue_api.rs): the rescuer's candidates, the rescue library written
// with the command that relinks the symbols, and the remap's report with its command. The command is sent as one `/api/cmd` step by the dialog.
import type { Cmd, LibrarySymbol } from "./types";
import { postJson } from "./schControlClient";
import { ApiError } from "./client";

/** One `RESCUE_SYMBOL_LIB_TABLE_CANDIDATE`: what the symbols name, what they will name, `GetActionDescription`, the two symbols the previews draw, and the instances. */
export interface RescueCandidate {
  requested: string;
  new_id: string;
  action: string;
  cache: LibrarySymbol | null;
  lib: LibrarySymbol | null;
  unit: number;
  body_style: number;
  instances: Array<{ reference: string; value: string }>;
}

export interface RescueCandidates {
  /** `SCH_SCREENS::HasNoFullyDefinedLibIds`: no symbol names a library -- the candidates are the legacy rescuer's (`LEGACY_RESCUER`). */
  legacy: boolean;
  /** The rescue library's nickname, `<project>-rescue`. */
  library: string;
  candidates: RescueCandidate[];
}

export async function fetchRescueCandidates(): Promise<RescueCandidates> {
  const r = await fetch("/api/sch/rescue", { cache: "no-store" });
  if (!r.ok) throw new ApiError(`/api/sch/rescue: HTTP ${r.status}`);
  const j = (await r.json()) as RescueCandidates & { error?: string };
  if (j.error) throw new ApiError(j.error);
  return j;
}

/** The chosen candidates rescued: the library written and put in the project's table; the command that relinks the symbols comes back. */
export async function rescueSymbols(requested: readonly string[]): Promise<{ library: string; cmd?: Cmd }> {
  const j = await postJson<{ ok?: boolean; error?: string; library: string; cmd?: Cmd }>("/api/sch/rescue", { requested });
  if (j.error) throw new ApiError(j.error);
  return j;
}

export interface RemapLine {
  severity: "info" | "warning" | "action";
  text: string;
}

/** `DIALOG_SYMBOL_REMAP::OnRemapSymbols` on the server, with the rescue candidates the Project Rescue Helper kept (`requested`; absent, every one): the report,
 *  and the command that names each symbol's library (none when nothing was mapped). */
export async function remapSymbols(requested?: readonly string[]): Promise<{ report: RemapLine[]; cmd?: Cmd }> {
  const j = await postJson<{ ok?: boolean; error?: string; report: RemapLine[]; cmd?: Cmd }>("/api/sch/remap", requested ? { requested } : {});
  if (j.error) throw new ApiError(j.error);
  return j;
}

/**
 * Whether the board is a KiCad 5 project `eda import-kicad` brought in with no cache library, opened for the first time (`POST /api/sch/rescue/on_open`, the
 * mark spent): `OpenProjectFiles` runs Rescue Symbols, not on demand, as such a project opens (UI-LEFT-3 S3-16).
 */
export async function rescueOnOpen(): Promise<boolean> {
  const j = await postJson<{ ok?: boolean; pending?: boolean }>("/api/sch/rescue/on_open", {});
  return !!j.pending;
}
