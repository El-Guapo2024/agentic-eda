// The Symbol Editor's point editor -- the same `SCH_POINT_EDITOR` as the schematic's (eeschema/tools/sch_point_editor.cpp at 8303b2ad, `m_isSymbolEditor`), on one
// selected `SCH_SHAPE` of the `LIB_SYMBOL` (a rectangle, circle, arc, bezier or polyline) or `SCH_TEXTBOX` (UI-LEFT-2 YY1; the behaviours are kicad-port/eePointEdit.ts):
// the graphic as its behaviour sees it, in the editor's internal nm with +y down, and what a drag makes of it in the symbol's mm with +y up.
//
// - `RECTANGLE_POINT_EDIT_BEHAVIOR::dragPinsOnEdge`: a side's drag moves the pins whose root is on the side (not at its ends) by as much, with "Keep pins attached when
//   dragging edges" on (`SYMBOL_EDITOR_SETTINGS::m_dragPinsAlongWithEdges`, `drag_pins_along_with_edges`, true by default); the pins of the rectangle's unit, or of every
//   unit for a rectangle common to them (`GetGraphicalPins( aEdgeUnit, 0 )`), each moved alone (`pin->Move`: no twin in the other units). One commit with the shape.
// - The arc edit mode is the Symbol Editor's own (`SYMBOL_EDITOR_SETTINGS::m_ArcEditMode`, its `editing.arc_edit_mode`), not the schematic's: `changeArcEditMode` writes
//   `symCfg` when `m_isSymbolEditor`.
// - `SCH_POINT_EDITOR::Main` does nothing on a symbol that is not editable or is an alias (`IsSymbolEditable`, `IsSymbolAlias`: a derived symbol's graphics are its parent's).
// - A symbol's rectangle holds no corner radius here (`LibrarySymbolGraphic`'s rectangle has none), so it has no radius point; a symbol's text box neither.
//
// Pure: no React, no DOM.
import type { LibrarySymbolGraphic, LibrarySymbolPin, MmPoint } from "../api/types";
import { eeDrag, eeMakePoints, pinsOnEdge, type EeDragResult, type EeEditPoints, type EeEditShape, type EeHandle, type P } from "./eePointEdit";
import { DEFAULT_ARC_EDIT_MODE, incrementArcEditMode, type ArcEditMode } from "./pcbPointEdit";
import type { Measure } from "./schTextBox";
import { textBoxMinSizeNm, type LibTextBox } from "./symTextBox";

const nm = (q: MmPoint): P => [Math.round(q.x * 1e6) || 0, Math.round(-q.y * 1e6) || 0];
/** Internal nm back to the symbol's mm, on its 0.1 um grid (`schIUScale`). */
const mm = (q: readonly [number, number]): MmPoint => ({ x: Math.round(q[0] / 100) / 1e4 || 0, y: Math.round(-q[1] / 100) / 1e4 || 0 });

/** The graphic as its behaviour sees it, or null for a text (the point editor has no behaviour for an `SCH_TEXT`). */
export function symEditShape(g: LibrarySymbolGraphic, measure: Measure): EeEditShape | null {
  switch (g.kind) {
    case "rectangle":
      return { kind: "rectangle", start: nm(g.start), end: nm(g.end) };
    case "circle": {
      // `parseSymbolCircle`: the end is on the radius to the right of the centre
      const c = nm(g.center);
      return { kind: "circle", center: c, end: [c[0] + Math.round(g.radius_mm * 1e6), c[1]] };
    }
    case "arc":
      return { kind: "arc", start: nm(g.start), mid: nm(g.mid), end: nm(g.end) };
    case "bezier":
      return { kind: "bezier", start: nm(g.start), c1: nm(g.c1), c2: nm(g.c2), end: nm(g.end) };
    case "polyline":
      return { kind: "polygon", pts: g.pts.map(nm) };
    case "text_box": {
      const min = textBoxMinSizeNm(g, measure);
      return { kind: "text_box", start: nm(g.start), end: nm(g.end), minSize: [min.w, min.h] };
    }
    case "text":
      return null;
  }
}

/** The graphic as the behaviour left it, its unit, body style, stroke and fill as they were. */
export function symGraphicFrom(e: EeEditShape, was: LibrarySymbolGraphic): LibrarySymbolGraphic {
  switch (e.kind) {
    case "rectangle":
      return was.kind === "rectangle" ? { ...was, start: mm(e.start), end: mm(e.end) } : was;
    case "text_box":
      return was.kind === "text_box" ? { ...was, start: mm(e.start), end: mm(e.end) } : was;
    case "circle":
      return was.kind === "circle" ? { ...was, center: mm(e.center), radius_mm: Math.round(Math.hypot(e.end[0] - e.center[0], e.end[1] - e.center[1]) / 100) / 1e4 } : was;
    case "arc":
      return was.kind === "arc" ? { ...was, start: mm(e.start), mid: mm(e.mid), end: mm(e.end) } : was;
    case "bezier":
      return was.kind === "bezier" ? { ...was, start: mm(e.start), c1: mm(e.c1), c2: mm(e.c2), end: mm(e.end) } : was;
    case "polygon":
      return was.kind === "polyline" ? { ...was, pts: e.pts.map(mm) } : was;
  }
}

