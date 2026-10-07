// The point editor's rows of the UI-actions sweep (docs/parity/UI-ACTIONS.md): Move Corner To..., Move Midpoint To...,
// Edit Corners..., Remove Corner, Chamfer Corner and the three arc editing modes (plus Cycle Arc Editing Mode). Each cites
// PCB_POINT_EDITOR (pcbnew/tools/pcb_point_editor.cpp at 8303b2ad) or EDIT_TOOL; the maths is kicad-port/pcbPointEdit.ts and
// the dialogs components/PcbPointEditDialogs.tsx. Called from `registerPcbEditSweep`.
//
// KiCad's "active point" is the handle the pointer is over; the context menu was opened where the pointer was, so the actions read
// that place (`state.pcbx.menuCursorUm`, else the live cursor).

import { createElement } from "react";
import type { Shape } from "../api/types";
import {
  ARC_EDIT_MODE_LABEL,
  activeEditPoint,
  chamferCorner,
  incrementArcEditMode,
  moveRingPoint,
  moveShapePoint,
  nearestVertex,
  removeCorner,
  ringEditPoints,
  shapeEditPoints,
  shapeToCmd,
  type ArcEditMode,
  type EditPoint,
} from "../kicad-port/pcbPointEdit";
import { PointEntryDialog } from "../components/PcbPointEditDialogs";
import { openSweepDialog } from "./pcbSweepDialogs";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";

/** What the point editor edits: the one selected zone (its outline) or graphic shape. */
export type PointItem = { kind: "zone"; id: string; ring: [number, number][] } | { kind: "shape"; id: string; shape: Shape };

/** `PCB_POINT_EDITOR::makePoints` runs for a single selected item: a zone (not a teardrop) or any graphic shape. */
export function pointItemOf(board: SweepCtx["state"]["board"], selection: ReadonlySet<string>): PointItem | null {
  if (!board || selection.size !== 1) return null;
  const id = [...selection][0]!;
  const zone = board.routing?.zones.find((z) => z.id === id);
  if (zone) return zone.teardrop ? null : { kind: "zone", id, ring: zone.outline.map((p) => [p[0], p[1]] as [number, number]) };
  const shape = board.drawings?.shapes.find((s) => s.id === id);
  return shape ? { kind: "shape", id, shape } : null;
}

/** The ring of a polygon-like item (a zone's outline, a polygon shape's points), else null. */
export function ringOf(item: PointItem): [number, number][] | null {
  if (item.kind === "zone") return item.ring;
  return item.shape.kind === "polygon" ? item.shape.pts.map((p) => [p[0], p[1]] as [number, number]) : null;
}

/** The handles of an item. */
export function pointsOfItem(item: PointItem): EditPoint[] {
  return item.kind === "zone" ? ringEditPoints(item.ring) : shapeEditPoints(item.shape);
}

/** How near the pointer must be to a handle to count as on it, in um (the zone corner handles' own reach). */
export function handleToleranceUm(viewScale: number): number {
  return Math.max(150, 6 / viewScale);
}

