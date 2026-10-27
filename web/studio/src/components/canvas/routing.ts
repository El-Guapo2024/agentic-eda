// Helpers for the interactive routing tool (Canvas.tsx) and the plain
// drawing tools (shape/zone). The route tool (X) is now backed by
// `crates/pns` -- KiCad's own push-and-shove router, ported -- driven
// through `crates/cli/src/route_api.rs`'s `POST /api/route/*` session
// (gap #7): every call here resolves obstacles exactly the way real
// pcbnew's interactive router does (walkaround/shove/mark-obstacles),
// server-side. This module is the thin client-side half of that --
// turning a click/move/key into the right API call and `DrawState`
// update -- not a second copy of the routing algorithm. `posture45`
// below is unrelated to the route tool now; it remains the plain-45-
// degree-snap helper the shape tools (segment/rect) still use for their
// own, much simpler, single-segment preview.
import type { BoardState } from "../../api/types";
import type { RouterSettings } from "../../kicad-port/routerSettings";
import { routeAttemptFinish, routeCancel, routeContinueFromEnd, routeFinish, routeFix, routeStart } from "../../api/client";
import type { Action, StudioApi } from "../../state/store";
import type { Dispatch } from "react";
import { drawStateFromPreview, type RouteDrawState } from "../../kicad-port/routeTool";
import { shapeSecondPoint } from "../../kicad-port/pcbShapeSnap";
import type { AngleSnapMode } from "../../kicad-port/pcbParityState";
import { advanceRouteQueue } from "./routeQueue";

/** `X` / the first click after arming the route tool: start a session from
 * whatever pad/via/track-end is at `(x, y)`. Shows an error toast and
 * leaves `drawState` untouched if there's nothing routable there --
 * `isStartingPointRoutable`'s own refusal, ported. `settings` is
 * `state.routerSettings` (`Ctrl+<`'s own dialog, components/
 * RouterSettingsDialog.tsx) -- read fresh at the start of every session,
 * same as every other "current pick" this app's route/via tools read
 * (track width, via preset). */
/** `width` undefined: the server's `ImportSizes` width of the start net (the width list on "use net class"), which the reply carries. */
export async function startInteractiveRoute(x: number, y: number, layer: string, width: number | undefined, settings: RouterSettings, dispatch: Dispatch<Action>, startItem?: string | null, autoTrackWidth?: boolean): Promise<boolean> {
  const preview = await routeStart(x, y, layer, width, settings, autoTrackWidth);
  // `syncZone`'s warning (KiCad's `KIDIALOG`): a rule area the router cannot handle is left out of its world
  for (const w of preview.warnings ?? []) dispatch({ type: "TOAST", message: w, kind: "info" });
  if (!preview.ok) {
    dispatch({ type: "TOAST", message: preview.message ?? "Start a route from a pad, via, or track end.", kind: "error" });
    return false;
  }
  const draw: RouteDrawState = { kind: "route", net: preview.net ?? "", layer: preview.layer, width: preview.width ?? width ?? 250_000, pts: preview.head, colliding: preview.colliding, runs: preview.runs, via: preview.via, snappedEnd: preview.snapped_end, displaced: preview.displaced, displacedVias: preview.displaced_vias, startItem: startItem ?? undefined, sizes: preview.sizes };
  // `ROUTER_TOOL::prepareInteractive`: the route starts on `getStartLayer`'s layer -- the start item's when it is not on the active one -- and that
  // becomes the active layer until the route ends (r5 R5-1)
  if (preview.layer && preview.layer !== layer) {
    draw.originalActiveLayer = layer;
    dispatch({ type: "SET_ACTIVE_LAYER", layer: preview.layer });
  }
  dispatch({ type: "SET_DRAW_STATE", draw });
  return true;
}

/** A click while routing: fix the current head. Finishes the whole
 * connection automatically when the head reached a same-net anchor
 * (`real_end`), same as a plain click landing on one in real pcbnew. */
