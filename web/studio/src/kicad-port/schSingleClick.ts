// `SCH_DRAWING_TOOLS::SingleClickPlace` (eeschema/tools/sch_drawing_tools.cpp at 8303b2ad): before a click of the no-connect, junction or bus entry tool places
// anything it asks the screen for an item of the tool's own type at the cursor (`screen->GetItem( cursorPos, 0, type )`, `SCH_SCREEN::GetItem` with an
// accuracy of 0, so the item's own `HitTest( aPosition, 0 )`). Found, a no-connect flag is removed ("No connects can be "toggled"/removed by clicking on them
// again", "Remove No Connect Flag") and any other kind is left as it is; nothing new is placed either way.

type P = readonly [number, number];

interface SingleClickSheet {
  no_connects?: ReadonlyArray<{ id: string; at: readonly [number, number] }>;
  bus_entries?: ReadonlyArray<{ id: string; at: readonly [number, number]; size: readonly [number, number] }>;
  junctions?: ReadonlyArray<{ id: string; at: readonly [number, number] }>;
}

/** `TestSegmentHit( aPosition, m_pos, GetEnd(), 0 )`: the point is on the segment, its ends included. */
function onSegment(p: P, a: P, b: P): boolean {
  const cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
  if (cross !== 0) return false;
  return Math.min(a[0], b[0]) <= p[0] && p[0] <= Math.max(a[0], b[0]) && Math.min(a[1], b[1]) <= p[1] && p[1] <= Math.max(a[1], b[1]);
}

/**
 * The id of the item of `kind` the click at `at` lands on, or null. A no-connect flag and a junction are hit at their position (`SCH_NO_CONNECT::HitTest`
 * and `SCH_JUNCTION::HitTest` grow their box by the accuracy, 0 here, about a point the size of the flag; a click is snapped to the grid the flag sits on, so
 * its own point is the one that matters), a bus entry anywhere on its line from `at` to `at + size` (`SCH_BUS_ENTRY_BASE::HitTest`).
 */
export function existingSingleClickItem(sheet: SingleClickSheet, kind: "no_connect" | "junction" | "bus_entry", at: P): string | null {
  const same = (q: readonly [number, number]) => q[0] === at[0] && q[1] === at[1];
  if (kind === "no_connect") return sheet.no_connects?.find((nc) => same(nc.at))?.id ?? null;
  if (kind === "junction") return sheet.junctions?.find((j) => same(j.at))?.id ?? null;
  return sheet.bus_entries?.find((e) => onSegment(at, e.at, [e.at[0] + e.size[0], e.at[1] + e.size[1]]))?.id ?? null;
}
