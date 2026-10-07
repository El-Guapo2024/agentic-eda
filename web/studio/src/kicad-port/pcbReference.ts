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
 * `moveSelectionBy`: every movable item of `refs` by the one vector `v`. Parts, vias and texts are moved to their
 * new position, shapes by the offset -- the same verbs `commitMove` uses. Items this app cannot move (tracks, zones,
 * dimensions) are skipped, like the move tool itself.
 */
export function moveSelectionCommands(board: AnchorBoard, refs: readonly string[], v: Vec): Cmd[] {
  const cmds: Cmd[] = [];
  if (v.x === 0 && v.y === 0) return cmds;
  for (const ref of refs) {
    const item = movableItem(board, ref);
    if (!item) continue;
    switch (item.kind) {
      case "part":
        cmds.push({ op: "move_to", part: ref, x: item.at[0] + v.x, y: item.at[1] + v.y });
        break;
      case "via":
        cmds.push({ op: "move_via", id: ref, x: item.at[0] + v.x, y: item.at[1] + v.y });
        break;
      case "shape":
        cmds.push({ op: "move_shape", id: ref, dx: v.x, dy: v.y });
        break;
      case "text":
        cmds.push({ op: "move_text", id: ref, x: item.at[0] + v.x, y: item.at[1] + v.y });
        break;
    }
  }
  return cmds;
}

/** The board items `itemPosition` can place: what `movableItem` knows, plus tracks and zones. */
export interface PositionBoard extends AnchorBoard {
  routing?: {
    vias: readonly { id: string; x: number; y: number }[];
    tracks?: readonly { id: string; pts: readonly (readonly [number, number])[] }[];
    zones?: readonly { id: string; outline: readonly (readonly [number, number])[] }[];
  } | null;
}

/**
 * `BOARD_ITEM::GetPosition()` of a picked item (`DIALOG_POSITION_RELATIVE::UpdatePickedItem` anchors on it): a
 * footprint's anchor, a via/text position, a shape's first defining point, a track's start, a zone's first corner.
 */
export function itemPosition(board: PositionBoard, id: string): Vec | null {
  const movable = movableItem(board, id);
  if (movable) return { x: movable.at[0], y: movable.at[1] };
  const track = board.routing?.tracks?.find((t) => t.id === id);
  if (track?.pts[0]) return { x: track.pts[0][0], y: track.pts[0][1] };
  const zone = board.routing?.zones?.find((z) => z.id === id);
  if (zone?.outline[0]) return { x: zone.outline[0][0], y: zone.outline[0][1] };
  return null;
}

/**
 * Where the carried selection is anchored after a paste. A clipboard saved by Copy with Reference Point
 * (`selection.SetReferencePoint( refPoint ); io.SaveSelection( ... )`) is carried by that point; a plain copy has
 * no reference of its own here, so it is carried from the cursor.
 */
export function pasteMoveOrigin(reference: Vec | undefined | null, cursor: Vec | null): Vec | null {
  return reference ?? cursor;
}
