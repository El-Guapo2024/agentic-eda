// A via's diameter on each copper layer (UI-LEFT-2 PP12): `PCB_VIA::GetWidth( aLayer )` (pcbnew/pcb_track.cpp at KiCad 8303b2ad) is `PADSTACK::Size( aLayer ).x`
// (pcbnew/padstack.cpp), the layer's own in a front/inner/back or custom padstack (`(padstack (mode ..) (layer .. (size ..)) ..)`, the via's `props.padstack`),
// else the via's one diameter (`d`, the front's). And how `PCB_PAINTER::draw( const PCB_VIA*, aLayer )` puts those diameters on the canvas: one ring on each
// copper layer of the via's span that it flashes on (`LAYER_VIA_COPPER_START + layer`, `ViewGetLayers`), each at that layer's diameter, stacked as the
// layers are (`GAL_LAYER_ORDER`: a via's copper sits with its copper layer, F.Cu over the inner layers over B.Cu, and `SetTopLayer` puts the active copper
// layer's over all of them).
import type { Via } from "../api/types";

type ViaSizes = Pick<Via, "d" | "props">;

/** `PCB_VIA::GetWidth( aLayer )`: the layer's own diameter in a padstack that is not a normal one (`Inner` for each inner layer of a front/inner/back stack;
 * a layer a custom stack does not name falls back to the front's, `PADSTACK::CopperLayer`), else the via's. */
export function viaWidthOn(via: ViaSizes, layer: string): number {
  const stack = via.props?.padstack;
  if (!stack || layer === "F.Cu") return via.d;
  const key = stack.mode === "front_inner_back" && layer !== "B.Cu" ? "Inner" : layer;
  return stack.sizes[key] ?? via.d;
}

/** The widest of the via's diameters: `GetEffectiveShape( UNDEFINED_LAYER )` (`std::max( width, GetWidth( layer ) )` over its unique layers), and the router's
 * preview of a via (`ROUTER_PREVIEW_ITEM`: the shape of the layer with the `largestDiameter`). */
export function viaMaxDiameter(via: ViaSizes): number {
  const stack = via.props?.padstack;
  return stack ? Math.max(via.d, ...Object.values(stack.sizes)) : via.d;
}

/** Whether the via's padstack gives some layer a diameter of its own (a normal stack, or one whose layers all match the front, draws as one disc). */
export function hasLayerDiameters(via: ViaSizes): boolean {
  const stack = via.props?.padstack;
  return !!stack && Object.values(stack.sizes).some((d) => d !== via.d);
}

/**
 * The via's copper rings as the canvas stacks them, bottom first: each copper layer of its span (`from`..`to`, the board's copper `copper` front to back) that
 * it flashes on (`flashed`, when it removes unconnected layers: `PCB_VIA::FlashLayer`) and that is shown (`visible`), at the layer's own diameter
 * ([`viaWidthOn`]), in the copper layers' order -- B.Cu at the bottom, then the inner layers from the last to In1.Cu, then F.Cu (`GAL_LAYER_ORDER`) -- with
 * the active copper layer moved to the top (`PCB_DRAW_PANEL_GAL::SetTopLayer`: `VIA_COPPER_LAYER_FOR( aLayer )`).
 */
export function viaLayerStack(
  via: ViaSizes & Pick<Via, "from" | "to" | "flashed">,
  copper: readonly string[],
  activeLayer: string | null | undefined,
  visible: (layer: string) => boolean
): { layer: string; d: number }[] {
  const a = copper.indexOf(via.from);
  const b = copper.indexOf(via.to);
  const span = a < 0 || b < 0 ? [...copper] : copper.slice(Math.min(a, b), Math.max(a, b) + 1);
  const drawn = span.filter((l) => (via.flashed ? via.flashed.includes(l) : true) && visible(l));
  const order = [...drawn].reverse();
  const at = activeLayer ? order.indexOf(activeLayer) : -1;
  if (at >= 0) order.push(...order.splice(at, 1));
  return order.map((layer) => ({ layer, d: viaWidthOn(via, layer) }));
}
