// The cursor of a move, without moving the pointer. When a KiCad move starts, the tool takes the point of the held items nearest the pointer (`BestDragOrigin`) as their
// reference point and -- with "Warp mouse pointer to origin of moved object", the default -- warps the pointer onto it (`controls->SetCursorPosition( m_cursor, false )`:
// pcbnew/tools/edit_tool_move_fct.cpp `EDIT_TOOL::doMoveSelection`, eeschema/tools/sch_move_tool.cpp `SCH_MOVE_TOOL::initializeMoveOperation`). From then on the cursor
// the snapping sees is the held point plus how far the pointer has travelled, and that is what `BestSnapAnchor` snaps; the held items go to where it lands.
//
// A page may not move the pointer (and the studio never does), so the same cursor is computed from the pointer's travel alone: the held point plus (the pointer now minus the
// pointer where the items were grabbed). The items stay under the pointer where they were picked up, and the held point is what lands on the grid or an anchor.
export type Pt = readonly [number, number];

/** `held` + (`pointer` - `grabbed`): where the pointer would be had it been warped onto the held point when the move started. */
export function heldCursor(held: Pt, grabbed: Pt, pointer: Pt): [number, number] {
  return [held[0] + (pointer[0] - grabbed[0]), held[1] + (pointer[1] - grabbed[1])];
}
