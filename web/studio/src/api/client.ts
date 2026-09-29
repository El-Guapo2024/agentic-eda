// Thin wrapper around the HTTP API `crates/cli/src/studio.rs` exposes.
// Every edit goes through POST /api/cmd, using the same `Cmd` verbs the
// CLI's `eda board <verb>` uses (crates/cli/src/board.rs `step()`), so a
// CLI edit and a UI edit are indistinguishable in activity.jsonl beyond
// the actor name. This module never writes files itself — it only POSTs.

import type { BoardState, Cmd, CmdReply, RouteReply } from "./types";

export class ApiError extends Error {}

async function getJson<T>(url: string): Promise<T> {
  const r = await fetch(url, { cache: "no-store" });
  if (!r.ok) throw new ApiError(`${url}: HTTP ${r.status}`);
  return (await r.json()) as T;
}

/** Changes whenever the board, activity.jsonl, or the routing job does. Poll this; refetch state only when it changes. */
export function fetchVersion(): Promise<string> {
  return getJson<string>("/api/version");
}

export async function fetchState(): Promise<BoardState> {
  const s = await getJson<BoardState & { error?: string }>("/api/state");
  if (s.error) throw new ApiError(s.error);
  return s;
}

/** The schematic view (SVG text), for the "switch to schematic editor" toolbar action. */
export async function fetchSchematicSvg(): Promise<string> {
  const r = await fetch("/api/schematic.svg", { cache: "no-store" });
  const text = await r.text();
  if (!r.ok) throw new ApiError(text || "no schematic for this board");
  return text;
}

/**
 * Apply one board command. `strict` mirrors the CLI's `--strict`: refuse
 * a move that adds gate failures rather than applying it anyway. This is
 * the *only* non-KiCad toggle in the whole app (see the Strict switch in
 * the status bar).
 */
export async function postCmd(cmd: Cmd, strict: boolean): Promise<CmdReply> {
  const r = await fetch("/api/cmd", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ cmd, strict }),
  });
  return (await r.json()) as CmdReply;
}

/** Kick off `eda board route` in the background; poll /api/state's `job` field for progress. */
export async function postRoute(): Promise<RouteReply> {
  const r = await fetch("/api/route", { method: "POST" });
  return (await r.json()) as RouteReply;
}
