// R, Shift+R, F and E on the text Draw Text carries (`DRAWING_TOOL::PlaceText`, pcbnew/tools/drawing_tool.cpp, KiCad 8303b2ad; round 6 A6-6). The text is the
// selection while it is carried (`m_toolMgr->RunAction<EDA_ITEM*>( ACTIONS::selectItem, text )` after its dialog), and the tool passes every event it does
// not take on (`evt->SetPassEvent()`), so Rotate and Flip reach `EDIT_TOOL` with the carried text selected; `PCB_ACTIONS::properties` the tool takes itself and
// opens the text's dialog (`frame()->OnEditItemRequest( text )`). Pure: the action runner applies them to the draw state.
//
// `EDIT_TOOL::Rotate` and `Flip` act about the selection's reference point; `updateModificationPoint` makes that the text's own position for one item (the
// text is not `IsMoving()` and the tool is not dragging), so the text turns and flips in place, under the cursor. KiCad's tools also stage the text in a
// commit (`commit->Modify( item )`) and push it as an undo step "Rotate" / "Change Side / Flip" though the text is not on the board; the studio's carried text
// is not on the board either, and nothing of it is undone.
import type { CmdText } from "../api/types";
import type { FlipDirection } from "./pcbTransform";

/** Millidegrees in [0, 360000) (`EDA_ANGLE::Normalize`). */
function normalize(milli: number): number {
  const m = milli % 360_000;
  return m < 0 ? m + 360_000 : m;
}

/** `PCB_TEXT::Rotate( GetPosition(), angle )`: the text turns by `degCcw` (counter-clockwise positive, KiCad's) about its own position, which stays. */
export function rotateCarriedText(text: CmdText, degCcw: number): CmdText {
  return { ...text, angle: normalize(text.angle + Math.round(degCcw * 1000)) };
}

/**
 * `BOARD::FlipLayer` (`::FlipLayer( aLayer, GetCopperLayerCount() )`, common/layer_id.cpp): a front layer and its back one swap; an inner copper layer In(k)
 * becomes In(n - 1 - k) on a board of `copperCount` layers (n >= 4); any other layer stays.
 */
export function flipTextLayer(layer: string, copperCount: number): string {
  if (layer.startsWith("F.")) return `B.${layer.slice(2)}`;
  if (layer.startsWith("B.")) return `F.${layer.slice(2)}`;
  const inner = /^In(\d+)\.Cu$/.exec(layer);
  if (inner && copperCount >= 4) {
    const max = copperCount - 3;
    const flipped = Math.max(0, Math.min(max, copperCount - 3 - (Number(inner[1]) - 1)));
    return `In${flipped + 1}.Cu`;
  }
  return layer;
}

/** `BOARD_ITEM::IsSideSpecific` of a text: on a copper layer or a front or back technical layer (`LSET::SideSpecificMask()`). */
function sideSpecific(layer: string): boolean {
  return /\.Cu$/.test(layer) || /^[FB]\.(SilkS|Silkscreen|Mask|Paste|Adhes|Adhesive|CrtYd|Courtyard|Fab)$/.test(layer);
}

/**
 * `PCB_TEXT::Flip( GetPosition(), direction )`: left-right negates the angle, top-bottom makes it 180 minus it (the position, mirrored about itself, stays);
 * the text goes to the flipped layer (`BOARD::FlipLayer`), and a side-specific one toggles its mirror (`if( IsSideSpecific() ) SetMirrored( !IsMirrored() )`,
 * asked after the layer changed). The angle is kept in [0, 360) degrees, the board's text angle (KiCad's -90 is the same turn as its 270).
 */
export function flipCarriedText(text: CmdText, direction: FlipDirection, copperCount: number): CmdText {
  const angle = normalize(direction === "left_right" ? -text.angle : 180_000 - text.angle);
  const layer = flipTextLayer(text.layer, copperCount);
  return { ...text, angle, layer, mirror: sideSpecific(layer) ? !text.mirror : text.mirror };
}
