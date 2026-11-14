// A library symbol's plain text (`SCH_TEXT` on `LAYER_DEVICE`, `(text ..)`) on a placed symbol, as KiCad's canvas draws it (UI-LEFT-3 S3-5).
//
// `SCH_PAINTER::draw( const SCH_SYMBOL* )` (eeschema/sch_painter.cpp) copies the library symbol, puts every item of the copy through the symbol's orientation
// (`OrientAndMirrorSymbolItems( &tempSymbol, aSymbol->GetOrientation() )`, eeschema/symb_transforms_utils.cpp: `n_rots` quarter turns counter-clockwise, then
// `MirrorVertically` for `mirror_x`, `MirrorHorizontally` for `mirror_y`), moves it to the symbol's position, sets the text of a text that has variables to
// `expandLibItemTextVars( text, aSymbol )` (the server's `text_shown`), and draws each text as a symbol's text (`draw( const SCH_TEXT* )` on `LAYER_DEVICE`: at the
// centre of its `GetBoundingBox`, its left or right edge for a left or right justification, centred vertically -- kicad-port/schTextSpecs.ts `deviceTextDrawSpec`).
//
// What the orientation does to a text (eeschema/sch_text.cpp):
//   - `SCH_TEXT::Rotate( center, true )`: the anchor turned about the centre, then `Rotate90( false )` -- a vertical text that turns flips its horizontal
//     justification (`GetTextAngle() == ANGLE_VERTICAL && !aClockwise` -> `FlipHJustify()`), and the angle toggles between horizontal and vertical;
//   - `MirrorHorizontally` / `MirrorVertically` on `LAYER_DEVICE`: `NormalizeJustification( false )` (the anchor moved to the centre of the text's box), the anchor
//     mirrored, the justification along the mirror's axis flipped (left and right of a horizontal text for a left-right mirror, top and bottom of a vertical one;
//     top and bottom of a horizontal text for a top-bottom mirror, left and right of a vertical one), then `NormalizeJustification( true )` with the new
//     justification. The half box added and taken back are of equal size and opposite sign, so the anchor ends where a plain mirror of it puts it.
// So the anchor goes through the symbol's transform like every other point of it, and only the direction and justification need working out here. (The plot,
// `SCH_TEXT::Plot`, turns and flips differently; the derived `.kicad_sch` is written for that, `eda_model::sym_textbox::plotted_text_attrs`.)
// KiCad 25ec2335 is the same. Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { ItemOrientation } from "./symTextBoxPlaced";

export type HAlign = "left" | "center" | "right";
export type VAlign = "top" | "center" | "bottom";

const flipH = (h: HAlign): HAlign => (h === "left" ? "right" : h === "right" ? "left" : h);
const flipV = (v: VAlign): VAlign => (v === "top" ? "bottom" : v === "bottom" ? "top" : v);

/** `OrientAndMirrorSymbolItems` on a symbol's text: whether it runs up the sheet and its justification after `o`'s turns and mirrors (see the module doc). */
export function orientedDeviceText(vertical: boolean, h: HAlign, v: VAlign, o: ItemOrientation): { vertical: boolean; h: HAlign; v: VAlign } {
  let vert = vertical;
  let hh = h;
  let vv = v;
  for (let i = 0; i < o.nRots; i++) {
    // `Rotate90( false )`
    if (vert) hh = flipH(hh);
    vert = !vert;
  }
  // `MirrorVertically` (mirror_x): a horizontal text's top and bottom, a vertical one's left and right
  if (o.mirrorX) {
    if (!vert) vv = flipV(vv);
    else hh = flipH(hh);
  }
  // `MirrorHorizontally` (mirror_y): a horizontal text's left and right, a vertical one's top and bottom
  if (o.mirrorY) {
    if (!vert) hh = flipH(hh);
    else vv = flipV(vv);
  }
  return { vertical: vert, h: hh, v: vv };
}
