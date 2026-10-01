// Pure glue between the backend's inline-drag session (crates/pns::dragger,
// driven through crates/cli/src/route_api.rs's drag_{start,move,finish}
// endpoints) and this app's `DrawState` -- the `D` counterpart of
// routeTool.ts's route glue, same split: this module turns a `DragPreview`
// reply into the "drag" DrawState patch Canvas.tsx's painter already knows
// how to draw, and stays on the dependency-free side of the line every
// other kicad-port module is already on (no React/DOM/JSX, no import of
// state/store.tsx -- see routeTool.ts's own header comment on why). The
// request-guard/move-throttle this needs are the exact same ones
// routeTool.ts already exports (route and drag sessions are mutually
// exclusive -- never both active at once -- so Canvas.tsx reuses one pair
// of refs for both).
//
// router_tool.cpp's InlineDrag loop this ports (the state machine, not the
// PNS algorithm itself -- that's crates/pns::dragger):
//   D (nothing selected, or a single track/via already selected)
//                  -> grab whatever track segment/corner or via is under
//                     the cursor (components/canvas/dragging.ts's
//                     `findDraggableAt`, approximating
//                     `ACTIONS::selectionCursor` + `NeighboringSegmentFilter`)
//   mouse move     -> live preview (shove/mark-obstacles already resolved
//                     server-side, same as the route tool)
//   click          -> fix the drag at the cursor (one commit -- a drag has
//                     no multi-leg concept the way a route does)
//   Esc            -> cancel, nothing committed
import type { DragPreview } from "../api/types";

/** Mirrors `state/store.tsx`'s `DrawState`'s "drag" variant exactly --
 * duplicated rather than imported, same convention `RouteDrawState` already
 * uses (see routeTool.ts). store.tsx is the source of truth for the real
 * shape; keep this in sync with it by hand. */
export interface DragDrawState {
  kind: "drag";
  /** `"corner"`: `pts` is the whole stretched line, one end following the
   * cursor (`Dragger::candidate`'s `Corner` branch). `"via"`: `pts` is just
   * the via's own live position (one point) -- its attached tracks' live
   * shape is `fanout`, not `pts`. */
  dragKind: "corner" | "via";
  /** Not echoed by `DragPreview` -- captured once from the board at
   * `drag_start` and kept across every subsequent preview, same as
   * `RouteDrawState.net`. */
  net: string | null;
  layer: string;
  /** Corner drag only (the dragged track's own width); 0 for a via drag. */
  width: number;
  /** Via drag only -- the via's own diameter, for drawing it at its live
   * position (DragPreview carries no size of its own, see that type's doc). */
  viaDiameter?: number;
  pts: [number, number][];
  colliding?: boolean;
  displaced?: { layer: string; pts: [number, number][] }[];
  displacedVias?: { source_via: string; x: number; y: number }[];
  /** Via drag only: the via's own directly-attached tracks, already
   * stretched to follow it live -- see `DragPreview.fanout`'s own doc. */
  fanout?: { layer: string; width: number; pts: [number, number][] }[];
}

/** The `DrawState` "drag" patch for a successful `DragPreview` reply,
 * keeping whatever session-local fields (`dragKind`, `net`, `layer`,
 * `width`, `viaDiameter`) the server doesn't echo back -- same shape as
 * `routeTool.ts`'s `drawStateFromPreview`. */
export function dragStateFromPreview(current: DragDrawState, preview: DragPreview): DragDrawState {
  return {
    ...current,
    pts: preview.pts,
    colliding: preview.colliding,
    displaced: preview.displaced,
    displacedVias: preview.displaced_vias,
    fanout: preview.fanout,
  };
}
