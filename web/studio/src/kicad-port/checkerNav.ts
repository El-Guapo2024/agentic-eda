// Next / Previous Marker: stepping the Checker window's list of violations.
//
//   common/rc_item.cpp  RC_TREE_MODEL::NextMarker / PrevMarker   (ACTIONS::nextMarker / prevMarker, common.Checker.*)
//   pcbnew/tools/drc_tool.cpp  DRC_TOOL::NextMarker / PrevMarker; eeschema/tools/sch_inspection_tool.cpp SCH_INSPECTION_TOOL::NextMarker / PrevMarker
//
// The tools bring the DRC or ERC dialog up and the tree model moves its selection by one marker; selecting a marker is what zooms the canvas to
// it. `current` is the index of the marker the list has selected, or null for none.

/** `NextMarker`: the marker after the current one, the first when none is selected; null when the current one is the last (nothing changes). */
export function nextMarkerIndex(count: number, current: number | null): number | null {
  if (count <= 0) return null;
  if (current === null || current < 0 || current >= count) return 0;
  return current + 1 < count ? current + 1 : null;
}

/**
 * `PrevMarker`: the marker before the current one. With none selected the loop never meets the current node, so the last marker is the one
 * "before" it; null when the current one is the first (nothing changes).
 */
export function prevMarkerIndex(count: number, current: number | null): number | null {
  if (count <= 0) return null;
  if (current === null || current < 0 || current >= count) return count - 1;
  return current > 0 ? current - 1 : null;
}
