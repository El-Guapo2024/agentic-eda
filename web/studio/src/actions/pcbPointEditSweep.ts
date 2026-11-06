// The point editor's rows of the UI-actions sweep (docs/parity/UI-ACTIONS.md): Move Corner To..., Move Midpoint To...,
// Edit Corners..., Remove Corner, Chamfer Corner and the three arc editing modes (plus Cycle Arc Editing Mode). Each cites
// PCB_POINT_EDITOR (pcbnew/tools/pcb_point_editor.cpp at 8303b2ad) or EDIT_TOOL; the maths is kicad-port/pcbPointEdit.ts and
// the dialogs components/PcbPointEditDialogs.tsx. Called from `registerPcbEditSweep`.
//
// KiCad's "active point" is the handle the pointer is over; the context menu was opened where the pointer was, so the actions read
// that place (`state.pcbx.menuCursorNm`, else the live cursor).

import { createElement } from "react";
import type { Shape } from "../api/types";
import {
  ARC_EDIT_MODE_LABEL,
  activeEditPoint,
  chamferContourCorner,
  contourEditPoints,
  incrementArcEditMode,
  moveContourPoint,
  moveShapePoint,
  nearestContourVertex,
  shapeEditPoints,
  shapeToCmd,
  type ArcEditMode,
  type EditPoint,
} from "../kicad-port/pcbPointEdit";
import { canRemoveContourCorner, removeContourCorner, zoneContours, zoneOutlineCmd } from "../kicad-port/zonePointEditor";
import { PointEntryDialog } from "../components/PcbPointEditDialogs";
import { reshapeCmds } from "../kicad-port/shapeFill";
import { deleteCmd } from "../kicad-port/pcbConvert";
import { openSweepDialog } from "./pcbSweepDialogs";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";

/** What the point editor edits: the one selected zone (its outline and its holes, `ZONE::Outline()`) or graphic shape. */
export type PointItem = { kind: "zone"; id: string; ring: [number, number][]; holes: [number, number][][] } | { kind: "shape"; id: string; shape: Shape };

/** `PCB_POINT_EDITOR::makePoints` runs for a single selected item: a zone (not a teardrop) or any graphic shape. */
export function pointItemOf(board: SweepCtx["state"]["board"], selection: ReadonlySet<string>): PointItem | null {
  if (!board || selection.size !== 1) return null;
  const id = [...selection][0]!;
  const zone = board.routing?.zones.find((z) => z.id === id);
  if (zone) {
    if (zone.teardrop) return null;
    const [ring, ...holes] = zoneContours(zone);
    return { kind: "zone", id, ring: ring!, holes };
  }
  const shape = board.drawings?.shapes.find((s) => s.id === id);
  return shape ? { kind: "shape", id, shape } : null;
}

/** The ring of a polygon-like item (a zone's outline, a polygon shape's points), else null. */
export function ringOf(item: PointItem): [number, number][] | null {
  if (item.kind === "zone") return item.ring;
  return item.shape.kind === "polygon" ? item.shape.pts.map((p) => [p[0], p[1]] as [number, number]) : null;
}

/** The contours of a polygon-like item in `SHAPE_POLY_SET` order: a zone's outline then its holes, a polygon shape's one ring (the IR polygon has no holes); else null. */
export function contoursOf(item: PointItem): [number, number][][] | null {
  if (item.kind === "zone") return [item.ring, ...item.holes];
  const ring = ringOf(item);
  return ring ? [ring] : null;
}

/** The handles of an item: every contour's vertices and sides for a zone (`BuildForPolyOutline` with holes). */
export function pointsOfItem(item: PointItem): EditPoint[] {
  return item.kind === "zone" ? contourEditPoints([item.ring, ...item.holes]) : shapeEditPoints(item.shape);
}

/** `PCB_POINT_EDITOR::CanRemoveCorner` for the handle `point` of `item`: a vertex (not a side) of a zone or polygon whose removal leaves the polygon whole. */
export function canRemoveItemCorner(item: PointItem, point: EditPoint | null): boolean {
  const contours = contoursOf(item);
  return !!contours && point?.kind === "corner" && point.id.startsWith("v") && canRemoveContourCorner(contours, Number(point.id.slice(1)));
}

/** How near the pointer must be to a handle to count as on it, in um (the zone corner handles' own reach). */
export function handleToleranceNm(viewScale: number): number {
  return Math.max(150_000, 6 / viewScale);
}

