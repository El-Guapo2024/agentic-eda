// Save, Save All, Revert and Save Library As of the two library editors into the libraries of the library tables: the rules of `SYMBOL_EDIT_FRAME::Save`,
// `saveCurrentSymbol`, `saveLibrary`, `saveAllLibraries`, `Revert`, `SaveLibraryAs` (eeschema/symbol_editor/symbol_editor.cpp) and of
// `FOOTPRINT_EDITOR_CONTROL::Save` / `SaveAs` / `Revert`, `FOOTPRINT_EDIT_FRAME::RevertFootprint` and `SaveLibraryAs` (pcbnew/tools/footprint_editor_control.cpp,
// pcbnew/footprint_libraries_utils.cpp, pcbnew/load_select_footprint.cpp), commit 8303b2ad, with KiCad's words. Pure: the flows are
// actions/librarySaveOps.ts, the server's side is crates/cli/src/library_save_api.rs.
//
// The design's copy of a library's symbol or footprint (`Lib:Name`) is the library's buffer; Save writes it into the library's file, Revert puts the file's
// version back. The project library (`eda`) is the design's own, with no file: an edit of it is saved as it is made.
import type { Cmd, LibraryFootprint, LibrarySymbol } from "../api/types";
import { joinLibName, PROJECT_LIBRARY, saveAsSymbolName, splitLibName, symbolLibIdError, symbolNameInUse } from "./libraryNames";
import { PIN_GLYPH } from "./libraryTreeState";

/** `SYMBOL_EDIT_FRAME::saveCurrentSymbol` / `Save` on a library that cannot be written: the warning, and below it the way on (OK is Save Library As). */
export function notWritableMessage(lib: string): string {
  return `Symbol library '${lib}' is not writable.`;
}
export const MUST_SAVE_ELSEWHERE = "You must save to a different location.";

/** `saveSymbolCopyAs`' validator on a library that cannot be written. */
export function readOnlySaveAsMessage(lib: string, symbolName: string): string {
  return `Library '${lib}' is read-only. Choose a different library to save the symbol '${symbolName}' to.`;
}

/** `ConfirmRevertDialog`: "Revert '%s' to last version saved?", "Your current changes will be permanently lost.", Revert / Cancel. */
export function revertQuestion(name: string): string {
  return `Revert '${name}' to last version saved?`;
}
export const REVERT_DETAIL = "Your current changes will be permanently lost.";

/** `saveLibrary( aLibrary, true )`'s file dialog title. */
export function saveLibraryAsTitle(lib: string): string {
  return `Save Library '${lib}' As...`;
}
/** `FOOTPRINT_EDIT_FRAME::SaveLibraryAs` -> `CreateNewLibrary( _( "Save Footprint Library As" ), .. )`. */
export const FOOTPRINT_SAVE_LIBRARY_AS_TITLE = "Save Footprint Library As";

/** `SYMBOL_LIBRARY_SAVE_AS_FILEDLG_HOOK`'s radio buttons the studio offers: the global table is KiCad's (never written here), and a file no table names is not written. */
export const TABLE_OPTIONS = [
  { value: "project", label: "Add new project library table entry" },
  { value: "replace", label: "Update existing library table entry" },
] as const;
export type TableOption = (typeof TABLE_OPTIONS)[number]["value"];

/** The library an item id is in; a bare id is the project library's. */
export function libraryOfId(id: string): string {
  return splitLibName(id).lib || PROJECT_LIBRARY;
}

/** A library of the tables (written to a file) and not the project library (the design's own). */
export function isTableLibrary(lib: string): boolean {
  return lib !== "" && lib !== PROJECT_LIBRARY;
}

/**
 * `SYMBOL_EDIT_FRAME::Save`: with the library tree shown, the library of what it has selected (`GetTreeLIBID().GetUniStringLibNickname()`: a library row, or
 * a symbol's library); with nothing selected there, `saveCurrentSymbol` -- the open symbol's library. `null`: nothing to save.
 */
