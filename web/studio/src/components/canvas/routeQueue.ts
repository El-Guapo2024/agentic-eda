// The loop of router_tool.cpp `ROUTER_TOOL::RouteSelected` (kicad-port/routeSelectedLoop.ts is its control
// flow). A connection that is finished -- or skipped with `cancelCurrentItem` ("Cancel Current Item") --
// moves on to the next one; Escape (`IsCancelInteractive` while `m_inRouteSelected`) ends the whole run.
// The canvas owns the live route session, so the loop is kept here as a queue the session-ending calls
// (routing.ts `finishInteractiveRoute`, the skip action, Escape) advance or clear -- the studio's
// equivalent of the C++ call stack.
import type { Dispatch } from "react";
import { routeCancel } from "../../api/client";
import { RouteSelectedLoop, type QueueOutcome } from "../../kicad-port/routeSelectedLoop";
import type { Action } from "../../state/store";

export type { QueueOutcome };

let current: RouteSelectedLoop | null = null;

/** `m_inRouteSelected`: true from the start of a RouteSelected run until its last item ends or Escape. */
export function routeQueueActive(): boolean {
  return current !== null && current.active;
}

/**
 * Start the loop: `runs[i]` starts connection i and reports whether it left a live session. The first one runs now; the next
 * ones after `advanceRouteQueue`. When the last is over the tool is popped (`frame->PopTool`) and `onEnd` runs; a cancelled
 * loop does neither.
 */
export function startRouteQueue(runs: Array<() => Promise<QueueOutcome>>, dispatch: Dispatch<Action>, onEnd: (() => void) | null = null): Promise<void> {
  const loop: RouteSelectedLoop = new RouteSelectedLoop(runs, () => {
    if (current === loop) current = null;
    dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
    onEnd?.();
  });
  current = loop;
  return loop.advance();
}

/** The loop's "next item" -- called when the live route ends. No-op without a loop. */
export async function advanceRouteQueue(): Promise<void> {
  await current?.advance();
}

/** `m_cancelled = true`: drop the rest of the loop. Returns whether there was one. */
export function clearRouteQueue(): boolean {
  const had = routeQueueActive();
  current?.cancel();
  current = null;
  return had;
}

/**
 * `cancelCurrentItem`: end the route being drawn and go on to the next queued connection. Without a loop (a plain route
 * session) it just ends the session -- the route tool stays armed, as in the C++ main loop.
 */
export async function skipCurrentRoute(dispatch: Dispatch<Action>): Promise<void> {
  await routeCancel();
  dispatch({ type: "SET_DRAW_STATE", draw: null });
  await advanceRouteQueue();
}
