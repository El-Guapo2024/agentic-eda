// Helpers for the differential-pair routing tool (Canvas.tsx, `6`) --
// the DIFF_PAIR_PLACER counterpart of routing.ts's single-track helpers,
// same split: thin client-side glue (turn a click/move into the right
// `/api/route/dp_*` call and `DrawState` update), not a second copy of
// `eda_pns::diff_pair`'s own geometry.
import { dpAttemptFinish, dpContinueFromEnd, dpFinish, dpFix, dpMove, dpStart, dpToggleVia } from "../../api/client";
import type { Action, StudioApi } from "../../state/store";
import type { Dispatch } from "react";
import { dpStateFromPreview, dpViaRequest, type DpDrawState } from "../../kicad-port/dpTool";
import type { RouterSettings } from "../../kicad-port/routerSettings";
import type { LayerPair } from "../../kicad-port/layerPairs";
import { withEventSizes } from "../../kicad-port/routerSizes";
import { planThroughVia } from "./routerViaArm";
import { keepRouterSizesPairs, startRouterSizesPairs } from "../../kicad-port/routerVia";

/** `6` / the first click after arming the diff-pair tool: start a session
 * from whatever pad/via/track-end is at `(x, y)`, on a net with a
 * recognized differential-pair suffix and a matching item for the other
 * half already on the board (`dp_coupled_net_name`). `settings` are the
 * router's (the pair is walked, shoved or only marked per its mode).
 * Shows an error toast -- KiCad's own message -- and leaves `drawState`
 * untouched otherwise. */
/** `startWithVia`: V was pressed before the pair started (`m_startWithVia`, R7-R3): the pair starts with its vias armed, sized as V sizes them
 *  (`size`: none for the net class's), to Place Through Via's implicit target (`performRouting`: `handleLayerSwitch( ACT_PlaceThroughVia, true )`: the
 *  nearest ratsnest anchor's layer of the P net, else the other layer of the layer pair `pair`, UI-LEFT R5). */
export async function startDiffPairRoute(x: number, y: number, layer: string, dispatch: Dispatch<Action>, dims?: { width: number; gap: number; viaGap?: number }, settings?: RouterSettings, autoTrackWidth?: boolean, startWithVia?: { size: { diameter: number; drill: number } | null; pair: LayerPair; copper: readonly string[] }, boardPair?: LayerPair): Promise<void> {
  // `dims`: the "Differential Pair Dimensions..." dialog's custom width and gap, when it was used.
  const preview = await dpStart(x, y, layer, dims, settings, autoTrackWidth);
  if (!preview.ok) {
    dispatch({ type: "TOAST", message: preview.message ?? "Start from a differential-pair net (needs a +/-/P/N suffix and a matching pad already on the board).", kind: "error" });
    return;
  }
  const draw: DpDrawState = dpStateFromPreview({ kind: "diffpair", netA: preview.net_a, netB: preview.net_b, layer: preview.layer, width: preview.width, ptsA: [], ptsB: [], sizes: preview.sizes }, preview);
  // `prepareInteractive`: the pair starts on `getStartLayer`'s layer, the active one until it ends (r5 R5-1); `m_originalActiveLayer` is saved
  // whatever the start layer, and given back when it ends (R6-R6)
  draw.originalActiveLayer = layer;
  if (preview.layer && preview.layer !== layer) dispatch({ type: "SET_ACTIVE_LAYER", layer: preview.layer });
  // `prepareInteractive`: the router's sizes as the last route left them (`m_savedSizes`, UI-LEFT-2 RR3; one `ROUTER` for routes and pairs) plus the
  // board's layer pair
  const pair = boardPair ?? startWithVia?.pair;
  if (pair) draw.sizesPairs = startRouterSizesPairs(pair);
  if (startWithVia) {
    const plan = await planThroughVia({ x, y }, draw.layer, startWithVia.copper, startWithVia.pair, draw.sizesPairs);
    if (plan.ok) {
      const armed = await dpToggleVia(true, plan.toLayer, startWithVia.size?.diameter, startWithVia.size?.drill);
      if (armed.ok) {
        draw.placingVia = true;
        draw.pendingViaLayer = plan.toLayer;
        draw.sizesPairs = keepRouterSizesPairs(plan.sizesPairs);
      }
    }
  }
  dispatch({ type: "SET_DRAW_STATE", draw });
}

