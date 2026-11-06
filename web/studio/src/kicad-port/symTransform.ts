// The Symbol Editor's Rotate and Mirror of a selection of every kind of item (eeschema/tools/symbol_editor_edit_tool.cpp at KiCad 8303b2ad):
//
// - `SYMBOL_EDITOR_EDIT_TOOL::Rotate`: every selected item turns a quarter, counter-clockwise on screen (`rotateCCW`, R) or clockwise (`rotateCW`, Shift+R), about one
//   point -- the item's own `GetPosition()` when it is the only one selected, else `GetNearestHalfGridPosition( selection.GetCenter() )`.
// - `SYMBOL_EDITOR_EDIT_TOOL::Mirror`: `mirrorH` (`MirrorHorizontally`, left-right about a vertical line) and `mirrorV` (`MirrorVertically`, top-bottom), about the same point;
//   a field selected alone only flips its justification (`GetFlippedAlignment`).
//
// Each item's own transform is its class's: `SCH_PIN::RotatePin` / `MirrorHorizontallyPin` / `MirrorVerticallyPin`, `SCH_SHAPE::Rotate` / `MirrorHorizontally` /
// `MirrorVertically` (`EDA_SHAPE::rotate` and `flip`: an arc swaps its ends when mirrored, so it still runs counter-clockwise), `SCH_TEXT::Rotate` (`Rotate90( false )`) and
// its `LAYER_DEVICE` mirrors, `SCH_TEXTBOX` (the box turns or flips as a rectangle, the text's angle toggles or its justification flips), `SCH_FIELD::Rotate` and its mirrors.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`). Lengths are the wire's mm, +y up (the file's frame). KiCad works in its internal units (0.1 um), +y down: a
// quarter turn counter-clockwise on screen takes (dx, dy) to (-dy, dx) here, and every coordinate is rounded back to 0.1 um, as KiCad's are integers.
import type { FieldHAlign, FieldVAlign, LibrarySymbolGraphic, LibrarySymbolPin, MmPoint } from "../api/types";
import type { FieldRow } from "./symFields";

/** Schematic internal units per mm (`SCH_IU_PER_MM`). */
const IU_PER_MM = 1e4;
const round = (v: number): number => Math.round(v * IU_PER_MM) / IU_PER_MM;
const pt = (x: number, y: number): MmPoint => ({ x: round(x), y: round(y) });

/** `RotatePoint( p, centre, ccw ? ANGLE_90 : ANGLE_270 )`, in the symbol's +y up frame. */
export function rotatePoint(p: MmPoint, c: MmPoint, ccw: boolean): MmPoint {
  const dx = p.x - c.x;
  const dy = p.y - c.y;
  return ccw ? pt(c.x - dy, c.y + dx) : pt(c.x + dy, c.y - dx);
}

/** The mirror image of `p` about the vertical line `x = c` (`h`, `MirrorHorizontally`) or the horizontal line `y = c` (`v`, `MirrorVertically`). */
export function mirrorPoint(p: MmPoint, axis: MirrorAxis, c: number): MmPoint {
  return axis === "h" ? pt(2 * c - p.x, p.y) : pt(p.x, 2 * c - p.y);
}

/** `h`: `SCH_ACTIONS::mirrorH` (left-right); `v`: `mirrorV` (top-bottom, KiCad's `xAxis`). */
export type MirrorAxis = "h" | "v";

/** `EDA_TEXT::GetTextAngle().IsVertical()`: a text at 90 (or 270) degrees. */
const isVertical = (angleDeg: number): boolean => Math.abs(((angleDeg % 180) + 180) % 180) === 90;

/** `FlipHJustify` / `GetFlippedAlignment`: left and right swap, centre stays. */
export const flippedH = (a: FieldHAlign | undefined): FieldHAlign => (a === "left" ? "right" : a === "right" ? "left" : "center");
export const flippedV = (a: FieldVAlign | undefined): FieldVAlign => (a === "top" ? "bottom" : a === "bottom" ? "top" : "center");

/** `CalcArcCenter( start, mid, end )`, in internal units (the arc's `m_arcCenter` is an integer point), back in mm. */
export function arcCenter(start: MmPoint, mid: MmPoint, end: MmPoint): MmPoint {
  const a = [start.x * IU_PER_MM, start.y * IU_PER_MM];
  const b = [mid.x * IU_PER_MM, mid.y * IU_PER_MM];
  const c = [end.x * IU_PER_MM, end.y * IU_PER_MM];
  const d = 2 * (a[0]! * (b[1]! - c[1]!) + b[0]! * (c[1]! - a[1]!) + c[0]! * (a[1]! - b[1]!));
  if (d === 0) return pt((start.x + end.x) / 2, (start.y + end.y) / 2);
  const sq = (p: number[]) => p[0]! * p[0]! + p[1]! * p[1]!;
  const x = (sq(a) * (b[1]! - c[1]!) + sq(b) * (c[1]! - a[1]!) + sq(c) * (a[1]! - b[1]!)) / d;
  const y = (sq(a) * (c[0]! - b[0]!) + sq(b) * (a[0]! - c[0]!) + sq(c) * (b[0]! - a[0]!)) / d;
  return { x: Math.round(x) / IU_PER_MM, y: Math.round(y) / IU_PER_MM };
}

