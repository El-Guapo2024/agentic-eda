// Thin wrapper around the HTTP API `crates/cli/src/studio.rs` exposes.
// Every edit goes through POST /api/cmd, using the same `Cmd` verbs the
// CLI's `eda board <verb>` uses (crates/cli/src/board.rs `step()`), so a
// CLI edit and a UI edit are indistinguishable in activity.jsonl beyond
// the actor name. This module never writes files itself — it only POSTs.

import type {
  BoardGlbResult,
  BoardState,
  Cmd,
  CmdReply,
  DragPreview,
  DrcReport,
  ErcReport,
  FillReport,
  FootprintLibraryNames,
  LibraryFootprint,
  Ratsnest,
  RouteFixReply,
  RouteMode,
  RoutePreview,
  RouteReply,
  Schematic,
  SchematicSymbol,
  Um,
} from "./types";

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

/**
 * The schematic as structured data (symbols/pins/wires/labels), for the
 * Schematic Editor's own KiCad-style renderer. /api/schematic.svg (a
 * single baked image) still exists on the backend but nothing in this
 * app fetches it anymore.
 */
export async function fetchSchematic(): Promise<Schematic> {
  const s = await getJson<Schematic & { error?: string }>("/api/schematic");
  if (s.error) throw new ApiError(s.error);
  // Defensive defaults for the lib_symbols/power_symbols/no_connects
  // fields, and per-symbol/per-label fields, the Eeschema-port merge
  // adds (see types.ts's Schematic doc comment): a backend built before
  // that merge lands, or simply an empty schematic, may send any of
  // these absent or null rather than the real value -- painter.ts and
  // layout.ts should never need an `?? {}`/`?? []`/`?? 1` of their own
  // for this. `title_block` is genuinely optional (nullable in the type
  // too) so it's passed through as-is; `lib_id: null` on a symbol is
  // also a real, meaningful value (this app's own "no real graphics
  // resolved yet" signal), not defaulted away.
  return {
    ...s,
    lib_symbols: s.lib_symbols ?? {},
    power_symbols: s.power_symbols ?? [],
    no_connects: s.no_connects ?? [],
    title_block: s.title_block ?? null,
    symbols: (s.symbols ?? []).map((sym) => {
      // `mirror` replaces an earlier `mirrored: boolean` (see types.ts's
      // SchematicSymbol doc comment) that could only ever express one of
      // KiCad's two mirror axes -- this app's own prior rendering always
      // treated that boolean as what KiCad calls "mirror y" (horizontal
      // flip), so a backend that still sends the old shape is read the
      // same way rather than silently losing its mirroring altogether.
      const legacy = sym as SchematicSymbol & { mirrored?: boolean };
      return { ...sym, lib_id: sym.lib_id ?? null, unit: sym.unit ?? 1, body_style: sym.body_style ?? 1, mirror: sym.mirror ?? (legacy.mirrored ? "y" : null) };
    }),
    labels: (s.labels ?? []).map((l) => ({ ...l, scope: l.scope ?? "local", shape: l.shape ?? null })),
  };
}

/**
 * KiCad's own ratsnest (crates/connectivity: Delaunay + Kruskal MST
 * between connectivity clusters, ground-truthed against kicad-cli's own
 * unconnected-item list) -- replaces this app's earlier client-side
 * per-net MST-over-pad-centers approximation (components/canvas/
 * ratsnest.ts, removed) now that the backend computes the real thing.
 */
export async function fetchRatsnest(): Promise<Ratsnest> {
  const r = await getJson<Ratsnest & { error?: string }>("/api/ratsnest");
  if (r.error) throw new ApiError(r.error);
  return r;
}

/** The ported KiCad DRC engine (crates/drc), run fresh server-side on every call -- no caching, matching studio.rs's own doc comment on why (cheap enough on these board sizes). */
export async function fetchDrc(): Promise<DrcReport> {
  const r = await getJson<DrcReport & { error?: string }>("/api/drc");
  if (r.error) throw new ApiError(r.error);
  return r;
}

/** `eda_kicad::check_erc` (gap #4), run fresh server-side on every call -- same no-caching reasoning as `fetchDrc`. */
export async function fetchErc(): Promise<ErcReport> {
  const r = await getJson<ErcReport & { error?: string }>("/api/erc");
  if (r.error) throw new ApiError(r.error);
  return r;
}

/** `crates/zone-filler`'s real KiCad fill algorithm, run fresh server-side on every call (B/Ctrl+B -- see state/store.tsx's `zoneFill`). */
export async function fetchFill(): Promise<FillReport> {
  const r = await getJson<FillReport & { error?: string }>("/api/fill");
  if (r.error) throw new ApiError(r.error);
  return r;
}

/**
 * GET /api/board.glb once. Never throws for the "still building" or
 * "kicad-cli failed" cases -- those are ordinary, well-formed answers
 * (see BoardGlbResult's doc comment in ./types) -- only for a genuine
 * transport/HTTP-level problem. Callers that want the export to finish
 * poll this themselves (see Viewer3D.tsx); this function makes exactly
 * one request.
 */
