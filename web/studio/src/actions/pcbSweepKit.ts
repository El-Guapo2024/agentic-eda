// What the pcbnew sweep files (pcbEditSweep.ts, pcbRouterSweep.ts, pcbConvertSweep.ts, ...) share: the
// registry context, and the helpers every board-edit handler uses (the tab guard, `RequestSelection` with
// a filter, and "send these commands as one undo step, then select what the edit made").

import type { Dispatch } from "react";
import type { Action, StudioApi, StudioState } from "../state/store";
import type { BoardState, Cmd } from "../api/types";
import { fetchState } from "../api/client";

export interface SweepCtx {
  state: StudioState;
  dispatch: Dispatch<Action>;
  api: StudioApi;
  /** `PCB_SELECTION_TOOL::RequestSelection`: the selection, or the item under the cursor when nothing is selected. */
  requestSelection: () => string[];
}

/** Every item id of the board that can come out of a shape/track edit (to find what an edit created). */
export function itemIds(board: BoardState | null): Set<string> {
  const out = new Set<string>();
  if (!board) return out;
  for (const s of board.drawings?.shapes ?? []) out.add(s.id);
  for (const t of board.drawings?.texts ?? []) out.add(t.id);
  for (const t of board.routing?.tracks ?? []) out.add(t.id);
  for (const v of board.routing?.vias ?? []) out.add(v.id);
  for (const z of board.routing?.zones ?? []) out.add(z.id);
  return out;
}

export interface SweepHelpers {
  toast: (message: string, kind?: "info" | "error") => void;
  /** Same guard `useActionRunner` puts on every board edit: a stray key on the schematic tab must never reach a board command. */
  pcbOnly: (fn: () => void) => () => void;
  lockedIds: ReadonlySet<string>;
  requestFiltered: (accept: (id: string) => boolean) => string[];
  applyEdit: (cmds: Cmd[], removed?: readonly string[], message?: string | null, opts?: { keep?: boolean }) => Promise<boolean>;
}

export function createSweepHelpers(ctx: SweepCtx): SweepHelpers {
  const { state, dispatch, api } = ctx;
  const toast = (message: string, kind: "info" | "error" = "info") => dispatch({ type: "TOAST", message, kind });
  const pcbOnly =
    (fn: () => void) =>
    () => {
      if (state.tab === "pcb") fn();
    };
  const lockedIds = new Set(state.board?.locked ?? []);

  /**
   * `RequestSelection( clientFilter )`: the selection (or the hovered item), with the items the filter rejects and
   * the locked ones dropped from the selection itself (`FilterCollectorForLockedItems`).
   */
  const requestFiltered = (accept: (id: string) => boolean): string[] => {
    const ids = ctx.requestSelection();
    const out = ids.filter((id) => accept(id) && !lockedIds.has(id));
    if (state.selection.size > 0 && out.length !== ids.length) dispatch({ type: "SET_SELECTION", refs: out });
    return out;
  };

  const applyEdit = (cmds: Cmd[], removed: readonly string[] = [], message: string | null = null, opts: { keep?: boolean } = {}): Promise<boolean> => applyCommands(api, dispatch, cmds, removed, message, opts);

  return { toast, pcbOnly, lockedIds, requestFiltered, applyEdit };
}

/**
 * Send `cmds` as one undo step, then select what the edit made (every id that did not exist before) plus whatever of
 * the old selection survived -- the C++ keeps modified items selected and adds the created ones. The new ids are read
 * from the backend directly: the store's own state only catches up on its next render, and its board refresh also
 * drops every non-footprint id from the selection. `keep: false` selects only what the edit created (an outset
 * deselects the originals).
 */
export async function applyCommands(api: StudioApi, dispatch: Dispatch<Action>, cmds: Cmd[], removed: readonly string[] = [], message: string | null = null, opts: { keep?: boolean } = {}): Promise<boolean> {
  const toast = (text: string) => dispatch({ type: "TOAST", message: text, kind: "info" });
  if (cmds.length === 0) {
    if (message) toast(message);
    return false;
  }
  const before = itemIds(api.getState().board);
  const previous = [...api.getState().selection];
  const ok = await api.cmdBatch(cmds);
  if (!ok) return false;
  const fresh = await fetchState().catch(() => null);
  const after = itemIds(fresh);
  const gone = new Set(removed);
  const keep = opts.keep === false ? [] : previous.filter((id) => !gone.has(id) && (after.has(id) || !!fresh?.parts.some((p) => p.ref === id)));
  const created = [...after].filter((id) => !before.has(id));
  dispatch({ type: "SET_SELECTION", refs: [...keep, ...created] });
  if (message) toast(message);
  return true;
}
