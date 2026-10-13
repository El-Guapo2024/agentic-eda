// Helpers for the differential-pair routing tool (Canvas.tsx, `6`) --
// the DIFF_PAIR_PLACER counterpart of routing.ts's single-track helpers,
// same split: thin client-side glue (turn a click/move into the right
// `/api/route/dp_*` call and `DrawState` update), not a second copy of
// `eda_pns::diff_pair`'s own geometry.
import { dpFinish, dpFix, dpMove, dpStart, dpToggleVia } from "../../api/client";
import type { Action, StudioApi } from "../../state/store";
import type { Dispatch } from "react";
import { dpStateFromPreview, dpViaRequest, type DpDrawState } from "../../kicad-port/dpTool";
import type { RouterSettings } from "../../kicad-port/routerSettings";

/** `6` / the first click after arming the diff-pair tool: start a session
 * from whatever pad/via/track-end is at `(x, y)`, on a net with a
 * recognized differential-pair suffix and a matching item for the other
 * half already on the board (`dp_coupled_net_name`). `settings` are the
 * router's (the pair is walked, shoved or only marked per its mode).
 * Shows an error toast -- KiCad's own message -- and leaves `drawState`
 * untouched otherwise. */
export async function startDiffPairRoute(x: number, y: number, layer: string, dispatch: Dispatch<Action>, dims?: { width: number; gap: number; viaGap?: number }, settings?: RouterSettings): Promise<void> {
  // `dims`: the "Differential Pair Dimensions..." dialog's custom width and gap, when it was used.
  const preview = await dpStart(x, y, layer, dims, settings);
  if (!preview.ok) {
    dispatch({ type: "TOAST", message: preview.message ?? "Start from a differential-pair net (needs a +/-/P/N suffix and a matching pad already on the board).", kind: "error" });
    return;
  }
  const draw: DpDrawState = dpStateFromPreview({ kind: "diffpair", netA: preview.net_a, netB: preview.net_b, layer: preview.layer, width: preview.width, ptsA: [], ptsB: [] }, preview);
  dispatch({ type: "SET_DRAW_STATE", draw });
}

/** `V` while a pair is routed (`ROUTER_TOOL::onViaCommand` -> `handleLayerSwitch`): arm or disarm the pair of vias, then ask for the preview again so the vias show. */
export async function toggleDiffPairVia(draw: DpDrawState, cursor: { x: number; y: number }, toLayer: string, dispatch: Dispatch<Action>): Promise<void> {
  const request = dpViaRequest(draw, toLayer);
  const reply = await dpToggleVia(request.enabled, request.toLayer);
  if (!reply.ok) return;
  const preview = await dpMove(cursor.x, cursor.y);
  if (preview.ok) dispatch({ type: "SET_DRAW_STATE", draw: { ...dpStateFromPreview(draw, preview), pendingViaLayer: request.enabled ? request.toLayer : undefined } });
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
