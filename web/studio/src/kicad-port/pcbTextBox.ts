// The board's text box (`PCB_TEXTBOX`) as the studio lays it out and draws it: where its four corners are for the text's angle, the box the margins leave the text, how the text wraps to
// it and where each line is anchored, the smallest the box may be, where a click picks it, the box the Draw Text Box tool starts with and the text a dialog edits.
//
// Ported from `PCB_TEXTBOX::GetDrawPos`, `::GetShownText` (the column width), `::GetMinSize`, `::HitTest`, `::GetLegacyTextMargin` (pcbnew/pcb_textbox.cpp), `EDA_SHAPE::GetCornersInSequence`
// (common/eda_shape.cpp), `FONT::LinebreakText` and `FONT::getLinePositions` (common/font/font.cpp) and `DRAWING_TOOL::DrawRectangle` (pcbnew/tools/drawing_tool.cpp), all at KiCad
// 8303b2ad. The wrapping is the greedy break at spaces `LinebreakText` does (a word wider than the column keeps its line to itself); the stroke font's own metrics decide the widths,
// so the caller passes the measuring function.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardTextBox, CmdTextBox, TextBoxHAlign, TextBoxVAlign } from "../api/pcbItemTypes";
import type { BoardState, PointXY, Um } from "../api/types";
import { isBackLayer, lineThicknessUm, textStyleOf } from "./layerDefaults";

export type P = [number, number];
/** How wide a line of text is, micrometres. */
export type Measure = (text: string, sizeUm: number) => number;

/** `METRICS::m_InterlinePitch` and `STROKE_FONT::GetInterline`'s `LEGACY_FACTOR`: a line is 1.68 * 0.9583 glyph heights below the one before. */
export const INTERLINE = 1.68 * 0.9583;
/** `TEXT_MIN_SIZE_MM` and `TEXT_MAX_SIZE_MM`. */
export const TEXT_MIN_SIZE_UM = 1;
export const TEXT_MAX_SIZE_UM = 250_000;

/** `GetPenSizeForNormal` / `GetPenSizeForBold`: an eighth and a fifth of the glyph. */
export function autoPenUm(sizeUm: number, bold: boolean): number {
  return Math.round(sizeUm / (bold ? 5 : 8));
}

/** `ClampTextPenSize( aPenSize, aSize )`: the pen may not be wider than a sixth of the smaller glyph side (`aStrict` false). */
export function clampPenUm(penUm: number, sizeUm: number): number {
  return Math.min(penUm, Math.round(sizeUm / 6));
}

/** `PCB_TEXTBOX::GetLegacyTextMargin`: half the border and three quarters of the text height. */
export function legacyMarginUm(strokeWidthUm: number, sizeUm: number): number {
  return Math.round(strokeWidthUm / 2) + Math.round(sizeUm * 0.75);
}

const isCardinal = (angleMilli: number): boolean => angleMilli % 90_000 === 0;
const rad = (angleMilli: number): number => (angleMilli / 1000) * (Math.PI / 180);

/** The extent `[x0, y0, x1, y1]` of the box's corners (the border's width is not in it). */
export function boxExtent(b: Pick<BoardTextBox, "start" | "end" | "pts">): [number, number, number, number] {
  const pts = b.pts.length > 0 ? b.pts : [b.start, b.end];
  let [x0, y0, x1, y1] = [Infinity, Infinity, -Infinity, -Infinity];
  for (const [x, y] of pts) {
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x);
    y1 = Math.max(y1, y);
  }
  return [x0, y0, x1, y1];
}

/**
 * `EDA_SHAPE::GetCornersInSequence( angle )`: the four corners in the order the text's frame has them -- top left, top right, bottom right, bottom left of the text as it reads. A
 * rectangle takes them from its box, a polygon from the corners furthest left, up, right and down.
 */
