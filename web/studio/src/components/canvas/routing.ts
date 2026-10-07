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
import type { BoardState, RouteMode } from "../../api/types";
import { routeCancel, routeFinish, routeFix, routeStart } from "../../api/client";
import type { Action, StudioApi } from "../../state/store";
import type { Dispatch } from "react";
import { drawStateFromPreview, type RouteDrawState } from "../../kicad-port/routeTool";
import { posture90 } from "../../kicad-port/pcbEditActions";
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
export async function startInteractiveRoute(x: number, y: number, layer: string, width: number, settings: { mode: RouteMode; removeLoops: boolean }, dispatch: Dispatch<Action>): Promise<boolean> {
  const preview = await routeStart(x, y, layer, width, settings.mode, settings.removeLoops);
  if (!preview.ok) {
    dispatch({ type: "TOAST", message: preview.message ?? "Start a route from a pad, via, or track end.", kind: "error" });
    return false;
  }
  const draw: RouteDrawState = { kind: "route", net: preview.net ?? "", layer: preview.layer, width, pts: preview.head, colliding: preview.colliding, runs: preview.runs, via: preview.via, snappedEnd: preview.snapped_end, displaced: preview.displaced, displacedVias: preview.displaced_vias };
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
  if (reply.preview) dispatch({ type: "SET_DRAW_STATE", draw: drawStateFromPreview(draw, reply.preview) });
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

/** Esc while routing (or dragging -- `routeCancel`'s own doc comment:
 * `POST /api/route/cancel` drops whatever session is active on the shared
 * `Router`, a route or a drag alike): cancel server-side (fire-and-forget
 * -- the UI doesn't need to wait for the ack) and clear the local preview
 * immediately. */
export function cancelInteractiveRoute(dispatch: Dispatch<Action>): void {
  void routeCancel();
  dispatch({ type: "SET_DRAW_STATE", draw: null });
}

export interface RouteAnchor {
  net: string;
  layer: string | null;
  at: [number, number];
  /** For messaging only -- what the anchor actually is. */
  from: string;
}

/** Nearest anchor (a pad, a via, or a track endpoint) within `thresholdUm`, or null. Ties broken by distance, then by this search order (pads first, matching pcbnew: a pad "wins" a track landing exactly on it). Used by the standalone via tool (which has no multi-step session of its own) and as `startInteractiveRoute`'s own pre-flight hint for the "nothing routable here" toast. */
export function findRouteAnchor(board: BoardState, xUm: number, yUm: number, thresholdUm: number): RouteAnchor | null {
  let best: RouteAnchor | null = null;
  let bestD = thresholdUm;
  const consider = (net: string | null, layer: string | null, ax: number, ay: number, from: string) => {
    if (!net) return;
    const d = Math.hypot(ax - xUm, ay - yUm);
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
 * cycled by `pcbnew.EditorControl.lineModeNext`): `direct` = free angle
 * (`LEADER_MODE::DIRECT`), `45` = nearest 45-degree ray (`DEG45`), `90` =
 * orthogonal (`DEG90`). The shape tools' segment/rect preview and click
 * both go through this so they can never disagree.
 */
export function constrainByAngleMode(mode: AngleSnapMode, from: [number, number], to: [number, number]): [number, number] {
  if (mode === "direct") return to;
  if (mode === "90") return posture90(from, to);
  return posture45(from, to);
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