export function symbolSaveLibrary(treeShown: boolean, selectedLibs: readonly string[], treeSelection: readonly string[], current: string | null): string | null {
  if (treeShown) {
    if (treeSelection.length > 0) return libraryOfId(treeSelection[0]!);
    if (selectedLibs.length > 0) return selectedLibs[0]!;
  }
  return current ? libraryOfId(current) : null;
}

/** `GetTargetLibId()`: what Revert and Save Library As work on -- the tree's selected symbol or library row, else the open symbol. */
export type SymbolTarget = { kind: "symbol"; id: string } | { kind: "library"; lib: string } | { kind: "none" };

export function symbolTarget(treeSelection: readonly string[], selectedLibs: readonly string[], current: string | null): SymbolTarget {
  if (treeSelection.length > 0) return { kind: "symbol", id: treeSelection[0]! };
  if (selectedLibs.length > 0) return { kind: "library", lib: selectedLibs[0]! };
  return current ? { kind: "symbol", id: current } : { kind: "none" };
}

/**
 * `FOOTPRINT_EDITOR_CONTROL::Save`: only when the tree's target is the loaded footprint (`GetTargetFPID() == GetLoadedFPID()`): the tree's selected footprint,
 * a selected library row (which is never the loaded footprint), else the loaded one.
 */
export function footprintSaveTarget(treeSelection: string | null, selectedLibs: readonly string[], loaded: string | null): string | null {
  if (!loaded) return null;
  if (treeSelection !== null) return treeSelection === loaded ? loaded : null;
  return selectedLibs.length > 0 ? null : loaded;
}

/**
 * `RevertSymbol` / `RevertLibrary` as one undoable batch: each design symbol of the target goes back to what the library's file holds (`published` is the
 * server's to keep, `put_library_symbol` overwrite). Reverting a library also drops the symbols its file never had (`m_libs.erase( it )`: the whole buffer
 * goes); a single symbol the file never had has no saved version to go back to.
 */
export function revertCommands(saved: Readonly<Record<string, LibrarySymbol | null>>, wholeLibrary: boolean): Cmd[] {
  const cmds: Cmd[] = [];
  // a derived symbol's parent first (the verb wants the parent in the library before what extends it)
  const ids = Object.keys(saved).sort((a, b) => (saved[a]?.extends ? 1 : 0) - (saved[b]?.extends ? 1 : 0) || a.localeCompare(b));
  for (const id of ids) {
    const sym = saved[id];
    if (sym) cmds.push({ op: "put_library_symbol", symbol: { ...sym, lib_id: id }, overwrite: true });
  }
  if (wholeLibrary) for (const id of ids) if (!saved[id]) cmds.push({ op: "delete_library_symbol", lib_id: id });
  return cmds;
}

/** `RevertFootprint`'s restore of the loaded footprint from the library's file. */
export function revertFootprintCommand(id: string, saved: LibraryFootprint): Cmd {
  return { op: "put_library_footprint", footprint: { ...saved, name: id }, overwrite: true };
}

/**
 * `saveAllLibraries( false )`: every modified library is saved; one that cannot be written is, when it is the only modified one, the OK/Cancel question that
 * leads to Save Library As, and among several only a message (`m_infoBar->ShowMessageFor( msg + "  " + msg2 )`) and it is skipped.
 */
export function saveAllPlan(modifiedLibraries: readonly string[], writable: ReadonlyMap<string, boolean>): { save: string[]; readOnly: string[]; askAboutReadOnly: boolean } {
  const libs = modifiedLibraries.filter(isTableLibrary);
  const readOnly = libs.filter((l) => writable.get(l) === false);
  return { save: libs.filter((l) => writable.get(l) !== false), readOnly, askAboutReadOnly: libs.length === 1 && readOnly.length === 1 };
}

