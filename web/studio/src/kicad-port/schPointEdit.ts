// The schematic's point editor on its drawn graphics -- `SCH_POINT_EDITOR` on one selected `SCH_SHAPE` (rectangle, circle, arc, bezier, polyline), `SCH_TEXTBOX` or
// `SCH_RULE_AREA` (eeschema/tools/sch_point_editor.cpp at 8303b2ad; the behaviours are kicad-port/eePointEdit.ts): a graphic as the behaviour sees it, and what a drag
// makes of the graphic (`sch_edit` `edit_graphic`, one undo step: `commit.Push( _( "Move Point" ) )` on the mouse-up). UI-LEFT-2 SS3, after S13's arc editor
// (kicad-port/schArcEdit.ts, whose mode setting this reads).
//
// - A circle is kept as its centre and radius (`(circle (center ..) (radius ..))`): its end point is where `parseSchCircle` puts it, the radius to the right of the
//   centre (`SetEnd( center + VECTOR2I( radius, 0 ) )`); a drag of the end gives the radius the distance to it. KiCad keeps a drawn circle's end where it was clicked
//   until the file is read again; here it is back on the right once the drag lands.
// - A text box's `GetMinSize()`: its text laid out in the box as it is being dragged (kicad-port/schTextBox.ts) -- the lines' height plus the top and bottom margins
//   (the left and right ones for a vertical text) across the text, none along it; nothing for an empty text.
// - A polyline is open (`parseSchPolyLine` leaves its chain open: `SCH_SHAPE::AddPoint`'s `SetClosed( false )`) and a rule area closed (`parseSchRuleArea`'s
//   `SetClosed( true )`); the painter strokes them so (components/schematic/schGraphicsPainter.ts, UI-LEFT-2 SS21).
// - Create / Remove Corner (`addCornerCondition`, `addCorner`, `removeCornerCondition`, `removeCorner`) are the Symbol Editor's port (eePointEdit.ts) on the graphic's
//   shape: an open polyline's closing side is no candidate of `addCorner`, and the corner removed is the edited point, the point editor's vertex under the cursor.
//
// Pure: no React, no DOM; the caller passes the stroke font's measure.
import type { SchGraphic, SchGraphicShape } from "../api/schEditTypes";
import { addCorner, canAddCorner, eeDrag, eeFindHandle, eeMakePoints, handlePos, removableCorner, removeCorner, type EeDragResult, type EeEditPoints, type EeEditShape, type EeHandle, type P } from "./eePointEdit";
import type { ArcEditMode } from "./pcbPointEdit";
import { CAP_HEIGHT_RATIO, INTERLINE_PITCH, textBoxMargins, wrapText, type Measure } from "./schTextBox";
import { kicadTextBox } from "./kicadText";
import { resolveSchFont } from "./schDefaultFont";
import { schTextBoxDrawSpec } from "./schTextSpecs";

/** `SCH_SHAPE`'s default line width (`schIUScale.MilsToIU( DEFAULT_LINE_WIDTH_MILS )`, 6 mil), what a text box's default margin is made from. */
const DEFAULT_WIDTH_NM = 152_400;

const p = (q: { x: number; y: number }): P => [q.x, q.y];
const xy = (q: readonly [number, number]) => ({ x: Math.round(q[0]), y: Math.round(q[1]) });

/** The graphics the point editor has a behaviour for (`pointEditorTypes` and `makePointsAndBehavior`'s shapes). */
export function hasPointEditor(g: SchGraphic): boolean {
  return ["rectangle", "circle", "arc", "bezier", "polygon", "text_box", "rule_area"].includes(g.shape.type);
}

/**
 * `SCH_TEXTBOX::GetMinSize` of a box with its text: the text's height and the margins across the text's direction (vertical text: a width, horizontal: a height). In an outline
 * face (`face`, the box's `text_attrs` font; none is the default font) the height is `GetTextBox`'s in that font, the text broken to the box in it (UI-LEFT-3 C3-3).
 */
