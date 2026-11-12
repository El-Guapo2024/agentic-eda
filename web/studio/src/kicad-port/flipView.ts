// Flip Board View (`pcbnew.Control.flipBoard`, `PCB_DISPLAY_OPTIONS::m_FlipBoardView`, KiCad 8303b2ad): the board seen from its other side. KiCad's painter
// keeps a text readable there unless it belongs to one side of the board (`PCB_PAINTER::draw` of a `PCB_TEXT`, a `PCB_TEXTBOX` and a dimension:
// `if( m_gal->IsFlippedX() && !aItem->IsSideSpecific() ) attrs.m_Mirrored = !attrs.m_Mirrored`), so front silkscreen reads backwards from behind and a
// user layer's note does not.

/** `LSET::SideSpecificMask()`: the technical layers of the front and the back and every copper layer -- what `BOARD_ITEM::IsSideSpecific` asks first of the
 * item's layers. Both the file names (`F.SilkS`) and the canonical ones (`F.Silkscreen`) count. */
export function isSideSpecificLayer(layer: string): boolean {
  if (layer.endsWith(".Cu")) return true;
  const m = /^([FB])\.(.+)$/.exec(layer);
  if (!m) return false;
  return ["SilkS", "Silkscreen", "Mask", "Paste", "Adhes", "Adhesive", "CrtYd", "Courtyard", "Fab"].includes(m[2]!);
}

/** `BOARD_ITEM::IsSideSpecific` (KiCad 25ec2335): a layer of `SideSpecificMask()`, or a layer the board types front or back (`board->GetLayerType( m_layer )` is
 * `LT_FRONT` or `LT_BACK`: a user layer Board Setup > Board Editor Layers made "Off-board, front" or "Off-board, back"). `types` is the board's
 * `layer_setup.types`, by the file's layer names. */
export function isSideSpecific(layer: string, types?: Readonly<Record<string, string>> | null): boolean {
  if (isSideSpecificLayer(layer)) return true;
  const t = types?.[layer];
  return t === "front" || t === "back";
}

/** Is a text on `layer` drawn with its mirror toggled in the flipped view (`IsFlippedX() && !IsSideSpecific()`)? */
export function keptReadableInFlippedView(viewFlipped: boolean | undefined, layer: string, types?: Readonly<Record<string, string>> | null): boolean {
  return !!viewFlipped && !isSideSpecific(layer, types);
}
