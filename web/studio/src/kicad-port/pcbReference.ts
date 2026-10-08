// The arithmetic of the three commands that work from a point the user picks:
//
//   InteractiveMove.moveWithReference   EDIT_TOOL::doMoveSelection (edit_tool_move_fct.cpp): the move is carried by
//                                       the picked point instead of the point nearest the cursor.
//   InteractiveMove.copyWithReference   EDIT_TOOL::copyToClipboard (edit_tool.cpp): the clipboard remembers the picked
//                                       point as the selection's reference, and the paste is anchored on it.
//   PositionRelative.interactiveOffsetTool
//                                       POSITION_RELATIVE_TOOL::InteractiveOffset (position_relative_tool.cpp): two
//                                       clicks give a vector, DIALOG_OFFSET_ITEM edits it, the selection moves by the
//                                       difference.
//
// `moveSelectionBy` is the studio's one-undo-step batch of move commands (the C++ pushes one BOARD_COMMIT).

import type { Cmd } from "../api/types";
import { movableItem, type AnchorBoard } from "./pcbEditActions";

export interface Vec {
  x: number;
  y: number;
}

/**
 * `InteractiveOffset`'s ruler. The first click (the reference point on the item to move) is the ruler's END and
 * stays put; the second click (the point to measure the new offset from) is its ORIGIN. The dialog starts with
 * `offsetVector = End - Origin`.
 */
export function offsetVector(itemPoint: Vec, referencePoint: Vec): Vec {
  return { x: itemPoint.x - referencePoint.x, y: itemPoint.y - referencePoint.y };
}

/**
 * What the selection moves by once the dialog returned `edited`:
 * `toReferencePtVector + offsetVector`, with `toReferencePtVector = Origin - End`. Accepting the dialog without
 * changing anything moves nothing; typing a new offset puts the picked item point at `reference + edited`.
 */
export function interactiveOffsetMove(itemPoint: Vec, referencePoint: Vec, edited: Vec): Vec {
  return { x: referencePoint.x - itemPoint.x + edited.x, y: referencePoint.y - itemPoint.y + edited.y };
}

/**
 * `moveSelectionBy`: every movable item of `refs` by the one vector `v`, as one `move_items` -- a footprint, track, via, zone, graphic, text,
 * dimension or group. Ids that are no item the move tool carries (an unplaced footprint, an unknown id) are skipped, like the move tool itself.
 */
export function moveSelectionCommands(board: AnchorBoard, refs: readonly string[], v: Vec): Cmd[] {
  if (v.x === 0 && v.y === 0) return [];
  const ids = refs.filter((ref) => movableItem(board, ref));
  return ids.length === 0 ? [] : [{ op: "move_items", ids, dx: v.x, dy: v.y }];
}

/** The board items `itemPosition` can place: everything `movableItem` knows. */
export type PositionBoard = AnchorBoard;

/**
 * `BOARD_ITEM::GetPosition()` of a picked item (`DIALOG_POSITION_RELATIVE::UpdatePickedItem` anchors on it): a
 * footprint's anchor, a via/text position, a shape's first defining point, a track's start, a zone's first corner.
 */
export function itemPosition(board: PositionBoard, id: string): Vec | null {
  const movable = movableItem(board, id);
  return movable ? { x: movable.at[0], y: movable.at[1] } : null;
}

/**
 * Where the carried selection is anchored after a paste. A clipboard saved by Copy with Reference Point
 * (`selection.SetReferencePoint( refPoint ); io.SaveSelection( ... )`) is carried by that point; a plain copy has
 * no reference of its own here, so it is carried from the cursor.
 */
export function pasteMoveOrigin(reference: Vec | undefined | null, cursor: Vec | null): Vec | null {
  return reference ?? cursor;
}
