// The Footprint Editor's point editor operations (UI-LEFT-3 F3-2): `PCB_POINT_EDITOR::addCorner` (Create Corner, Insert), `removeCorner` (Remove Corner), `chamferCorner`
// (Chamfer Corner) and `movePoint` (Move Corner To... / Move Midpoint To...), pcbnew/tools/pcb_point_editor.cpp at 8303b2ad -- the tool the Footprint Editor shares with
// the board -- on the one selected graphic or zone (not a locked one: `OnSelectionChange`). The maths is kicad-port/fpPointEdit.ts; each operation is one undo step.
//
// KiCad's edited point is the edit point under the pointer (`m_editedPoint`), and `addCorner` / `chamferCorner` read the cursor (`GetCursorPosition()`, on the grid):
// a hotkey takes the cursor where it is, the context menu where it was opened (the canvas hands that point in).
import { createElement } from "react";
import type { FootprintEditorApi } from "../state/footprintEditorStore";
import { snapGridNm } from "../components/canvas/gridHelper";
import { PointEntryDialog } from "../components/PcbPointEditDialogs";
import { openSweepDialog } from "./pcbSweepDialogs";
import { fpItemLocked } from "../kicad-port/fpItems";
import type { EditPoint } from "../kicad-port/pcbPointEdit";
import type { Shape } from "../api/types";
import {
  fpAddCorner,
  fpCanAddCorner,
  fpCanChamferCorner,
  fpCanRemoveCorner,
  fpChamferCorner,
  fpMovePoint,
  fpMovePointTitle,
  fpPointAt,
  fpPointEditTarget,
  fpRemoveCorner,
  fpZoneAddCorner,
  fpZoneCanRemoveCorner,
  fpZoneChamferCorner,
  fpZoneDrag,
  fpZoneEditTarget,
  fpZonePointAt,
  fpZoneRemoveCorner,
  fpZoneWith,
  type FpZoneCornerEdit,
  type FpZoneEdit,
} from "../kicad-port/fpPointEdit";

export type FpPointOp = "addCorner" | "removeCorner" | "chamferCorner" | "moveCorner" | "moveMidpoint";

/** The KiCad action of each operation (`PCB_ACTIONS::pointEditorAddCorner` ..). */
export const FP_POINT_OP_ACTION: Readonly<Record<FpPointOp, string>> = {
  addCorner: "pcbnew.PointEditor.addCorner",
  removeCorner: "pcbnew.PointEditor.removeCorner",
  chamferCorner: "pcbnew.PointEditor.chamferCorner",
  moveCorner: "pcbnew.InteractiveEdit.moveCorner",
  moveMidpoint: "pcbnew.InteractiveEdit.moveMidpoint",
};

/** How near the pointer must be to an edit point to be on it (the canvas' own reach for a drag). */
export const fpEditPointToleranceNm = (scale: number): number => Math.max(100_000, 10 / (scale || 1));

/** What the point editor holds -- the one selected graphic or zone (not a locked one) -- and the edit point under `at`; null without one. */
export type FpPointEditHeld = { kind: "shape"; shape: Shape; point: EditPoint | null } | { kind: "zone"; zone: FpZoneEdit; point: EditPoint | null };

export function fpPointEditAt(api: FootprintEditorApi, at: readonly [number, number]): FpPointEditHeld | null {
  const st = api.getState();
  const fp = st.footprint;
  if (!fp || st.activeTool !== "select") return null;
  const locked = (id: string) => fpItemLocked(fp, id);
  const tol = fpEditPointToleranceNm(st.view.scale);
  const shape = fpPointEditTarget(fp.graphics, st.selection, locked);
  if (shape) return { kind: "shape", shape, point: fpPointAt(shape, at, tol) };
  const zone = fpZoneEditTarget(fp.items?.zones ?? [], st.selection, locked);
  return zone ? { kind: "zone", zone, point: fpZonePointAt(zone, at, tol) } : null;
}