export function textBoxMinSize(g: SchGraphic, start: P, end: P, measure: Measure, face?: string | null): P {
  const s = g.shape;
  if (s.type !== "text_box" || s.text === "") return [0, 0];
  const width = g.width_nm && g.width_nm > 0 ? g.width_nm : DEFAULT_WIDTH_NM;
  const [ml, mt, mr, mb] = textBoxMargins(s, width);
  const vertical = Math.round((s.angle ?? 0) / 1000) % 180 === 90;
  const font = resolveSchFont(face, !!s.bold, !!s.italic);
  if (font.kind === "outline") {
    const look = { start, end, margins: [ml, mt, mr, mb] as const, text: s.text, angleDeg: vertical ? 90 : 0, size_nm: s.size_nm, h: s.h_align ?? "left", v: s.v_align ?? "top", bold: !!s.bold, italic: !!s.italic };
    const b = kicadTextBox(schTextBoxDrawSpec(look, font), font);
    const textHeight = Math.abs(b.y1 - b.y0);
    return vertical ? [textHeight + ml + mr, 0] : [0, textHeight + mt + mb];
  }
  const along = vertical ? Math.abs(end[1] - start[1]) : Math.abs(end[0] - start[0]);
  // `GetShownText`'s column: the box less the margins along the text; `GetMinSize`: the text's height and the margins across it
  const lines = wrapText(s.text, Math.max(along - (vertical ? mt + mb : ml + mr), 0), s.size_nm, measure);
  const textHeight = Math.round(s.size_nm * CAP_HEIGHT_RATIO + (lines.length - 1) * s.size_nm * INTERLINE_PITCH);
  return vertical ? [textHeight + ml + mr, 0] : [0, textHeight + mt + mb];
}

/** The graphic as its behaviour sees it, or null for a graphic the point editor has none for (a directive label, an image, a table ...). */
export function schEditShape(g: SchGraphic, measure: Measure, face?: string | null): EeEditShape | null {
  const s = g.shape;
  switch (s.type) {
    case "rectangle":
      return { kind: "rectangle", start: p(s.start), end: p(s.end), radius: s.corner_radius_nm ?? 0 };
    case "circle":
      return { kind: "circle", center: p(s.center), end: [s.center.x + s.radius_nm, s.center.y] };
    case "arc":
      return { kind: "arc", start: p(s.start), mid: p(s.mid), end: p(s.end) };
    case "bezier":
      return { kind: "bezier", start: p(s.start), c1: p(s.c1), c2: p(s.c2), end: p(s.end) };
    case "polygon":
      return { kind: "polygon", pts: s.pts.map(p), closed: false };
    case "rule_area":
      return { kind: "polygon", pts: s.pts.map(p), closed: true, ruleArea: true };
    case "text_box":
      return { kind: "text_box", start: p(s.start), end: p(s.end), minSize: textBoxMinSize(g, p(s.start), p(s.end), measure, face) };
    default:
      return null;
  }
}

/** The graphic's shape as the behaviour left it (its other properties as they were). */
export function schShapeFrom(e: EeEditShape, was: SchGraphicShape): SchGraphicShape {
  switch (e.kind) {
    case "rectangle":
      if (was.type !== "rectangle") return was;
      return { ...was, start: xy(e.start), end: xy(e.end), ...(e.radius ? { corner_radius_nm: e.radius } : { corner_radius_nm: undefined }) };
    case "text_box":
      return was.type === "text_box" ? { ...was, start: xy(e.start), end: xy(e.end) } : was;
    case "circle":
      return was.type === "circle" ? { ...was, center: xy(e.center), radius_nm: Math.round(Math.hypot(e.end[0] - e.center[0], e.end[1] - e.center[1])) } : was;
    case "arc":
      return { type: "arc", start: xy(e.start), mid: xy(e.mid), end: xy(e.end) };
    case "bezier":
      return { type: "bezier", start: xy(e.start), c1: xy(e.c1), c2: xy(e.c2), end: xy(e.end) };
    case "polygon":
      return was.type === "rule_area" || was.type === "polygon" ? { ...was, pts: e.pts.map(xy) } : was;
  }
}