export function registerPcbPointEditSweep(m: Map<string, () => void>, ctx: SweepCtx): void {
  const { state, dispatch } = ctx;
  const board = state.board;
  const { toast, pcbOnly, lockedIds, applyEdit } = createSweepHelpers(ctx);

  /** The single selected editable item and the handle under the pointer (`m_editedPoint`). */
  const target = (): { item: PointItem; points: EditPoint[]; active: EditPoint | null; at: [number, number] } | null => {
    const item = pointItemOf(board, state.selection);
    if (!item || lockedIds.has(item.id)) return null;
    const cursor = state.pcbx.menuCursorUm ?? state.cursorUm;
    if (!cursor) return null;
    const points = pointsOfItem(item);
    return { item, points, active: activeEditPoint(points, [cursor.x, cursor.y], handleToleranceUm(state.view.scale)), at: [cursor.x, cursor.y] };
  };

  /** Replace the item's geometry: a zone outline in place, a shape as a new shape (its id is its geometry's). */
  const applyGeometry = (item: PointItem, next: { ring: [number, number][] } | { shape: ReturnType<typeof shapeToCmd> }): void => {
    if ("ring" in next) {
      if (item.kind === "zone") {
        void applyEdit([{ op: "set_zone_outline", id: item.id, outline: next.ring.map(([x, y]) => ({ x, y })) }]);
        return;
      }
      if (item.shape.kind !== "polygon") return;
      void applyEdit([{ op: "delete_shape", id: item.id }, { op: "add_shape", shape: shapeToCmd({ ...item.shape, pts: next.ring }) }], [item.id]);
      return;
    }
    if (item.kind === "shape") void applyEdit([{ op: "delete_shape", id: item.id }, { op: "add_shape", shape: next.shape }], [item.id]);
  };

  // ------------------------------------------------- Move Corner To... / Move Midpoint To...
  // PCB_POINT_EDITOR::movePoint: WX_PT_ENTRY_DIALOG on the active point, then `updateItem` with the typed position.
  const movePoint = (kind: "corner" | "midpoint") =>
    pcbOnly(() => {
      const t = target();
      if (!t?.active || t.active.kind !== kind) return; // `HasCorner()` / `HasMidpoint()`
      const point = t.active;
      openSweepDialog({
        kind: "element",
        element: createElement(PointEntryDialog, {
          title: kind === "midpoint" ? "Move Midpoint to Location" : "Move Corner to Location",
          x: point.pos[0],
          y: point.pos[1],
          onOk: (x: number, y: number) => {
            if (t.item.kind === "zone" || (t.item.kind === "shape" && t.item.shape.kind === "polygon")) {
              const ring = ringOf(t.item);
              const moved = ring ? moveRingPoint(ring, point, [x, y]) : null;
              if (moved) applyGeometry(t.item, { ring: moved });
              return;
            }
            if (t.item.kind !== "shape") return;
            const next = moveShapePoint(t.item.shape, point, [x, y], state.pcbx.arcEditMode);
            if (!next) {
              toast("That position is not possible for this point.");
              return;
            }
            applyGeometry(t.item, { shape: next });
          },
        }),
      });
    });
  m.set("pcbnew.InteractiveEdit.moveCorner", movePoint("corner"));
  m.set("pcbnew.InteractiveEdit.moveMidpoint", movePoint("midpoint"));

  // ------------------------------------------------------------------------ Remove Corner
  // PCB_POINT_EDITOR::removeCorner (enabled by `CanRemoveCorner`): the active vertex of a zone or polygon, while more than three remain.
  m.set(
    "pcbnew.PointEditor.removeCorner",
    pcbOnly(() => {
      const t = target();
      if (!t?.active || t.active.kind !== "corner" || !t.active.id.startsWith("v")) return;
      const ring = ringOf(t.item);
      const next = ring ? removeCorner(ring, Number(t.active.id.slice(1))) : null;
      if (next) applyGeometry(t.item, { ring: next });
    })
  );

  // ----------------------------------------------------------------------- Chamfer Corner
  // PCB_POINT_EDITOR::chamferCorner: the vertex nearest the pointer of a zone or polygon.
  m.set(
    "pcbnew.PointEditor.chamferCorner",
    pcbOnly(() => {
      const t = target();
      const ring = t ? ringOf(t.item) : null;
      if (!t || !ring) return;
      const next = chamferCorner(ring, nearestVertex(ring, t.at));
      if (!next) {
        toast("That corner cannot be chamfered.");
        return;
      }
      applyGeometry(t.item, { ring: next });
    })
  );

  // ----------------------------------------------------------------------- Edit Corners...
  // EDIT_TOOL::EditVertices -> PCB_BASE_EDIT_FRAME::OpenVertexEditor: one selected polygon or zone.
  m.set(
    "pcbnew.InteractiveEdit.editVertices",
    pcbOnly(() => {
      const item = pointItemOf(board, state.selection);
      if (!item || lockedIds.has(item.id) || !ringOf(item)) {
        toast("Select a single polygon or zone to edit its corners.");
        return;
      }
      dispatch({ type: "PCBX", patch: { vertexEditorOpen: true } });
    })
  );

  // ------------------------------------------------------------------- arc editing modes
  // PCB_POINT_EDITOR::changeArcEditMode: the parameter of each action (or `IncrementArcEditMode` for the cycle) becomes the mode.
  const setMode = (mode: ArcEditMode | "cycle") => () => {
    const next = mode === "cycle" ? incrementArcEditMode(state.pcbx.arcEditMode) : mode;
    dispatch({ type: "PCBX", patch: { arcEditMode: next } });
    toast(`Arc editing: ${ARC_EDIT_MODE_LABEL[next]}`);
  };
  m.set("pcbnew.PointEditor.arcKeepCenter", pcbOnly(setMode("keep_center_adjust_angle_radius")));
  m.set("pcbnew.PointEditor.arcKeepEndpoint", pcbOnly(setMode("keep_endpoints_or_start_direction")));
  m.set("pcbnew.PointEditor.arcKeepRadius", pcbOnly(setMode("keep_center_ends_adjust_angle")));
  m.set("common.Interactive.cycleArcEditMode", pcbOnly(setMode("cycle")));
}
