// The schematic editor's default font (Preferences > Schematic Editor > Display Options > "Default font:", `EESCHEMA_SETTINGS::m_Appearance.default_font`, UI-LEFT-3 S3-29)
// for what measures the sheet outside its painting: the hit tests and bounds of its texts, fields and pins. The painter runs inside `withDefaultFace` (kicadText.ts); the
// Symbol Editor's default is the stroke font (`SYMBOL_EDIT_FRAME`: `SetDefaultFont( wxEmptyString )`), so nothing of it reads this.
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import { lineAdvance, resolveFont, withDefaultFace, type KicadFont } from "./kicadText";

let face: string | null = null;

/** The schematic's default font changed (the studio's preferences). */
export function setSchDefaultFont(f: string | null | undefined): void {
  face = f || null;
}

/** The schematic's default font; null is the stroke font. */
export function schDefaultFont(): string | null {
  return face;
}

/** `GetDrawFont` of a schematic text with the face `textFace` (none: the default font). */
export function resolveSchFont(textFace: string | null | undefined, bold = false, italic = false): KicadFont {
  return withDefaultFace(face, () => resolveFont(textFace, bold, italic));
}

/** How wide a line of a schematic text in `textFace` is (none: the default font -- a pin's name or number, `PIN_LAYOUT_CACHE`'s): `strokeMeasure` for the stroke font. */
export function schPlainMeasure(strokeMeasure: (text: string, sizeNm: number) => number, textFace: string | null | undefined = null): (text: string, sizeNm: number) => number {
  const font = resolveSchFont(textFace);
  return font.kind === "stroke" ? strokeMeasure : (text, size) => lineAdvance(text, font, [size, size]);
}
