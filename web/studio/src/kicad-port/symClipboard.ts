// The Symbol Editor's Copy / Paste text (`SYMBOL_EDIT_FRAME::CopySymbolToClipboard` / `DuplicateSymbol( true )`): KiCad puts the
// bare `(symbol "Name" ...)` forms of the copied symbols on the system clipboard, one after the other
// (`SCH_IO_KICAD_SEXPR::FormatLibSymbol`), and reads them back with `ParseLibSymbols`, which accepts exactly such a run. The
// studio's derived `.kicad_sym` of one symbol is a whole `(kicad_symbol_lib ...)` file around the same form, so the clipboard text
// is that file with the wrapper taken off -- which also makes a copy pasteable into KiCad itself.

/**
 * The `(symbol ...)` forms of a `.kicad_sym` file as `export_kicad_sym` writes it (`(kicad_symbol_lib ...` on the first line,
 * the symbols, a closing `)` last): everything between the wrapper's first line and its closing parenthesis.
 */
export function bareSymbolForms(libraryText: string): string {
  const open = libraryText.indexOf("\n");
  const close = libraryText.lastIndexOf(")");
  if (open < 0 || close <= open) return libraryText;
  return `${libraryText.slice(open + 1, close).trimEnd()}\n`;
}

/** The clipboard text for several symbols: their forms one after the other. */
export function clipboardTextForSymbols(libraryTexts: readonly string[]): string {
  return libraryTexts.map(bareSymbolForms).join("");
}

/** Whether a clipboard text looks like symbol forms at all (so a paste of unrelated text says so instead of a parse error). */
export function looksLikeSymbolText(text: string): boolean {
  const t = text.trimStart();
  return t.startsWith("(symbol") || t.startsWith("(kicad_symbol_lib");
}

// Copy's images of the selection (UI-LEFT-2 YY5; components/symbol/symbolClipboardImage.ts draws them): the box and the bitmap's size.
export type ClipBox = readonly [number, number, number, number];
type Box = ClipBox;

/** `clipboardBboxInflation`. */
export const CLIPBOARD_BBOX_INFLATION = 0.02;
/** `clipboardMaxBitmapSize`. */
export const CLIPBOARD_MAX_BITMAP_SIZE = 4096;

/** The selection's box (internal nm), merged and grown as `Copy` grows it (`bbox.Inflate( w * 0.02, h * 0.02 )`); null for an empty one. */
export function clipboardBox(boxes: readonly (Box | null)[]): Box | null {
  let b: [number, number, number, number] | null = null;
  for (const x of boxes) {
    if (!x) continue;
    b = b ? [Math.min(b[0], x[0]), Math.min(b[1], x[1]), Math.max(b[2], x[2]), Math.max(b[3], x[3])] : [x[0], x[1], x[2], x[3]];
  }
  if (!b) return null;
  const w = b[2] - b[0];
  const h = b[3] - b[1];
  if (!(w > 0 && h > 0)) return null;
  const dx = Math.trunc(w * CLIPBOARD_BBOX_INFLATION);
  const dy = Math.trunc(h * CLIPBOARD_BBOX_INFLATION);
  return [b[0] - dx, b[1] - dy, b[2] + dx, b[3] + dy];
}

/** The bitmap's size: the box at the view's scale, cut down to 4096 pixels on its longer side with the aspect kept (`renderSymbolToImageWithAlpha`). */
export function bitmapSize(box: Box, scalePxPerNm: number): { width: number; height: number; scale: number } {
  let scale = scalePxPerNm;
  let width = Math.round((box[2] - box[0]) * scale);
  let height = Math.round((box[3] - box[1]) * scale);
  if (width > CLIPBOARD_MAX_BITMAP_SIZE || height > CLIPBOARD_MAX_BITMAP_SIZE) {
    const down = CLIPBOARD_MAX_BITMAP_SIZE / Math.max(width, height);
    width = Math.round(width * down);
    height = Math.round(height * down);
    scale *= down;
  }
  return { width, height, scale };
}

