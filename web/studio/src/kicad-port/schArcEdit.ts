// The schematic's arc point editor -- `SCH_POINT_EDITOR` (eeschema/tools/sch_point_editor.cpp at 8303b2ad) on one selected arc: its four edit points from
// `EDA_ARC_POINT_EDIT_BEHAVIOR::MakePoints` (common/tool/point_editor_behavior.cpp) -- the start, the middle (`GetArcMid`), the end and the centre, with the
// indicator lines from the centre to the two ends -- and what dragging one does (`UpdateItem`), in the mode `EESCHEMA_SETTINGS::m_Drawing.arc_edit_mode`
// holds ("editing.arc_edit_mode", KEEP_CENTER_ADJUST_ANGLE_RADIUS by default; `SCH_POINT_EDITOR::changeArcEditMode` sets it from the three
// `pointEditorArcKeep*` actions and Ctrl+Space's `cycleArcEditMode`). The arc maths is the board's port of the same behaviour (kicad-port/pcbPointEdit.ts
// `editArc`), which runs in nanometres, the schematic's unit here. UI-LEFT S13.
//
// INACTIVE since UI-LEFT-2 SS3: `arcEditPoints`, `findArcPoint` and `moveArcPoint` were the arc-only editor; the schematic's point editor of every shape
// (kicad-port/eePointEdit.ts, schPointEdit.ts) does the arc too. The mode setting (`schArcEditMode`, `setSchArcEditMode`) is still the one it reads.
import type { SchGraphicShape } from "../api/schEditTypes";
import { arcCenter } from "./pcbGeom";
import { DEFAULT_ARC_EDIT_MODE, editArc, incrementArcEditMode, type ArcEditMode } from "./pcbPointEdit";

type P = [number, number];

/** `ARC_POINTS`' order in `MakePoints`: start, mid, end, centre. */
export type ArcRole = "start" | "mid" | "end" | "center";

export interface ArcEditPoints {
  points: Array<{ role: ArcRole; pos: P }>;
  /** `AddIndicatorLine( center, start )`, `AddIndicatorLine( center, end )`. */
  lines: Array<[P, P]>;
}

type ArcShape = Extract<SchGraphicShape, { type: "arc" }>;

const pt = (p: { x: number; y: number }): P => [p.x, p.y];

/** `EDA_ARC_POINT_EDIT_BEHAVIOR::MakePoints` of an arc kept as three points (its centre is `CalcArcCenter` of them, `GetArcMid` is the stored middle). */
export function arcEditPoints(shape: ArcShape): ArcEditPoints {
  const start = pt(shape.start);
  const mid = pt(shape.mid);
  const end = pt(shape.end);
  const c = arcCenter({ start, mid, end });
  const center: P = [Math.round(c[0]), Math.round(c[1])];
  return {
    points: [
      { role: "start", pos: start },
      { role: "mid", pos: mid },
      { role: "end", pos: end },
      { role: "center", pos: center },
    ],
    lines: [
      [center, start],
      [center, end],
    ],
  };
}

/** `EDIT_POINTS::FindPoint`: the edit point within `tolerance` of `at` (the first in `MakePoints`' order), or null. */
export function findArcPoint(e: ArcEditPoints, at: readonly [number, number], tolerance: number): ArcRole | null {
  return e.points.find((p) => Math.abs(p.pos[0] - at[0]) <= tolerance && Math.abs(p.pos[1] - at[1]) <= tolerance)?.role ?? null;
}

/**
 * `EDA_ARC_POINT_EDIT_BEHAVIOR::UpdateItem` for the point `role` dragged to `to` (the cursor, `BestSnapAnchor` on the graphics grid): the arc it gives, or
 * null when the drag makes no arc (an end on the other end). The centre in either keep-centre mode moves the whole arc; the middle there sets the radius.
 */
export function moveArcPoint(shape: ArcShape, role: ArcRole, to: readonly [number, number], mode: ArcEditMode): ArcShape | null {
  const a = editArc({ start: pt(shape.start), mid: pt(shape.mid), end: pt(shape.end) }, role, [to[0], to[1]], mode);
  if (!a) return null;
  if (a.start[0] === a.end[0] && a.start[1] === a.end[1]) return null;
  const r = (p: readonly [number, number]) => ({ x: Math.round(p[0]), y: Math.round(p[1]) });
  return { type: "arc", start: r(a.start), mid: r(a.mid), end: r(a.end) };
}

/** `EESCHEMA_SETTINGS::m_Drawing.arc_edit_mode`, kept as KiCad keeps it, in the user's settings (this browser's), not the project's. */
const KEY = "eda.sch.arcEditMode";
const MODES: readonly ArcEditMode[] = ["keep_center_adjust_angle_radius", "keep_center_ends_adjust_angle", "keep_endpoints_or_start_direction"];
let mode: ArcEditMode | null = null;

export function schArcEditMode(): ArcEditMode {
  if (mode) return mode;
  try {
    const v = globalThis.localStorage?.getItem(KEY);
    mode = v && (MODES as readonly string[]).includes(v) ? (v as ArcEditMode) : DEFAULT_ARC_EDIT_MODE;
  } catch {
    mode = DEFAULT_ARC_EDIT_MODE;
  }
  return mode;
}

/** `SCH_POINT_EDITOR::changeArcEditMode`: the action's mode, or the next one (`IncrementArcEditMode`) for `cycleArcEditMode`. */
export function setSchArcEditMode(next: ArcEditMode | "cycle"): ArcEditMode {
  mode = next === "cycle" ? incrementArcEditMode(schArcEditMode()) : next;
  try {
    globalThis.localStorage?.setItem(KEY, mode);
  } catch {
    // no storage: the mode lasts the session
  }
  return mode;
}
