// The schematic's point editor on the items that are not drawn shapes -- the `SCH_POINT_EDITOR` behaviours of a hierarchical sheet, an image, a graphic line and a
// table cell (eeschema/tools/sch_point_editor.cpp at KiCad 8303b2ad: `SHEET_POINT_EDIT_BEHAVIOR`, `BITMAP_POINT_EDIT_BEHAVIOR`, `LINE_POINT_EDIT_BEHAVIOR`,
// `SCH_TABLECELL_POINT_EDIT_BEHAVIOR` on common/tool/point_editor_behavior.cpp's `EDA_TABLECELL_POINT_EDIT_BEHAVIOR`) -- and the one entry the canvas calls for
// every item the point editor works on (`makePointsAndBehavior`'s switch), the drawn shapes going to kicad-port/schPointEdit.ts. UI-LEFT-3 S3-2 (the sheet) and
// S3-4 (the image, the line, the cell).
//
// - The sheet: its four corners and an `EDIT_LINE` per side. A drag pins the edited corner or side inside `GetMinWidth` / `GetMinHeight` (the pins on the top and
//   bottom sides, or on the left and right ones, with their text: `SCH_SHEET_PIN::GetBoundingBox`, bumped out to the next 50 mil, and never under 500 x 150 mils);
//   the sheet's corner moves without its pins (`SetPositionIgnoringPins`) and `Resize` puts every pin back on its side. The no-connects on the pins and the wire and
//   bus ends on them follow. Committed as `resize_sheet` (crates/ops/src/sch_sheets.rs), which does the same to the design.
// - The image: its four corners and its transform origin (`REFIMG_ORIGIN`). A corner scales the image about the origin by how far the corner is from it now over how
//   far it was (a corner pulled across the origin stops on it), never under 50 mils a side; the origin's drag moves the origin only. Committed as the Scale row
//   commits (kicad-port/schItemProperties.ts): `edit_graphic` with the new centre and scale and `set_image_offset`, in one step.
// - The graphic line: its start and its end. Another graphic line with an end on one of them (`MakePoints`' scan: the last one found for each end) has that end
//   follow. Committed as `set_line_ends` of every line that moved, in one step. A line is a chain of points here: its first and last are KiCad's start and end.
//   A drag that puts a line's start on its end is committed as KiCad commits it: the zero-length graphic line stays (`SCHEMATIC::CleanUp` takes only wires and buses).
// - The table cell: a point in the middle of its right side (`COL_WIDTH`) and one in the middle of its bottom (`ROW_HEIGHT`), in the cell's own frame; the drag
//   sets the cell's end there and gives the last column (row) it spans the width (height) that leaves (`table.SetColWidth` / `SetRowHeight`), the cell's other
//   columns (rows) kept. Committed as the Properties panel's "Column Width" / "Row Height" (`sch_table` `edit_cells` on the cell of that column).
//
// Differs: an image's scale is
// kept in millionths (`scale_ppm`), KiCad's is a double. The table's preview moves the table's lines, corners and text with the edited column or row; its text is laid
// out again by the server when the drag lands (KiCad re-lays it at each step).
//
// Pure: no React, no DOM.
import type { BoardTable, BoardTableCell, Cmd, PointXY, Schematic, SchLine, Sheet, SheetPin } from "../api/types";
import type { SchGraphic } from "../api/schEditTypes";
import { movedPoints, pinEditedCorner, RECT_BOT, RECT_BOTLEFT, RECT_BOTRIGHT, RECT_LEFT, RECT_RIGHT, RECT_TOP, RECT_TOPLEFT, RECT_TOPRIGHT, type EeEditPoints, type EeHandle, type EeLine, type P } from "./eePointEdit";
import type { ArcEditMode } from "./pcbPointEdit";
import { kiRound } from "./pcbGeom";
import { DEFAULT_PPI, imageSizeNm, SCALE_PPM } from "./refImage";
import { schDrag, schPointEditor, schShapeFrom } from "./schPointEdit";
import { sheetPinBox, sheetPinItems, sheetPinSide, type Measure } from "./schSheetPinItems";

const MILS = 25_400;
/** `MIN_SHEET_WIDTH` / `MIN_SHEET_HEIGHT` (eeschema/sch_sheet.h), mils. */
export const MIN_SHEET_WIDTH_NM = 500 * MILS;
export const MIN_SHEET_HEIGHT_NM = 150 * MILS;
/** `BITMAP_POINT_EDIT_BEHAVIOR::UpdateItem`: "Clamp the scaling to a minimum of 50 mils". */
const MIN_IMAGE_NM = 50 * MILS;

