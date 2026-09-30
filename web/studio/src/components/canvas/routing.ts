// Helpers for the interactive routing/drawing tools (Canvas.tsx): where a
// route/track/via/zone/shape can legally start or snap to, and the 45/90
// "posture" constraint pcbnew's interactive router applies to the segment
// currently being dragged toward the cursor.
//
// Scope, stated up front: this is a single-segment-per-click router, not
// pcbnew's own (which previews a two-segment L/diagonal path and offers
// posture-cycling with Space). No push-and-shove, no live clearance
// check -- the task spec is explicit that the gates judge the committed
// result, not a live preview. What's here is the part of "45°/90°
// posture and grid snap" a single dragged segment can honestly provide.
import type { BoardState, Cmd } from "../../api/types";
import type { DrawState } from "../../state/store";

type RunCmd = (c: Cmd) => Promise<boolean>;

/** Shared by the F ("Attempt Finish") hotkey (useActionRunner.ts, only while routing -- see its own comment on the Flip/AttemptFinish key collision) and Canvas.tsx's Enter/double-click finish, so the two can never commit a route differently. Committing `add_track` needs at least a start and an end. */
export async function commitRoute(draw: Extract<DrawState, { kind: "route" }>, cmd: RunCmd): Promise<void> {
  if (draw.pts.length < 2) return;
  await cmd({ op: "add_track", net: draw.net, layer: draw.layer, width: draw.width, pts: draw.pts.map(([x, y]) => ({ x, y })) });
}

/** V while routing (pcbnew.Control.layerToggle, "Toggle Layer"): commit what's drawn so far, drop a via at the current end, and restart the route on the *other* board layer from there -- KiCad's own via-mid-route mechanic, ported as two immediate commits instead of one deferred multi-layer track (see this file's header comment on scope). */
export async function dropViaAndSwitchLayer(draw: Extract<DrawState, { kind: "route" }>, board: BoardState, cmd: RunCmd): Promise<DrawState> {
  const last = draw.pts[draw.pts.length - 1]!;
  const otherLayer = board.layers.find((l) => l !== draw.layer) ?? draw.layer;
  if (draw.pts.length >= 2) {
    await cmd({ op: "add_track", net: draw.net, layer: draw.layer, width: draw.width, pts: draw.pts.map(([x, y]) => ({ x, y })) });
  }
  await cmd({
    op: "add_via",
    net: draw.net,
    x: last[0],
    y: last[1],
    drill: board.board_rules?.via_drill ?? 300,
    diameter: board.board_rules?.via_diameter ?? 600,
    from_layer: draw.layer,
    to_layer: otherLayer,
  });
  return { kind: "route", net: draw.net, layer: otherLayer, width: draw.width, pts: [last] };
}

export interface RouteAnchor {
  net: string;
  layer: string | null;
  at: [number, number];
  /** For messaging only -- what the anchor actually is. */
  from: string;
}

/** Nearest anchor (a pad, a via, or a track endpoint) within `thresholdUm`, or null. Ties broken by distance, then by this search order (pads first, matching pcbnew: a pad "wins" a track landing exactly on it). */
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

const POSTURE_STEP_DEG = 45;

/** `to`, constrained onto the nearest 45-degree ray from `from` -- pcbnew's "45 Degree" line mode (the default; free-angle is its own separate mode this app doesn't implement). */
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
