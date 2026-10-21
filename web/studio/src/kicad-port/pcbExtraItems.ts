// The board items GAPS.md item 24 (package P16) added -- text boxes today; reference images, points and barcodes as their steps land -- seen as one family by the code that
// walks every kind of item (what an id names, where `GetPosition()` puts it, its bounding box, which layer it is on, the verb that deletes it). The hubs that dispatch on the kind
// of an item (`itemKind`, `itemBounds`, the selection, the delete tool, the move tool's carry) ask this file for the kinds they do not know, so each holds a line for all of them.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardTextBox } from "../api/pcbItemTypes";
import type { BoardState, Cmd } from "../api/types";
import { textBoxBounds, textBoxHitDistance } from "./pcbTextBox";

/** The kinds of item this file knows. */
export type ExtraKind = "textbox";

/** Every kind of the family, for the tables that list the kinds an item can be (what a plain drag of a clicked item picks up, ...). */
export const EXTRA_KINDS: readonly ExtraKind[] = ["textbox"];

export type Box = [number, number, number, number];

/** One item of the family, whatever its kind: what the generic code needs of it. */
export interface ExtraItem {
  kind: ExtraKind;
  id: string;
  layer: string;
  /** `BOARD_ITEM::GetPosition()`. */
  position: [number, number];
  /** `GetBoundingBox()`, border and ink included. */
  bounds: Box;
  /** The state record, for the code that knows the kind. */
  textBox?: BoardTextBox;
}

const ofTextBox = (b: BoardTextBox): ExtraItem => ({
  kind: "textbox",
  id: b.id,
  layer: b.layer,
  position: b.pts.length > 0 ? [b.pts[0]![0], b.pts[0]![1]] : [b.start[0], b.start[1]],
  bounds: textBoxBounds(b),
  textBox: b,
});

/** Every item of the family the board has, in the order the state lists them. */
export function extraItems(board: Pick<BoardState, "drawings"> | null | undefined): ExtraItem[] {
  return (board?.drawings?.text_boxes ?? []).map(ofTextBox);
}

/** The item of the family `id` names, or undefined. */
export function extraItemById(board: Pick<BoardState, "drawings"> | null | undefined, id: string): ExtraItem | undefined {
  const b = board?.drawings?.text_boxes?.find((t) => t.id === id);
  return b ? ofTextBox(b) : undefined;
}

/** Every id of the family (for the sets that tell new items from old ones). */
export function extraItemIds(board: Pick<BoardState, "drawings"> | null | undefined): string[] {
  return (board?.drawings?.text_boxes ?? []).map((b) => b.id);
}

/** How far the point `(x, y)` is from the item, for a click's "sloppiness": 0 when it hits (`HitTest`). */
export function extraHitDistance(item: ExtraItem, x: number, y: number): number {
  if (item.textBox) return textBoxHitDistance(item.textBox, x, y);
  return Infinity;
}

/** The verb that deletes the item (`EDIT_TOOL::DeleteItems`). */
export function extraDeleteCmd(item: Pick<ExtraItem, "kind" | "id">): Cmd {
  return { op: "pcb_item", verb: "delete_text_box", id: item.id };
}

/** The row of the disambiguation menu for the item (`GetItemDescription`: "PCB text box '%s' on %s"). */
export function extraItemLabel(board: Pick<BoardState, "drawings"> | null | undefined, id: string): string {
  const item = extraItemById(board, id);
  if (item?.textBox) {
    const text = item.textBox.shown ?? item.textBox.text;
    return `Text box "${text.length > 24 ? `${text.slice(0, 23)}...` : text}" [${item.layer}]`;
  }
  return id;
}
