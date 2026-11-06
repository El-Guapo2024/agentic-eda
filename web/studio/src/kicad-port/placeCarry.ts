// Place Footprint's carry (pcbnew/tools/board_editor_control.cpp `BOARD_EDITOR_CONTROL::PlaceFootprint` at 8303b2ad, its placement-grid frame from KiCad 25ec2335),
// UI-LEFT-2 PP4:
//
//   cursorPos = controls->GetCursorPosition( !evt->DisableGridSnapping() );     the grid point under the cursor (`GAL::GetGridPoint`), the cursor itself with Ctrl
//   ... fp && ( evt->IsMotion() || evt->IsAction( &ACTIONS::refreshPreview ) ) ...
//       fp->SetPosition( cursorPos ); applyPlacementFrameOrientation();       the footprint's anchor there, turned to the placement grid's frame under it
//   ... the pick:  fp->SetPosition( cursorPos ); prevFrameAngle = ANGLE_0; applyPlacementFrameOrientation(); ... PostAction( ACTIONS::refreshPreview )
//
// So the footprint's anchor follows the grid point under the cursor -- not a move's `BestDragOrigin` / `BestSnapAnchor`, which would put one of its pads on a
// nearby anchor -- and it is turned to the frame the moment it is picked (the `refreshPreview` posted after the pick), so a click that never moved it puts it
// down there turned. The studio's carry is the Move tool's (`StudioApi.placeNewItems` with `gridCarry`): this module gives it the anchor to move and the drop of
// a click with no motion. Pure: no store, no DOM.
import type { BoardState } from "../api/types";
import { carryFrameTurn } from "./gridFrame";
import type { CarryOp } from "./pcbCarry";
import { carryStart, type Snap } from "./pcbTransform";

type P = readonly [number, number];

/** The anchor of the one footprint a Place Footprint carry holds (`fp->GetPosition()`), which the grid point under the cursor moves; null for anything else. */
export function gridCarryAnchor(board: BoardState, refs: readonly string[]): [number, number] | null {
  if (refs.length !== 1) return null;
  const part = board.parts.find((q) => q.ref === refs[0] && q.placed);
  return part?.at ? [part.at[0], part.at[1]] : null;
}

/** What the carry's drop applies: the items, how far (the anchor to `cursor`), the frame turn (`ops`), and the points R and F act about. */
export interface GridCarryDrop {
  refs: string[];
  dxNm: number;
  dyNm: number;
  ops?: CarryOp[];
  frameDeg?: number;
  grabNm: [number, number];
  pivotNm?: [number, number];
  flipPivotNm?: [number, number];
}

/**
 * The drop of a carry the pointer never moved (no preview yet): the footprint on `cursor` -- the grid point under the click, already snapped -- turned to the placement
 * grid's frame there from `frameStartDeg` (`prevFrameAngle`: 0 for a footprint fresh from the library). Null when the selection is not one carried footprint.
 */
export function gridCarryDrop(board: BoardState, selection: readonly string[], cursor: P, frameStartDeg: number | undefined, stepDeg: number, snap?: Snap): GridCarryDrop | null {
  const start = carryStart(board, selection, snap);
  const at = gridCarryAnchor(board, start.refs);
  if (!at) return null;
  const [dx, dy] = [cursor[0] - at[0], cursor[1] - at[1]];
  const frame = carryFrameTurn(board, start.refs, [], dx, dy, at, frameStartDeg, stepDeg);
  return { refs: start.refs, dxNm: dx, dyNm: dy, ops: frame?.ops, frameDeg: frame?.frameDeg, grabNm: at, pivotNm: start.pivotNm, flipPivotNm: start.flipPivotNm };
}
