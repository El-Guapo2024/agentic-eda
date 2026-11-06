// Flip Board View (`pcbnew.Control.flipBoard`, `PCB_DISPLAY_OPTIONS::m_FlipBoardView`, KiCad 8303b2ad): the board seen from its other side. KiCad's painter
// keeps a text readable there unless it belongs to one side of the board (`PCB_PAINTER::draw` of a `PCB_TEXT`, a `PCB_TEXTBOX` and a dimension:
// `if( m_gal->IsFlippedX() && !aItem->IsSideSpecific() ) attrs.m_Mirrored = !attrs.m_Mirrored`), so front silkscreen reads backwards from behind and a
// user layer's note does not.

/** `LSET::SideSpecificMask()`: the technical layers of the front and the back and every copper layer -- what `BOARD_ITEM::IsSideSpecific` asks of the
 * item's layers (a user layer typed front or back is side specific too, which this board state does not tell). Both the file names (`F.SilkS`) and the
 * canonical ones (`F.Silkscreen`) count. */
export function isSideSpecificLayer(layer: string): boolean {
  if (layer.endsWith(".Cu")) return true;
  const m = /^([FB])\.(.+)$/.exec(layer);
  if (!m) return false;
  return ["SilkS", "Silkscreen", "Mask", "Paste", "Adhes", "Adhesive", "CrtYd", "Courtyard", "Fab"].includes(m[2]!);
}

/** Is a text on `layer` drawn with its mirror toggled in the flipped view (`IsFlippedX() && !IsSideSpecific()`)? */
export function keptReadableInFlippedView(viewFlipped: boolean | undefined, layer: string): boolean {
  return !!viewFlipped && !isSideSpecificLayer(layer);
}