export async function fetchBoardGlb(): Promise<BoardGlbResult> {
  const r = await fetch("/api/board.glb", { cache: "no-store" });
  const contentType = r.headers.get("content-type") ?? "";
  if (contentType.includes("application/json")) {
    const j = (await r.json()) as { status: "pending" } | { status: "failed"; error: string };
    return j.status === "pending" ? { status: "pending" } : { status: "failed", error: j.error };
  }
  if (!r.ok) throw new ApiError(`/api/board.glb: HTTP ${r.status}`);
  return { status: "ready", bytes: await r.arrayBuffer() };
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

/**
 * The backend's own undo/redo (crates/cli/src/board.rs: two snapshot
 * stacks under the board's directory, see that file's comments) -- the
 * one piece of backend logic this task allowed beyond serving the app.
 * `ok: false` just means the stack is empty ("nothing to undo/redo"),
 * not a failure worth alarming over.
 *
 * `domain` scopes which tab's last edit this reverts/replays -- store.tsx's
 * `api.undo`/`redo` always pass the current tab, so Ctrl+Z on the
 * Schematic tab can no longer silently undo a PCB edit (GAPS.md #15), and
 * the Footprint Editor tab (GAPS.md #8) gets the same independent scope
 * ("footprint_editor") -- see `board::undo`'s own doc for the full
 * mechanism.
 */
export async function postUndo(domain: "pcb" | "schematic" | "footprint_editor"): Promise<CmdReply> {
  const r = await fetch("/api/undo", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ domain }) });
  return (await r.json()) as CmdReply;
}

export async function postRedo(domain: "pcb" | "schematic" | "footprint_editor"): Promise<CmdReply> {
  const r = await fetch("/api/redo", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ domain }) });
  return (await r.json()) as CmdReply;
}

// ---------------------------------------------------------- footprint editor
//
// GAPS.md #8. `fetchFootprint` polls the one footprint currently open the
// same way `fetchState`/`fetchSchematic` poll their own tab's document;
// `fetchFootprintLibraryNames` is the "Open from Library" picker's list.

export async function fetchFootprint(name: string): Promise<LibraryFootprint> {
  const f = await getJson<LibraryFootprint & { error?: string }>(`/api/footprint?name=${name}`);
  if (f.error) throw new ApiError(f.error);
  return f;
}

export async function fetchFootprintLibraryNames(): Promise<FootprintLibraryNames> {
  const r = await getJson<FootprintLibraryNames & { error?: string }>("/api/footprint_library");
  if (r.error) throw new ApiError(r.error);
  return r;
}

// ------------------------------------------------------- interactive router
//
// Gap #7's push-and-shove router (crates/pns), driven through
// crates/cli/src/route_api.rs. See api/types.ts's own doc comment on the
// session these calls share -- `routeStart` opens it, `routeFinish`/
// `routeCancel` close it, everything between just reads/advances it.

async function postJson<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  return (await r.json()) as T;
}

export function routeStart(x: Um, y: Um, layer: string, width: Um, mode: RouteMode): Promise<RoutePreview> {
  return postJson("/api/route/start", { x, y, layer, width, mode });
}

/** `flipPosture`/`width` fold in the `/` and `W` hotkeys -- see
 * `Router::flip_posture`'s own doc comment on why they ride along with the
 * next cursor move instead of being their own round trip. */
export function routeMove(x: Um, y: Um, flipPosture?: boolean, width?: Um): Promise<RoutePreview> {
  return postJson("/api/route/move", { x, y, flip_posture: flipPosture, width });
}

export function routeFix(x: Um, y: Um): Promise<RouteFixReply> {
  return postJson("/api/route/fix", { x, y });
}

export function routeUndoSegment(): Promise<{ ok: boolean; popped: boolean }> {
  return postJson("/api/route/undo_segment", {});
}

export function routeToggleVia(enabled: boolean, diameter?: Um, drill?: Um, toLayer?: string): Promise<{ ok: boolean }> {
  return postJson("/api/route/via", { enabled, diameter, drill, to_layer: toLayer });
}

export function routeFinish(x: Um, y: Um): Promise<CmdReply> {
  return postJson("/api/route/finish", { x, y });
}

export function routeCancel(): Promise<{ ok: boolean }> {
  return postJson("/api/route/cancel", {});
}

// `D`: drag an existing track segment/corner or via, keeping its
// connections (gap #7 stage 5). Not yet wired into Canvas.tsx's move tool
// (see docs/parity/GAPS.md #7's own tracking note) -- these three calls
// are the full backend surface a future drag-tool integration needs;
// `routeCancel` above already ends a drag session too (it's the same
// backend session as a route, see `RoutePreview`'s own doc comment).

export function routeDragStart(x: Um, y: Um, layer: string): Promise<DragPreview> {
  return postJson("/api/route/drag_start", { x, y, layer });
}

export function routeDragMove(x: Um, y: Um): Promise<DragPreview> {
  return postJson("/api/route/drag_move", { x, y });
}

export function routeDragFinish(x: Um, y: Um): Promise<CmdReply> {
  return postJson("/api/route/drag_finish", { x, y });
}
