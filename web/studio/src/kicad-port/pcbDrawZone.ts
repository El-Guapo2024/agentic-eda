// A click of `DRAWING_TOOL::DrawZone` (pcbnew/tools/drawing_tool.cpp at 8303b2ad), which draws zones, rule areas, cutouts, similar zones, via stitchings and graphic
// polygons, over `POLYGON_GEOM_MANAGER` (kicad-port/polygonGeom.ts, the port the schematic's rule area runs on):
//
//   LEADER_MODE angleSnap = GetAngleSnapMode();
//   if( evt->Modifier( MD_CTRL ) ) angleSnap = LEADER_MODE::DIRECT;
//   ...
//   polyGeomMgr.SetLeaderMode( angleSnap );
//   ...
//   const bool endPolygon = evt->IsDblClick( BUT_LEFT ) || evt->IsAction( &PCB_ACTIONS::closeOutline ) || polyGeomMgr.NewPointClosesOutline( cursorPos );
//   if( endPolygon ) { polyGeomMgr.SetFinished(); polyGeomMgr.Reset(); cleanup(); m_frame->PopTool( aEvent ); break; }
//   else if( polyGeomMgr.AddPoint( cursorPos ) ) ...
//
// A click on the first corner closes the outline; in the 45- and 90-degree line modes a click locks in the leader's bend and its end (two corners).
import { addPoint, newPointClosesOutline, setCursorPosition, type LeaderMode, type PolyGeom, type Pt } from "./polygonGeom";

export type AngleSnapMode = "direct" | "90" | "45";

/** `GetAngleSnapMode()`, `LEADER_MODE::DIRECT` while Ctrl is held. */
export function leaderModeOf(mode: AngleSnapMode | undefined, ctrl: boolean): LeaderMode {
  if (ctrl || mode === "direct") return "direct";
  return mode === "90" ? "deg90" : "deg45";
}

const geomOf = (pts: readonly Pt[], mode: LeaderMode): PolyGeom => ({ mode, locked: pts.map((p): Pt => [p[0], p[1]]), leader: [], loop: [] });

/** What a click at `at` does to the corners `pts`: closes the outline (on the first corner), or locks in the next corner(s). */
export function zoneClick(pts: readonly Pt[], at: Pt, mode: LeaderMode): { finish: true } | { finish: false; pts: [number, number][] } {
  const g = setCursorPosition(geomOf(pts, mode), at);
  if (newPointClosesOutline(g, at)) return { finish: true };
  return { finish: false, pts: addPoint(g, at).locked.map((p): [number, number] => [p[0], p[1]]) };
}

/** `m_leaderPts` for the cursor at `cursor`: the rubber band from the last corner, bent in the 45- and 90-degree modes. Empty with no corner. */
export function zoneLeader(pts: readonly Pt[], cursor: Pt, mode: LeaderMode): [number, number][] {
  return setCursorPosition(geomOf(pts, mode), cursor).leader.map((p): [number, number] => [p[0], p[1]]);
}