export async function fixInteractiveRoute(x: number, y: number, draw: RouteDrawState, dispatch: Dispatch<Action>, api: Pick<StudioApi, "refresh">): Promise<void> {
  const reply = await routeFix(x, y);
  if (!reply.ok) {
    dispatch({ type: "TOAST", message: reply.message ?? "Route error.", kind: "error" });
    return;
  }
  if (reply.blocked) {
    dispatch({ type: "TOAST", message: "Can't fix here -- still colliding.", kind: "error" });
    return;
  }
  if (reply.real_end) {
    await finishInteractiveRoute(x, y, dispatch, api);
    return;
  }
  // a fix places the armed via and goes on on its other layer (`Router::fix` takes the pending via): nothing is armed any more, as in KiCad
  // (`IsPlacingVia()` is false after `FixRoute`), so V or a via command arms the next one rather than taking this one back
  if (reply.preview) dispatch({ type: "SET_DRAW_STATE", draw: { ...drawStateFromPreview(draw, reply.preview), placingVia: false, pendingViaLayer: undefined } });
}

/** Finish the route at `(x, y)` -- Enter/double-click/"F" (AttemptFinish),
 * and the automatic finish inside `fixInteractiveRoute` above. Shared by
 * every one of those call sites so they can never disagree about what
 * finishing a route commits (the same role `commitRoute` played for the
 * old client-only router). Commits through `Cmd::CommitRoute`
 * server-side, then refreshes the board so the new track/via show up
 * without waiting for the next 700ms version poll. */
export async function finishInteractiveRoute(x: number, y: number, dispatch: Dispatch<Action>, api: Pick<StudioApi, "refresh">): Promise<void> {
  const reply = await routeFinish(x, y);
  if (!reply.ok) dispatch({ type: "TOAST", message: reply.message || "Could not finish the route.", kind: "error" });
  dispatch({ type: "SET_DRAW_STATE", draw: null });
  await api.refresh();
  // RouteSelected's loop: the next queued connection starts once this one is done.
  await advanceRouteQueue();
}

/**
 * `F`, AttemptFinish (`ROUTER_TOOL`'s `routerAttemptFinish` -> `ROUTER::Finish`): the route is taken to the nearest unconnected anchor of
 * its net and committed when it gets there; otherwise it goes on, drawn to where the head got (KiCad warps the cursor there,
 * `WarpMouseCursor( m_router->Placer()->CurrentEnd() )`; a page cannot move the pointer, so the next mouse move takes the head on).
 */
export async function attemptFinishInteractiveRoute(x: number, y: number, draw: RouteDrawState, dispatch: Dispatch<Action>, api: Pick<StudioApi, "refresh">): Promise<void> {
  const reply = await routeAttemptFinish(x, y);
  if (!reply.ok) {
    dispatch({ type: "TOAST", message: reply.message ?? "Could not finish the route.", kind: "error" });
    return;
  }
  if (reply.finished) {
    dispatch({ type: "SET_DRAW_STATE", draw: null });
    await api.refresh();
    await advanceRouteQueue();
    return;
  }
  if (reply.preview?.ok) dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(draw, reply.preview) });
}

/**
 * Ctrl+E, Route From Other End (`ROUTER_TOOL::routerContinueFromEnd` -> `ROUTER::ContinueFromEnd`), before or after the first fix:
 * what the route has fixed is committed (one undo step, nothing at all if nothing was fixed), a new route starts at the far end
 * of the connection and heads for where this one's head ended. `at` is that end -- the head's last point. A refusal ("nothing is
 * left to connect to") toasts and leaves the route as it was.
 */
export async function continueInteractiveRouteFromEnd(at: [number, number], draw: RouteDrawState, dispatch: Dispatch<Action>, api: Pick<StudioApi, "refresh">): Promise<void> {
  const reply = await routeContinueFromEnd(at[0], at[1]);
  if (!reply.ok) {
    dispatch({ type: "TOAST", message: reply.message ?? "Route From Other End failed.", kind: "error" });
    return;
  }
  if (reply.committed) await api.refresh();
  // the new route starts at the far end: whatever the old one started on is no longer what it must not end on
  dispatch({ type: "SET_DRAW_STATE", draw: { ...drawStateFromPreview(draw, reply), placingVia: false, pendingViaLayer: undefined, startItem: undefined } });
}