/** `REFIMAGE_POINTS`: the transform origin after the four corners. */
export const REFIMG_ORIGIN = RECT_BOTRIGHT + 1;
/** `LINE_POINTS`. */
export const LINE_START = 0;
export const LINE_END = 1;
/** `TABLECELL_POINTS`. */
export const COL_WIDTH = 0;
export const ROW_HEIGHT = 1;

const side = (a: number, b: number): EeLine => ({ a, b, center: true, draw: false });
/** The four sides of a box (`RECTANGLE_LINES`: top, right, bottom, left), each dragged by its middle. */
const BOX_LINES: EeLine[] = [side(RECT_TOPLEFT, RECT_TOPRIGHT), side(RECT_TOPRIGHT, RECT_BOTRIGHT), side(RECT_BOTRIGHT, RECT_BOTLEFT), side(RECT_BOTLEFT, RECT_TOPLEFT)];
const pt = (q: readonly [number, number]): PointXY => ({ x: q[0], y: q[1] });
const same = (a: readonly [number, number], b: readonly [number, number]) => a[0] === b[0] && a[1] === b[1];
const isPoint = (h: EeHandle, i: number) => h.kind === "point" && h.index === i;
const isLine = (h: EeHandle, i: number) => h.kind === "line" && h.index === i;

// ------------------------------------------------------------------------------------------------------------------------------------ the sheet

/** `bumpToNextGrid` (eeschema/sch_sheet.cpp): a value pushed out to the next 50 mil line, away from it when it is on one (C++'s truncating division and remainder). */
export function bumpToNextGrid(v: number, direction: number): number {
  const grid = 50 * MILS;
  const base = Math.trunc(v / grid);
  const excess = Math.abs(v % grid);
  // (`+ 0`: no negative zero out of a truncated -0.x)
  if (direction > 0) return (base + 1) * grid + 0;
  return (excess > 0 ? base * grid : (base - 1) * grid) + 0;
}

/** The pins of a sheet as the selection tool has them (their side and the size of their text), for their boxes. */
function pinsOf(sch: Schematic, sheet: Sheet) {
  return sheetPinItems(sch).filter((p) => p.sheetId === sheet.id);
}

/**
 * `SCH_SHEET::GetMinWidth( aFromLeft )`: the pins on the top and bottom sides, by their boxes, bumped out to the 50 mil grid -- from the left side to the rightmost of them
 * (`aFromLeft`, the right side is being dragged) or from the leftmost to the right side -- and never under `MIN_SHEET_WIDTH`.
 */
export function sheetMinWidth(sch: Schematic, sheet: Sheet, fromLeft: boolean, measure: Measure): number {
  let left = sheet.at[0] + sheet.size[0];
  let right = sheet.at[0];
  for (const p of pinsOf(sch, sheet)) {
    if (p.side !== "top" && p.side !== "bottom") continue;
    const b = sheetPinBox(p, measure);
    left = Math.min(left, b.minX);
    right = Math.max(right, b.maxX);
  }
  left = bumpToNextGrid(left, -1);
  right = bumpToNextGrid(right, 1);
  const pinMin = left >= right ? 0 : fromLeft ? right - sheet.at[0] : sheet.at[0] + sheet.size[0] - left;
  return Math.max(pinMin, MIN_SHEET_WIDTH_NM);
}

/** `SCH_SHEET::GetMinHeight( aFromTop )`: the same over the pins on the left and right sides. */
export function sheetMinHeight(sch: Schematic, sheet: Sheet, fromTop: boolean, measure: Measure): number {
  let top = sheet.at[1] + sheet.size[1];
  let bottom = sheet.at[1];
  for (const p of pinsOf(sch, sheet)) {
    if (p.side !== "left" && p.side !== "right") continue;
    const b = sheetPinBox(p, measure);
    top = Math.min(top, b.minY);
    bottom = Math.max(bottom, b.maxY);
  }
  top = bumpToNextGrid(top, -1);
  bottom = bumpToNextGrid(bottom, 1);
  const pinMin = top >= bottom ? 0 : fromTop ? bottom - sheet.at[1] : sheet.at[1] + sheet.size[1] - top;
  return Math.max(pinMin, MIN_SHEET_HEIGHT_NM);
}

