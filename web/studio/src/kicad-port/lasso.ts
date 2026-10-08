// The lasso selection: the freehand / click-by-click polygon of the selection tool and the box-against-polygon test it
// selects with.
//
//   common/tool/actions.cpp                ACTIONS::selectSetLasso / selectSetRect
//   pcbnew/tools/pcb_selection_tool.cpp    PCB_SELECTION_TOOL::SetSelectPoly / SetSelectRect / SelectPolyArea / SelectMultiple
//   eeschema/tools/sch_selection_tool.cpp  SCH_SELECTION_TOOL::SetSelectPoly / SetSelectRect / selectLasso / SelectMultiple
//   libs/kimath/src/geometry/geometry_utils.cpp  KIGEOM::BoxHitTest( SHAPE_LINE_CHAIN, BOX2I, bool ) -> ShapeHitTest
//
// The direction the polygon is drawn in picks the mode (`SelectPolyArea`: "Auto mode: clockwise = inside, counterclockwise =
// touching"), and an item is selected when its box lies inside the polygon (inside mode) or touches it (touching mode).
import type { Box } from "./itemBoxes";

export type Pt = readonly [number, number];

/** What the selection tool's drag does: `SELECTION_MODE::INSIDE_RECTANGLE` (the default) or the lasso. */
export type SelectionAreaMode = "rect" | "lasso";

/** `selectSetLasso` / `selectSetRect`: the mode they set (`SetSelectPoly` -> `INSIDE_LASSO`, `SetSelectRect` -> `INSIDE_RECTANGLE`). */
export function selectionModeForAction(action: "selectSetLasso" | "selectSetRect"): SelectionAreaMode {
  return action === "selectSetLasso" ? "lasso" : "rect";
}

/** `SHAPE_LINE_CHAIN::Area( false )`: the signed shoelace area of the polygon in the screen's y-down plane (positive = clockwise on screen). */
export function signedArea(poly: readonly Pt[]): number {
  let a = 0;
  for (let i = 0; i < poly.length; i++) {
    const [x0, y0] = poly[i]!;
    const [x1, y1] = poly[(i + 1) % poly.length]!;
    a += x0 * y1 - x1 * y0;
  }
  return a / 2;
}

/**
 * The lasso mode the polygon drawn so far has: "clockwise = inside, counterclockwise = touching". KiCad's y axis points
 * down like ours, so a positive area is a clockwise loop on screen. A polygon with no area yet (a single point or a straight
 * line) is `touching`: `selectionMode` starts as `TOUCHING_LASSO` and only a positive area changes it.
 */
export function lassoContained(poly: readonly Pt[]): boolean {
  return signedArea(poly) > 0;
}

/** Whether the closed segment `a`-`b` meets the closed box (Cohen-Sutherland, `ClipLine`). */
export function segmentMeetsBox(a: Pt, b: Pt, box: Box): boolean {
  const [x0, y0, x1, y1] = box;
  const code = (x: number, y: number) => (y < y0 ? 2 : y > y1 ? 1 : 0) | (x < x0 ? 4 : x > x1 ? 8 : 0);
  let [ax, ay] = a;
  let [bx, by] = b;
  let ca = code(ax, ay);
  let cb = code(bx, by);
  for (let guard = 0; guard < 16; guard++) {
    if ((ca | cb) === 0) return true;
    if (ca & cb) return false;
    const out = ca !== 0 ? ca : cb;
    let x: number;
    let y: number;
    if (out & 1) {
      y = y1;
      x = ax + ((bx - ax) * (y - ay)) / (by - ay);
    } else if (out & 2) {
      y = y0;
      x = ax + ((bx - ax) * (y - ay)) / (by - ay);
    } else if (out & 8) {
      x = x1;
      y = ay + ((by - ay) * (x - ax)) / (bx - ax);
    } else {
      x = x0;
      y = ay + ((by - ay) * (x - ax)) / (bx - ax);
    }
    if (out === ca) {
      ax = x;
      ay = y;
      ca = code(ax, ay);
    } else {
      bx = x;
      by = y;
      cb = code(bx, by);
    }
  }
  return true;
}

/** Even-odd point-in-polygon. */
export function pointInPoly(p: Pt, poly: readonly Pt[]): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i]!;
    const [xj, yj] = poly[j]!;
    if (yi > p[1] !== yj > p[1] && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

/**
 * `KIGEOM::BoxHitTest( poly, box, contained )` for a closed polygon: `ShapeHitTest` of the box (`SHAPE_RECT`) --
 *   contained: `collidesAll() && !intersectsAny()`: the box meets the polygon's area and no edge of the polygon touches the box,
 *              i.e. the box lies wholly inside the polygon;
 *   touching:  `collidesAny()`: the box and the polygon's area share a point.
 */
export function boxHitTestPolygon(poly: readonly Pt[], box: Box, contained: boolean): boolean {
  if (poly.length < 3) return false;
  let edgeTouches = false;
  for (let i = 0; i < poly.length && !edgeTouches; i++) edgeTouches = segmentMeetsBox(poly[i]!, poly[(i + 1) % poly.length]!, box);
  if (!contained) {
    if (edgeTouches) return true;
    // no edge reaches the box: it is wholly inside the polygon, or wholly outside it
    return pointInPoly([box[0], box[1]], poly);
  }
  return !edgeTouches && pointInPoly([box[0], box[1]], poly);
}

/**
 * `SelectMultiple`'s collection: the ids whose boxes hit the lasso, sorted by position ("Sort the filtered selection by rows and columns":
 * ascending y, then x, by the item's own position -- its box's top-left stands in for `GetPosition()`).
 */
export function lassoHits(boxes: ReadonlyMap<string, Box>, poly: readonly Pt[], contained: boolean): string[] {
  const hits: { id: string; x: number; y: number }[] = [];
  for (const [id, box] of boxes) if (boxHitTestPolygon(poly, box, contained)) hits.push({ id, x: box[0], y: box[1] });
  hits.sort((a, b) => a.y - b.y || a.x - b.x);
  return hits.map((h) => h.id);
}

/**
 * How the lasso's result changes the selection (`SelectMultiple( aArea, aSubtractive, aExclusiveOr )`): the hits are added to what
 * was kept (a drag without a modifier cleared the selection first), removed from it with the subtractive modifier, or toggled with
 * exclusive-or.
 */
export function applyAreaSelection(current: readonly string[], hits: readonly string[], mode: "set" | "add" | "subtract" | "toggle"): string[] {
  const out = new Set(mode === "set" ? [] : current);
  for (const id of hits) {
    if (mode === "subtract") out.delete(id);
    else if (mode === "toggle") {
      if (out.has(id)) out.delete(id);
      else out.add(id);
    } else out.add(id);
  }
  return [...out];
}

/** Adds a freehand sample to the lasso only when it moved at least `minDistance` from the last one (the path of a drag has a point per mouse event). */
export function appendLassoPoint(poly: readonly Pt[], p: Pt, minDistance: number): Pt[] {
  const last = poly[poly.length - 1];
  if (last && Math.hypot(p[0] - last[0], p[1] - last[1]) < minDistance) return poly as Pt[];
  return [...poly, p];
}