/** One row of the Save As dialogs' library list (`GetLibraryItemsForListDialog`: Library, Description; the pinned libraries first, with the pin glyph). */
export interface LibraryRow {
  name: string;
  description: string;
  writable: boolean;
  pinned: boolean;
}

/**
 * `GetLibraryItemsForListDialog`: every library of the tables (KiCad lists them all and its validator refuses a read-only one), the pinned ones first, each part in the
 * tables' order; the project library (the design's own) and the libraries the dialog made (`extra`) with them. Hidden libraries are left out.
 */
export function saveAsLibraryRows(libs: readonly { name: string; description: string; writable: boolean; hidden?: boolean }[], pinned: ReadonlySet<string>, extra: readonly string[] = []): LibraryRow[] {
  const rows: LibraryRow[] = [{ name: PROJECT_LIBRARY, description: "The project's own library (in design.json)", writable: true, pinned: pinned.has(PROJECT_LIBRARY) }];
  for (const l of libs) if (!l.hidden && l.name !== PROJECT_LIBRARY) rows.push({ name: l.name, description: l.description, writable: l.writable, pinned: pinned.has(l.name) });
  for (const name of extra) if (!rows.some((r) => r.name === name)) rows.push({ name, description: "", writable: true, pinned: pinned.has(name) });
  return [...rows.filter((r) => r.pinned), ...rows.filter((r) => !r.pinned)];
}

/** A row's label in the list: the pin glyph for a pinned library. */
export function libraryRowLabel(row: LibraryRow): string {
  return row.pinned ? `${PIN_GLYPH}${row.name}` : row.name;
}

/**
 * The `lib_id`s a library has, for Save As's name checks (`SymbolNameInUse`, `FootprintExists`): the design's own entries of the library and the items its file
 * holds (`items`: bare names).
 */
export function takenInLibrary(lib: string, items: readonly string[], designIds: readonly string[]): Set<string> {
  const taken = new Set(designIds);
  if (isTableLibrary(lib)) for (const item of items) taken.add(`${lib}:${item}`);
  return taken;
}

/** `FootprintExists( newLib, newName )`: the design's footprint ids (`Lib:Name`, the project library's bare) and the library file's footprints. */
export function footprintExists(lib: string, name: string, items: readonly string[], designIds: readonly string[]): boolean {
  const id = lib === PROJECT_LIBRARY ? name : `${lib}:${name}`;
  return designIds.includes(id) || (isTableLibrary(lib) && items.includes(name));
}

/** `SymbolNameInUse` over a library's names (any case). */
export function nameInLibrary(lib: string, name: string, taken: Iterable<string>): boolean {
  return symbolNameInUse(lib === PROJECT_LIBRARY ? `${PROJECT_LIBRARY}:${name}` : `${lib}:${name}`, taken);
}

/** `SYMBOL_TREE_SYNCHRONIZING_ADAPTER::GetValue`: "mark modified items with an asterisk". */
export function modifiedLabel(label: string, modified: boolean): string {
  return modified ? `${label} *` : label;
}

/** The default name of Save Library As's file: the library's nickname (`fn.SetName( aLibrary ); fn.SetExt( .kicad_sym )`), a footprint library's `.pretty`. */
export function saveLibraryAsDefaultName(kind: "symbol" | "footprint", lib: string): string {
  return kind === "symbol" ? `${lib}.kicad_sym` : `${lib}.pretty`;
}

/**
 * `saveSymbolCopyAs`' validator up to the inheritance checks, in its order: "A library must be specified.", "Symbol must have a name." (the name trimmed, blanks to `_`,
 * `getSymbolName`), a name the library id cannot hold, a library that cannot be written. The answer is the error, or the name and the copy's `lib_id`.
 */
