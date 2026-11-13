// The Footprint Editor's point editor on its graphic shapes (UI-LEFT-2 FF7): `PCB_POINT_EDITOR` (pcbnew/tools/pcb_point_editor.cpp at KiCad 8303b2ad), the tool the
// Footprint Editor shares with the board, on a selection of one graphic -- a line, a rectangle, a circle, an arc, a polygon or a Bezier. Its edit points and what a drag of
// one does are the board's port (kicad-port/pcbPointEdit.ts `shapeEditPoints`, `activeEditPoint`, `moveShapePoint`); the arc behaves in the Footprint Editor's own mode,
// `FOOTPRINT_EDITOR_SETTINGS::m_ArcEditMode` (Preferences > Footprint Editor > Editing Options, "Arc editing mode:"; `PCB_POINT_EDITOR::changeArcEditMode` sets it from
// the three `pointEditorArcKeep*` actions and Ctrl+Space). The preference keeps the page's index (`m_arcEditMode->GetSelection()`), `arcEditModeToEnum` turns it
// into the mode.
//
// The corner operations of the point editor (UI-LEFT-3 F3-2), as the board's: Create Corner (`PCB_POINT_EDITOR::addCorner`, Insert -- a segment or an arc split in two at
// the point of it nearest the cursor, a polygon given a corner on its nearest side), Remove Corner (`removeCorner`, on the corner under the cursor of a polygon of more
// than three corners -- of three, the polygon goes), Chamfer Corner (`chamferCorner`, the polygon's corner nearest the cursor), Move Corner To... and Move Midpoint
// To... (`movePoint`, the point under the cursor to a typed place); and, as `PCB_POINT_EDITOR::OnSelectionChange` has it (`if( !item || item->IsLocked() ) return 0;`),
// no edit points on a locked graphic.
//
// Pure: the canvas (components/footprint/FootprintCanvas.tsx) runs the drag and commits it with `fp_item set_graphic` (one undo step, the shape's looks kept by id); the
// corner operations are actions/fpPointEditActions.ts.
import type { CmdShape, Shape } from "../api/types";
import { activeEditPoint, chamferContourCorner, cmdShapeToShape, moveContourPoint, moveShapePoint, nearestContourVertex, shapeEditPoints, shapeToCmd, type ArcEditMode, type EditPoint } from "./pcbPointEdit";
import { addCornerToContours, splitArcAtCursor, splitSegmentAt, type P } from "./pcbAddCorner";
import { canRemoveContourCorner, removeContourCorner } from "./zonePointEditor";

/** `arcEditModeToEnum` / `arcEditModeToComboIndex` of `PANEL_EDIT_OPTIONS` (pcbnew/dialogs/panel_edit_options.cpp): the choice's index -> the mode. */
export const FP_ARC_EDIT_MODES: readonly ArcEditMode[] = ["keep_center_adjust_angle_radius", "keep_endpoints_or_start_direction", "keep_center_ends_adjust_angle"];

/** `arcEditModeToEnum( aIndex )`: an index out of range is KEEP_CENTER_ADJUST_ANGLE_RADIUS ("Invalid index for ARC_EDIT_MODE"). */
export function fpArcEditMode(index: number): ArcEditMode {
  return FP_ARC_EDIT_MODES[index] ?? "keep_center_adjust_angle_radius";
}

/** `arcEditModeToComboIndex( aMode )`. */
export function fpArcEditModeIndex(mode: ArcEditMode): number {
  const i = FP_ARC_EDIT_MODES.indexOf(mode);
  return i < 0 ? 0 : i;
}

/**
 * The graphic the point editor works on: the one selected graphic of the footprint (`PCB_POINT_EDITOR::OnSelectionChange`: `selection.Size() == 1`), as the display shape;
 * none when it is locked (`if( !item || item->IsLocked() ) return 0;`, UI-LEFT-3 F3-2).
 */
export function fpPointEditTarget(graphics: readonly CmdShape[], selection: ReadonlySet<string>, locked: (id: string) => boolean = () => false): Shape | null {
  if (selection.size !== 1) return null;
  const id = [...selection][0]!;
  const g = graphics.find((s) => s.id === id);
  return g && !locked(id) ? cmdShapeToShape(g, id) : null;
}

/** The edit point of `shape` under the pointer (`EDIT_POINTS::FindPoint` within `tolNm`), or null. */
export function fpPointAt(shape: Shape, at: readonly [number, number], tolNm: number): EditPoint | null {
  return activeEditPoint(shapeEditPoints(shape), at, tolNm);
}

/** The shape with `point` dragged to `to` (`updateItem`), in the Footprint Editor's arc mode; null when the drag makes no shape. The id is the shape's own. */
export function fpPointDrag(shape: Shape, point: EditPoint, to: readonly [number, number], arcModeIndex: number): CmdShape | null {
  const next = moveShapePoint(shape, point, to, fpArcEditMode(arcModeIndex));
  return next ? { ...next, id: shape.id } : null;
}

// ------------------------------------------------------------------------------------------------------------------- the corner operations (F3-2)

/** What a corner operation does to the graphic: its new shape (`commit.Modify`), the graphic split in two (`Split Segment` / `Split Arc`: the graphic is the first half, a
 * copy of it with its looks the second), or the graphic removed (`commit.Remove( item )`). */
export type FpCornerEdit = { kind: "set"; shape: CmdShape } | { kind: "split"; first: CmdShape; second: CmdShape } | { kind: "remove" };

/** A polygon's contours in `SHAPE_POLY_SET` order (its outline, then its holes). */
const contoursOf = (s: Extract<Shape, { kind: "polygon" }>): P[][] => [s.pts.map((p) => [p[0], p[1]] as P), ...(s.holes ?? []).map((h) => h.map((p) => [p[0], p[1]] as P))];

