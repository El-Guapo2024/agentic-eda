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
