// How the schematic strokes a drawn polyline and a rule area -- `SCH_PAINTER::draw( const SCH_SHAPE* )` for `SHAPE_T::POLY` (eeschema/sch_painter.cpp at 8303b2ad,
// the same at 25ec2335) and `EDA_SHAPE::IsClosed` (common/eda_shape.cpp). UI-LEFT-2 SS21.
//
// - The outline is stroked along the points of `MakeEffectiveShapes( true )`: one `SHAPE_SEGMENT` per segment of the chain (`SHAPE_LINE_CHAIN::SegmentCount()`, which
//   counts the closing side only for a closed chain), their starts and the last one's end, handed to `GAL::DrawPolygon`, whose stroke is the polyline through them
//   (`OPENGL_GAL::drawPolygon` -> `drawPolyline`; Cairo's `drawPoly` moves to the first point and lines to the others, no `cairo_close_path`).
// - A schematic polyline is an open chain: `parseSchPolyLine` builds it with `SCH_SHAPE::AddPoint`, whose first point opens the outline with `SetClosed( false )`, and the
//   writer (`formatPoly`) writes its points only, so a file never closes it. A rule area's polyline is closed (`parseSchRuleArea`: "The polygon must be closed, it is a
//   schematic closed polyline": `SetClosed( true )`).
// - A fill is of the closed polygon either way (the GAL tesselates the points, `gluTessBeginContour` closing the contour), and so is a hatch (`UpdateHatching` closes
//   a copy of an open chain).
//
// Pure: no React, no DOM.

export type P = readonly [number, number];

/** `EDA_SHAPE::IsClosed()` of a schematic `POLY` graphic: a rule area's outline is closed, a drawn polyline's open. */
export function schPolyIsClosed(type: "polygon" | "rule_area"): boolean {
  return type === "rule_area";
}

/**
 * The points `SCH_PAINTER::draw( SCH_SHAPE )` strokes for a `POLY`: the chain's points, and the first again after the last when the chain is closed (its closing
 * segment's end); nothing for a chain of fewer than two points (no segment, `polySegments.empty()`).
 */
export function schPolyStrokePoints<T extends P>(pts: readonly T[], closed: boolean): T[] {
  if (pts.length < 2) return [];
  return closed ? [...pts, pts[0]!] : [...pts];
}