/** One drag step: what the behaviour makes of the graphic (a text box's minimum size is the box's as the drag has it, as `GetMinSize` reads the box it edits). */
export function symDrag(g: LibrarySymbolGraphic, h: EeHandle, to: readonly [number, number], mode: ArcEditMode, measure: Measure): EeDragResult | null {
  const e = symEditShape(g, measure);
  if (!e) return null;
  const first = eeDrag(e, h, to, mode);
  if (!first || e.kind !== "text_box" || first.shape.kind !== "text_box" || g.kind !== "text_box") return first;
  const min = textBoxMinSizeNm({ ...g, start: mm(first.shape.start), end: mm(first.shape.end) } as LibTextBox, measure);
  return eeDrag({ ...e, minSize: [min.w, min.h] }, h, to, mode);
}

/** The edit points of a graphic as a drag leaves it (or as it is), and the graphic as drawn. */
export function symPointEditor(g: LibrarySymbolGraphic, drag: { handle: EeHandle; to: readonly [number, number] } | null, mode: ArcEditMode, measure: Measure): { edit: EeEditPoints; graphic: LibrarySymbolGraphic } | null {
  const e = symEditShape(g, measure);
  if (!e) return null;
  const dragged = drag ? symDrag(g, drag.handle, drag.to, mode, measure) : null;
  const shown = dragged?.shape ?? e;
  return { edit: eeMakePoints(shown), graphic: dragged ? symGraphicFrom(shown, g) : g };
}

/**
 * `dragPinsOnEdge` for a finished drag: the pins of `g`'s unit (every pin for a graphic common to all units) whose root is on the side the drag moved, each with where it
 * goes (mm). `rootOf` gives a pin's root (`SCH_PIN::GetPinRoot`) in internal nm. Nothing when the setting is off or the drag moved no side.
 */
export function pinsDraggedWithEdge(g: LibrarySymbolGraphic, result: EeDragResult, pins: readonly LibrarySymbolPin[], rootOf: (p: LibrarySymbolPin) => P, keepAttached: boolean): { id: string; x: number; y: number }[] {
  if (!keepAttached || !result.edge || g.kind !== "rectangle") return [];
  // `GetGraphicalPins( aEdgeUnit, 0 )`: a pin of the unit or common to all of them, of any body style
  const candidates = pins.filter((p) => p.id && (g.unit === 0 || p.unit === 0 || p.unit === g.unit));
  const ids = new Set(pinsOnEdge(result.edge, candidates.map((p) => ({ id: p.id!, root: rootOf(p) }))));
  const [dx, dy] = result.edge.delta;
  return candidates.filter((p) => ids.has(p.id!)).map((p) => ({ id: p.id!, x: Math.round((p.at.x + dx / 1e6) * 1e4) / 1e4, y: Math.round((p.at.y - dy / 1e6) * 1e4) / 1e4 }));
}

// ------------------------------------------------------------------------------------------------------------------- settings

/** `SYMBOL_EDITOR_SETTINGS::m_ArcEditMode` (the Symbol Editor's `editing.arc_edit_mode`), kept in this browser as KiCad keeps it in the user's settings. */
const ARC_KEY = "eda.symedit.arcEditMode";
const MODES: readonly ArcEditMode[] = ["keep_center_adjust_angle_radius", "keep_center_ends_adjust_angle", "keep_endpoints_or_start_direction"];
let arcMode: ArcEditMode | null = null;

export function symArcEditMode(): ArcEditMode {
  if (arcMode) return arcMode;
  try {
    const v = globalThis.localStorage?.getItem(ARC_KEY);
    arcMode = v && (MODES as readonly string[]).includes(v) ? (v as ArcEditMode) : DEFAULT_ARC_EDIT_MODE;
  } catch {
    arcMode = DEFAULT_ARC_EDIT_MODE;
  }
  return arcMode;
}

/** `SCH_POINT_EDITOR::changeArcEditMode` in the Symbol Editor: the action's mode, or the next (`IncrementArcEditMode`) for `cycleArcEditMode`. */
export function setSymArcEditMode(next: ArcEditMode | "cycle"): ArcEditMode {
  arcMode = next === "cycle" ? incrementArcEditMode(symArcEditMode()) : next;
  try {
    globalThis.localStorage?.setItem(ARC_KEY, arcMode);
  } catch {
    // no storage: the mode lasts the session
  }
  return arcMode;
}