/** `SHEET_POINT_EDIT_BEHAVIOR::MakePoints`: the four corners and the four sides. */
export function sheetEditPoints(sheet: Pick<Sheet, "at" | "size">): EeEditPoints {
  const [x, y] = sheet.at;
  const [r, b] = [x + sheet.size[0], y + sheet.size[1]];
  return { points: [{ pos: [x, y] }, { pos: [r, y] }, { pos: [x, b] }, { pos: [r, b] }], lines: BOX_LINES };
}

/** `SHEET_POINT_EDIT_BEHAVIOR::UpdateItem`: where the sheet's corner and how big it is after handle `h` was dragged to `to`, or null for a handle that names nothing. */
export function sheetDrag(sch: Schematic, sheet: Sheet, h: EeHandle, to: readonly [number, number], measure: Measure): { at: P; size: P } | null {
  const e = sheetEditPoints(sheet);
  if ((h.kind === "point" && !e.points[h.index]) || (h.kind === "line" && !e.lines[h.index])) return null;
  const pts = movedPoints(e, h, to);
  const c = { tl: pts[RECT_TOPLEFT]!, tr: pts[RECT_TOPRIGHT]!, bl: pts[RECT_BOTLEFT]!, br: pts[RECT_BOTRIGHT]! };
  let editedTopRight = isPoint(h, RECT_TOPRIGHT);
  let editedBotLeft = isPoint(h, RECT_BOTLEFT);
  const editedBotRight = isPoint(h, RECT_BOTRIGHT);
  if (isLine(h, RECT_RIGHT)) editedTopRight = true;
  else if (isLine(h, RECT_BOT)) editedBotLeft = true;
  pinEditedCorner(h, sheetMinWidth(sch, sheet, editedTopRight || editedBotRight, measure), sheetMinHeight(sch, sheet, editedBotLeft || editedBotRight, measure), c);
  const [w, hgt] = sheet.size;
  if (h.kind === "point") return { at: c.tl, size: [c.br[0] - c.tl[0], c.br[1] - c.tl[1]] };
  switch (h.index) {
    case RECT_TOP:
      return { at: [sheet.at[0], c.tl[1]], size: [w, c.br[1] - c.tl[1]] };
    case RECT_LEFT:
      return { at: [c.tl[0], sheet.at[1]], size: [c.br[0] - c.tl[0], hgt] };
    case RECT_BOT:
      return { at: [sheet.at[0], sheet.at[1]], size: [w, c.br[1] - c.tl[1]] };
    default:
      return { at: [sheet.at[0], sheet.at[1]], size: [c.br[0] - c.tl[0], hgt] };
  }
}

/** `SetPositionIgnoringPins( at )` then `Resize( size )`: every pin back on the side it was on, at its place along it clamped to the side's ends (`ConstrainOnEdge( pos, false )`). */
export function resizedSheet(sheet: Sheet, at: P, size: P): Sheet {
  const [l, t, r, b] = [at[0], at[1], at[0] + size[0], at[1] + size[1]];
  const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), hi);
  const pins: SheetPin[] = sheet.pins.map((p) => {
    const [x, y] = p.at;
    switch (sheetPinSide(sheet, p.at)) {
      case "top":
        return { ...p, at: [clamp(x, l, r), t] };
      case "bottom":
        return { ...p, at: [clamp(x, l, r), b] };
      case "right":
        return { ...p, at: [r, clamp(y, t, b)] };
      default:
        return { ...p, at: [l, clamp(y, t, b)] };
    }
  });
  return { ...sheet, at: [at[0], at[1]], size: [size[0], size[1]], pins };
}

/**
 * The sheet in view as the drag has `sheet` (its preview): the sheet resized, the wire and bus points and the no-connects that were on a pin on it where the pin went
 * (`m_connectedWires`, `m_noConnects`), and the sheet's fields along with the side they are beyond (`Resize` places the name and file fields again).
 */