export function registerPcbPointEditSweep(m: Map<string, () => void>, ctx: SweepCtx): void {
  const { state, dispatch } = ctx;
  const board = state.board;
  const { toast, pcbOnly, lockedIds, applyEdit } = createSweepHelpers(ctx);

  /** The single selected editable item and the handle under the pointer (`m_editedPoint`). */
  const target = (): { item: PointItem; points: EditPoint[]; active: EditPoint | null; at: [number, number] } | null => {
    const item = pointItemOf(board, state.selection);
    if (!item || lockedIds.has(item.id)) return null;
    const cursor = state.pcbx.menuCursorNm ?? state.cursorNm;
    if (!cursor) return null;
    const points = pointsOfItem(item);
    return { item, points, active: activeEditPoint(points, [cursor.x, cursor.y], handleToleranceNm(state.view.scale)), at: [cursor.x, cursor.y] };
  };

  /**
   * Replace the item's geometry in place: a zone's outline and holes (one `set_zone_outline`, the holes with it when the zone has or had some), a shape's
   * geometry (`replace_shape`: it keeps its id and its group).
   */
  const applyGeometry = (item: PointItem, next: { contours: [number, number][][] } | { shape: ReturnType<typeof shapeToCmd> }, keepFill = false): void => {
    if ("contours" in next) {
      if (item.kind === "zone") {
        void applyEdit([zoneOutlineCmd(item.id, next.contours, item.holes.length > 0, keepFill)]);
        return;
      }
      if (item.shape.kind !== "polygon" || !next.contours[0]) return;
      const re = reshapeCmds(board, item.id, shapeToCmd({ ...item.shape, pts: next.contours[0] }));
      void applyEdit(re.cmds, re.removed);
      return;
    }
    if (item.kind === "shape") {
      const re = reshapeCmds(board, item.id, next.shape);
      void applyEdit(re.cmds, re.removed);
    }
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
              const contours = contoursOf(t.item);
              const moved = contours ? moveContourPoint(contours, point, [x, y]) : null;
              if (moved) applyGeometry(t.item, { contours: moved });
              return;
            }
            if (t.item.kind !== "shape") return;
            const next = moveShapePoint(t.item.shape, point, [x, y], state.prefs.pcbArcEditMode);
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
  // PCB_POINT_EDITOR::removeCorner (enabled by `CanRemoveCorner`): the active vertex of a zone or polygon -- of its outline while more than three remain,
  // of a hole always (a hole of three corners goes whole, `RemoveContour`). `removeCorner` itself removes the item when its outline has three corners or
  // fewer (`commit.Remove( item )`), which the menu's condition keeps from being asked.
  m.set(
    "pcbnew.PointEditor.removeCorner",
    pcbOnly(() => {
      const t = target();
      if (!t?.active || t.active.kind !== "corner" || !t.active.id.startsWith("v")) return;
      const contours = contoursOf(t.item);
      const next = contours ? removeContourCorner(contours, Number(t.active.id.slice(1))) : null;
      if (!next) return;
      if (next.kind === "remove_item") {
        if (!board) return;
        // `m_toolMgr->RunAction( ACTIONS::selectionClear ); commit.Remove( item )`
        void applyEdit([deleteCmd(board, t.item.id)], [t.item.id], null, { keep: false });
        return;
      }
      // `removeCorner`: `zone->SetNeedRefill( true )`, the fill as it was (UI-LEFT P29)
      applyGeometry(t.item, { contours: next.contours }, true);
    })
  );

  // ----------------------------------------------------------------------- Chamfer Corner
  // PCB_POINT_EDITOR::chamferCorner: the vertex nearest the pointer of a zone (its outline or a hole) or polygon.
  m.set(
    "pcbnew.PointEditor.chamferCorner",
    pcbOnly(() => {
      const t = target();
      const contours = t ? contoursOf(t.item) : null;
      if (!t || !contours) return;
      const next = chamferContourCorner(contours, nearestContourVertex(contours, t.at));
      if (!next) {
        toast("That corner cannot be chamfered.");
        return;
      }
      // `chamferCorner`: `zone->SetNeedRefill( true )`, the fill as it was
      applyGeometry(t.item, { contours: next }, true);
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
  // PCB_POINT_EDITOR::changeArcEditMode: the parameter of each action (or `IncrementArcEditMode` for the cycle) becomes the mode -- the board editor's setting
  // (`frame()->GetPcbNewSettings()->m_ArcEditMode = mode`), which Preferences > PCB Editor > Editing Options shows as "Arc editing mode:" (UI-LEFT-2 FF7).
  const setMode = (mode: ArcEditMode | "cycle") => () => {
    const next = mode === "cycle" ? incrementArcEditMode(state.prefs.pcbArcEditMode) : mode;
    dispatch({ type: "PATCH_PREFERENCES", patch: { pcbArcEditMode: next } });
    toast(`Arc editing: ${ARC_EDIT_MODE_LABEL[next]}`);
  };
  // (`SCH_POINT_EDITOR::changeArcEditMode` takes the same four actions for the schematic's own mode, registered before: another tab keeps its handler)
  const chain = (name: string, fn: () => void) => {
    const other = m.get(name);
    m.set(name, () => (state.tab === "pcb" ? fn() : other?.()));
  };
  chain("pcbnew.PointEditor.arcKeepCenter", setMode("keep_center_adjust_angle_radius"));
  chain("pcbnew.PointEditor.arcKeepEndpoint", setMode("keep_endpoints_or_start_direction"));
  chain("pcbnew.PointEditor.arcKeepRadius", setMode("keep_center_ends_adjust_angle"));
  chain("common.Interactive.cycleArcEditMode", setMode("cycle"));
}