// ------------------------------------------------------------------------------------------------------------------------------- positions

/** `SCH_ITEM::GetPosition()` of a graphic -- `EDA_SHAPE::getPosition` (an arc's centre, a polygon's first vertex, else the start; a circle's start is its centre), a text's
 * position, a text box's start: the point Rotate and Mirror turn a lone item about. */
export function graphicPosition(g: LibrarySymbolGraphic): MmPoint {
  switch (g.kind) {
    case "rectangle":
    case "text_box":
    case "bezier":
      return g.start;
    case "polyline":
      return g.pts[0] ?? { x: 0, y: 0 };
    case "circle":
      return g.center;
    case "arc":
      return arcCenter(g.start, g.mid, g.end);
    case "text":
      return g.at;
  }
}

// ------------------------------------------------------------------------------------------------------------------------------- pins

/** `SCH_PIN::RotatePin`: the position turns about `c`, the orientation with it (Right -> Up -> Left -> Down counter-clockwise; the file's angle +90). */
export function rotatePin(p: LibrarySymbolPin, c: MmPoint, ccw: boolean): LibrarySymbolPin {
  const angle = (((p.angle_deg + (ccw ? 90 : -90)) % 360) + 360) % 360;
  return { ...p, at: rotatePoint(p.at, c, ccw), angle_deg: angle };
}

/** `SCH_PIN::MirrorHorizontallyPin` (Right and Left swap) / `MirrorVerticallyPin` (Up and Down swap). */
export function mirrorPin(p: LibrarySymbolPin, axis: MirrorAxis, c: number): LibrarySymbolPin {
  const a = ((Math.round(p.angle_deg) % 360) + 360) % 360;
  let angle = a;
  if (axis === "h" && (a === 0 || a === 180)) angle = a === 0 ? 180 : 0;
  if (axis === "v" && (a === 90 || a === 270)) angle = a === 90 ? 270 : 90;
  return { ...p, at: mirrorPoint(p.at, axis, c), angle_deg: angle };
}

// ------------------------------------------------------------------------------------------------------------------------------- graphics

/** The graphic turned a quarter about `c`: `SCH_SHAPE::Rotate` (`EDA_SHAPE::rotate`: every point; a rectangle by a cardinal angle stays a rectangle), `SCH_TEXT::Rotate` (the
 * position turns, then `Rotate90( false )`: a vertical text flips its horizontal justification, and the angle toggles between 0 and 90 whichever way it turns), `SCH_TEXTBOX::Rotate`
 * (the box turns, the text's angle toggles). */
export function rotateGraphic(g: LibrarySymbolGraphic, c: MmPoint, ccw: boolean): LibrarySymbolGraphic {
  const r = (p: MmPoint) => rotatePoint(p, c, ccw);
  switch (g.kind) {
    case "rectangle":
      return { ...g, start: r(g.start), end: r(g.end) };
    case "polyline":
      return { ...g, pts: g.pts.map(r) };
    case "circle":
      return { ...g, center: r(g.center) };
    case "arc":
      return { ...g, start: r(g.start), mid: r(g.mid), end: r(g.end) };
    case "bezier":
      return { ...g, start: r(g.start), c1: r(g.c1), c2: r(g.c2), end: r(g.end) };
    case "text": {
      const vertical = isVertical(g.angle_deg);
      return { ...g, at: r(g.at), angle_deg: vertical ? 0 : 90, h_align: vertical ? flippedH(g.h_align) : (g.h_align ?? "center") };
    }
    case "text_box":
      return { ...g, start: r(g.start), end: r(g.end), angle_deg: isVertical(g.angle_deg) ? 0 : 90 };
  }
}

/**
 * The graphic mirrored about `x = c` (`h`) or `y = c` (`v`): `EDA_SHAPE::flip` (every point; an arc's start and end swap), `SCH_TEXT::MirrorHorizontally` / `MirrorVertically` of a
 * `LAYER_DEVICE` text (the position mirrors, and the justification across the mirror flips: the horizontal one of a horizontal text mirrored left-right, the vertical one of a
 * vertical text; the other way round top-bottom -- `NormalizeJustification` before and after moves the text by its box's half, which cancels), `SCH_TEXTBOX::MirrorHorizontally`
 * / `MirrorVertically` (the box flips; "Text is NOT really mirrored; it just has its justification flipped": the horizontal justification of a horizontal text left-right, of a
 * vertical one top-bottom).
 */