export function withResizedSheet(sch: Schematic, sheet: Sheet, next: Sheet): Schematic {
  const moves = sheet.pins.map((p, i) => [p.at, next.pins[i]!.at] as const).filter(([a, b]) => !same(a, b));
  const follow = (q: [number, number]): [number, number] => moves.find(([a]) => same(a, q))?.[1] ?? q;
  const [ol, ot, or, ob] = [sheet.at[0], sheet.at[1], sheet.at[0] + sheet.size[0], sheet.at[1] + sheet.size[1]];
  const [nl, nt, nr, nb] = [next.at[0], next.at[1], next.at[0] + next.size[0], next.at[1] + next.size[1]];
  const shift = <F extends { at: [number, number] }>(f: F): F => ({ ...f, at: [f.at[0] + (f.at[0] >= or ? nr - or : nl - ol), f.at[1] + (f.at[1] >= ob ? nb - ob : nt - ot)] });
  const shown: Sheet = { ...next, fields: next.fields?.map(shift), user_fields_drawn: next.user_fields_drawn?.map(shift) };
  return {
    ...sch,
    sheets: sch.sheets.map((s) => (s.id === sheet.id ? shown : s)),
    wires: moves.length ? sch.wires.map((w) => (w.pts.some((q) => moves.some(([a]) => same(a, q))) ? { ...w, pts: w.pts.map(follow) } : w)) : sch.wires,
    no_connects: moves.length ? sch.no_connects.map((n) => (moves.some(([a]) => same(a, n.at)) ? { ...n, at: follow(n.at) } : n)) : sch.no_connects,
  };
}

// ------------------------------------------------------------------------------------------------------------------------------------ the image

type ImageShape = Extract<SchGraphic["shape"], { type: "image" }>;

/** An image as its behaviour sees it: its centre, its size (`REFERENCE_IMAGE::GetSize`), its scale and its transform origin's offset from the centre. */
export interface ImageState {
  at: P;
  size: P;
  scalePpm: number;
  offset: P;
}

export function imageState(g: SchGraphic, sch: Schematic): ImageState | null {
  if (g.shape.type !== "image") return null;
  const s = g.shape;
  const o = sch.image_offsets?.[g.id];
  const scalePpm = s.scale_ppm ?? SCALE_PPM;
  return { at: [s.at.x, s.at.y], size: imageSizeNm(s.px ?? [0, 0], s.ppi ?? DEFAULT_PPI, scalePpm), scalePpm, offset: [o?.x ?? 0, o?.y ?? 0] };
}

/** `BITMAP_POINT_EDIT_BEHAVIOR::MakePoints`: the corners (`GetPosition() -/+ GetSize() / 2`) and the transform origin. */
export function imageEditPoints(im: ImageState): EeEditPoints {
  const half: P = [Math.trunc(im.size[0] / 2), Math.trunc(im.size[1] / 2)];
  const tl: P = [im.at[0] - half[0], im.at[1] - half[1]];
  const br: P = [im.at[0] + half[0], im.at[1] + half[1]];
  return { points: [{ pos: tl }, { pos: [br[0], tl[1]] }, { pos: [tl[0], br[1]] }, { pos: br }, { pos: [im.at[0] + im.offset[0], im.at[1] + im.offset[1]] }], lines: [] };
}

const sign = (v: number) => (v > 0 ? 1 : v < 0 ? -1 : 0);

/** `BITMAP_POINT_EDIT_BEHAVIOR::UpdateItem` after point `h` went to `to`: the image's centre, scale and offset, or null for a handle that names nothing. */
export function imageDrag(im: ImageState, h: EeHandle, to: readonly [number, number]): ImageState | null {
  if (h.kind !== "point" || h.index > REFIMG_ORIGIN) return null;
  const e = imageEditPoints(im);
  const pts = movedPoints(e, h, to);
  const origin = pts[REFIMG_ORIGIN]!;
  if (h.index === REFIMG_ORIGIN) {
    // "Moving the transform origin": `newOffset = xfrmOrigin - ( topLeft + botRight ) / 2`
    const tl = pts[RECT_TOPLEFT]!;
    const br = pts[RECT_BOTRIGHT]!;
    return { ...im, offset: [origin[0] - Math.trunc((tl[0] + br[0]) / 2), origin[1] - Math.trunc((tl[1] + br[1]) / 2)] };
  }
  const oldOrigin: P = [im.at[0] + im.offset[0], im.at[1] + im.offset[1]];
  const half = (v: number) => Math.trunc(v / 2);
  const [w, hh] = im.size;
  const oldCorner: P =
    h.index === RECT_TOPLEFT
      ? [im.at[0] - half(w), im.at[1] - half(hh)]
      : h.index === RECT_TOPRIGHT
        ? [im.at[0] + half(w), im.at[1] - half(hh)]
        : h.index === RECT_BOTLEFT
          ? [im.at[0] - half(w), im.at[1] + half(hh)]
          : [im.at[0] + half(w), im.at[1] + half(hh)];
  // "Turn in the respective vectors from the origin"
  let nc: P = [pts[h.index]![0] - origin[0], pts[h.index]![1] - origin[1]];
  const oc: P = [oldCorner[0] - oldOrigin[0], oldCorner[1] - oldOrigin[1]];
  // "If we tried to cross the origin, clamp it to stop it"
  if (sign(nc[0]) !== sign(oc[0]) || sign(nc[1]) !== sign(oc[1])) nc = [0, 0];
  const newLength = Math.hypot(nc[0], nc[1]);
  const oldLength = Math.hypot(oc[0], oc[1]);
  let ratio = oldLength > 0 ? newLength / oldLength : 1;
  // "Clamp the scaling to a minimum of 50 mils": `VECTOR2I newSize = oldSize * ratio` (truncated)
  const newWidth = Math.max(Math.trunc(w * ratio), MIN_IMAGE_NM);
  const newHeight = Math.max(Math.trunc(hh * ratio), MIN_IMAGE_NM);
  ratio = Math.min(newWidth / w, newHeight / hh);
  if (!(ratio > 0) || !Number.isFinite(ratio)) return null;
  // `SetImageScale( GetImageScale() * ratio )` -> `scaleBy( ratio )`: about the transform origin, which stays put
  const offset: P = [kiRound(im.offset[0] * ratio), kiRound(im.offset[1] * ratio)];
  const at: P = [kiRound(oldOrigin[0] - im.offset[0] * ratio), kiRound(oldOrigin[1] - im.offset[1] * ratio)];
  const scalePpm = Math.round(im.scalePpm * ratio);
  return { at, size: [Math.round((w * scalePpm) / im.scalePpm), Math.round((hh * scalePpm) / im.scalePpm)], scalePpm, offset };
}