/**
 * Which operations the context menu offers at `at` (`makeShapeModificationMenu`'s rows with the point editor's conditions): Move Corner To... with a corner under the
 * pointer (`HasCorner()`), Move Midpoint To... with a side's middle (`HasMidpoint()`), Create Corner on a zone, segment, polygon or arc (`Count( 1 ) && canAddCornerCondition`),
 * Remove Corner by `CanRemoveCorner`, Chamfer Corner on a zone or polygon (`canChamferCornerCondition`).
 */
export function fpPointOpsAt(api: FootprintEditorApi, at: readonly [number, number]): FpPointOp[] {
  const t = fpPointEditAt(api, at);
  if (!t) return [];
  const out: FpPointOp[] = [];
  if (t.point?.kind === "corner") out.push("moveCorner");
  if (t.point?.kind === "midpoint") out.push("moveMidpoint");
  if (t.kind === "zone" || fpCanAddCorner(t.shape)) out.push("addCorner");
  if (t.kind === "zone" ? fpZoneCanRemoveCorner(t.zone, t.point) : fpCanRemoveCorner(t.shape, t.point)) out.push("removeCorner");
  if (t.kind === "zone" || fpCanChamferCorner(t.shape)) out.push("chamferCorner");
  return out;
}

/** Run `op` with the pointer at `at` (world nm). `arcModeIndex`: the Footprint Editor's arc editing mode (the preference's index). */
export function runFpPointOp(api: FootprintEditorApi, op: FpPointOp, at: readonly [number, number], arcModeIndex: number): void {
  const t = fpPointEditAt(api, at);
  if (!t) return;
  const point = t.point;
  // `getViewControls()->GetCursorPosition()`: the cursor on the grid
  const g = snapGridNm(api.getState().gridNm) || 1;
  const cursor: [number, number] = [Math.round(at[0] / g) * g, Math.round(at[1] / g) * g];
  if (t.kind === "zone") {
    const z = t.zone;
    const apply = (edit: FpZoneCornerEdit | null) => {
      if (edit) void api.zoneEdit(z.id, edit.kind === "remove" ? null : fpZoneWith(z, edit.contours));
    };
    if (op === "addCorner") return apply(fpZoneAddCorner(z, cursor));
    if (op === "removeCorner") return apply(fpZoneRemoveCorner(z, point));
    if (op === "chamferCorner") return apply(fpZoneChamferCorner(z, cursor));
    if (!point) return;
    return openMoveDialog(point, (x, y) => {
      const moved = fpZoneDrag(z, point, [x, y]);
      if (moved) void api.zoneEdit(z.id, fpZoneWith(z, moved));
    });
  }
  const { shape } = t;
  const id = shape.id;
  if (!id) return;
  switch (op) {
    case "addCorner": {
      const edit = fpAddCorner(shape, cursor);
      if (edit) void api.cornerEdit(id, edit);
      return;
    }
    case "removeCorner": {
      const edit = fpRemoveCorner(shape, point);
      if (edit) void api.cornerEdit(id, edit);
      return;
    }
    case "chamferCorner": {
      const edit = fpChamferCorner(shape, cursor);
      if (edit) void api.cornerEdit(id, edit);
      return;
    }
    case "moveCorner":
    case "moveMidpoint": {
      // `movePoint`: on whatever point is under the pointer (`HasPoint()`), titled by its kind
      if (!point) return;
      openMoveDialog(point, (x, y) => {
        const next = fpMovePoint(shape, point, [x, y], arcModeIndex);
        if (next) void api.cornerEdit(id, { kind: "set", shape: next });
      });
      return;
    }
  }
}

/** `movePoint`'s `WX_PT_ENTRY_DIALOG` on `point`, titled by its kind; OK hands the typed place on. */
function openMoveDialog(point: EditPoint, onOk: (x: number, y: number) => void): void {
  openSweepDialog({ kind: "element", element: createElement(PointEntryDialog, { title: fpMovePointTitle(point), x: point.pos[0], y: point.pos[1], onOk }) });
}