export function cornersInSequence(b: Pick<BoardTextBox, "start" | "end" | "pts">, angleMilli: number): [P, P, P, P] {
  const angle = ((angleMilli % 360_000) + 360_000) % 360_000;
  if (b.pts.length === 0 || isCardinal(angle)) {
    const [x0, y0, x1, y1] = boxExtent(b);
    const [lt, rt, rb, lb]: [P, P, P, P] = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    if (angle === 90_000) return [lb, lt, rt, rb];
    if (angle === 180_000) return [rb, lb, lt, rt];
    if (angle === 270_000) return [rt, rb, lb, lt];
    return [lt, rt, rb, lb];
  }
  const corners: P[] = b.pts.map((p) => [p[0], p[1]]);
  while (corners.length < 4) {
    const last = corners[corners.length - 1] ?? [0, 0];
    corners.push([last[0] + 10, last[1] + 10]);
  }
  let [minX, maxX, minY, maxY] = [corners[0]!, corners[0]!, corners[0]!, corners[0]!];
  for (const c of corners) {
    if (c[0] < minX[0]) minX = c;
    if (c[0] > maxX[0]) maxX = c;
    if (c[1] < minY[1]) minY = c;
    if (c[1] > maxY[1]) maxY = c;
  }
  if (angle < 90_000) return [minX, minY, maxX, maxY];
  if (angle < 180_000) return [maxY, minX, minY, maxX];
  if (angle < 270_000) return [maxX, maxY, minX, minY];
  return [minY, maxX, maxY, minX];
}