function imageCmds(id: string, was: ImageState, now: ImageState): Cmd[] {
  const cmds: Cmd[] = [];
  if (!same(was.at, now.at) || was.scalePpm !== now.scalePpm) cmds.push({ op: "sch_edit", verb: "edit_graphic", id, graphic: { shape: { type: "image", at: pt(now.at), scale_ppm: now.scalePpm } } } as Cmd);
  if (!same(was.offset, now.offset)) cmds.push({ op: "sch_edit", verb: "set_image_offset", id, offset: pt(now.offset) } as Cmd);
  return cmds;
}

// ------------------------------------------------------------------------------------------------------------------------------------ the graphic line

/** An end of another graphic line that is on one of this line's ends: the line and which of its ends (`STARTPOINT` / `ENDPOINT`). */
export interface LineLink {
  id: string;
  end: "start" | "end";
}

const first = (l: SchLine): P => [l.pts[0]![0], l.pts[0]![1]];
const last = (l: SchLine): P => [l.pts[l.pts.length - 1]![0], l.pts[l.pts.length - 1]![1]];

/**
 * `LINE_POINT_EDIT_BEHAVIOR::MakePoints`: the line's start and end, each with the graphic line connected there -- every other line on the notes layer is tested in turn,
 * its start then its end against this line's start, then against its end (one `else if` chain, so a line counts for one end only), and the last one found wins.
 */
export function lineLinks(lines: readonly SchLine[], line: SchLine): [LineLink | null, LineLink | null] {
  let start: LineLink | null = null;
  let end: LineLink | null = null;
  const [s, e] = [first(line), last(line)];
  for (const t of lines) {
    if (t.id === line.id || t.pts.length < 2) continue;
    if (same(first(t), s)) start = { id: t.id, end: "start" };
    else if (same(last(t), s)) start = { id: t.id, end: "end" };
    else if (same(first(t), e)) end = { id: t.id, end: "start" };
    else if (same(last(t), e)) end = { id: t.id, end: "end" };
  }
  return [start, end];
}

export function lineEditPoints(line: SchLine): EeEditPoints {
  return { points: [{ pos: first(line) }, { pos: last(line) }], lines: [] };
}

/** `LINE_POINT_EDIT_BEHAVIOR::UpdateItem`: the lines as the drag of point `h` to `to` leaves them -- this one, and the connected ends set to its ends. */
export function lineDrag(lines: readonly SchLine[], line: SchLine, h: EeHandle, to: readonly [number, number]): Map<string, SchLine> | null {
  if (h.kind !== "point" || h.index > LINE_END || line.pts.length < 2) return null;
  const pts = movedPoints(lineEditPoints(line), h, to);
  const out = new Map<string, SchLine>();
  const setEnds = (l: SchLine, start: P | null, end: P | null): SchLine => {
    const next = l.pts.map((q) => [q[0], q[1]] as [number, number]);
    if (start) next[0] = [start[0], start[1]];
    if (end) next[next.length - 1] = [end[0], end[1]];
    return { ...l, pts: next };
  };
  out.set(line.id, setEnds(line, pts[LINE_START]!, pts[LINE_END]!));
  const links = lineLinks(lines, line);
  links.forEach((link, i) => {
    if (!link) return;
    const other = out.get(link.id) ?? lines.find((l) => l.id === link.id);
    if (!other) return;
    const p = pts[i]!;
    out.set(link.id, link.end === "start" ? setEnds(other, p, null) : setEnds(other, null, p));
  });
  return out;
}

