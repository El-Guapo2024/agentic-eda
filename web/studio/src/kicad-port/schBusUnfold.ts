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