export function symbolSaveAsCheck(lib: string, typed: string, rows: readonly Pick<LibraryRow, "name" | "writable">[]): { error: string } | { name: string; libId: string } {
  if (lib === "") return { error: "A library must be specified." };
  const name = saveAsSymbolName(typed);
  if (name === "") return { error: "Symbol must have a name." };
  const libId = joinLibName(lib, name);
  const illegal = symbolLibIdError(libId);
  if (illegal) return { error: illegal };
  if (rows.find((r) => r.name === lib)?.writable === false) return { error: readOnlySaveAsMessage(lib, name) };
  return { name, libId };
}

// ------------------------------------------------------------------------------------------------ Delete, Rename, New Footprint (UI-LEFT-2 LL1, LL2)

/** `SYMBOL_EDITOR_CONTROL::CutCopyDelete` on a library the tables lack (`ShowInfoBarError`). */
export function symbolLibraryNotFound(lib: string): string {
  return `Symbol library '${lib}' not found.`;
}

/** `SYMBOL_EDIT_FRAME::DeleteSymbolFromLibrary` of a modified symbol (`IsOK`, Confirmation, Yes / No). */
export function deleteModifiedQuestion(name: string): string {
  return `The symbol '${name}' has been modified.\nDo you want to remove it from the library?`;
}

/** `DeleteSymbolFromLibrary` of a base symbol: "Deleting a base symbol will delete all symbols derived from it.\n\n", the base " (base)" and each derived symbol a line. */
export function deleteBaseMessage(base: string, derived: readonly string[]): string {
  return `Deleting a base symbol will delete all symbols derived from it.\n\n${base} (base)\n${derived.map((d) => `${d}\n`).join("")}`;
}

/** `FOOTPRINT_EDIT_FRAME::DeleteFootprintFromLibrary`'s question (`IsOK`). */
export function deleteFootprintQuestion(name: string, lib: string): string {
  return `Delete footprint '${name}' from library '${lib}'?`;
}

/** Its status text once the footprint is deleted (`SetStatusText`). */
export function footprintDeletedStatus(name: string, lib: string): string {
  return `Footprint '${name}' deleted from library '${lib}'`;
}

/** `DeleteFootprintFromLibrary` of a library that cannot be written (`ShowInfoBarError`), and `FootprintSave`'s refusal (with the folder's path). */
export function footprintLibraryReadOnly(lib: string): string {
  return `Library '${lib}' is read only.`;
}

/** `FOOTPRINT_EDITOR_CONTROL::tryToSaveFootprintInLibrary` on a library that cannot be written (`ShowInfoBarWarning`): the footprint is in no library. */
export function newFootprintReadOnlyWarning(lib: string): string {
  return `The footprint could not be added to the selected library ('${lib}'). This library is read-only.`;
}

/** `FOOTPRINT_EDIT_FRAME::AddFootprintToBoard`'s info bar for a footprint of a library that cannot be written, and its link. */
export function readOnlyFootprintBanner(lib: string): string {
  return `Editing footprint from read-only library ${lib}.`;
}
export const SAVE_AS_EDITABLE_COPY = "Save as editable copy";

/** `PCB_BASE_FRAME::CreateNewFootprint`'s name made unique in the library (`FootprintExists( aLibName, aFootprintName )`): `Untitled`, `Untitled_1`, ... */
export function uniqueNewFootprintName(taken: (name: string) => boolean, base = "Untitled"): string {
  let name = base;
  for (let idx = 1; taken(name); idx++) name = `${base}_${idx}`;
  return name;
}

/**
 * `FOOTPRINT_EDIT_FRAME::GetTargetFPID()` for New Footprint: the library of the tree's selected footprint, else of its selected library row, else of the loaded
 * footprint; "" when there is none (KiCad's footprint "will need to be saved manually to assign to a library": here, the project library's).
 */
export function newFootprintLibrary(treeSelection: string | null, selectedLibs: readonly string[], loaded: string | null): string {
  if (treeSelection) return splitLibName(treeSelection).lib;
  if (selectedLibs.length > 0) return selectedLibs[0]!;
  return loaded ? splitLibName(loaded).lib : "";
}