function lineCmds(lines: readonly SchLine[], moved: Map<string, SchLine>): Cmd[] {
  const cmds: Cmd[] = [];
  for (const [id, now] of moved) {
    const was = lines.find((l) => l.id === id);
    if (!was) continue;
    const s = same(first(was), first(now)) ? null : pt(first(now));
    const e = same(last(was), last(now)) ? null : pt(last(now));
    if (s || e) cmds.push({ op: "sch_edit", verb: "set_line_ends", id, ...(s ? { start: s } : {}), ...(e ? { end: e } : {}) } as Cmd);
  }
  return cmds;
}

// ------------------------------------------------------------------------------------------------------------------------------------ the table cell

/** `KiCad's RotatePoint` of a vector by the cell's text angle (0 or 90 degrees on a sheet): `( x, y )` -> `( y, -x )` for 90. */
function rot(v: P, angle: number): P {
  const a = (((Math.round(angle / 1000) % 360) + 360) % 360) as number;
  if (a === 90) return [v[1], -v[0]];
  if (a === 180) return [-v[0], -v[1]];
  if (a === 270) return [-v[1], v[0]];
  return v;
}

/** `SCH_TABLE::Normalize` for one cell: its start (its column's left, its row's top) and its end (past the columns and rows it spans), turned about the table's position. */
export function cellRect(t: BoardTable, cell: BoardTableCell): { start: P; end: P } {
  let x = 0;
  for (let c = 0; c < cell.col; c++) x += t.col_widths[c] ?? 0;
  let y = 0;
  for (let r = 0; r < cell.row; r++) y += t.row_heights[r] ?? 0;
  let w = 0;
  for (let c = cell.col; c < cell.col + Math.max(cell.col_span, 1); c++) w += t.col_widths[c] ?? 0;
  let h = 0;
  for (let r = cell.row; r < cell.row + Math.max(cell.row_span, 1); r++) h += t.row_heights[r] ?? 0;
  const s = rot([x, y], t.angle);
  const e = rot([x + w, y + h], t.angle);
  return { start: [t.x + s[0], t.y + s[1]], end: [t.x + e[0], t.y + e[1]] };
}

/** `EDA_TABLECELL_POINT_EDIT_BEHAVIOR::MakePoints`: `COL_WIDTH` at `GetEnd() - ( 0, GetRectangleHeight() / 2 )`, `ROW_HEIGHT` at `GetEnd() - ( GetRectangleWidth() / 2, 0 )`. */
export function cellEditPoints(t: BoardTable, cell: BoardTableCell): EeEditPoints {
  const { start, end } = cellRect(t, cell);
  const rh = end[1] - start[1];
  const rw = end[0] - start[0];
  return { points: [{ pos: [end[0], end[1] - Math.trunc(rh / 2)] }, { pos: [end[0] - Math.trunc(rw / 2), end[1]] }], lines: [] };
}

/** What a cell drag sets: the width of column `col` or the height of row `row` (the last one the cell spans). */
export type CellChange = { col: number; width: number } | { row: number; height: number };

/** `SCH_TABLECELL_POINT_EDIT_BEHAVIOR::UpdateItem`: the cell's end goes to the edited point (one coordinate of it) and the last column or row it spans takes up the difference. */
export function cellDrag(t: BoardTable, cell: BoardTableCell, h: EeHandle, to: readonly [number, number]): CellChange | null {
  if (h.kind !== "point" || h.index > ROW_HEIGHT) return null;
  const { start } = cellRect(t, cell);
  const rotated = Math.round(t.angle / 1000) % 180 !== 0;
  const colSpan = Math.max(cell.col_span, 1);
  const rowSpan = Math.max(cell.row_span, 1);
  const lastCol = cell.col + colSpan - 1;
  const lastRow = cell.row + rowSpan - 1;
  const others = (sizes: number[], from: number, n: number) => sizes.slice(from, from + n - 1).reduce((a, b) => a + b, 0);
  const editsColumn = rotated ? h.index === ROW_HEIGHT : h.index === COL_WIDTH;
  if (editsColumn) {
    // not turned: `SetEnd( COL_WIDTH.x, .. )`, the width `GetRectangleWidth()`; turned: `SetEnd( .., ROW_HEIGHT.y )`, the width `abs( GetRectangleHeight() )`
    const span = rotated ? Math.abs(to[1] - start[1]) : to[0] - start[0];
    return { col: lastCol, width: span - others(t.col_widths, cell.col, colSpan) };
  }
  const span = rotated ? to[0] - start[0] : to[1] - start[1];
  return { row: lastRow, height: span - others(t.row_heights, cell.row, rowSpan) };
}

