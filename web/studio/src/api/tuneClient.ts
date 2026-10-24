// The requests of the length tuner (crates/cli/src/tune_api.rs): hover a track, click to start a pattern, move the cursor to where it ends, click to put it on the
// board; the patterns that are on the board.
import type { Nm } from "./types";
import type { TuneFixReply, TuneHoverReply, TuneKind, TunePatternReply, TunePatternRow, TuneSessionReply, TuneSettings } from "./tuneTypes";

async function postJson<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  return (await r.json()) as T;
}

const round = (v: number) => Math.round(v);

export interface TuneAt {
  x: Nm;
  y: Nm;
  /** How far from the track's edge a click still picks it. */
  tolerance?: Nm;
  /** The active layer: preferred when more than one track is under the cursor. */
  layer?: string | null;
  mode: TuneKind;
  settings?: TuneSettings;
}

function atBody(a: TuneAt) {
  return { x: round(a.x), y: round(a.y), tolerance: a.tolerance == null ? undefined : round(a.tolerance), layer: a.layer ?? undefined, mode: a.mode, settings: a.settings };
}

/** The status box for the track under the cursor (`updateHoverStatus`): nothing is changed. */
export const tuneHover = (a: TuneAt) => postJson<TuneHoverReply>("/api/tune/hover", atBody(a));
/** The first click: a pattern is started on the track under the cursor. */
export const tuneStart = (a: TuneAt) => postJson<TuneSessionReply>("/api/tune/start", atBody(a));
/** The cursor moved: the meander is fitted from the start of the pattern to it. */
export const tuneMove = (x: Nm, y: Nm) => postJson<TuneSessionReply>("/api/tune/move", { x: round(x), y: round(y) });
/** The `1`-`4` keys. */
export type TuneStepKey = "amplitude_up" | "amplitude_down" | "spacing_up" | "spacing_down";
export const tuneStep = (what: TuneStepKey) => postJson<TuneSessionReply>("/api/tune/step", { what });
/** The dialog's settings for the pattern being placed. */
export const tuneSettings = (settings: TuneSettings) => postJson<TuneSessionReply>("/api/tune/settings", { settings });
/** The second click: the pattern is put on the board (one undo step). The end of an edit is this too. */
export const tuneFix = (x?: Nm, y?: Nm) => postJson<TuneFixReply>("/api/tune/fix", x == null || y == null ? {} : { x: round(x), y: round(y) });
/** Escape. */
export const tuneCancel = () => postJson<{ ok: boolean; had_session: boolean }>("/api/tune/cancel", {});

/** The patterns of the board, as the Generators Manager lists them. */
export const tunePatterns = () => postJson<{ ok: boolean; message?: string; patterns: TunePatternRow[] }>("/api/tune/patterns", {});
/** A pattern with the target a design rule would give it. */
export const tunePattern = (id: string) => postJson<TunePatternReply>("/api/tune/pattern", { id });
/** The pattern tuned again with these settings (and new ends, if given). */
export const tuneUpdate = (id: string, settings: TuneSettings, ends?: { origin?: { x: Nm; y: Nm }; end?: { x: Nm; y: Nm } }) =>
  postJson<{ ok: boolean; message?: string }>("/api/tune/update", { id, settings, ...ends });
/** The pattern's tracks go back to the baseline and the pattern is gone. */
export const tuneRemove = (id: string) => postJson<{ ok: boolean; message?: string }>("/api/tune/remove", { id });
/** Every pattern of the list (all of them without one) tuned again, one undo step. */
export const tuneRegenerate = (ids?: string[]) => postJson<{ ok: boolean; message?: string; regenerated?: number; failed?: string[] }>("/api/tune/regenerate", ids ? { ids } : {});

/** `genStartEdit`: a pattern of the board is edited through its four edit points. */
export const tuneEditStart = (id: string, settings?: TuneSettings) => postJson<TuneSessionReply>("/api/tune/edit_start", { id, settings });
/** `genUpdateEdit`: an edit point is at (x, y) (0 the start, 1 the end, 2 the width of the meanders and their side, 3 the spacing), or the settings change. */
export const tuneEditMove = (move: { handle: number; x: Nm; y: Nm } | { settings: TuneSettings }) => postJson<TuneSessionReply>("/api/tune/edit_move", "settings" in move ? move : { handle: move.handle, x: round(move.x), y: round(move.y) });
