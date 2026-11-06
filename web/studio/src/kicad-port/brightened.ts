// The items the clarification menu's row under the pointer stands for, brightened (common/tool/selection_tool.cpp `SELECTION_TOOL::doSelectionMenu` at
// 8303b2ad: `TA_CHOICE_MENU_UPDATE` -> `highlight( current, BRIGHTENED, &highlightGroup )`, the group added to the view), UI-LEFT-2 PP19:
//
//   PCB_RENDER_SETTINGS::GetColor     "Selection disambiguation": `if( aItem->IsBrightened() ) return color.Brightened( m_selectFactor ).WithAlpha( 0.8 );`
//                                     -- the layer's colour, brightened by `m_selectFactor` (0.5, `RENDER_SETTINGS::RENDER_SETTINGS`), at 0.8 alpha, before the
//                                     high-contrast and net-highlight branches
//   VIEW_GROUP highlightGroup         the item drawn again in the overlay, over the board
//
// So the board painter draws the items once more on top, through a colour scope that brightens whatever colour they set (components/canvas/painter.ts). This
// module picks out what to draw: the board with only those items, a pad as its footprint with that pad alone, a footprint's own text as its footprint with that
// text alone. Pure: no store, no DOM.
import type { BoardState, FieldInfo, Part } from "../api/types";
import { brightened, cssOf, parseColor, withAlpha } from "./color4d";
import { splitCarried } from "./pcbCarry";
import { padIds, padParent } from "./pcbItems";

/** `RENDER_SETTINGS::m_selectFactor`. */
export const SELECT_FACTOR = 0.5;

/** `color.Brightened( m_selectFactor ).WithAlpha( 0.8 )` of a CSS colour; a colour that is not one (a gradient's name) is left as it is. */
export function brightenedCss(css: string): string {
  const c = parseColor(css);
  return c ? cssOf(withAlpha(brightened(c, SELECT_FACTOR), 0.8)) : css;
}

/** A footprint reduced to some of its pads or texts: no drawings, no fields, its courtyard a point (drawn as nothing). */
function reduced(part: Part, keep: { pads?: Set<string>; texts?: Set<string> }): Part {
  const ids = padIds(part);
  const pads = (part.pads ?? []).filter((_, i) => keep.pads?.has(ids[i]!));
  const at = pads[0] ? [pads[0].x, pads[0].y] : (part.at ?? [0, 0]);
  return {
    ...part,
    pads,
    texts: (part.texts ?? []).filter((t) => keep.texts?.has(t.id)),
    graphics: [],
    fields: [],
    courtyard: [at[0]!, at[1]!, at[0]!, at[1]!],
  };
}

/** The board holding only the items `ids` names (and the fields named on their own), for the brightened pass. */
export function brightenedBoard(board: BoardState, ids: readonly string[]): { board: BoardState; fields: FieldInfo[] } {
  const pads = new Map<string, Set<string>>();
  const texts = new Map<string, Set<string>>();
  const rest: string[] = [];
  for (const id of ids) {
    const parent = padParent(board, id);
    if (parent && parent !== id) {
      (pads.get(parent) ?? pads.set(parent, new Set()).get(parent)!).add(id);
      continue;
    }
    const at = id.lastIndexOf("~t");
    if (at > 0 && board.parts.some((p) => p.ref === id.slice(0, at) && p.texts?.some((t) => t.id === id))) {
      const ref = id.slice(0, at);
      (texts.get(ref) ?? texts.set(ref, new Set()).get(ref)!).add(id);
      continue;
    }
    rest.push(id);
  }
  const lit = splitCarried(board, rest);
  const extra: Part[] = [];
  for (const part of board.parts) {
    if (!part.placed) continue;
    const p = pads.get(part.ref);
    const t = texts.get(part.ref);
    if (p || t) extra.push(reduced(part, { pads: p, texts: t }));
  }
  return { board: { ...lit.moving, parts: [...lit.moving.parts, ...extra] }, fields: lit.fields };
}