/** The table as `change` leaves it, for the preview: its sizes, and every point past the edited column's (row's) far line moved by the change along the column (row). */
export function tableWithChange(t: BoardTable, change: CellChange): BoardTable {
  const isCol = "col" in change;
  const index = isCol ? change.col : change.row;
  const sizes = isCol ? t.col_widths : t.row_heights;
  const d = (isCol ? change.width : change.height) - (sizes[index] ?? 0);
  if (d === 0) return t;
  const u = rot(isCol ? [1, 0] : [0, 1], t.angle);
  // the far line of the column (row): how far along `u` it is from the table's position
  const far = sizes.slice(0, index + 1).reduce((a, b) => a + b, 0);
  const along = (q: readonly [number, number]) => (q[0] - t.x) * u[0] + (q[1] - t.y) * u[1];
  const mv = (q: [number, number]): [number, number] => (along(q) >= far - 1 ? [q[0] + d * u[0], q[1] + d * u[1]] : q);
  const nextSizes = sizes.map((s, i) => (i === index ? s + d : s));
  const [x0, y0, x1, y1] = t.bbox;
  const b0 = mv([x0, y0]);
  const b1 = mv([x1, y1]);
  return {
    ...t,
    ...(isCol ? { col_widths: nextSizes } : { row_heights: nextSizes }),
    bbox: [Math.min(b0[0], b1[0]), Math.min(b0[1], b1[1]), Math.max(b0[0], b1[0]), Math.max(b0[1], b1[1])],
    borders: t.borders.map((l) => ({ ...l, a: mv(l.a), b: mv(l.b) })),
    cells: t.cells.map((c) => {
      const past = isCol ? c.col > index : c.row > index;
      const shiftText = (q: [number, number]): [number, number] => (past ? [q[0] + d * u[0], q[1] + d * u[1]] : q);
      const corners = c.corners.map(mv) as BoardTableCell["corners"];
      const lines = (c as { lines?: Array<{ text: string; x: number; y: number }> }).lines;
      const drawPos = (c as { draw_pos?: [number, number] }).draw_pos;
      return {
        ...c,
        corners,
        ...(lines ? { lines: lines.map((l) => { const [x, y] = shiftText([l.x, l.y]); return { ...l, x, y }; }) } : {}),
        ...(drawPos ? { draw_pos: shiftText(drawPos) } : {}),
      };
    }),
  };
}

// ------------------------------------------------------------------------------------------------------------------------------------ every item

/** What the point editor has on one selected item: its edit points as the drag leaves them, the sheet in view as the drag leaves it, and what the drop sends. */
export interface SchItemEdit {
  id: string;
  kind: "graphic" | "sheet" | "image" | "line" | "cell";
  /** The drawn shape (kind `graphic`) as the drag has it. */
  graphic?: SchGraphic;
  edit: EeEditPoints;
  /** The sheet in view with the item (and what follows it) as the drag has it; the sheet itself when nothing is dragged. */
  shown: (sch: Schematic) => Schematic;
  /** The drop's commands, one undo step (`commit.Push( _( "Move Point" ) )`); empty when the drag changed nothing. */
  cmds: Cmd[];
}

/** A table cell's id: `<table id>:<row>:<col>`. */
function cellOf(sch: Schematic, id: string): { table: BoardTable; cell: BoardTableCell } | null {
  for (const t of sch.tables ?? []) {
    if (!id.startsWith(`${t.id}:`)) continue;
    const cell = t.cells.find((c) => c.id === id);
    if (cell) return { table: t, cell };
  }
  return null;
}

