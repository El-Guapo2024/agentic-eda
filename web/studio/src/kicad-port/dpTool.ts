// Pure glue between the backend's differential-pair router session
// (crates/pns::dp_placer, KiCad's DIFF_PAIR_PLACER, driven through
// crates/cli/src/route_api.rs's dp_{start,move,fix,via,finish} endpoints)
// and this app's `DrawState` -- the `6` counterpart of routeTool.ts's
// single-track glue. Same split: this module turns a `DiffPairPreview`
// reply into the "diffpair" DrawState patch Canvas.tsx's painter already
// knows how to draw, and stays on the dependency-free side of the line
// every other kicad-port module is already on.
//
// router_tool.cpp's own DIFF_PAIR_PLACER loop this ports (the state
// machine, not the geometry -- that's crates/pns::dp_placer):
//   6 (nothing selected) -> start from whatever pad/via/track-end is
//                           under the cursor, on a net with a recognized
//                           differential-pair suffix (components/canvas/
//                           diffPairRouting.ts); the partner is the nearest
//                           item of the complementary net
//   mouse move            -> live preview of both lines at once, coupled:
//                           walked around what is in the way, or pushed
//                           through it (Shove), per the router mode
//   V                     -> the head ends in a pair of vias a via gap
//                           apart; the next click places them and the
//                           route goes on from them on the other layer
//   click                 -> fix the current head as a permanent leg
//   double-click/Enter    -> finish (same commit as a click reaching a
//                            coupled pair of same-net anchors)
//   Esc                   -> cancel, nothing committed
//   Backspace             -> undo the last fixed leg
//   /                     -> flip posture (straight-then-diagonal or the
//                            other way round)
import type { DiffPairPreview } from "../api/types";
import type { RouteSizesInfo } from "./routerSizes";

/** A via of the pair, as the preview carries it (one per net). */
export interface DpDrawVia {
  net: string | null;
  x: number;
  y: number;
  diameter: number;
  fromLayer: string;
  toLayer: string;
}

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
  /** The edge-to-edge gap the pair is routed at. */
  gap?: number;
  ptsA: [number, number][];
  ptsB: [number, number][];
  colliding?: boolean;
  runsA?: { layer: string; pts: [number, number][] }[];
  runsB?: { layer: string; pts: [number, number][] }[];
  snappedEnd?: boolean;
  /** `V` is on: the head ends in `vias`. `pendingViaLayer` is the layer the route goes on to after them. */
  placingVia?: boolean;
  pendingViaLayer?: string;
  vias?: DpDrawVia[];
  fixedVias?: DpDrawVia[];
  /** Shove mode: what the head would push aside. */
  displaced?: { layer: string; pts: [number, number][] }[];
  displacedVias?: { source_via: string; x: number; y: number }[];
  /** The active layer when the route started (`ROUTER_TOOL::prepareInteractive`: `m_originalActiveLayer`, then `SetActiveLayer( getStartLayer( .. ) )`):
   * the active layer follows the route while it runs (`syncRouterAndFrameLayer`) and goes back to this one when it ends (`finishInteractive`), R6-R6. */
  originalActiveLayer?: string;
  /** What the message panel shows of the route's sizes (the start reply's `sizes`; kicad-port/routerSizes.ts). */
  sizes?: RouteSizesInfo;
}

function viaOf(v: NonNullable<DiffPairPreview["vias"]>[number]): DpDrawVia {
  return { net: v.net, x: v.x, y: v.y, diameter: v.diameter, fromLayer: v.from_layer, toLayer: v.to_layer };
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
    gap: preview.gap ?? current.gap,
    ptsA: preview.head_a,
    ptsB: preview.head_b,
    colliding: preview.colliding,
    runsA: preview.runs_a,
    runsB: preview.runs_b,
    snappedEnd: preview.snapped_end,
    placingVia: preview.placing_via ?? false,
    vias: (preview.vias ?? []).map(viaOf),
    fixedVias: (preview.fixed_vias ?? []).map(viaOf),
    displaced: preview.displaced ?? [],
    displacedVias: preview.displaced_vias ?? [],
  };
}

/**
 * `V` while a pair is routed (`ROUTER_TOOL::onViaCommand` -> `handleLayerSwitch`): the pair of vias is armed, with the layer the route goes
 * on to after them (`toLayer`: the other end of the layer pair, see `otherLayerOfPair`); a second `V` takes the vias away again.
 */
export function dpViaRequest(draw: DpDrawState, toLayer: string, size?: { diameter: number; drill: number }): { enabled: boolean; toLayer: string; diameter?: number; drill?: number } {
  // `handleLayerSwitch`: `sizes.SetViaDiameter( bds.GetCurrentViaSize() ); sizes.SetViaDrill( bds.GetCurrentViaDrill() )` unless the net class's vias
  // are in use -- the chosen via list entry or custom size (`routerVia.ts` `chosenViaOf`), as a single track's V (R7-R2). None: the server's rules.
  const enabled = !draw.placingVia;
  return enabled && size ? { enabled, toLayer, diameter: size.diameter, drill: size.drill } : { enabled, toLayer };
}
