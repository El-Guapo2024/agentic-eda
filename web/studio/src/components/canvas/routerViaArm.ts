// Place Through Via's implicit target, the part of `ROUTER_TOOL::handleLayerSwitch( ACT_PlaceThroughVia, true )` that needs the router (pcbnew/router/
// router_tool.cpp at 8303b2ad): "Try to switch to the nearest ratnest item's layer if we have one" -- `m_router->GetNearestRatnestAnchor( otherEnd,
// otherEndLayers, otherEndItem )`, asked of the server (`/api/route/ratsnest_anchor`), then kicad-port/routerVia.ts `planViaCommand` decides. Used by V
// while a route or a pair runs (actions/routerViaActions.ts) and by the via a route or a pair started after V arms (`performRouting`: "If the user
// pressed 'V' before starting to route, enable via placement now", components/canvas/routing.ts, diffPairRouting.ts), UI-LEFT R5.

import { routeRatsnestAnchor } from "../../api/client";
import type { LayerPair } from "../../kicad-port/layerPairs";
import { layerPairMap, planViaCommand, VIA_COMMANDS, type ViaPlan, type ViaRefusal } from "../../kicad-port/routerVia";

/** The router's sizes' layer pairs a route starts with: `prepareInteractive`'s `sizes.AddLayerPair( m_Route_Layer_TOP, m_Route_Layer_BOTTOM )`. */
export function startSizesPairs(pair: LayerPair): Record<string, string> {
  return layerPairMap(pair.a, pair.b);
}

/**
 * Plan Place Through Via (`ACT_PlaceThroughVia`, V) for the route or pair in progress with its head at `at`: the nearest unconnected anchor's first layer
 * (`otherEndLayers.Start()`), the board's layer pair when there is none, and the router's sizes' pairs (`sizesPairs`) for the "same layer as the currently
 * active layer" case. `copper` is the board's copper layers in stack order.
 */
export async function planThroughVia(at: { x: number; y: number }, currentLayer: string, copper: readonly string[], layerPair: LayerPair, sizesPairs: Readonly<Record<string, string>> | undefined): Promise<ViaPlan | ViaRefusal> {
  const anchor = await routeRatsnestAnchor(at.x, at.y).catch(() => null);
  const ratsnestLayer = anchor?.ok && anchor.found && anchor.layers ? anchor.layers[0] : null;
  return planViaCommand({ cmd: VIA_COMMANDS["pcbnew.InteractiveRouter.PlaceVia"]!, currentLayer, copper, layerPair, ratsnestLayer, sizesPairs: sizesPairs ?? startSizesPairs(layerPair) });
}
