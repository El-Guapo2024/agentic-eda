// The Footprint Editor's point editor on its graphic shapes (UI-LEFT-2 FF7): `PCB_POINT_EDITOR` (pcbnew/tools/pcb_point_editor.cpp at KiCad 8303b2ad), the tool the
// Footprint Editor shares with the board, on a selection of one graphic -- a line, a rectangle, a circle, an arc, a polygon or a Bezier. Its edit points and what a drag of
// one does are the board's port (kicad-port/pcbPointEdit.ts `shapeEditPoints`, `activeEditPoint`, `moveShapePoint`); the arc behaves in the Footprint Editor's own mode,
// `FOOTPRINT_EDITOR_SETTINGS::m_ArcEditMode` (Preferences > Footprint Editor > Editing Options, "Arc editing mode:"; `PCB_POINT_EDITOR::changeArcEditMode` sets it from
// the three `pointEditorArcKeep*` actions and Ctrl+Space). The preference keeps the page's index (`m_arcEditMode->GetSelection()`), `arcEditModeToEnum` turns it
// into the mode.
//
// Pure: the canvas (components/footprint/FootprintCanvas.tsx) runs the drag and commits it with `fp_item set_graphic` (one undo step, the shape's looks kept by id).
import type { CmdShape, Shape } from "../api/types";
import { activeEditPoint, cmdShapeToShape, moveShapePoint, shapeEditPoints, type ArcEditMode, type EditPoint } from "./pcbPointEdit";

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

/** The graphic the point editor works on: the one selected graphic of the footprint (`PCB_POINT_EDITOR::OnSelectionChange`: `selection.Size() == 1`), as the display shape. */
export function fpPointEditTarget(graphics: readonly CmdShape[], selection: ReadonlySet<string>): Shape | null {
  if (selection.size !== 1) return null;
  const id = [...selection][0]!;
  const g = graphics.find((s) => s.id === id);
  return g ? cmdShapeToShape(g, id) : null;
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
