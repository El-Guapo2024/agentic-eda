// The selection rectangle of the library editors' selection tools (the board and the schematic have their own: canvas/selectionCandidates.ts
// `collectBoxSelection`, schematic/boxSelection.ts).
//
//   pcbnew/tools/pcb_selection_tool.cpp    PCB_SELECTION_TOOL::SelectRectArea  (the Footprint Editor)
//   eeschema/tools/sch_selection_tool.cpp  SCH_SELECTION_TOOL::selectMultiple (the Symbol Editor)
//
// "Left to right" (the drag's end right of its start) selects what lies wholly inside the box (`INSIDE_RECTANGLE`, `isWindowSelection`); right to left
// what it touches (`TOUCHING_RECTANGLE`, `isGreedy`). The test is on each item's box (`ItemBoxes`), as the lasso's (`lassoHits`) is.
import type { Box } from "./itemBoxes";
import { lassoHits, type Pt } from "./lasso";
import { isCrossingSelection } from "./selection";

/** The rectangle from the drag's start to its end, as the closed polygon `lassoHits` tests boxes against. */
export function rectPolygon(a: Pt, b: Pt): Pt[] {
  return [
    [a[0], a[1]],
    [b[0], a[1]],
    [b[0], b[1]],
    [a[0], b[1]],
  ];
}

/** The ids the rectangle from `a` to `b` selects: inside it when dragged left to right, touching it when dragged right to left. */
export function rectSelectionHits(boxes: ReadonlyMap<string, Box>, a: Pt, b: Pt): string[] {
  const crossing = isCrossingSelection(a[0], b[0]);
  const poly = rectPolygon(a, b);
  // `lassoHits`' polygon test takes the polygon's winding for nothing: inside / touching is the argument
  return lassoHits(boxes, poly, !crossing);
}