/** Is there a behaviour for item `id` (`pointEditorTypes` and `makePointsAndBehavior`)? Locked items and items of no behaviour have none. */
export function schItemPointEditor(
  sch: Schematic,
  id: string,
  drag: { handle: EeHandle; to: readonly [number, number] } | null,
  mode: ArcEditMode,
  measure: Measure
): SchItemEdit | null {
  const g = (sch.graphics ?? []).find((x) => x.id === id);
  if (g && g.shape.type === "image") {
    const im = imageState(g, sch);
    if (!im) return null;
    const moved = drag ? imageDrag(im, drag.handle, drag.to) : null;
    const now = moved ?? im;
    const shape = g.shape;
    const shownGraphic: SchGraphic = { ...g, shape: { ...shape, at: pt(now.at), scale_ppm: now.scalePpm } as ImageShape };
    return {
      id,
      kind: "image",
      edit: imageEditPoints(now),
      shown: (s) => (moved ? { ...s, graphics: (s.graphics ?? []).map((x) => (x.id === id ? shownGraphic : x)), image_offsets: { ...(s.image_offsets ?? {}), [id]: pt(now.offset) } } : s),
      cmds: moved ? imageCmds(id, im, moved) : [],
    };
  }
  if (g) {
    // a text box's minimum size is its text's in its face (UI-LEFT-3 C3-3)
    const face = sch.graphic_text_attrs?.[g.id]?.font ?? null;
    const ed = schPointEditor(g, drag, mode, measure, face);
    if (!ed) return null;
    const res = drag ? schDrag(g, drag.handle, drag.to, mode, measure, face) : null;
    const shape = res ? schShapeFrom(res.shape, g.shape) : null;
    const { id: _id, ...rest } = g;
    return {
      id,
      kind: "graphic",
      graphic: ed.graphic,
      edit: ed.edit,
      shown: (s) => (drag ? { ...s, graphics: (s.graphics ?? []).map((x) => (x.id === id ? ed.graphic : x)) } : s),
      cmds: shape && JSON.stringify(shape) !== JSON.stringify(g.shape) ? [{ op: "sch_edit", verb: "edit_graphic", id, graphic: { ...rest, shape } } as Cmd] : [],
    };
  }
  const sheet = sch.sheets.find((s) => s.id === id);
  if (sheet) {
    const r = drag ? sheetDrag(sch, sheet, drag.handle, drag.to, measure) : null;
    const changed = r && !(same(r.at, sheet.at) && same(r.size, sheet.size));
    const next = changed ? resizedSheet(sheet, r.at, r.size) : sheet;
    return {
      id,
      kind: "sheet",
      edit: sheetEditPoints(next),
      shown: (s) => (changed ? withResizedSheet(s, sheet, next) : s),
      cmds: changed ? [{ op: "sch_edit", verb: "resize_sheet", id, at: pt(r.at), width_nm: r.size[0], height_nm: r.size[1] } as Cmd] : [],
    };
  }
  const line = (sch.lines ?? []).find((l) => l.id === id);
  if (line) {
    if (line.pts.length < 2) return null;
    const lines = sch.lines ?? [];
    const moved = drag ? lineDrag(lines, line, drag.handle, drag.to) : null;
    const cmds = moved ? lineCmds(lines, moved) : [];
    return {
      id,
      kind: "line",
      edit: lineEditPoints(moved?.get(id) ?? line),
      shown: (s) => (moved ? { ...s, lines: (s.lines ?? []).map((l) => moved.get(l.id) ?? l) } : s),
      cmds,
    };
  }
  const tc = cellOf(sch, id);
  if (tc) {
    const change = drag ? cellDrag(tc.table, tc.cell, drag.handle, drag.to) : null;
    const before = change ? ("col" in change ? tc.table.col_widths[change.col] : tc.table.row_heights[change.row]) : undefined;
    const after = change ? ("col" in change ? change.width : change.height) : undefined;
    const real = change && after !== before ? change : null;
    const table = real ? tableWithChange(tc.table, real) : tc.table;
    const cell = table.cells.find((c) => c.id === id) ?? tc.cell;
    const target: [number, number] | null = real ? ("col" in real ? [tc.cell.row, real.col] : [real.row, tc.cell.col]) : null;
    return {
      id,
      kind: "cell",
      edit: cellEditPoints(table, cell),
      shown: (s) => (real ? { ...s, tables: (s.tables ?? []).map((t) => (t.id === table.id ? table : t)) } : s),
      cmds:
        real && target
          ? [{ op: "sch_table", verb: "edit_cells", id: tc.table.id, cells: [target], edit: "col" in real ? { col_width_nm: real.width } : { row_height_nm: real.height } } as Cmd]
          : [],
    };
  }
  return null;
}
