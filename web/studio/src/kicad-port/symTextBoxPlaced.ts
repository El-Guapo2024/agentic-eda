// The text of a library symbol's text box (`SCH_TEXTBOX` on `LAYER_DEVICE`, `(text_box ..)`) on a placed symbol, as KiCad's canvas draws it.
//
// `SCH_PAINTER::draw( const SCH_SYMBOL* )` (eeschema/sch_painter.cpp) copies the library symbol, puts every item of the copy through the symbol's orientation
// (`OrientAndMirrorSymbolItems( &tempSymbol, aSymbol->GetOrientation() )`, eeschema/symb_transforms_utils.cpp) and moves it to the symbol's position, sets the text of a
// box that has variables to `expandLibItemTextVars( text, aSymbol )` (the server's `text_box_shown`), and draws the box as a sheet's (`draw( const SCH_TEXTBOX* )`: the text
// at `GetDrawPos()`, turned by `GetDrawRotation()`, wrapped to the box). The corners go through the symbol's transform like every other point of it; here is what the
// orientation does to the text: each quarter turn (`SCH_TEXTBOX::Rotate`) runs it the other way, `MirrorVertically` swaps a left and a right justification of a vertical
// text and `MirrorHorizontally` those of a horizontal one ("Text is NOT really mirrored; it just has its justification flipped"), and nothing changes the vertical
// justification. (kicad-cli's plot, `SCH_TEXTBOX::Plot`, keeps the justification of a mirrored box: the derived `.kicad_sch` is written for that, `eda_model::sym_textbox`.)
// KiCad 25ec2335 is the same. Pure: no React, no DOM (compiled by `npm run test:unit`).
import { symbolTransformMatrix, type Matrix } from "../components/schematic/transform";

/** One entry of `symbols_orientations_list` (`ORIENT_MIRROR`): the quarter turns counter-clockwise an item gets, then a top-bottom (`mirror_x`) and a left-right (`mirror_y`) mirror. */
export interface ItemOrientation {
  nRots: 0 | 1 | 2 | 3;
  mirrorX: boolean;
  mirrorY: boolean;
}

/**
 * `SCH_SYMBOL::GetOrientation`'s `rotate_values`, in its order, each as the (turn, mirror) whose `SetOrientation` matrix `symbolTransformMatrix` has and with its
 * `symbols_orientations_list` entry. `SYM_MIRROR_Y` and `SYM_MIRROR_Y + SYM_ORIENT_0` are one transform and one entry.
 */
const ROTATE_VALUES: ReadonlyArray<{ rot: number; mirror: "x" | "y" | null; o: ItemOrientation }> = [
  { rot: 0, mirror: null, o: { nRots: 0, mirrorX: false, mirrorY: false } },
  { rot: 90, mirror: null, o: { nRots: 1, mirrorX: false, mirrorY: false } },
  { rot: 180, mirror: null, o: { nRots: 2, mirrorX: false, mirrorY: false } },
  { rot: 270, mirror: null, o: { nRots: 3, mirrorX: false, mirrorY: false } },
  { rot: 0, mirror: "x", o: { nRots: 0, mirrorX: true, mirrorY: false } },
  { rot: 90, mirror: "x", o: { nRots: 1, mirrorX: true, mirrorY: false } },
  { rot: 270, mirror: "x", o: { nRots: 3, mirrorX: true, mirrorY: false } },
  { rot: 0, mirror: "y", o: { nRots: 0, mirrorX: false, mirrorY: true } },
  { rot: 90, mirror: "y", o: { nRots: 1, mirrorX: false, mirrorY: true } },
  { rot: 180, mirror: "y", o: { nRots: 2, mirrorX: false, mirrorY: true } },
  { rot: 270, mirror: "y", o: { nRots: 3, mirrorX: false, mirrorY: true } },
];

const sameMatrix = (a: Matrix, b: Matrix) => a.x1 === b.x1 && a.y1 === b.y1 && a.x2 === b.x2 && a.y2 === b.y2;

/**
 * The orientation `OrientAndMirrorSymbolItems` puts the items of a symbol placed with the transform `m` through: the first of `GetOrientation`'s list whose matrix is `m`.
 * Eight transforms, twelve names: a mirrored one is found as "mirror X" with 0, 90 or 270 before "mirror Y", so of the mirrored ones only the unturned left-right mirror
 * is done as a left-right one.
 */
export function itemOrientationOf(m: Matrix): ItemOrientation {
  for (const v of ROTATE_VALUES) if (sameMatrix(symbolTransformMatrix(v.rot, v.mirror), m)) return v.o;
  return ROTATE_VALUES[0]!.o;
}

export type HAlign = "left" | "center" | "right";

/**
 * `OrientAndMirrorSymbolItems` on a text box's text: whether it runs up the box and its horizontal justification once the item had `o`'s turns (`SCH_TEXTBOX::Rotate`:
 * `SetTextAngle( GetTextAngle() == ANGLE_VERTICAL ? ANGLE_HORIZONTAL : ANGLE_VERTICAL )`) and mirrors (`MirrorVertically` for a vertical text, `MirrorHorizontally` for a
 * horizontal one: left and right swap, centre stays).
 */
export function orientedText(vertical: boolean, hAlign: HAlign, o: ItemOrientation): { vertical: boolean; hAlign: HAlign } {
  const v = o.nRots % 2 === 1 ? !vertical : vertical;
  const flip = (h: HAlign): HAlign => (h === "left" ? "right" : h === "right" ? "left" : h);
  let h = hAlign;
  if (o.mirrorX && v) h = flip(h);
  if (o.mirrorY && !v) h = flip(h);
  return { vertical: v, hAlign: h };
}

/** Whether a text box's `angle_deg` (0 or 90) runs its text up the box (`GetTextAngle().IsVertical()`). */
export function isVerticalAngle(angleDeg: number): boolean {
  return ((Math.round(angleDeg) % 180) + 180) % 180 === 90;
}
