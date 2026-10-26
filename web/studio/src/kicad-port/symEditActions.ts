// Pure parts of the Symbol Editor's hotkey actions (`eeschema.SymbolLibraryControl.newSymbol`,
// Ctrl+N; `eeschema.SymbolLibraryControl.saveLibraryAs`, Ctrl+Shift+S).
//
// `SYMBOL_EDIT_FRAME::CreateNewSymbol` (symbol_editor/symbol_editor.cpp) opens `DIALOG_LIB_NEW_SYMBOL`
// for a name and refuses one the library already has ("Symbol name already exists"). The
// project's own library is the only destination this studio has, so the studio's `NewSymbolDialog`
// asks for the name -- `GetName`'s form (trimmed, spaces to underscores, LIB_ID specials escaped)
// -- and `newSymbolNameCheck` is its validator plus the names the library has. The dialog starts
// on `uniqueSymbolLibId`'s free name ("Untitled", then "Untitled_1", "Untitled_2"... until unused,
// the same scheme `fpEditActions.ts:uniqueFootprintName` uses), so Ctrl+N + OK still makes one.
import { joinLibName, saveAsSymbolName, symbolLibIdError } from "./libraryNames";

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

/** `DIALOG_LIB_NEW_SYMBOL::GetName`: the typed name trimmed, its spaces underscores, its `LIB_ID` specials escaped. */
export function newSymbolName(typed: string): string {
  return saveAsSymbolName(typed);
}

export interface NewSymbolNameCheck {
  /** The symbol's name in the library (`GetName`). */
  name: string;
  /** Its full lib_id in `lib`. */
  libId: string;
  /** The validator's refusal, or null: `TransferDataFromWindow` runs `m_validator( GetName() )`, which says "Symbol must have a name." for an empty one. */
  error: string | null;
  /** The library already has the name (`SymbolNameInUse`); KiCad asks "Symbol '%s' already exists in library '%s'." with an Overwrite button. */
  exists: boolean;
}

/** `DIALOG_LIB_NEW_SYMBOL`'s name validator against the names the libraries already hold (`existingLibIds`, whole lib_ids). */
export function newSymbolNameCheck(typed: string, existingLibIds: readonly string[], lib = PROJECT_SYMBOL_LIB): NewSymbolNameCheck {
  const name = newSymbolName(typed);
  const libId = joinLibName(lib, name);
  const error = symbolLibIdError(libId);
  return { name, libId, error, exists: error === null && existingLibIds.includes(libId) };
}
