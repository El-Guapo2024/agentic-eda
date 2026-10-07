// Helpers for the differential-pair routing tool (Canvas.tsx, `6`) --
// the DIFF_PAIR_PLACER counterpart of routing.ts's single-track helpers,
// same split: thin client-side glue (turn a click/move into the right
// `/api/route/dp_*` call and `DrawState` update), not a second copy of
// `eda_pns::diff_pair`'s own geometry.
import { dpFinish, dpFix, dpStart } from "../../api/client";
import type { Action, StudioApi } from "../../state/store";
import type { Dispatch } from "react";
import { dpStateFromPreview, type DpDrawState } from "../../kicad-port/dpTool";

/** `6` / the first click after arming the diff-pair tool: start a session
 * from whatever pad/via/track-end is at `(x, y)`, on a net with a
 * recognized differential-pair suffix and a matching pad for the other
 * half already on the board (`dp_coupled_net_name`). Shows an error toast
 * and leaves `drawState` untouched otherwise. */
export async function startDiffPairRoute(x: number, y: number, layer: string, dispatch: Dispatch<Action>, dims?: { width: number; gap: number }): Promise<void> {
  // `dims`: the "Differential Pair Dimensions..." dialog's custom width and gap, when it was used.
  const preview = await dpStart(x, y, layer, dims);
  if (!preview.ok) {
    dispatch({ type: "TOAST", message: preview.message ?? "Start from a differential-pair net (needs a +/-/P/N suffix and a matching pad already on the board).", kind: "error" });
    return;
  }
  const draw: DpDrawState = { kind: "diffpair", netA: preview.net_a, netB: preview.net_b, layer: preview.layer, width: preview.width, ptsA: preview.head_a, ptsB: preview.head_b, colliding: preview.colliding, runsA: preview.runs_a, runsB: preview.runs_b, snappedEnd: preview.snapped_end };
  dispatch({ type: "SET_DRAW_STATE", draw });
}

/** A click while routing a pair: fix the current head. Finishes the whole
 * connection automatically when both lines reached their own coupled
 * same-net anchors at once (`real_end`), same as the single-track route
 * tool's own `fixInteractiveRoute`. */
export async function fixDiffPairRoute(x: number, y: number, draw: DpDrawState, dispatch: Dispatch<Action>, api: Pick<StudioApi, "refresh">): Promise<void> {
  const reply = await dpFix(x, y);
  if (!reply.ok) {
    dispatch({ type: "TOAST", message: reply.message ?? "Diff pair route error.", kind: "error" });
    return;
  }
  if (reply.blocked) {
    dispatch({ type: "TOAST", message: "Can't fix here -- still colliding.", kind: "error" });
    return;
  }
  if (reply.real_end) {
    await finishDiffPairRoute(x, y, dispatch, api);
    return;
  }
  if (reply.preview) dispatch({ type: "SET_DRAW_STATE", draw: dpStateFromPreview(draw, reply.preview) });
}

/** Finish the pair at `(x, y)`. Shared by every finish entry point (a
 * click reaching a real end, Enter/double-click/"F" via `finishDraw`) so
 * they can never disagree about what finishing a pair commits. */
export async function finishDiffPairRoute(x: number, y: number, dispatch: Dispatch<Action>, api: Pick<StudioApi, "refresh">): Promise<void> {
  const reply = await dpFinish(x, y);
  if (!reply.ok) dispatch({ type: "TOAST", message: reply.message || "Could not finish the diff pair.", kind: "error" });
  dispatch({ type: "SET_DRAW_STATE", draw: null });
  await api.refresh();
}
