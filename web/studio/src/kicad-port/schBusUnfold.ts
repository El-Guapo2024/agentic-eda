// Geometry of `C` (SCH_LINE_WIRE_BUS_TOOL::UnfoldBus / doUnfoldBus): where the new bus entry roots.
//
//   VECTOR2I pos = aPos.value_or( getViewControls()->GetCursorPosition() );   // the grid-snapped cursor
//   pos = bus->GetSeg().NearestPoint( pos );                                  // exactly on the bus, to connect
//
// "If the bus segment is H or V, this will be on the selection grid, if it's not, it might not be" -- so the entry
// roots at the segment point nearest the SNAPPED cursor, not nearest the raw one (which would leave an off-grid entry
// and a wire that ERC flags as off-grid).

/** `SEG::NearestPoint`: the point of segment a-b closest to q, rounded to whole micrometres (the schematic's unit). */
export function nearestPointOnSegment(a: readonly [number, number], b: readonly [number, number], q: readonly [number, number]): [number, number] {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const lenSq = dx * dx + dy * dy;
  if (lenSq === 0) return [a[0], a[1]];
  const t = Math.max(0, Math.min(1, ((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / lenSq));
  return [Math.round(a[0] + t * dx), Math.round(a[1] + t * dy)];
}

/** The bus entry's default size: a 100 mil diagonal (`SCH_BUS_WIRE_ENTRY( pos )` ends at `pos + ( 100 mil, 100 mil )`). */
export const BUS_ENTRY_SIZE_NM = 2_540_000;

/**
 * The size of the unfold's bus entry while the cursor moves before the first click (`doDrawSegments`, "Update the bus unfold posture based on the
 * mouse movement"): the entry leans toward the cursor, each axis negative when the cursor is on the minus side of the entry's root
 * (`flipX = cursor_delta.x < 0`, `size.x = std::abs( size.x ) * xSign`). The wire then starts at the entry's far end (`SetStartPoint( entry->GetEnd() )`).
 */
export function unfoldEntrySize(origin: readonly [number, number], cursor: readonly [number, number], size: readonly [number, number] = [BUS_ENTRY_SIZE_NM, BUS_ENTRY_SIZE_NM]): [number, number] {
  const xSign = cursor[0] - origin[0] < 0 ? -1 : 1;
  const ySign = cursor[1] - origin[1] < 0 ? -1 : 1;
  return [Math.abs(size[0]) * xSign, Math.abs(size[1]) * ySign];
}

/**
 * The first click of the unfold (`doDrawSegments`: "First click when unfolding places the label and wire-to-bus entry"): the entry keeps the posture
 * the click gives it, the wire's first point moves to the entry's end, and the member's label goes where the click is (the label ghost followed the
 * cursor, `m_busUnfold.label->SetPosition( cursorPos )`).
 */
export function placeUnfoldLabel(entryAt: readonly [number, number], size: readonly [number, number], click: readonly [number, number]): { size: [number, number]; start: [number, number]; labelAt: [number, number] } {
  const s = unfoldEntrySize(entryAt, click, size);
  return { size: s, start: [entryAt[0] + s[0], entryAt[1] + s[1]], labelAt: [click[0], click[1]] };
}
