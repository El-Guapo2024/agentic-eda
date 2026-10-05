// The symbol editor's "Draw Lines" and "Draw Polygons" tools (`eeschema.SymbolDrawing.drawSymbolLines` / `drawSymbolPolygon`,
// `SYMBOL_EDITOR_DRAWING_TOOLS::doDrawShape` in eeschema/tools/symbol_editor_drawing_tools.cpp). In this KiCad version both actions pass
// `SHAPE_T::POLY` and run the very same loop -- the two menu entries differ only by name -- and the shape is built by `EDA_SHAPE`'s
// `beginEdit` / `continueEdit` / `calcEdit` / `endEdit` (common/eda_shape.cpp):
//
//   first click             `beginEdit`: the outline is `[A, A']` -- the first vertex and a floating one on top of it
//   mouse move              `calcEdit`: the floating vertex follows the cursor
//   later click             `continueEdit`: "do not add zero-length segments" -- a new floating vertex is appended only when the
//                           previous vertex is not already where the cursor is
//   double click / Enter    `CalcEdit( GetPosition() )` (the floating vertex goes to the first vertex, "Close shape"), then `EndEdit`:
//                           with more than two points, if the last two are equal the outline is marked closed, otherwise the floating
//                           vertex is dropped and the outline stays open
//   Escape                  the shape being drawn is dropped, the tool stays armed (a second Escape leaves the tool)
//
// The studio keeps only the *fixed* vertices (the floating one is the cursor, drawn at paint time), so the machine below works on those.
// A new shape takes `m_lastFillStyle`, which starts as `FILL_T::NO_FILL` and is only ever re-set from the shapes the tool itself made, so a
// line or polygon drawn here is never filled until its Properties say so.
//
// Two honest differences, both about things KiCad cannot show: a closed outline is stored as a polyline that ends where it starts (the
// `.kicad_sym` format has no "closed" flag, `formatPoly` writes only the points -- so `[A, B, C, A]`; KiCad's own chain would then hold a
// harmless extra `A`), and a "shape" of a single point (a click and an immediate double click) adds nothing, where KiCad would store a
// zero-length line nobody can see or select.

export type PolyPt = [number, number];

/** First click: `beginEdit( cursorPos )`. */
export function polyBegin(p: PolyPt): PolyPt[] {
  return [p];
}

/** A later click: `continueEdit( cursorPos )`. The vertex is added unless it sits on the previous one (a zero-length segment). */
export function polyContinue(fixed: readonly PolyPt[], cursor: PolyPt): PolyPt[] {
  const last = fixed[fixed.length - 1];
  if (last && last[0] === cursor[0] && last[1] === cursor[1]) return [...fixed];
  return [...fixed, cursor];
}

export interface PolyResult {
  /** The vertices the outline ends up with. */
  pts: PolyPt[];
  /** `true` when the last vertex is the first one again (`Outline( 0 ).SetClosed( true )`). */
  closed: boolean;
}

/**
 * Double click / Enter: `CalcEdit( first vertex )` then `EndEdit( true )`. With the floating vertex on the first one, `endEdit` keeps the
 * outline (closed) only when the last fixed vertex is also the first one -- the person clicked back on the start -- and otherwise drops the
 * floating vertex and leaves the outline open. Fewer than two vertices is no shape (see this file's header).
 */
export function polyFinish(fixed: readonly PolyPt[]): PolyResult | null {
  if (fixed.length < 2) return null;
  const first = fixed[0]!;
  const last = fixed[fixed.length - 1]!;
  const closed = last[0] === first[0] && last[1] === first[1];
  return { pts: fixed.map((p) => [p[0], p[1]] as PolyPt), closed };
}

/** The rubber band while drawing: the fixed vertices and the floating one under the cursor (`CalcEdit( cursorPos )`). */
export function polyPreview(fixed: readonly PolyPt[], cursor: PolyPt | null): PolyPt[] {
  return cursor ? [...fixed, cursor] : [...fixed];
}