export function mirrorGraphic(g: LibrarySymbolGraphic, axis: MirrorAxis, c: number): LibrarySymbolGraphic {
  const m = (p: MmPoint) => mirrorPoint(p, axis, c);
  switch (g.kind) {
    case "rectangle":
      return { ...g, start: m(g.start), end: m(g.end) };
    case "polyline":
      return { ...g, pts: g.pts.map(m) };
    case "circle":
      return { ...g, center: m(g.center) };
    case "arc":
      return { ...g, start: m(g.end), mid: m(g.mid), end: m(g.start) };
    case "bezier":
      return { ...g, start: m(g.start), c1: m(g.c1), c2: m(g.c2), end: m(g.end) };
    case "text": {
      const vertical = isVertical(g.angle_deg);
      // left-right: a horizontal text's horizontal justification, a vertical text's vertical one; top-bottom the other way round
      const flipsH = (axis === "h") !== vertical;
      return flipsH ? { ...g, at: m(g.at), h_align: flippedH(g.h_align) } : { ...g, at: m(g.at), v_align: flippedV(g.v_align) };
    }
    case "text_box": {
      const flips = axis === "h" ? !isVertical(g.angle_deg) : isVertical(g.angle_deg);
      return { ...g, start: m(g.start), end: m(g.end), h_align: flips ? flippedH(g.h_align) : g.h_align };
    }
  }
}

// ------------------------------------------------------------------------------------------------------------------------------- fields

/** `SCH_FIELD::Rotate`: a vertical field becomes horizontal (left and right swap when it turns counter-clockwise), a horizontal one vertical (they swap when it turns
 * clockwise); the position turns about `c`. */
export function rotateField(f: FieldRow, c: MmPoint, ccw: boolean): FieldRow {
  const vertical = isVertical(f.angle_deg);
  const swap = vertical ? ccw : !ccw;
  return { ...f, angle_deg: vertical ? 0 : 90, h_align: swap ? flippedH(f.h_align) : f.h_align, at: rotatePoint(f.at, c, ccw) };
}

/** `SCH_FIELD::MirrorHorizontally` / `MirrorVertically`: the position only. */
export function mirrorField(f: FieldRow, axis: MirrorAxis, c: number): FieldRow {
  return { ...f, at: mirrorPoint(f.at, axis, c) };
}

/** `SYMBOL_EDITOR_EDIT_TOOL::Mirror` of a field selected alone: its justification flips (`GetFlippedAlignment`), the vertical one top-bottom (`xAxis`), the horizontal one left-right. */
export function mirrorLoneField(f: FieldRow, axis: MirrorAxis): FieldRow {
  return axis === "v" ? { ...f, v_align: flippedV(f.v_align) } : { ...f, h_align: flippedH(f.h_align) };
}

// ------------------------------------------------------------------------------------------------------------------------------- the selection

/** What a Rotate or Mirror changes: the selected pins, graphics and fields as they are after it. */
export interface TransformResult {
  pins: LibrarySymbolPin[];
  graphics: LibrarySymbolGraphic[];
  fields: FieldRow[];
}

/** The selection: its pins, graphics and fields, and the multi-item pivot (`GetNearestHalfGridPosition( selection.GetCenter() )`, `symEditItems.ts` `rotatePivot`). */
export interface TransformSelection {
  pins: readonly LibrarySymbolPin[];
  graphics: readonly LibrarySymbolGraphic[];
  fields: readonly FieldRow[];
  /** The pivot of a selection of several items; unused for one. */
  centre: MmPoint | null;
}

const count = (s: TransformSelection) => s.pins.length + s.graphics.length + s.fields.length;

/** `selection.GetSize() == 1 ? item->GetPosition() : GetNearestHalfGridPosition( selection.GetCenter() )`. */
export function transformPivot(s: TransformSelection): MmPoint | null {
  if (count(s) === 0) return null;
  if (count(s) > 1) return s.centre;
  if (s.pins[0]) return s.pins[0].at;
  if (s.graphics[0]) return graphicPosition(s.graphics[0]);
  return s.fields[0]!.at;
}

/** `SYMBOL_EDITOR_EDIT_TOOL::Rotate`: every selected item a quarter turn about the pivot. */
export function rotateSelection(s: TransformSelection, ccw: boolean): TransformResult | null {
  const c = transformPivot(s);
  if (!c) return null;
  return { pins: s.pins.map((p) => rotatePin(p, c, ccw)), graphics: s.graphics.map((g) => rotateGraphic(g, c, ccw)), fields: s.fields.map((f) => rotateField(f, c, ccw)) };
}

/** `SYMBOL_EDITOR_EDIT_TOOL::Mirror`: every selected item mirrored about the pivot's x (`h`) or y (`v`); a field alone flips its justification instead. */
export function mirrorSelection(s: TransformSelection, axis: MirrorAxis): TransformResult | null {
  const c = transformPivot(s);
  if (!c) return null;
  if (count(s) === 1 && s.fields.length === 1) return { pins: [], graphics: [], fields: [mirrorLoneField(s.fields[0]!, axis)] };
  const at = axis === "h" ? c.x : c.y;
  return { pins: s.pins.map((p) => mirrorPin(p, axis, at)), graphics: s.graphics.map((g) => mirrorGraphic(g, axis, at)), fields: s.fields.map((f) => mirrorField(f, axis, at)) };
}
