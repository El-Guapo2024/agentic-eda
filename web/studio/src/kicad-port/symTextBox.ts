// The text box of a library symbol (`SCH_TEXTBOX` on `LAYER_DEVICE`, `(text_box ...)` of a `.kicad_sym`) as the Symbol Editor lays it out and draws it: where its corners are in the
// editor's space, the margin it has, how its text wraps and lies in it (the layout `kicad-port/schTextBox.ts` ports for the sheet's text boxes), the smallest it may be, where a click
// picks it, and what the tool starts a new one with. Ports of `SCH_TEXTBOX::GetLegacyTextMargin`, `GetDrawPos`, `GetMinSize`, `HitTest` (eeschema/sch_textbox.cpp), of the box a
// click-and-click makes in `SYMBOL_EDITOR_DRAWING_TOOLS::doDrawShape` (eeschema/tools/symbol_editor_drawing_tools.cpp) and of the end of
// `DIALOG_TEXT_PROPERTIES::TransferDataFromWindow`, which makes a box as big as its text needs.
//
// The graphic keeps its corners in the symbol's own mm with Y up (`LibrarySymbolGraphic`); everything here that is `Nm` is the editor's internal space, micrometres with Y down.
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { FieldHAlign, FieldVAlign, LibraryFill, LibrarySymbolGraphic } from "../api/types";
import { defaultPenNm } from "./schText";
import { interlineNm } from "./schTextItem";
import { layoutTextBox, type Measure, type TextBoxLayout } from "./schTextBox";

export type LibTextBox = Extract<LibrarySymbolGraphic, { kind: "text_box" }>;

/** `cfg->m_Defaults.text_size` = `DEFAULT_TEXT_SIZE` (eeschema/default_values.h): 50 mil. */
export const DEFAULT_TEXT_BOX_SIZE_MM = 1.27;

/** The width of a border switched off: -1 of a schematic's internal units (0.1 um), what `STROKE_PARAMS` holds and a file says as `(width -0.0001)`. */
export const NO_BORDER_MM = -0.0001;