/** Esc while routing (or dragging -- `routeCancel`'s own doc comment:
 * `POST /api/route/cancel` ends whatever session is active on the shared
 * `Router`, a route or a drag alike): clear the local preview immediately
 * and end the session server-side. A route or a pair commits what its
 * clicks fixed there (`ROUTER_TOOL::performRouting`: the cancel branch
 * breaks out of the loop and `CommitRouting()` follows), so the board is
 * refreshed when the reply says something was committed. */
export function cancelInteractiveRoute(dispatch: Dispatch<Action>, api?: Pick<StudioApi, "refresh">): void {
  dispatch({ type: "SET_DRAW_STATE", draw: null });
  void routeCancel().then((reply) => (reply.committed && api ? api.refresh() : undefined));
}

export interface RouteAnchor {
  net: string;
  layer: string | null;
  at: [number, number];
  /** For messaging only -- what the anchor actually is. */
  from: string;
}

/** Nearest anchor (a pad, a via, or a track endpoint) within `thresholdNm`, or null. Ties broken by distance, then by this search order (pads first, matching pcbnew: a pad "wins" a track landing exactly on it). Used by the standalone via tool (which has no multi-step session of its own) and as `startInteractiveRoute`'s own pre-flight hint for the "nothing routable here" toast. */
export function findRouteAnchor(board: BoardState, xNm: number, yNm: number, thresholdNm: number): RouteAnchor | null {
  let best: RouteAnchor | null = null;
  let bestD = thresholdNm;
  const consider = (net: string | null, layer: string | null, ax: number, ay: number, from: string) => {
    if (!net) return;
    const d = Math.hypot(ax - xNm, ay - yNm);
    if (d <= bestD) {
      bestD = d;
      best = { net, layer, at: [ax, ay], from };
    }
  };
  for (const part of board.parts) {
    if (!part.placed) continue;
    for (const pad of part.pads ?? []) consider(pad.net, null, pad.x, pad.y, `${part.ref}.${pad.num}`);
  }
  if (board.routing) {
    for (const via of board.routing.vias) consider(via.net, null, via.x, via.y, `via ${via.id}`);
    for (const track of board.routing.tracks) {
      const first = track.pts[0];
      const last = track.pts[track.pts.length - 1];
      if (first) consider(track.net, track.layer, first[0], first[1], `track ${track.id}`);
      if (last) consider(track.net, track.layer, last[0], last[1], `track ${track.id}`);
    }
  }
  return best;
}

/**
 * `to` constrained by the current line mode (`PCBNEW_SETTINGS::m_AngleSnapMode`,
 * cycled by `pcbnew.EditorControl.lineModeNext`) as a line's end: `direct` = free angle
 * (`LEADER_MODE::DIRECT`), `45` = `GetVectorSnapped45` (the cursor's x or y kept), `90` =
 * `GetVectorSnapped90`. The shape tools go through `shapeSecondPoint`
 * (kicad-port/pcbShapeSnap.ts), which adds the Ctrl modifier and a rectangle's own rule.
 */
export function constrainByAngleMode(mode: AngleSnapMode, from: [number, number], to: [number, number]): [number, number] {
  return shapeSecondPoint("segment", mode, false, from, to);
}

const POSTURE_STEP_DEG = 45;

/** `to`, constrained onto the nearest 45-degree ray from `from` -- used by
 * the shape tools' (segment/rect) own single-segment preview. The route
 * tool no longer needs this client-side: the backend's `Direction45`
 * (`crates/pns/src/direction45.rs`) resolves posture server-side and
 * sends back the real head to draw (see `previewInteractiveRoute`). */
export function posture45(from: [number, number], to: [number, number]): [number, number] {
  const dx = to[0] - from[0];
  const dy = to[1] - from[1];
  const dist = Math.hypot(dx, dy);
  if (dist < 1) return from;
  const rawDeg = (Math.atan2(dy, dx) * 180) / Math.PI;
  const snappedDeg = Math.round(rawDeg / POSTURE_STEP_DEG) * POSTURE_STEP_DEG;
  const rad = (snappedDeg * Math.PI) / 180;
  return [from[0] + Math.cos(rad) * dist, from[1] + Math.sin(rad) * dist];
}
