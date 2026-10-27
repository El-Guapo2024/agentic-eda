// Pure glue between the backend's interactive router (crates/pns, driven
// through crates/cli/src/route_api.rs) and this app's `DrawState` --
// turning a `RoutePreview` reply into the "route" DrawState patch
// Canvas.tsx's painter already knows how to draw, and a small guard
// against an out-of-order async reply (a slow response to an old mouse
// position landing after a newer one already updated the screen)
// clobbering the display. No network calls live here -- see
// api/client.ts's `route*` functions and Canvas.tsx's own use of them,
// which is what actually needs a browser to exercise; this module is the
// part that's plain, synchronous, unit-testable logic.
//
// router_tool.cpp's interaction loop this ports (the state machine, not
// the PNS algorithms themselves -- those are crates/pns):
//   X              -> start a route from whatever's under the cursor
//   mouse move     -> live preview (walkaround/shove/mark-obstacles
//                     already resolved server-side)
//   click          -> fix the current head as a permanent leg
//   double-click/
//   End/Enter      -> finish (same commit as a click that reaches a
//                     same-net anchor, just forced)
//   Esc            -> cancel, nothing committed
//   Backspace      -> undo the last fixed leg
//   V              -> arm "drop a via here, continue on the other layer"
//   /              -> flip posture
//   W              -> cycle track width
import type { RoutePreview } from "../api/types";
import type { RouteSizesInfo } from "./routerSizes";

// Mirrors `state/store.tsx`'s `DrawState`'s "route" variant exactly --
// duplicated rather than imported so this module stays on the
// dependency-free side of the line every other kicad-port module is
// already on (tsconfig.test.json's own doc comment: no React/DOM/JSX in
// the import graph). store.tsx is the source of truth for the real shape;
// keep this in sync with it by hand.
export interface RouteDrawState {
  kind: "route";
  net: string;
  layer: string;
  width: number;
  pts: [number, number][];
  colliding?: boolean;
  runs?: { layer: string; pts: [number, number][] }[];
  via?: { x: number; y: number; diameter: number; drill: number } | null;
  snappedEnd?: [number, number] | null;
  displaced?: { layer: string; pts: [number, number][] }[];
  /** `shove` mode only: other vias the live head would push aside. */
  displacedVias?: { source_via: string; x: number; y: number }[];
  placingVia?: boolean;
  pendingViaLayer?: string;
  /** The id of the pad, via or track segment the route started on (`m_startItem`): the route does not end on it, so the cursor within a grid of it does not snap back onto it. */
  startItem?: string;
  /** The active layer when the route started (`ROUTER_TOOL::prepareInteractive`: `m_originalActiveLayer`, then `SetActiveLayer( getStartLayer( .. ) )`):
   * the active layer follows the route while it runs (`syncRouterAndFrameLayer`) and goes back to this one when it ends (`finishInteractive`), R6-R6. */
  originalActiveLayer?: string;
  /** What the message panel shows of the route's sizes (the start reply's `sizes`; kicad-port/routerSizes.ts). */
  sizes?: RouteSizesInfo;
  /** `m_TempOverrideTrackWidth`: a width was chosen while routing (W, the width menu), so the route no longer keeps the width it inherited. */
  widthOverride?: boolean;
}

/** The `DrawState` "route" patch for a successful `RoutePreview` reply,
 * keeping whatever session-local fields (`net`, `placingVia`,
 * `pendingViaLayer`) the server doesn't echo back. */
export function drawStateFromPreview(current: RouteDrawState, preview: RoutePreview): RouteDrawState {
  // The reply to a move that chose a width (W, the width menu) says what was routed: the board's minimum when the choice was below it (R6-R2,
  // `ImportSizes`' `std::max( bds.m_TrackMinWidth, GetCurrentTrackWidth() )`). Any other reply keeps the width the studio holds.
  const raised = preview.width_source === "board minimum track width";
  return {
    ...current,
    net: preview.net ?? current.net,
    layer: preview.layer,
    width: preview.width_source != null && preview.width != null ? preview.width : current.width,
    sizes: raised && current.sizes ? { ...current.sizes, width_source: preview.width_source! } : current.sizes,
    pts: preview.head,
    colliding: preview.colliding,
    runs: preview.runs,
    via: preview.via,
    snappedEnd: preview.snapped_end,
    displaced: preview.displaced,
    displacedVias: preview.displaced_vias,
  };
}

/**
 * A monotonically increasing request token so an async caller can tell
 * whether its reply is still the most recent one asked for. Usage:
 * ```
 * const guard = createRequestGuard();
 * const token = guard.next();
 * const reply = await routeMove(x, y);
 * if (guard.isCurrent(token)) applyReply(reply); // else: a newer move already superseded this one
 * ```
 */
export function createRequestGuard() {
  let latest = 0;
  return {
    next(): number {
      latest += 1;
      return latest;
    },
    isCurrent(token: number): boolean {
      return token === latest;
    },
  };
}

/** Plain time-based throttle gate: `shouldSend(now)` returns `true` at
 * most once per `intervalMs`, and always `true` the first time. Doesn't
 * itself schedule a trailing call -- callers that want "also send the very
 * last position once movement stops" (recommended, so the preview settles
 * exactly where the cursor is) pair this with their own short debounce,
 * same as Canvas.tsx's existing `lastPointerScreenRef` pattern for
 * auto-pan. Kept framework-free (no `setTimeout`/`requestAnimationFrame`)
 * so it's trivially unit-testable with an injected clock. */
export function createMoveThrottle(intervalMs: number) {
  let last = -Infinity;
  return {
    shouldSend(now: number): boolean {
      if (now - last >= intervalMs) {
        last = now;
        return true;
      }
      return false;
    },
  };
}
