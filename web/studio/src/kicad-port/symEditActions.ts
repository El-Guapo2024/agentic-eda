// Pure parts of the Symbol Editor's hotkey actions (`eeschema.SymbolLibraryControl.newSymbol`,
// Ctrl+N; `eeschema.SymbolLibraryControl.saveLibraryAs`, Ctrl+Shift+S).
//
// `SYMBOL_EDIT_FRAME::CreateNewSymbol` (symbol_editor/symbol_editor.cpp) opens `DIALOG_LIB_NEW_SYMBOL`
// for a name and refuses one the library already has ("Symbol name already exists"). The
// project's own library is the only destination this studio has, so the default name stands in
// for the dialog: "Untitled", then "Untitled_1", "Untitled_2"... until unused -- the same
// scheme `fpEditActions.ts:uniqueFootprintName` uses for a new footprint.

/** The project library's lib_id prefix (`design.symbol_library` entries are "eda:NAME"). */
export const PROJECT_SYMBOL_LIB = "eda";

/** The lib_id of a new symbol: "eda:Untitled", made unique against every lib_id already known (any library). */
export function uniqueSymbolLibId(existingLibIds: readonly string[], base = "Untitled", lib = PROJECT_SYMBOL_LIB): string {
  const taken = new Set(existingLibIds);
  if (!taken.has(`${lib}:${base}`)) return `${lib}:${base}`;
  for (let i = 1; ; i++) {
    const candidate = `${lib}:${base}_${i}`;
    if (!taken.has(candidate)) return candidate;
  }
}
