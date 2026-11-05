// The Symbol Editor's bezier (`SCH_SHAPE` of `SHAPE_T::BEZIER`), at KiCad 8303b2ad:
//
// - drawn by `SCH_ACTIONS::drawBezier` -> `SYMBOL_EDITOR_DRAWING_TOOLS::DrawShape( SHAPE_T::BEZIER )` -> `doDrawShape`, which drives `EDA_SHAPE`'s edit state machine
//   (common/eda_shape.cpp `beginEdit` / `calcEdit` / `continueEdit` / `endEdit`): the first click sets all four points (state 1); the cursor then drags the end and the
//   second control point together; the second click fixes them (state 2) and the cursor drags the first control point; the third (state 3) and the cursor drags the second
//   control point; the fourth click ends it (`continueEdit` returns false at state 3). A double click or Enter (`finishInteractive`) ends it where the cursor has it.
//   Unfilled (`m_lastFillStyle`), "Add Curve";
// - edited by the point editor (`EDA_BEZIER_POINT_EDIT_BEHAVIOR`, common/tool/point_editor_behavior.cpp): four points -- start, control 1, control 2, end -- with the
//   indicator lines start to control 1 and control 2 to end; dragging one point sets that point only.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`). The clicks and the cursor are the editor's internal nm (+y down); the graphic is the wire's mm (+y up).
import type { LibrarySymbolGraphic, MmPoint } from "../api/types";

type P = [number, number];
export type LibBezier = Extract<LibrarySymbolGraphic, { kind: "bezier" }>;

/** The four points as `calcEdit` has them after `clicks` (1 to 4 of them) and with the cursor at `cursor`. */
export function bezierFromClicks(clicks: readonly P[], cursor: P | null): { start: P; c1: P; c2: P; end: P } | null {
  const s = clicks[0];
  if (!s) return null;
  const at = cursor ?? clicks[clicks.length - 1]!;
  switch (Math.min(clicks.length, 4)) {
    case 1:
      // state 1: `SetBezierC2( aPosition ); SetEnd( aPosition )`
      return { start: s, c1: s, c2: at, end: at };
    case 2:
      // state 2: `SetBezierC1( aPosition )`
      return { start: s, c1: at, c2: clicks[1]!, end: clicks[1]! };
    case 3:
      // state 3: `SetBezierC2( aPosition )`
      return { start: s, c1: clicks[2]!, c2: at, end: clicks[1]! };
    default:
      return { start: s, c1: clicks[2]!, c2: clicks[3]!, end: clicks[1]! };
  }
}

/** `continueEdit` of a bezier: false once the fourth click is in (state 3), when the shape is done. */
export const bezierDone = (clicks: readonly P[]): boolean => clicks.length >= 4;

const mm = (p: P): MmPoint => ({ x: p[0] / 1e6 || 0, y: -p[1] / 1e6 || 0 });
const nm = (p: MmPoint): P => [Math.round(p.x * 1e6) || 0, Math.round(-p.y * 1e6) || 0];

/** The library graphic the tool adds, on `unit` / `bodyStyle`, with the stroke and fill the other shapes of the tool get. */
export function bezierGraphic(points: { start: P; c1: P; c2: P; end: P }, unit: number, bodyStyle: number, strokeMm: number): LibBezier {
  return { kind: "bezier", unit, body_style: bodyStyle, start: mm(points.start), c1: mm(points.c1), c2: mm(points.c2), end: mm(points.end), stroke_mm: strokeMm, fill: "none" };
}

/** `EDA_BEZIER_POINT_EDIT_BEHAVIOR::MakePoints`: the four edit points (`BEZIER_START`, `BEZIER_CTRL_PT1`, `BEZIER_CTRL_PT2`, `BEZIER_END`) and the two indicator lines,
 *  in internal nm. */
export function bezierEditPoints(g: LibBezier): { points: [P, P, P, P]; lines: [P, P][] } {
  const pts: [P, P, P, P] = [nm(g.start), nm(g.c1), nm(g.c2), nm(g.end)];
  return { points: pts, lines: [[pts[0], pts[1]], [pts[2], pts[3]]] };
}

/** `EDA_BEZIER_POINT_EDIT_BEHAVIOR::UpdateItem`: the edited point -- 0 start, 1 control 1, 2 control 2, 3 end -- moved to `at` (internal nm), the others as they were. */
export function bezierWithPoint(g: LibBezier, index: number, at: P): LibBezier {
  const p = mm(at);
  const q = { x: Math.round(p.x * 1e4) / 1e4, y: Math.round(p.y * 1e4) / 1e4 };
  switch (index) {
    case 0:
      return { ...g, start: q };
    case 1:
      return { ...g, c1: q };
    case 2:
      return { ...g, c2: q };
    default:
      return { ...g, end: q };
  }
}
