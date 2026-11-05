// The Text tool's dialog rules (`DIALOG_TEXT_PROPERTIES`, eeschema/dialogs/dialog_text_properties.cpp, as `SYMBOL_EDITOR_DRAWING_TOOLS::TwoClickPlace`
// uses it for `eeschema.SymbolDrawing.placeSymbolText`, and `SYMBOL_EDITOR_EDIT_TOOL::editTextProperties` for a text that is there: UI-LEFT Y4).
import type { FieldHAlign, FieldVAlign, LibrarySymbolGraphic } from "../api/types";
import { defaultPenNm, textOrigin } from "./schText";

/** `cfg->m_Defaults.text_size` = `DEFAULT_TEXT_SIZE` (eeschema/default_values.h): 50 mil. */
export const DEFAULT_SYMBOL_TEXT_SIZE_MM = 1.27;

/** `NoPrintableChars`: the text is empty once its ends are trimmed -- `TwoClickPlace` drops such a text (`dlg.ShowModal() != wxID_OK || NoPrintableChars( text->GetText() )`). */
export function noPrintableChars(text: string): boolean {
  return text.trim() === "";
}

/** `TransferDataFromWindow`: "Don't allow text to disappear" -- `m_textSize.Validate( 0.01, 1000.0, EDA_UNITS::MM )`. */
export function textSizeError(sizeMm: number): string | null {
  if (!Number.isFinite(sizeMm) || sizeMm < 0.01 || sizeMm > 1000) return "Text size must be between 0.01 mm and 1000 mm.";
  return null;
}

/** The dialog's Horizontal / Vertical buttons: `ANGLE_HORIZONTAL` (0) or `ANGLE_VERTICAL` (90). */
export function textAngleDeg(vertical: boolean): number {
  return vertical ? 90 : 0;
}

/** A symbol's text (`SCH_TEXT` on `LAYER_DEVICE`), as the dialog reads and writes it. */
export type LibText = Extract<LibrarySymbolGraphic, { kind: "text" }>;

/** What `DIALOG_TEXT_PROPERTIES` shows for a symbol's text (the font, the colour, the hyperlink and "Private to symbol editor" are not part of a library text here). */
export interface TextForm {
  text: string;
  /** Text size, mm, as typed. */
  size: string;
  bold: boolean;
  italic: boolean;
  h: FieldHAlign;
  v: FieldVAlign;
  vertical: boolean;
  /** "Common to all units": `symbol->IsMultiUnit() && GetUnit() == 0`, enabled for a symbol of several units only. */
  commonUnits: boolean;
  /** "Common to all body styles": `IsMultiBodyStyle() && GetBodyStyle() == 0`. */
  commonStyles: boolean;
}

/** `DIALOG_TEXT_PROPERTIES::TransferDataToWindow` for a symbol's text: its text, size, bold, italic, the justification buttons (left/centre/right, top/centre/bottom),
 *  Horizontal or Vertical (`GetTextAngle() == ANGLE_VERTICAL`), and the two "Common to all ..." boxes. */
export function textFormOf(t: LibText, symbol: { unit_count: number; has_alternate_body_style: boolean }): TextForm {
  return {
    text: t.text,
    size: String(t.size_mm),
    bold: !!t.bold,
    italic: !!t.italic,
    h: t.h_align ?? "center",
    v: t.v_align ?? "center",
    vertical: Math.abs(t.angle_deg % 180) === 90,
    commonUnits: symbol.unit_count > 1 && t.unit === 0,
    commonStyles: symbol.has_alternate_body_style && t.body_style === 0,
  };
}

/**
 * `DIALOG_TEXT_PROPERTIES::TransferDataFromWindow`: the text as the dialog leaves it -- `SetText`, `SetTextSize`, `SetBold`, `SetItalic`, the justifications, the angle -- and in the
 * symbol editor `SetUnit( common ? 0 : GetUnit() )` and `SetBodyStyle( common ? 0 : GetBodyStyle() )` of the unit and body style the editor shows. A size out of 0.01 .. 1000 mm
 * is refused with the message (`m_textSize.Validate`).
 */
export function textFromForm(prev: LibText, form: TextForm, unit: number, bodyStyle: number): { ok: true; text: LibText } | { ok: false; error: string } {
  const sizeMm = Number(form.size);
  const bad = textSizeError(sizeMm);
  if (bad) return { ok: false, error: bad };
  return {
    ok: true,
    text: {
      ...prev,
      text: form.text,
      size_mm: sizeMm,
      bold: form.bold,
      italic: form.italic,
      h_align: form.h,
      v_align: form.v,
      angle_deg: textAngleDeg(form.vertical),
      unit: form.commonUnits ? 0 : unit,
      body_style: form.commonStyles ? 0 : bodyStyle,
    },
  };
}

/**
 * `SCH_TEXT::GetBoundingBox` of a symbol's text, in the editor's internal nm (+y down): the line of text where `SCH_PAINTER` draws it -- its baseline start `textOrigin` from the
 * anchor in the text's own axes, the cap height above it -- turned up the sheet for a vertical text. `measure` is the stroke font's advance.
 */
export function libTextBoxNm(t: Pick<LibText, "text" | "at" | "angle_deg" | "size_mm" | "bold" | "h_align" | "v_align">, measure: (text: string, sizeNm: number) => number): { minX: number; minY: number; maxX: number; maxY: number } {
  const sizeNm = t.size_mm * 1e6;
  const thickness = t.bold ? Math.round(sizeNm / 5) : defaultPenNm(sizeNm);
  const advance = measure(t.text, sizeNm);
  const [dx, dy] = textOrigin(advance, sizeNm, thickness, t.h_align ?? "center", t.v_align ?? "center");
  // the text's own box: from the baseline start, the advance along, the cap height above
  const u0 = dx - thickness / 2;
  const u1 = dx + advance + thickness / 2;
  const v0 = dy - sizeNm - thickness / 2;
  const v1 = dy + thickness / 2;
  const [ax, ay] = [t.at.x * 1e6, -t.at.y * 1e6];
  const vertical = Math.abs(t.angle_deg % 180) === 90;
  // `ctx.rotate( -PI / 2 )`: (u, v) of the text is (x + v, y - u) of the sheet
  const corners: [number, number][] = vertical ? [[ax + v0, ay - u0], [ax + v1, ay - u1]] : [[ax + u0, ay + v0], [ax + u1, ay + v1]];
  return { minX: Math.min(corners[0]![0], corners[1]![0]), minY: Math.min(corners[0]![1], corners[1]![1]), maxX: Math.max(corners[0]![0], corners[1]![0]), maxY: Math.max(corners[0]![1], corners[1]![1]) };
}