/**
 * One drag step of handle `h` of graphic `g` to `to` (the snapped cursor), in `mode`: the shape the behaviour makes (a text box's minimum size is that of the box as the
 * drag has it, as KiCad's `GetMinSize` reads the box it is editing), or null when it makes none.
 */
export function schDrag(g: SchGraphic, h: EeHandle, to: readonly [number, number], mode: ArcEditMode, measure: Measure, face?: string | null): EeDragResult | null {
  const e = schEditShape(g, measure, face);
  if (!e) return null;
  const first = eeDrag(e, h, to, mode);
  if (!first || e.kind !== "text_box" || first.shape.kind !== "text_box") return first;
  return eeDrag({ ...e, minSize: textBoxMinSize(g, first.shape.start, first.shape.end, measure, face) }, h, to, mode);
}

/** The edit points of a graphic as a drag leaves it (or as it is), and the graphic itself as drawn. */
export function schPointEditor(g: SchGraphic, drag: { handle: EeHandle; to: readonly [number, number] } | null, mode: ArcEditMode, measure: Measure, face?: string | null): { edit: EeEditPoints; graphic: SchGraphic } | null {
  const e = schEditShape(g, measure, face);
  if (!e) return null;
  const dragged = drag ? schDrag(g, drag.handle, drag.to, mode, measure, face) : null;
  const shown = dragged?.shape ?? e;
  return { edit: eeMakePoints(shown), graphic: dragged ? { ...g, shape: schShapeFrom(shown, g.shape) } : g };
}

/** A polyline or rule area as `addCorner` / `removeCorner` see it (`SCH_SHAPE_T` or `SCH_RULE_AREA_T` with a `POLY` shape), or null for anything else. */
function polyShape(g: SchGraphic): Extract<EeEditShape, { kind: "polygon" }> | null {
  const s = g.shape;
  if (s.type === "polygon") return { kind: "polygon", pts: s.pts.map(p), closed: false };
  if (s.type === "rule_area") return { kind: "polygon", pts: s.pts.map(p), closed: true, ruleArea: true };
  return null;
}

/**
 * The point editor's corner entries on one selected graphic with the pointer at `cursor` (the raw cursor, `GetCursorPosition( false )`) and `threshold` the world size of
 * `EDIT_POINT::POINT_SIZE`: `canAdd` is `addCornerCondition` (a polyline or rule area, the cursor on its outline); `removeIndex` is the vertex `removeCorner` takes
 * (`removeCornerCondition`: the edited point -- `FindPoint` of the point editor's points under the cursor -- on a vertex of a polyline of more than two points or a rule
 * area of more than three), or -1.
 */
export function schCornerEntries(g: SchGraphic, cursor: readonly [number, number], threshold: number): { canAdd: boolean; removeIndex: number } {
  const shape = polyShape(g);
  if (!shape) return { canAdd: false, removeIndex: -1 };
  const ed = eeMakePoints(shape);
  const handle = eeFindHandle(ed, cursor, threshold);
  const edited = handle?.kind === "point" ? handlePos(ed, handle) : null;
  return { canAdd: canAddCorner(shape, cursor, threshold), removeIndex: removableCorner(shape, edited) };
}

/** `SCH_POINT_EDITOR::addCorner` on graphic `g` at `at` (the cursor on the grid unless snapping is off): its shape with the new corner, or null for a graphic that has none. */
export function schAddCorner(g: SchGraphic, at: readonly [number, number]): SchGraphicShape | null {
  const shape = polyShape(g);
  return shape ? schShapeFrom(addCorner(shape, at), g.shape) : null;
}

/** `SCH_POINT_EDITOR::removeCorner` of vertex `idx` of graphic `g` (see [`schCornerEntries`]): its shape without it, or null when it may not go. */
export function schRemoveCorner(g: SchGraphic, idx: number): SchGraphicShape | null {
  const shape = polyShape(g);
  const next = shape ? removeCorner(shape, idx) : null;
  return next ? schShapeFrom(next, g.shape) : null;
}
