// High Contrast Mode's look of an item off the active layer (`pcbnew.Control.highContrastMode`, the Layers tab's "Inactive layers" Normal / Dim / Hide): ported
// from `PCB_RENDER_SETTINGS::GetColor`'s high-contrast branch (pcbnew/pcb_painter.cpp, KiCad 8303b2ad) and the layers `PCB_DRAW_PANEL_GAL::SetHighContrastLayer`
// makes high contrast with the active one. An inactive item is dimmed (its colour mixed into the background, kicad-port/color4d.ts `hiContrastDim`) or, in
// HIGH_CONTRAST_MODE::HIDDEN, not drawn (`COLOR4D::CLEAR`); some are not drawn in either mode (`hide`). Pure: the painter asks it per item.

/** How an item is drawn while High Contrast Mode is on. */
export type ContrastLook = "active" | "dim" | "hide";

/** `IsCopperLayer` of a layer name. */
const isCopper = (layer: string): boolean => /\.Cu$/.test(layer);

/** `LSET::PhysicalLayersMask()`: the copper layers and `AllBoardTechMask()` -- the silkscreens, masks, adhesives and pastes of both sides. */
export function isPhysicalLayer(layer: string): boolean {
  return isCopper(layer) || /^[FB]\.(SilkS|Silkscreen|Mask|Adhes|Adhesive|Paste)$/.test(layer);
}

/** An inactive item: dimmed, or not drawn in the Hide mode (`m_ContrastModeDisplay == HIGH_CONTRAST_MODE::HIDDEN`), or not drawn whatever the mode (`hide`). */
const inactive = (hiddenMode: boolean, hide = false): ContrastLook => (hide || hiddenMode ? "hide" : "dim");

/**
 * A pad's copper on `copperLayer` (`PAD_COPPER_LAYER_FOR( layer )`, which `GetColor` maps back to the copper layer): active when that layer is the primary one
 * (`m_highContrastLayers.count( aLayer )`). A surface-mount pad's copper is its side's; a through-hole pad is drawn once in the studio, so it is active when it
 * flashes on the primary copper layer (`FlashLayer( primary )`, `flashesOnPrimary`) -- the primary layer's copper is the one KiCad brings to the top.
 */
export function padContrast(primary: string, pad: { th: boolean; copperLayer: string; flashesOnPrimary: boolean }, hiddenMode: boolean): ContrastLook {
  if (pad.th) return isCopper(primary) && pad.flashesOnPrimary ? "active" : inactive(hiddenMode);
  return pad.copperLayer === primary ? "active" : inactive(hiddenMode);
}

/**
 * A pad's hole wall and hole (`LAYER_PAD_HOLEWALLS`, `LAYER_PAD_PLATEDHOLES`, `LAYER_NON_PLATEDHOLES`): "Pad holes are active is any physical layer is active"
 * (`LSET::PhysicalLayersMask().test( primary )`).
 */
export function padHoleContrast(primary: string, hiddenMode: boolean): ContrastLook {
  return isPhysicalLayer(primary) ? "active" : inactive(hiddenMode);
}

/** The via `GetColor` is asked about, as the board state has it. */
export interface ViaLike {
  /** `VIATYPE` when not a through via. */
  type?: "blind" | "buried" | "micro" | null;
  /** The copper layers it spans. */
  from: string;
  to: string;
  /** With `remove_unused_layers`: the copper layers that keep its ring (`FlashLayer`); absent when every one does. */
  flashed?: readonly string[] | null;
}

/** Does `via` span copper layer `layer` (`GetLayerSet().test( layer )`), by the board's copper order `copper` (front to back)? */
function viaSpans(via: ViaLike, layer: string, copper: readonly string[]): boolean {
  const a = copper.indexOf(via.from);
  const b = copper.indexOf(via.to);
  const i = copper.indexOf(layer);
  if (a < 0 || b < 0 || i < 0) return via.from === layer || via.to === layer;
  return i >= Math.min(a, b) && i <= Math.max(a, b);
}

/**
 * A via's ring (`LAYER_VIA_THROUGH`, `LAYER_VIA_BLIND`, `LAYER_VIA_BURIED`, `LAYER_VIA_MICROVIA`):
 * - a through via is active when the primary layer is copper and it flashes there; on a copper layer it does not flash on it is not drawn at all
 *   (`if( !via->FlashLayer( primary ) ) { isActive = false; if( IsCopperLayer( primary ) ) hide = true; }`), and with a technical layer primary it is dimmed;
 * - a blind, buried or micro via is active when it crosses the primary copper layer, and not drawn at all when it does not ("Target graphic is active if the
 *   via crosses the primary layer": `if( via->GetLayerSet().test( primary ) == 0 ) { isActive = false; hide = true; }`) -- its layer list is in the high
 *   contrast set only with a copper layer primary.
 */
export function viaContrast(primary: string, via: ViaLike, copper: readonly string[], hiddenMode: boolean): ContrastLook {
  if (via.type) return isCopper(primary) && viaSpans(via, primary, copper) ? "active" : "hide";
  if (!isCopper(primary)) return inactive(hiddenMode);
  const flashes = via.flashed ? via.flashed.includes(primary) : true;
  return flashes ? "active" : "hide";
}

/** The board painter's copper buckets (components/canvas/layers.ts `BUCKET_ORDER`), back to front; every inner layer is drawn in `in2_cu`'s. */
export const COPPER_BUCKETS: readonly string[] = ["b_cu", "in2_cu", "in1_cu", "f_cu"];

/**
 * The painter's bucket order with the active copper layer's bucket moved over the other copper ones: `PCB_DRAW_PANEL_GAL::SetTopLayer`, which
 * `PCB_EDIT_FRAME::SetActiveLayer` calls through `SetHighContrastLayer` whatever the contrast mode (`m_view->SetTopLayer( aLayer )`, the layer's zones, pads and
 * vias with it). Unchanged for a technical layer or none.
 */
export function activeCopperOnTop(order: readonly string[], activeLayer: string | null | undefined): string[] {
  if (!activeLayer || !isCopper(activeLayer)) return [...order];
  const key = activeLayer === "F.Cu" ? "f_cu" : activeLayer === "B.Cu" ? "b_cu" : "in2_cu";
  const at = order.indexOf(key);
  const last = order.reduce((acc, k, i) => (COPPER_BUCKETS.includes(k) ? i : acc), -1);
  if (at < 0 || at >= last) return [...order];
  const out = [...order];
  out.splice(at, 1);
  out.splice(last, 0, key);
  return out;
}
