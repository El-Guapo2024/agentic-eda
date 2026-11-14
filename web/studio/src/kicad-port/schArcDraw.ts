// KiCad 25ec2335's Draw Arc modes in the schematic (UI-LEFT-2 SS3): the mode setting the frame keeps (`APP_SETTINGS_BASE::m_ArcDrawMode`, eeschema's
// `editing.arc_draw_mode`, CENTER_START_END by default; `StartArcDrawMode` sets it from the action's parameter, `ARC_MODE_SESSION::HandleModeEvent` from a mode action
// or `cycleArcDrawMode` mid-draw), the five actions' names and labels (`SCH_ACTIONS::drawArcCenter` .. `drawArcStartDirEnd`, `DrawArcForMode`), and the tangent
// seed of a first click (`EE_GRAPHIC_TOOL::drawManagedShape`'s `findTangentSeed`: a graphic on the notes layer within `ARC_SEED_PICK_PIXELS` -- a drawn shape, or a
// graphic line).
//
// Pure: no React, no DOM.
import type { Schematic } from "../api/types";
import type { SchGraphic } from "../api/schEditTypes";
import type { ArcDrawMode } from "./arcGeom";
import { arcTangentSeedAt, ARC_SEED_PICK_PIXELS, type ArcTangentSeed, type SeedShape } from "./arcTangentSeed";
import { arcDrawModeSetting } from "./arcDrawSession";

/** `SCH_ACTIONS::DrawArcForMode`: the action of each mode, its friendly name and its tooltip (KiCad 25ec2335 eeschema/tools/sch_actions.cpp). */
export const ARC_MODE_ACTIONS: Record<ArcDrawMode, { action: string; label: string; tooltip: string; icon: string }> = {
  center_start_end: { action: "eeschema.InteractiveDrawing.drawArcCenter", label: "Draw Arc: Center, Start, End", tooltip: "Draw arcs by clicking the center, the start point, then the end point", icon: "add_arc_center" },
  start_end_mid: { action: "eeschema.InteractiveDrawing.drawArcStartEndMid", label: "Draw Arc: Start, End, Midpoint", tooltip: "Draw arcs by clicking the start point, the end point, then a point on the arc", icon: "add_arc_start_end_mid" },
  start_end_center: { action: "eeschema.InteractiveDrawing.drawArcStartEndCenter", label: "Draw Arc: Start, End, Center", tooltip: "Draw arcs by clicking the start point, the end point, then the center", icon: "add_arc_start_end_center" },
  tangent: { action: "eeschema.InteractiveDrawing.drawArcTangent", label: "Draw Arc: Tangent", tooltip: "Draw arcs tangent to the previous arc or to the line or arc ending at the start point", icon: "add_arc_tangent" },
  start_dir_end: { action: "eeschema.InteractiveDrawing.drawArcStartDirEnd", label: "Draw Arc: Start, Direction, End", tooltip: "Draw arcs by clicking the start point, a point giving the start direction, then the end point", icon: "add_arc_start_dir_end" },
};

/** `ACTIONS::cycleArcDrawMode` (Alt+/): "Cycle Arc Drawing Mode", "Switch to the next method of drawing arcs". */
export const CYCLE_ARC_DRAW_MODE = "common.InteractiveDrawing.cycleArcDrawMode";

/** `m_ArcDrawMode` of eeschema's settings (`editing.arc_draw_mode` of eeschema.json), kept in this browser (kicad-port/arcDrawSession.ts `arcDrawModeSetting`). */
const schMode = arcDrawModeSetting("eda.sch.arcDrawMode");

export function schArcDrawMode(): ArcDrawMode {
  return schMode.get();
}

export function setSchArcDrawMode(next: ArcDrawMode): ArcDrawMode {
  return schMode.set(next);
}

const xy = (p: { x: number; y: number }): [number, number] => [p.x, p.y];

/** A drawn graphic as a seed can start from it (`ArcTangentSeedAt` takes segments, arcs, beziers and polygons; a rectangle or circle gives none). */
function seedShapeOf(g: SchGraphic): SeedShape | null {
  const s = g.shape;
  switch (s.type) {
    case "arc":
      return { kind: "arc", start: xy(s.start), mid: xy(s.mid), end: xy(s.end) };
    case "bezier":
      return { kind: "bezier", start: xy(s.start), c1: xy(s.c1), c2: xy(s.c2), end: xy(s.end) };
    case "polygon":
      return { kind: "polygon", pts: s.pts.map(xy), closed: false };
    default:
      return null;
  }
}

/**
 * `findTangentSeed`: "Clicking a graphic on the drawing layer starts a tangent arc from it instead of placing a start point" -- the first drawn shape or graphic line (a
 * notes line of `sch.lines`) whose box, grown by the pick distance, holds `at` and that gives a seed there (`ArcTangentSeedAt`, within `ARC_SEED_PICK_PIXELS` at this
 * zoom). Null when the click is on none.
 */
export function schTangentSeedAt(sch: Schematic, at: readonly [number, number], scale: number): ArcTangentSeed | null {
  const tolerance = Math.round(ARC_SEED_PICK_PIXELS / (scale || 1));
  for (const g of sch.graphics ?? []) {
    const shape = seedShapeOf(g);
    const seed = shape ? arcTangentSeedAt(shape, at, tolerance) : null;
    if (seed) return seed;
  }
  for (const l of sch.lines ?? []) {
    const pts = l.pts.map((q): [number, number] => [q[0], q[1]]);
    for (let i = 0; i + 1 < pts.length; i++) {
      const seed = arcTangentSeedAt({ kind: "segment", start: pts[i]!, end: pts[i + 1]! }, at, tolerance);
      if (seed) return seed;
    }
  }
  return null;
}