/** The polygon `s` with `contours` (the first the outline, the rest its holes), as the command shape keeping the id and the looks. */
function polygonWith(s: Extract<Shape, { kind: "polygon" }>, contours: readonly (readonly P[])[]): CmdShape {
  const [outline, ...holes] = contours;
  return { ...shapeToCmd({ ...s, pts: outline!.map((p) => [p[0], p[1]] as [number, number]), holes: holes.length > 0 ? holes.map((h) => h.map((p) => [p[0], p[1]] as [number, number])) : undefined }), id: s.id };
}

/** `PCB_POINT_EDITOR::CanAddCorner` of a graphic: a segment, a polygon or an arc. */
export function fpCanAddCorner(s: Shape): boolean {
  return s.kind === "segment" || s.kind === "polygon" || s.kind === "arc";
}

/**
 * `PCB_POINT_EDITOR::addCorner` with the cursor at `cursor` (`GetCursorPosition()`, the cursor on the grid): a polygon takes a corner at the point of its nearest side
 * nearest the cursor (the middle of that side when that point is one of its ends); a segment or an arc is split there ("Split Segment", "Split Arc": the graphic now ends at
 * the point, a copy of it starts there). Null when nothing is done (a split at an end, another kind).
 */
export function fpAddCorner(s: Shape, cursor: readonly [number, number]): FpCornerEdit | null {
  const at: P = [cursor[0], cursor[1]];
  const cmd = (patch: Partial<Shape>): CmdShape => ({ ...shapeToCmd({ ...s, ...patch } as Shape), id: s.id });
  const pt = (p: P): [number, number] => [p[0], p[1]];
  switch (s.kind) {
    case "polygon": {
      const next = addCornerToContours(contoursOf(s), at);
      return next ? { kind: "set", shape: polygonWith(s, next) } : null;
    }
    case "segment": {
      const halves = splitSegmentAt(s.start, s.end, at);
      if (!halves) return null;
      return { kind: "split", first: cmd({ start: pt(halves[0].start), end: pt(halves[0].end) }), second: cmd({ start: pt(halves[1].start), end: pt(halves[1].end) }) };
    }
    case "arc": {
      const halves = splitArcAtCursor(s.start, s.mid, s.end, at);
      if (!halves) return null;
      const arcOf = (h: { start: P; mid: P; end: P }) => cmd({ start: pt(h.start), mid: pt(h.mid), end: pt(h.end) });
      return { kind: "split", first: arcOf(halves[0]), second: arcOf(halves[1]) };
    }
    default:
      return null;
  }
}

/** The global vertex index a polygon's edit point names (`v<i>`), else null (a midpoint, another kind's point). */
const vertexOf = (s: Shape, point: EditPoint | null): number | null => (s.kind === "polygon" && point?.kind === "corner" && /^v\d+$/.test(point.id) ? Number(point.id.slice(1)) : null);

/** `PCB_POINT_EDITOR::CanRemoveCorner`: the edited point is a corner (not a side) of a polygon whose outline keeps more than three corners (a hole's always goes). */
export function fpCanRemoveCorner(s: Shape, point: EditPoint | null): boolean {
  const v = vertexOf(s, point);
  return v !== null && s.kind === "polygon" && canRemoveContourCorner(contoursOf(s), v);
}

/** `PCB_POINT_EDITOR::removeCorner` of the edited point: the corner removed ("Remove Polygon Corner"), a hole of three corners removed, an outline of three the graphic. */
export function fpRemoveCorner(s: Shape, point: EditPoint | null): FpCornerEdit | null {
  const v = vertexOf(s, point);
  if (v === null || s.kind !== "polygon") return null;
  const r = removeContourCorner(contoursOf(s), v);
  if (!r) return null;
  return r.kind === "remove_item" ? { kind: "remove" } : { kind: "set", shape: polygonWith(s, r.contours) };
}

/** `PCB_POINT_EDITOR::CanChamferCorner` of a graphic: a polygon. */
export function fpCanChamferCorner(s: Shape): boolean {
  return s.kind === "polygon";
}

/** `PCB_POINT_EDITOR::chamferCorner`: the polygon's corner nearest the cursor chamfered ("Break Polygon Corner"); null when it cannot be (KiCad then changes nothing). */
export function fpChamferCorner(s: Shape, cursor: readonly [number, number]): FpCornerEdit | null {
  if (s.kind !== "polygon") return null;
  const contours = contoursOf(s);
  const next = chamferContourCorner(contours, nearestContourVertex(contours, cursor));
  return next ? { kind: "set", shape: polygonWith(s, next) } : null;
}

/** `HasCorner()` / `HasMidpoint()`: what the point under the cursor is -- Move Corner To... for a corner, Move Midpoint To... for a side's middle. */
export function fpMovePointTitle(point: EditPoint): string {
  return point.kind === "midpoint" ? "Move Midpoint to Location" : "Move Corner to Location";
}

/** `PCB_POINT_EDITOR::movePoint`: the point set to the typed place (`m_editedPoint->SetPosition( dlg.GetValue() ); updateItem()`), in the editor's arc mode; null when that makes no shape. */
export function fpMovePoint(s: Shape, point: EditPoint, to: readonly [number, number], arcModeIndex: number): CmdShape | null {
  if (s.kind === "polygon") {
    const moved = moveContourPoint(contoursOf(s), point, to);
    return moved ? polygonWith(s, moved) : null;
  }
  return fpPointDrag(s, point, to, arcModeIndex);
}
