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
 * (`m_highContrastLayers.count( aLayer )`). The board painter draws a pad on each copper layer it flashes on (UI-LEFT-2 PP3) and asks with `th: false`; `th`
 * asks of a through-hole pad drawn once (the Footprint Editor's painter): active when it flashes on the primary copper layer.
 */
export function padContrast(primary: string, pad: { th: boolean; copperLayer: string; flashesOnPrimary: boolean }, hiddenMode: boolean): ContrastLook {
  if (pad.th) return isCopper(primary) && pad.flashesOnPrimary ? "active" : inactive(hiddenMode);
  return pad.copperLayer === primary ? "active" : inactive(hiddenMode);
}

/**
 * A pad's hole wall and hole (`LAYER_PAD_HOLEWALLS`, `LAYER_PAD_PLATEDHOLES`, `LAYER_NON_PLATEDHOLES`): active when the layers made high contrast hold them --
 * `SetHighContrastLayer` adds them with a copper layer primary only -- and a physical layer is primary ("Pad holes are active is any physical layer is active",
 * `LSET::PhysicalLayersMask().test( primary )`): with a copper layer primary. (A technical layer primary leaves only itself high contrast, so a hole is dimmed.)
 */
export function padHoleContrast(primary: string, hiddenMode: boolean): ContrastLook {
  return isCopper(primary) && isPhysicalLayer(primary) ? "active" : inactive(hiddenMode);
}

/**
 * A via's copper on copper layer `layer` (`VIA_COPPER_LAYER_FOR( layer )`, which `GetColor` maps back to the copper layer): active when that layer is the
 * primary one (`m_highContrastLayers.count( aLayer )`). (`PCB_VIA::ViewGetLayers` puts a via on its copper layers, its holes and its net names; the via-type
 * layers `LAYER_VIA_THROUGH` .. are not among them, so `GetColor`'s cases of those do not apply to it.)
 */
export function viaCopperContrast(primary: string, layer: string, hiddenMode: boolean): ContrastLook {
  return layer === primary ? "active" : inactive(hiddenMode);
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
 * A via's hole and hole wall (`LAYER_VIA_HOLES`, `LAYER_VIA_HOLEWALLS`): high contrast only with a copper layer primary (`SetHighContrastLayer`), and then a
 * through via's is active ("A through via's hole is active if any physical layer is active"), a blind, buried or micro via's when it crosses the primary layer.
 */
export function viaHoleContrast(primary: string, via: ViaLike, copper: readonly string[], hiddenMode: boolean): ContrastLook {
  if (!isCopper(primary)) return inactive(hiddenMode);
  if (via.type) return viaSpans(via, primary, copper) ? "active" : inactive(hiddenMode);
  return "active";
}
