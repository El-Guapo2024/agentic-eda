// KiCad 25ec2335's Draw Arc session as eeschema runs it for both of its editors (`EE_GRAPHIC_TOOL::DrawArc` and `drawManagedShape`, eeschema/tools/ee_graphic_tool.cpp;
// `ARC_MODE_SESSION`, include/tool/arc_draw_behavior.h; `ARC_GEOM_MANAGER`, common/preview_items/arc_geom_manager.cpp, ported in arcGeom.ts): the schematic's arc tool and the
// Symbol Editor's run the same one (UI-LEFT-3 Y3-8; the schematic's S3-28 builds on it). What a click does in each of the five modes (`ARC_MODE_SESSION::PlacePoint`: a
// TANGENT arc's first click on a drawn graphic seeds it there), what a finished arc leaves for the next (`ArcTangentSeedNear` of the arc just committed while the mode is
// TANGENT), what finishing early keeps (`finishInteractive`), Backspace (`RemoveLastPoint`), the mode switched mid-draw (`HandleModeEvent` -> `SetMode`), and the mode each
// editor keeps (`APP_SETTINGS_BASE::m_ArcDrawMode`, `editing.arc_draw_mode`: one per application, CENTER_START_END at first).
//
// Points are the editor's internal space (nm, +y down) -- each editor converts its own.
// Pure: no React, no DOM.
import { ARC_DRAW_MODES, ARC_SET_ANGLE, ARC_SET_ORIGIN, arcClick, arcHasTangentSeed, arcRemoveLastPoint, arcSetMode, arcSetTangentSeed, arcToThreePoints, newArcGeomIn, type ArcDrawMode, type ArcGeom, type ArcPt, type ThreePointArc } from "./arcGeom";
import { arcTangentSeedNear, type ArcTangentSeed } from "./arcTangentSeed";

/** What a click of the session gives: the construction to keep (null: nothing in progress), and the arc to commit when the click completed one. */
export interface ArcSessionStep {
  geom: ArcGeom | null;
  arc: ThreePointArc | null;
  /** The click began an arc (`!started` in `drawManagedShape`: `ACTIONS::selectionClear` first). */
  started: boolean;
}

/**
 * One click of Draw Arc (`drawManagedShape`'s `IsClick( BUT_LEFT )` -> `ARC_MODE_SESSION::PlacePoint`, `aBehavior.AddPoint( cursorPos, true )`): a new arc starts in `mode`
 * (`StartArcDrawMode`); the first click of a TANGENT arc with no seed asks `seedAt` for a graphic under it (`findTangentSeed`) and starts from it instead of placing a point; the
 * click that completes the arc gives it, and -- the mode being TANGENT -- the next arc starts seeded from its end (`ArcTangentSeedNear( *arc, lastClick )`). A point the manager
 * refuses steps it back (`MULTISTEP_GEOM_MANAGER::AddPoint`).
 */
export function arcSessionClick(prev: ArcGeom | null, at: ArcPt, mode: ArcDrawMode, seedAt: (at: ArcPt) => ArcTangentSeed | null): ArcSessionStep {
  const started = !prev;
  const g = prev ?? newArcGeomIn(mode);
  if (g.mode === "tangent" && g.step === ARC_SET_ORIGIN && !g.hasSeed) {
    const seed = seedAt(at);
    if (seed) return { geom: arcSetTangentSeed(g, seed.start, seed.direction, seed.directionIsAxis), arc: null, started };
  }
  const { geom, arc } = arcClick(g, at, false);
  if (arc) {
    // "The mode can change mid-draw so chain from whatever the settings hold now"
    const seed = mode === "tangent" ? arcTangentSeedNear({ kind: "arc", start: arc.start, mid: arc.mid, end: arc.end }, at) : null;
    return { geom: seed ? arcSetTangentSeed(newArcGeomIn("tangent"), seed.start, seed.direction, seed.directionIsAxis) : null, arc, started };
  }
  return { geom, arc: null, started };
}

/** The arc a construction stands for now (`ApplyToShape`: the start and end in increasing angle, or the clicked ends of an exact mode), null while it has no sweep -- the preview. */
export function arcSessionShape(g: ArcGeom): ThreePointArc | null {
  if (g.step === ARC_SET_ORIGIN && !arcHasTangentSeed(g)) return null;
  return arcToThreePoints(g);
}

/**
 * `finishInteractive` / a double click (`SHAPE_DRAW_RESULT::FINISHED`): the arc goes in as it stands -- once the centre mode has its end angle, or an exact mode's arc is
 * determined (`aBehavior.ApplyToShape` has run) -- else nothing.
 */
export function arcSessionFinish(g: ArcGeom | null): ThreePointArc | null {
  if (!g) return null;
  return g.step === ARC_SET_ANGLE || (g.mode && g.mode !== "center_start_end") ? arcSessionShape(g) : null;
}

/** `deleteLastPoint` (Backspace): `ARC_MODE_SESSION::RemoveLastPoint` -- back a step; back at the first point nothing is left of the arc (a seed goes with it). */
export function arcSessionBack(g: ArcGeom): ArcGeom | null {
  const back = arcRemoveLastPoint(g);
  return back.step === ARC_SET_ORIGIN ? null : back;
}

/** `ARC_MODE_SESSION::HandleModeEvent` -> `SetMode`: the arc being drawn goes on in the new mode (the start kept between two modes that start at the start). */
export function arcSessionSetMode(g: ArcGeom | null, mode: ArcDrawMode): ArcGeom | null {
  return g ? arcSetMode(g, mode) : null;
}

/**
 * An editor's `m_ArcDrawMode` (`editing.arc_draw_mode` of its own settings file: eeschema.json, symbol_editor.json), kept in this browser under `key` as KiCad keeps it in the
 * user's settings; CENTER_START_END until set.
 */
export function arcDrawModeSetting(key: string): { get: () => ArcDrawMode; set: (mode: ArcDrawMode) => ArcDrawMode } {
  let mode: ArcDrawMode | null = null;
  return {
    get: () => {
      if (mode) return mode;
      try {
        const v = globalThis.localStorage?.getItem(key);
        mode = v && (ARC_DRAW_MODES as readonly string[]).includes(v) ? (v as ArcDrawMode) : "center_start_end";
      } catch {
        mode = "center_start_end";
      }
      return mode;
    },
    set: (next) => {
      mode = next;
      try {
        globalThis.localStorage?.setItem(key, next);
      } catch {
        // no storage: the mode lasts the session
      }
      return next;
    },
  };
}