/** The corners of the box in drawing order, for the border and the knockout: the polygon's, else the rectangle's four. */
export function outlineOf(b: Pick<BoardTextBox, "start" | "end" | "pts">): P[] {
  if (b.pts.length > 0) return b.pts.map((p) => [p[0], p[1]]);
  const [x0, y0, x1, y1] = boxExtent(b);
  return [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
}

/** `EDA_TEXT::UnescapeString`'s part that matters here: the text a dialog and the canvas show has `{dblquote}` as the quote it stands for. */
export function unescapeQuoted(s: string): string {
  return s.replace(/\{dblquote\}/g, '"');
}

/** `EscapeString( CTX_QUOTED_STR )`: what a dialog stores a quote as. */
export function escapeQuoted(s: string): string {
  return s.replace(/"/g, "{dblquote}");
}

/** The text the box shows: its variables expanded by the backend (`shown`), the quotes unescaped. */
export function shownTextOf(b: Pick<BoardTextBox, "text" | "shown">): string {
  return unescapeQuoted(b.shown ?? b.text);
}

/**
 * `FONT::LinebreakText`: each paragraph (split at a line feed) broken at spaces so that a line fits `columnUm - thicknessUm`; the space a line is broken at goes, a word wider than
 * the column stays whole on a line of its own.
 */
export function linebreakText(text: string, columnUm: number, sizeUm: number, thicknessUm: number, measure: Measure): string[] {
  const out: string[] = [];
  const space = measure(" ", sizeUm);
  const limit = columnUm - thicknessUm;
  for (const paragraph of text.split("\n")) {
    const words = paragraph.split(" ");
    let line = "";
    let lineWidth = 0;
    let first = true;
    for (const word of words) {
      const wordWidth = measure(word, sizeUm);
      if (first) {
        line = word;
        lineWidth = wordWidth;
        first = false;
      } else if (lineWidth + space + wordWidth > limit) {
        out.push(line);
        line = word;
        lineWidth = wordWidth;
      } else {
        line += ` ${word}`;
        lineWidth += space + wordWidth;
      }
    }
    out.push(line);
  }
  return out;
}

export interface BoxTextLine {
  text: string;
  /** Where the line's baseline starts, from the anchor, in the text's own axes (x along the text, y down), micrometres. */
  x: number;
  y: number;
}

export interface BoxTextLayout {
  /** `GetDrawPos`: where the text is anchored on the board. */
  anchor: P;
  /** The text's angle, counter-clockwise on the screen (the file's), radians. */
  angleRad: number;
  lines: BoxTextLine[];
}

/** `PCB_TEXTBOX::GetDrawPos( false )`: the point of the box the text hangs on, for its justification, the margins in from that side. */
export function drawPos(b: BoardTextBox): P {
  const c = cornersInSequence(b, b.angle);
  const mid = (a: P, d: P): P => [(a[0] + d[0]) / 2, (a[1] + d[1]) / 2];
  const midTop = mid(c[0], c[1]);
  const midBottom = mid(c[3], c[2]);
  const midLeft = mid(c[0], c[3]);
  const midRight = mid(c[1], c[2]);
  const center: P = [(c[0][0] + c[1][0] + c[2][0] + c[3][0]) / 4, (c[0][1] + c[1][1] + c[2][1] + c[3][1]) / 4];
  // `if( IsMirrored() != aIsFlipped )`: a mirrored box is read from its other side
  const h: TextBoxHAlign = b.mirror ? (b.halign === "left" ? "right" : b.halign === "right" ? "left" : "center") : b.halign;
  const v: TextBoxVAlign = b.valign;
  const anchors: Record<string, P> = {
    "left/top": c[0],
    "center/top": midTop,
    "right/top": c[1],
    "left/center": midLeft,
    "center/center": center,
    "right/center": midRight,
    "left/bottom": c[3],
    "center/bottom": midBottom,
    "right/bottom": c[2],
  };
  const anchor = anchors[`${h}/${v}`]!;
  const [ml, mt, mr, mb] = b.margins;
  let ox = h === "left" ? ml : h === "right" ? -mr : 0;
  let oy = v === "top" ? mt : v === "bottom" ? -mb : 0;
  // `RotatePoint( offset, GetDrawRotation() )`: the text angle is counter-clockwise on the screen (y down)
  const a = rad(b.angle);
  [ox, oy] = [ox * Math.cos(a) + oy * Math.sin(a), -ox * Math.sin(a) + oy * Math.cos(a)];
  return [Math.trunc(anchor[0] + ox), Math.trunc(anchor[1] + oy)];
}

/** The width the text wraps to (`GetShownText`): the first side of the text's frame less the margins ("top and bottom" for a box that is not horizontal, as in source). */
export function columnWidthUm(b: BoardTextBox): number {
  const c = cornersInSequence(b, b.angle);
  const col = Math.hypot(c[1][0] - c[0][0], c[1][1] - c[0][1]);
  const horizontal = b.angle % 180_000 === 0;
  return col - (horizontal ? b.margins[0] + b.margins[2] : b.margins[1] + b.margins[3]);
}

/**
 * Where the lines of the box's text go: wrapped to the column, then laid out as `FONT::getLinePositions` lays out a text anchored at `GetDrawPos`: the first line's top is `size * 1.17`
 * high with an interline more for every line after it, the block shifted up by all of it for a bottom justification and by half of it for a centre one, each line justified on its
 * own. `lineSpacing` is the file's multiplier (1.0 is the default).
 */
export function layoutBoxText(b: BoardTextBox, measure: Measure): BoxTextLayout {
  const size = b.size;
  const pen = b.thickness;
  const lines = linebreakText(shownTextOf(b), columnWidthUm(b), size, b.thickness_auto ? 0 : b.thickness, measure);
  const interline = Math.trunc(size * INTERLINE * (b.line_spacing_permille / 1000));
  const height = Math.trunc(size * 1.17) + (lines.length - 1) * interline;
  const offX = Math.trunc(pen / 1.52);
  let offY = Math.trunc(size - pen * 0.052);
  if (b.valign === "center") offY -= Math.trunc(height / 2);
  else if (b.valign === "bottom") offY -= height;
  return {
    anchor: drawPos(b),
    angleRad: rad(b.angle),
    lines: lines.map((text, i) => {
      const advance = Math.trunc(measure(text, size));
      const x = b.halign === "left" ? offX : b.halign === "center" ? -Math.trunc(advance / 2) : -(advance + offX);
      return { text, x, y: offY + i * interline };
    }),
  };
}

/**
 * `PCB_TEXTBOX::GetMinSize`: the box must be as high as its text and the margins across the text's lines (as wide, for text that runs up the box); a box with no text has no minimum. The
 * text's height is `EDA_TEXT::GetTextBox`: the line and the pen's reach, 17 % more for what hangs below the line, and an interline for each line after the first.
 */
export function minSizeUm(b: BoardTextBox, measure: Measure): { w: number; h: number } {
  if (shownTextOf(b) === "") return { w: 0, h: 0 };
  const lines = layoutBoxText(b, measure).lines.length;
  const ext = b.size + 2 * Math.round(b.thickness * 1.5);
  const interline = Math.trunc(b.size * INTERLINE * (b.line_spacing_permille / 1000));
  const textHeight = ext + Math.round(ext * 0.17) + (lines - 1) * interline;
  const horizontal = b.angle % 180_000 === 0;
  const need = textHeight + (horizontal ? b.margins[1] + b.margins[3] : b.margins[0] + b.margins[2]);
  const rotation = b.angle % 360_000;
  // "For 90/270 degree rotations, the text's natural height maps to screen x-axis."
  return rotation === 90_000 || rotation === 270_000 ? { w: need, h: 0 } : { w: 0, h: need };
}

/** The box grown to its minimum size (`DIALOG_TEXTBOX_PROPERTIES::TransferDataFromWindow`: the end moves away from the start until the box holds its text). */
export function grownToFit(b: BoardTextBox, measure: Measure): BoardTextBox {
  if (b.pts.length > 0) return b;
  const min = minSizeUm(b, measure);
  let [ex, ey] = b.end;
  if (min.w > 0 && Math.abs(b.end[0] - b.start[0]) < min.w) ex = b.end[0] >= b.start[0] ? b.start[0] + min.w : b.start[0] - min.w;
  if (min.h > 0 && Math.abs(b.end[1] - b.start[1]) < min.h) ey = b.end[1] >= b.start[1] ? b.start[1] + min.h : b.start[1] - min.h;
  return ex === b.end[0] && ey === b.end[1] ? b : { ...b, end: [ex, ey] };
}

/** `RotatePoint( pt, centre, angle )` for an angle counter-clockwise on the screen (y down); the quarter turns are exact. */
function rotateCcw(p: P, c: P, angleMilli: number): P {
  const a = ((angleMilli % 360_000) + 360_000) % 360_000;
  const [dx, dy] = [p[0] - c[0], p[1] - c[1]];
  if (a % 90_000 === 0) {
    const q = a / 90_000;
    return q === 0 ? [p[0], p[1]] : q === 1 ? [c[0] + dy, c[1] - dx] : q === 2 ? [c[0] - dx, c[1] - dy] : [c[0] - dy, c[1] + dx];
  }
  const r = rad(a);
  return [Math.round(c[0] + dx * Math.cos(r) + dy * Math.sin(r)), Math.round(c[1] - dx * Math.sin(r) + dy * Math.cos(r))];
}

/**
 * `PCB_TEXTBOX::SetTextAngle`: what Text Box Properties' Orientation does -- the box turns about its position by the difference, counter-clockwise, text and all. A rectangle turned by
 * anything but a quarter turn becomes the polygon of its four corners, and a polygon whose text is again at a multiple of 90 degrees a rectangle (`PCB_TEXTBOX::Rotate`).
 */
export function withTextAngle(b: BoardTextBox, angleMilli: number): BoardTextBox {
  const target = ((angleMilli % 360_000) + 360_000) % 360_000;
  const delta = (((target - b.angle) % 360_000) + 360_000) % 360_000;
  if (delta === 0) return b;
  const pivot: P = b.pts.length > 0 ? [b.pts[0]![0], b.pts[0]![1]] : [b.start[0], b.start[1]];
  let pts: P[];
  let start: P = b.start;
  let end: P = b.end;
  if (b.pts.length === 0) {
    if (delta % 90_000 === 0) {
      start = rotateCcw(b.start, pivot, delta);
      end = rotateCcw(b.end, pivot, delta);
      return { ...b, start, end, angle: target };
    }
    pts = [b.start, [b.end[0], b.start[1]], b.end, [b.start[0], b.end[1]]].map((q) => rotateCcw(q as P, pivot, delta));
  } else {
    pts = b.pts.map((q) => rotateCcw(q, pivot, delta));
  }
  const turned: BoardTextBox = { ...b, pts, start: pts[0]!, end: pts[2]!, angle: target };
  if (target % 90_000 !== 0) return turned;
  // A polygon at a multiple of 90 degrees is a rectangle again, started at the first corner of the text's frame.
  const c = cornersInSequence(turned, target);
  const [dx, dy] = [Math.abs(c[2][0] - c[0][0]), Math.abs(c[2][1] - c[0][1])];
  const c0 = c[0];
  start = c0;
  end = target === 90_000 ? [c0[0] + dx, c0[1] - dy] : target === 180_000 ? [c0[0] - dx, c0[1] - dy] : target === 270_000 ? [c0[0] - dx, c0[1] + dy] : [c0[0] + dx, c0[1] + dy];
  return { ...turned, pts: [], start, end };
}

/** The box's bounding box as `PCB_SHAPE::GetBoundingBox` has it: the corners and half the border's width on each side. */
export function textBoxBounds(b: BoardTextBox): [number, number, number, number] {
  const [x0, y0, x1, y1] = boxExtent(b);
  const w = b.border ? b.stroke_width / 2 : 0;
  return [x0 - w, y0 - w, x1 + w, y1 + w];
}

/** `PCB_TEXTBOX::HitTest( position, accuracy )`: anywhere in the box's bounding box, `accuracy` further out. Returns the distance from the box (0 inside it). */
export function textBoxHitDistance(b: BoardTextBox, x: number, y: number): number {
  const [x0, y0, x1, y1] = textBoxBounds(b);
  const dx = Math.max(x0 - x, 0, x - x1);
  const dy = Math.max(y0 - y, 0, y - y1);
  return Math.hypot(dx, dy);
}

/** The text box as the cmd shape a verb takes. */
export function textBoxToCmd(b: BoardTextBox): CmdTextBox {
  const pt = (p: readonly [number, number]): PointXY => ({ x: p[0], y: p[1] });
  return {
    id: b.id,
    layer: b.layer,
    text: b.text,
    start: pt(b.start),
    end: pt(b.end),
    pts: b.pts.map(pt),
    angle: b.angle,
    margin_left_um: b.margins[0],
    margin_top_um: b.margins[1],
    margin_right_um: b.margins[2],
    margin_bottom_um: b.margins[3],
    border: b.border,
    stroke_width_um: b.stroke_width,
    line_style: b.line_style,
    stroke_color: b.stroke_color,
    knockout: b.knockout,
    size_um: b.size,
    width_um: b.width !== b.size ? b.width : null,
    thickness_um: b.thickness_auto ? 0 : b.thickness,
    halign: b.halign,
    valign: b.valign,
    mirror: b.mirror,
    bold: b.bold,
    italic: b.italic,
    line_spacing_permille: b.line_spacing_permille === 1000 ? 0 : b.line_spacing_permille,
    face: b.face,
  };
}

/** The state shape of a cmd text box (what the dialog works on), for a box that is not on the board yet. */
export function boardTextBoxOf(c: CmdTextBox): BoardTextBox {
  const pt = (p: PointXY): [number, number] => [p.x, p.y];
  const size = c.size_um;
  const auto = !c.thickness_um;
  return {
    id: c.id ?? "",
    layer: c.layer,
    text: c.text,
    start: pt(c.start),
    end: pt(c.end),
    pts: (c.pts ?? []).map(pt),
    angle: c.angle ?? 0,
    margins: [c.margin_left_um, c.margin_top_um, c.margin_right_um, c.margin_bottom_um],
    border: c.border ?? true,
    stroke_width: c.stroke_width_um,
    line_style: c.line_style ?? "default",
    stroke_color: c.stroke_color ?? null,
    knockout: c.knockout ?? false,
    size,
    width: c.width_um ?? size,
    thickness: auto ? autoPenUm(Math.min(c.width_um ?? size, size), c.bold ?? false) : c.thickness_um!,
    thickness_auto: auto,
    halign: c.halign ?? "left",
    valign: c.valign ?? "center",
    mirror: c.mirror ?? false,
    bold: c.bold ?? false,
    italic: c.italic ?? false,
    line_spacing_permille: c.line_spacing_permille ? c.line_spacing_permille : 1000,
    face: c.face ?? null,
  };
}

/**
 * The text box the Draw Text Box tool makes between two corners on `layer` (`DRAWING_TOOL::DrawRectangle` with `m_textAttrs` from `BOARD_DESIGN_SETTINGS`): the layer's text size and pen,
 * italic when the class says so, mirrored on a back layer, left and top, a border of the layer's line width, the default margins.
 */
export function newTextBox(board: Pick<BoardState, "board_rules">, layer: string, start: P, end: P): BoardTextBox {
  const defaults = board.board_rules?.text_graphics;
  const style = textStyleOf(defaults, layer);
  const width = lineThicknessUm(defaults, layer);
  const margin = legacyMarginUm(width, style.heightUm);
  return boardTextBoxOf({
    layer,
    text: "",
    start: { x: start[0], y: start[1] },
    end: { x: end[0], y: end[1] },
    margin_left_um: margin,
    margin_top_um: margin,
    margin_right_um: margin,
    margin_bottom_um: margin,
    stroke_width_um: width,
    size_um: style.heightUm,
    width_um: style.widthUm !== style.heightUm ? style.widthUm : null,
    thickness_um: style.thicknessUm,
    halign: "left",
    valign: "top",
    mirror: isBackLayer(layer),
    italic: style.italic,
  });
}

export type { Um };
