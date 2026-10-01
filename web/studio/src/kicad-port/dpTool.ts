// Pure glue between the backend's differential-pair router session
// (crates/pns::diff_pair, driven through crates/cli/src/route_api.rs's
// dp_{start,move,fix,finish} endpoints) and this app's `DrawState` -- the
// `6` counterpart of routeTool.ts's single-track glue. Same split: this
// module turns a `DiffPairPreview` reply into the "diffpair" DrawState
// patch Canvas.tsx's painter already knows how to draw, and stays on the
// dependency-free side of the line every other kicad-port module is
// already on.
//
// router_tool.cpp's own DIFF_PAIR_PLACER loop this ports (the state
// machine, not the geometry -- that's crates/pns::diff_pair):
//   6 (nothing selected) -> start from whatever pad/via/track-end is
//                           under the cursor, on a net with a recognized
//                           differential-pair suffix (components/canvas/
//                           diffPairRouting.ts)
//   mouse move            -> live preview of both lines at once
//   click                 -> fix the current head as a permanent leg
//   double-click/Enter    -> finish (same commit as a click reaching a
//                            coupled pair of same-net anchors)
//   Esc                   -> cancel, nothing committed
//   Backspace             -> undo the last fixed leg
//   /                     -> flip posture (the spine's, same as `X`)
import type { DiffPairPreview } from "../api/types";

/** Mirrors `state/store.tsx`'s `DrawState`'s "diffpair" variant exactly --
 * duplicated rather than imported, same convention `RouteDrawState`/
 * `DragDrawState` already use. store.tsx is the source of truth for the
 * real shape; keep this in sync with it by hand. */
export interface DpDrawState {
  kind: "diffpair";
  netA: string | null;
  netB: string | null;
  layer: string;
  width: number;
  ptsA: [number, number][];
  ptsB: [number, number][];
  colliding?: boolean;
  runsA?: { layer: string; pts: [number, number][] }[];
  runsB?: { layer: string; pts: [number, number][] }[];
  snappedEnd?: boolean;
}

/** The `DrawState` "diffpair" patch for a successful `DiffPairPreview`
 * reply -- same shape as `routeTool.ts`'s `drawStateFromPreview`. */
export function dpStateFromPreview(current: DpDrawState, preview: DiffPairPreview): DpDrawState {
  return {
    ...current,
    netA: preview.net_a ?? current.netA,
    netB: preview.net_b ?? current.netB,
    layer: preview.layer,
    width: preview.width,
    ptsA: preview.head_a,
    ptsB: preview.head_b,
    colliding: preview.colliding,
    runsA: preview.runs_a,
    runsB: preview.runs_b,
    snappedEnd: preview.snapped_end,
  };
}