/** A box in the editor's space. */
export interface BoxNm {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

const um = (p: { x: number; y: number }): [number, number] => [p.x * 1e6, -p.y * 1e6];

/**
 * `SCH_TEXTBOX::GetLegacyTextMargin` for a box on `LAYER_DEVICE`: four fifths of the text height, in the schematic's 0.1 um. A box that has a margin of its own keeps it. (The
 * four margins are one number here: the greatest of a file's.)
 */
export function textBoxMarginMm(g: Pick<LibTextBox, "size_mm" | "margin_mm">): number {
  const own = g.margin_mm ?? 0;
  return own > 0 ? own : Math.round(g.size_mm * 0.8 * 10_000) / 10_000;
}

/** The two corners of the box, ordered, in the editor's space. */
export function boxOfTextBox(g: Pick<LibTextBox, "start" | "end">): BoxNm {
  const [x0, y0] = um(g.start);
  const [x1, y1] = um(g.end);
  return { left: Math.min(x0, x1), top: Math.min(y0, y1), right: Math.max(x0, x1), bottom: Math.max(y0, y1) };
}

/** Whether the text runs up the box (`ANGLE_VERTICAL`) rather than along it. */
export function isVertical(g: Pick<LibTextBox, "angle_deg">): boolean {
  return Math.round(g.angle_deg) % 180 === 90;
}

/** Where the lines of the text go (`SCH_TEXTBOX::GetDrawPos` and the wrapping of `GetShownText`): `layoutTextBox` on the box's corners, in the editor's space. */
export function layoutLibTextBox(g: LibTextBox, measure: Measure): TextBoxLayout {
  return layoutTextBox(
    {
      start: um(g.start),
      end: um(g.end),
      text: g.text,
      angle: isVertical(g) ? 90_000 : 0,
      sizeNm: g.size_mm * 1e6,
      hAlign: g.h_align,
      vAlign: g.v_align,
      marginNm: Math.round(textBoxMarginMm(g) * 1e6),
    },
    measure
  );
}

/**
 * `SCH_TEXTBOX::GetMinSize`: the box must be as high as its text and the top and bottom margins (as wide, for text that runs up it), whatever it is along the text; a box
 * with no text has no minimum. The text's height is `EDA_TEXT::GetTextBox`: the line and the pen's reach, 17 % more for what hangs below the line, and an interline for each
 * line after the first of the wrapped text.
 */
export function textBoxMinSizeNm(g: LibTextBox, measure: Measure): { w: number; h: number } {
  if (g.text === "") return { w: 0, h: 0 };
  const size = g.size_mm * 1e6;
  const pen = g.bold ? Math.round(size / 5) : defaultPenNm(size);
  const ext = size + 2 * Math.round(pen * 1.5);
  const lines = layoutLibTextBox(g, measure).lines.length;
  const textHeight = ext + Math.round(ext * 0.17) + (lines - 1) * interlineNm(size);
  const margins = 2 * Math.round(textBoxMarginMm(g) * 1e6);
  return isVertical(g) ? { w: textHeight + margins, h: 0 } : { w: 0, h: textHeight + margins };
}

/**
 * The end of `DIALOG_TEXT_PROPERTIES::TransferDataFromWindow` for a text box: a box smaller than its text needs is made bigger, from its start, in the way it was drawn (the far
 * corner moves on past the start in the direction it already was). Returns the same graphic when it is big enough.
 */
export function grownToFit(g: LibTextBox, measure: Measure): LibTextBox {
  const min = textBoxMinSizeNm(g, measure);
  const [sx, sy] = um(g.start);
  let [ex, ey] = um(g.end);
  let grown = false;
  if (min.w > 0 && Math.abs(ex - sx) < min.w) {
    ex = ex >= sx ? sx + min.w : sx - min.w;
    grown = true;
  }
  if (min.h > 0 && Math.abs(ey - sy) < min.h) {
    ey = ey >= sy ? sy + min.h : sy - min.h;
    grown = true;
  }
  if (!grown) return g;
  // the file is in 0.1 um: no float noise in the far corner
  const mm = (v: number) => Math.round(v / 100) / 10_000; // nm to mm, on the file's 100 nm
  return { ...g, end: { x: mm(ex), y: -mm(ey) } };
}

/** `SCH_TEXTBOX::HitTest( point, accuracy )`: a point is on the box when it is in it, the whole of it and not its border alone, inflated by the accuracy. 0 inside, else how far outside. */
export function textBoxDistanceNm(g: Pick<LibTextBox, "start" | "end">, x: number, y: number): number {
  const b = boxOfTextBox(g);
  const dx = Math.max(b.left - x, 0, x - b.right);
  const dy = Math.max(b.top - y, 0, y - b.bottom);
  return Math.hypot(dx, dy);
}

/** What the tool carries from one text box to the next (`m_lastTextBold`, `m_lastTextItalic`, `m_lastTextAngle`, `m_lastTextJust`, `m_lastFillStyle`, `m_lastStroke`). */
export interface LastTextBox {
  bold: boolean;
  italic: boolean;
  vertical: boolean;
  hAlign: FieldHAlign;
  fill: LibraryFill;
  strokeMm: number;
}

/** The tool's own start (`SYMBOL_EDITOR_DRAWING_TOOLS` constructor): upright, left, nothing bold, not filled, a border of the default width (`m_lastStroke( 0, ... )`). */
export const INITIAL_LAST_TEXT_BOX: LastTextBox = { bold: false, italic: false, vertical: false, hAlign: "left", fill: "none", strokeMm: 0 };

/**
 * The box two clicks make (`doDrawShape`, `isTextBox`): the first corner where the first click went, the opposite one where the second did, a text of 50 mil begun the way the last
 * box was (bold, italic, direction, justification, fill, border), empty until the dialog says what it holds. `a` and `b` are in the editor's space; `unit` and `bodyStyle` are
 * where the editor is (a graphic is drawn on its own unit and body style here).
 */
export function newTextBox(a: readonly [number, number], b: readonly [number, number], unit: number, bodyStyle: number, last: LastTextBox): LibTextBox {
  const mm = (v: number) => Math.round(v / 100) / 10_000; // nm to mm, on the file's 100 nm
  return {
    kind: "text_box",
    unit,
    body_style: bodyStyle,
    text: "",
    start: { x: mm(a[0]), y: -mm(a[1]) },
    end: { x: mm(b[0]), y: -mm(b[1]) },
    angle_deg: last.vertical ? 90 : 0,
    size_mm: DEFAULT_TEXT_BOX_SIZE_MM,
    bold: last.bold,
    italic: last.italic,
    h_align: last.hAlign,
    v_align: "top",
    // the constructor sets the four margins from the text size it has then; a later change of the size leaves them (`SCH_TEXTBOX::SCH_TEXTBOX`)
    margin_mm: textBoxMarginMm({ size_mm: DEFAULT_TEXT_BOX_SIZE_MM }),
    stroke_mm: last.strokeMm,
    fill: last.fill,
  };
}

/** What the tool remembers once a box was accepted: its looks, for the next one. */
export function lastOf(g: LibTextBox): LastTextBox {
  return { bold: !!g.bold, italic: !!g.italic, vertical: isVertical(g), hAlign: g.h_align, fill: g.fill, strokeMm: g.stroke_mm };
}

/** The alignment buttons of the dialog: the three horizontal and the three vertical ones. */
export const H_ALIGNS: readonly FieldHAlign[] = ["left", "center", "right"];
export const V_ALIGNS: readonly FieldVAlign[] = ["top", "center", "bottom"];