/** `V` while a pair is routed (`ROUTER_TOOL::onViaCommand` -> `handleLayerSwitch`): arm or disarm the pair of vias, then ask for the preview again so the vias show. */
export async function toggleDiffPairVia(draw: DpDrawState, cursor: { x: number; y: number }, toLayer: string, dispatch: Dispatch<Action>, size?: { diameter: number; drill: number }): Promise<void> {
  // `size`: the chosen via entry or custom size (`chosenViaOf`), none for the net class's (R7-R2)
  const request = dpViaRequest(draw, toLayer, size);
  const reply = await dpToggleVia(request.enabled, request.toLayer, request.diameter, request.drill);
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
  // `updateSizesAfterRouterEvent` after the fix (UI-LEFT R5): the message panel's clearance, and the pair's width and gap when asked again
  if (reply.preview) dispatch({ type: "SET_DRAW_STATE", draw: withEventSizes(dpStateFromPreview(draw, reply.preview), reply.sizes) });
}

/**
 * `F` (AttemptFinish) while a pair is routed: `routerAttemptFinish` -> `ROUTER::Finish`, whose trace is the pair placer's first, the P line (UI-LEFT R2). The
 * pair is finished on the coupled pair at the nearest unconnected anchor of the P net, one undo step; otherwise it goes on, and the cursor is warped to where
 * its end got (`controls()->WarpMouseCursor( m_router->Placer()->CurrentEnd(), true, true )` -- here the preview there is shown). It is never finished at
 * the cursor; Enter and the double-click still are (`finishInteractive`: `FixRoute( .., forceFinish = true, .. )`).
 */
export async function attemptFinishDiffPairRoute(x: number, y: number, draw: DpDrawState, dispatch: Dispatch<Action>, api: Pick<StudioApi, "refresh">): Promise<void> {
  const reply = await dpAttemptFinish(x, y);
  if (!reply.ok) {
    if (reply.message) dispatch({ type: "TOAST", message: reply.message, kind: "error" });
    return;
  }
  if (reply.finished) {
    dispatch({ type: "SET_DRAW_STATE", draw: null });
    await api.refresh();
    return;
  }
  if (reply.preview?.ok) dispatch({ type: "SET_DRAW_STATE", draw: dpStateFromPreview(draw, reply.preview) });
  // a fix at the anchor may have committed nothing yet but placed a leg (`FixRoute` returned false): the board is unchanged until the pair ends
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

/**
 * Ctrl+E while routing a pair, Route From Other End (`ROUTER_TOOL::routerContinueFromEnd` -> `ROUTER::ContinueFromEnd`, which restarts the placer of the
 * same mode): what the pair has fixed is committed (one undo step, nothing at all if nothing was fixed), and a new pair starts at the far end of the P net's
 * connection and heads for where this one's head ended. `at` is that end. A refusal ("nothing is left to connect to") toasts and leaves the pair as it was; a
 * far end a pair cannot start on ends the routing with KiCad's own message, after the commit.
 */
export async function continueDiffPairFromEnd(at: [number, number], draw: DpDrawState, dispatch: Dispatch<Action>, api: Pick<StudioApi, "refresh">): Promise<void> {
  const reply = await dpContinueFromEnd(at[0], at[1]);
  if (reply.committed) await api.refresh();
  if (!reply.ok) {
    dispatch({ type: "TOAST", message: reply.message ?? "Route From Other End failed.", kind: "error" });
    // the session is gone when the commit happened and the far end would not start a pair
    if (reply.committed !== undefined) dispatch({ type: "SET_DRAW_STATE", draw: null });
    return;
  }
  // the new pair starts at the far end: whatever the old one started on is no longer what it must not end on
  dispatch({ type: "SET_DRAW_STATE", draw: { ...dpStateFromPreview({ ...draw, runsA: [], runsB: [], fixedVias: [] }, reply), pendingViaLayer: undefined } });
}
