// What the Symbol Editor's library actions (`eeschema.SymbolLibraryControl.*`, `SYMBOL_EDITOR_CONTROL`) do, as plain async
// functions over the editor's store: each cites the KiCad function it ports (eeschema/tools/symbol_editor_control.cpp,
// eeschema/symbol_editor/symbol_editor.cpp, symbol_editor_import_export.cpp). The action registry
// (`libraryEditorActions.ts`) and the library tree's context menu call them; every edit is an `/api/cmd` verb with undo.
//
// The studio's library is the project library (`design.symbol_library`, entries keyed by `Lib:Name`); a symbol the tree shows that
// only a library file or the builtin table defines is read-only until it is opened (which makes an editable copy). Copy, Export,
// Duplicate and Save Copy As read any symbol; Delete and Rename need a project-library entry.
import type { Dispatch } from "react";
import type { Cmd, LibrarySymbol } from "../api/types";
import { fetchLibrarySymbolFlat, fetchSymbolEditorNames } from "../api/client";
import { fetchAnySymbol, fetchSymbolKicadSymText, parseSymbolText, pickTextFile, readClipboardText, saveCanvasPng, saveTextFile, writeClipboardText } from "../api/libraryClient";
import { ensureUniqueLibId, joinLibName, PROJECT_LIBRARY, splitLibName, symbolLibIdError } from "../kicad-port/libraryNames";
import { PROJECT_SYMBOL_LIB } from "../kicad-port/symEditActions";
import { clipboardTextForSymbols, looksLikeSymbolText } from "../kicad-port/symClipboard";
import { bareName, descendantsOf, treeConditions } from "../kicad-port/symFields";
import { planImport, planPaste } from "../kicad-port/symImportPlan";
import type { ConflictStrategy } from "../kicad-port/symSaveAsPlan";
import { askChoice, askConfirm, askText } from "../components/library/libraryDialogs";
import { notifyLibraryChanged } from "../components/library/useLibraryNames";
import type { SymAction, SymbolEditorApi } from "../state/symbolEditorStore";

export interface SymCtx {
  api: SymbolEditorApi;
  dispatch: Dispatch<SymAction>;
}

const info = (c: SymCtx, message: string) => c.dispatch({ type: "TOAST", message, kind: "info" });
const fail = (c: SymCtx, e: unknown) => c.dispatch({ type: "TOAST", message: e instanceof Error ? e.message : String(e), kind: "error" });

/** `GetTargetLibId` / `GetSelectedLibIds`: what the library tree has selected, else the open symbol. */
export function targetIds(c: SymCtx): string[] {
  const st = c.api.getState();
  return st.treeSelection.length > 0 ? st.treeSelection : st.libId ? [st.libId] : [];
}

async function names(): Promise<{ all: string[]; project: Set<string>; derived: Record<string, string> }> {
  const r = await fetchSymbolEditorNames();
  return { all: r.names, project: new Set(r.project ?? []), derived: r.derived ?? {} };
}

/** The library a new symbol lands in: the target's own (`getTargetLib()`), else the project library. */
export function targetLibrary(c: SymCtx): string {
  return splitLibName(targetIds(c)[0] ?? "").lib || PROJECT_SYMBOL_LIB;
}

async function runBatch(c: SymCtx, cmds: Cmd[]): Promise<boolean> {
  const ok = await c.api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
  notifyLibraryChanged();
  return ok;
}

function select(c: SymCtx, ids: string[]) {
  c.dispatch({ type: "SET_TREE_SELECTION", names: ids });
}

/** `SYMBOL_EDITOR_CONTROL::EditSymbol`: show the tree's selected symbol on the editor canvas. */
export async function editSymbol(c: SymCtx): Promise<void> {
  const id = targetIds(c)[0];
  if (!id) return info(c, "Select a symbol in the library tree first.");
  try {
    await c.api.openSymbol(id);
    notifyLibraryChanged();
  } catch (e) {
    fail(c, e);
  }
}

/**
 * `SCH_ACTIONS::symbolProperties` from the tree's context menu (`symbolSelectedCondition`): the Symbol Properties dialog is about the symbol on
 * the canvas, so a target that is not the open symbol is opened first.
 */
export async function symbolProperties(c: SymCtx): Promise<void> {
  const target = targetIds(c)[0];
  if (!target) return info(c, "Select a symbol in the library tree first.");
  try {
    if (c.api.getState().libId !== target) {
      await c.api.openSymbol(target);
      notifyLibraryChanged();
    }
    c.dispatch({ type: "SET_PROPERTIES_OPEN", open: true });
  } catch (e) {
    fail(c, e);
  }
}

/** `SYMBOL_EDIT_FRAME::CopySymbolToClipboard` (and, with `cut`, the delete that `CutCopyDelete` follows it with). */
export async function copySymbols(c: SymCtx, cut: boolean): Promise<void> {
  const ids = targetIds(c);
  if (ids.length === 0) return info(c, "Select a symbol in the library tree first.");
  try {
    const symbols: LibrarySymbol[] = [];
    const texts: string[] = [];
    const { derived } = await names();
    for (const id of ids) {
      // `CopySymbolToClipboard` copies `symbol->Flatten()`: a derived symbol goes as the flat symbol it stands for
      symbols.push(id in derived ? await fetchLibrarySymbolFlat(id) : (await fetchAnySymbol(id)).symbol);
      texts.push(await fetchSymbolKicadSymText(id));
    }
    const text = clipboardTextForSymbols(texts);
    const wrote = await writeClipboardText(text);
    c.dispatch({ type: "SET_COPIED", copied: { text, symbols } });
    info(c, `${ids.length === 1 ? ids[0] : `${ids.length} symbols`} ${cut ? "cut" : "copied"}${wrote ? "" : " (the browser would not share the clipboard; Paste Symbol still works here)"}`);
  } catch (e) {
    return fail(c, e);
  }
  if (cut) await deleteSymbols(c);
}

/** `SYMBOL_EDIT_FRAME::DuplicateSymbol( true )`: the clipboard's symbols are added to the target library, each under a free name, and the first is opened. */
export async function pasteSymbols(c: SymCtx): Promise<void> {
  try {
    const st = c.api.getState();
    const text = await readClipboardText();
    let symbols: LibrarySymbol[];
    if (st.copied && (text === null || text === st.copied.text)) {
      symbols = st.copied.symbols; // our own copy: no round trip, nothing lost
    } else if (text && looksLikeSymbolText(text)) {
      const parsed = await parseSymbolText(text);
      symbols = parsed.symbols;
      if (parsed.warnings.length) info(c, parsed.warnings.join("; "));
    } else {
      return info(c, "The clipboard does not hold a symbol.");
    }
    if (symbols.length === 0) return;
    const lib = targetLibrary(c);
    const { all } = await names();
    // each symbol under a free name (`ensureUniqueName`); a derived one (a file's clipboard text can hold them) follows its parent's name and comes after it
    const { puts, ids } = planPaste(symbols, lib, all, ensureUniqueLibId, joinLibName);
    const cmds = puts.map((sym): Cmd => ({ op: "put_library_symbol", symbol: { ...sym, published: false } }));
    if (!(await runBatch(c, cmds))) return;
    await c.api.openSymbol(ids[0]!);
    select(c, [ids[0]!]);
  } catch (e) {
    fail(c, e);
  }
}

/** `SYMBOL_EDIT_FRAME::DuplicateSymbol( false )`: a copy of the target under the next free name (`name_1`, ...), opened. */
export async function duplicateSymbol(c: SymCtx): Promise<void> {
  const id = targetIds(c)[0];
  if (!id) return info(c, "No symbol selected.");
  try {
    const { symbol } = await fetchAnySymbol(id);
    const { all } = await names();
    const libId = ensureUniqueLibId(id, all);
    if (!(await runBatch(c, [{ op: "put_library_symbol", symbol: { ...symbol, lib_id: libId, published: false } }]))) return;
    await c.api.openSymbol(libId);
    select(c, [libId]);
  } catch (e) {
    fail(c, e);
  }
}

/** `SYMBOL_EDIT_FRAME::DeleteSymbolFromLibrary`: every selected symbol is removed from the library; the open one leaves the canvas. */
export async function deleteSymbols(c: SymCtx): Promise<void> {
  const ids = targetIds(c);
  if (ids.length === 0) return info(c, "No symbol selected.");
  try {
    const { project, derived } = await names();
    const inProject = ids.filter((id) => project.has(id));
    const refused = ids.filter((id) => !project.has(id));
    if (refused.length > 0) fail(c, new Error(`${refused.join(", ")} ${refused.length === 1 ? "is" : "are"} not in the project library (defined by a library file or the built-in table), so there is nothing to delete.`));
    // "Deleting a base symbol will delete all symbols derived from it": asked for each base symbol, and the Cancel button leaves that one alone
    const deletable: string[] = [];
    for (const id of inProject) {
      const kids = descendantsOf(derived, id);
      if (kids.length > 0) {
        const message = `Deleting a base symbol will delete all symbols derived from it.\n\n${bareName(id)} (base)\n${kids.map(bareName).join("\n")}\n`;
        if (!(await askConfirm({ title: "Warning", message, okLabel: "Delete All Listed Symbols", cancelLabel: "Cancel" }))) continue;
      }
      deletable.push(id);
    }
    if (deletable.length === 0) return;
    // a symbol that goes with its base symbol is not deleted a second time
    const alone = deletable.filter((id) => !deletable.some((other) => other !== id && descendantsOf(derived, other).includes(id)));
    const open = c.api.getState().libId;
    const cmds: Cmd[] = alone.map((lib_id): Cmd => ({ op: "delete_library_symbol", lib_id }));
    if (!(await runBatch(c, cmds))) return;
    // the symbol on the canvas leaves it when it, or a symbol it derives from, was deleted (`GetParentChain( *GetCurSymbol() )` holds the deleted id)
    if (open && alone.some((id) => id === open || descendantsOf(derived, id).includes(open))) c.api.closeSymbol();
    select(c, []);
  } catch (e) {
    fail(c, e);
  }
}

/** `SYMBOL_EDITOR_CONTROL::RenameSymbol`: ask for a new name (an existing one is replaced only after an "Overwrite" confirmation). */
export async function renameSymbol(c: SymCtx): Promise<void> {
  const id = targetIds(c)[0];
  if (!id) return info(c, "Select a symbol in the library tree first.");
  try {
    const { all, project } = await names();
    if (!project.has(id)) return fail(c, new Error(`${id} is not in the project library (it comes from a library file or the built-in table); save a copy of it under a new name instead.`));
    const { lib, item } = splitLibName(id);
    const entered = await askText({
      title: "Change Symbol Name",
      label: "New name:",
      initial: item,
      okLabel: "Rename",
      validate: (v) => (v.trim() === "" ? "Symbol must have a name." : symbolLibIdError(joinLibName(lib, v))),
    });
    if (entered === null) return;
    const newId = joinLibName(lib, entered);
    if (newId === id) return;
    let overwrite = false;
    if (all.includes(newId)) {
      if (!(await askConfirm({ title: "Confirmation", message: `Symbol '${entered}' already exists in library '${lib || PROJECT_LIBRARY}'.`, okLabel: "Overwrite" }))) return;
      overwrite = true;
    }
    const wasOpen = c.api.getState().libId === id;
    if (!(await runBatch(c, [{ op: "rename_library_symbol", lib_id: id, new_lib_id: newId, overwrite }]))) return;
    if (wasOpen) await c.api.openSymbol(newId);
    select(c, [newId]);
  } catch (e) {
    fail(c, e);
  }
}

/** `SYMBOL_EDIT_FRAME::ExportSymbol`: the symbol as a `.kicad_sym` file, named after it in lower case. */
export async function exportSymbol(c: SymCtx): Promise<void> {
  const id = targetIds(c)[0];
  if (!id) return info(c, "There is no symbol selected to save.");
  try {
    saveTextFile(await fetchSymbolKicadSymText(id), `${splitLibName(id).item.toLowerCase()}.kicad_sym`);
  } catch (e) {
    fail(c, e);
  }
}

/** `SYMBOL_EDITOR_CONTROL::ExportView`: the canvas as a PNG, `<symbol name>.png`. */
export async function exportSymbolView(c: SymCtx): Promise<void> {
  const st = c.api.getState();
  if (!st.libId) return info(c, "No symbol to export.");
  const canvas = document.querySelector<HTMLCanvasElement>(".pcb-canvas-container canvas");
  if (!canvas) return info(c, "No symbol to export.");
  if (!(await saveCanvasPng(canvas, `${splitLibName(st.libId).item}.png`))) fail(c, new Error("Can't save the picture."));
}

/** `SYMBOL_EDIT_FRAME::ImportSymbol`: choose a `.kicad_sym`, read it, and let the person pick the symbols to bring in. */
export async function importSymbol(c: SymCtx): Promise<void> {
  try {
    const file = await pickTextFile(".kicad_sym,.sym");
    if (!file) return;
    const parsed = await parseSymbolText(file.text);
    c.dispatch({ type: "SET_IMPORT_REQUEST", request: { fileName: file.name, symbols: parsed.symbols, warnings: parsed.warnings, lib: targetLibrary(c) } });
  } catch (e) {
    fail(c, e);
  }
}

/**
 * `ImportSymbol`'s loop over the selected symbols, after the select dialog: each is stored in the library under its own name; a symbol
 * the library already has is skipped or overwritten as the dialog's resolution says (`CONFLICT_RESOLUTION::SKIP` / `OVERWRITE`). The first
 * imported symbol is loaded, and a skipped count is reported ("Imported %d symbol(s), skipped %d.").
 */
export async function performImport(c: SymCtx, lib: string, chosen: LibrarySymbol[], resolutions: Record<string, "skip" | "overwrite">, file: LibrarySymbol[] = chosen): Promise<void> {
  try {
    const { all } = await names();
    const taken = new Set(all);
    const wanted: LibrarySymbol[] = [];
    let skipped = 0;
    for (const sym of chosen) {
      const libId = joinLibName(lib, splitLibName(sym.lib_id).item);
      if (taken.has(libId) && (resolutions[sym.lib_id] ?? "overwrite") === "skip") {
        skipped++;
        continue;
      }
      wanted.push(sym);
    }
    if (wanted.length === 0) return info(c, skipped > 0 ? `Skipped ${skipped} symbol(s).` : "Nothing to import.");
    // a derived symbol comes after its parent, and brings the parent along when the library lacks it and it was not ticked
    const present = new Set(all.filter((n) => (splitLibName(n).lib || PROJECT_LIBRARY) === lib).map((n) => splitLibName(n).item));
    const cmds: Cmd[] = planImport(file, wanted, present).map((sym): Cmd => {
      const libId = joinLibName(lib, splitLibName(sym.lib_id).item);
      return { op: "put_library_symbol", symbol: { ...sym, lib_id: libId, published: false }, overwrite: taken.has(libId) };
    });
    const first = joinLibName(lib, splitLibName(wanted[0]!.lib_id).item);
    if (!(await runBatch(c, cmds))) return;
    await c.api.openSymbol(first);
    select(c, [first]);
    info(c, skipped > 0 ? `Imported ${wanted.length} symbol(s), skipped ${skipped}.` : `Imported ${wanted.length} symbol(s).`);
  } catch (e) {
    fail(c, e);
  }
}

/** `SYMBOL_EDIT_FRAME::saveSymbolCopyAs`: open the Save As dialog for the target symbol (`openCopy` = Save As, which loads the copy afterwards). */
export function saveSymbolAs(c: SymCtx, openCopy: boolean): void {
  const id = targetIds(c)[0];
  if (!id) return info(c, "There is no symbol selected to save.");
  c.dispatch({ type: "SET_SAVE_AS", request: { openCopy, libId: id } });
}

/** What the Save As dialog's validator asks about: the symbol and its parents (own entries, the symbol first), the symbol with everything it inherits, the names in use. */
export async function saveAsContext(c: SymCtx, libId: string): Promise<{ chain: LibrarySymbol[]; flat: LibrarySymbol; taken: Set<string>; derived: Record<string, string> }> {
  const { all, derived } = await names();
  const ids = [libId];
  for (let at = derived[libId]; at && ids.length < 64 && !ids.includes(at); at = derived[at]) ids.push(at);
  const chain: LibrarySymbol[] = [];
  for (const id of ids) chain.push((await fetchAnySymbol(id)).symbol);
  void c;
  return { chain, flat: libId in derived ? await fetchLibrarySymbolFlat(libId) : chain[0]!, taken: new Set(all), derived };
}

/**
 * The dialog's OK: `SYMBOL_SAVE_AS_HANDLER::DoSave` -- the symbols of `savePlan` (the copy, and with it its parents when it goes to another library) are stored; a name the library
 * already has is overwritten only after the caller's confirmation (`ID_OVERWRITE_CONFLICTS`). `openCopy` loads the copy afterwards.
 */
export async function performSaveAs(c: SymCtx, req: { libId: string; openCopy: boolean }, puts: LibrarySymbol[], taken: ReadonlySet<string>): Promise<boolean> {
  try {
    const cmds = puts.map((symbol): Cmd => ({ op: "put_library_symbol", symbol, overwrite: taken.has(symbol.lib_id) || symbol.lib_id === req.libId }));
    if (!(await runBatch(c, cmds))) return false;
    const copy = puts[puts.length - 1]!.lib_id; // the root comes first and the symbol itself last
    if (req.openCopy) await c.api.openSymbol(copy);
    select(c, [copy]);
    return true;
  } catch (e) {
    fail(c, e);
    return false;
  }
}

/** The wording of the Save As validator's conflict question, for the dialog. */
export async function askSaveAsConflicts(lib: string, symbolName: string, typed: string, conflicts: string[]): Promise<ConflictStrategy | null> {
  if (conflicts.length === 1 && conflicts[0] === typed) {
    return (await askConfirm({ title: "Confirmation", message: `Symbol '${typed}' already exists in library '${lib}'. Do you want to overwrite it?`, okLabel: "Overwrite" })) ? "overwrite" : null;
  }
  // "If there are conflicts in the parental chain, we need to ask the user if they want to overwrite all of them"
  const message = `The following symbols in the inheritance chain of '${symbolName}' already exist in library '${lib}':\n${conflicts.map((n) => `  ${n}\n`).join("")}\nDo you want to overwrite all of them, or rename the new symbols?`;
  const choice = await askChoice({ title: "Confirmation", message, okLabel: "Overwrite All", altLabel: "Rename All", cancelLabel: "Cancel" });
  return choice === "ok" ? "overwrite" : choice === "alt" ? "rename" : null;
}

/** `SYMBOL_EDITOR_CONTROL::AddSymbol` for `deriveFromExistingSymbol`: the Derive from Existing Symbol dialog on the tree's selected symbol (`CreateNewSymbol( target.GetLibItemName() )`). */
export function deriveFromExisting(c: SymCtx): void {
  const id = targetIds(c)[0];
  if (!id) return info(c, "Select a symbol in the library tree first.");
  c.dispatch({ type: "SET_DERIVE_REQUEST", parent: id });
}

/** `SYMBOL_EDITOR_CONTROL::FlattenSymbol`: the tree's selected symbol becomes the flat symbol it stands for (`UpdateSymbol( libMgr, Flatten() )`). */
export async function flattenSymbol(c: SymCtx): Promise<void> {
  const id = targetIds(c)[0];
  if (!id) return info(c, "No symbol to flatten");
  try {
    const { derived } = await names();
    if (!treeConditions(derived, id).isDerived) return fail(c, new Error("Symbol is not derived from another symbol."));
    await runBatch(c, [{ op: "flatten_symbol", lib_id: id }]);
  } catch (e) {
    fail(c, e);
  }
}

/** `SYMBOL_EDITOR_EDIT_TOOL::UpdateSymbolFields`: the dialog for the open symbol -- nothing without one, an error when it is not derived. */
export function updateSymbolFields(c: SymCtx): void {
  const st = c.api.getState();
  if (!st.symbol) return;
  if (!st.derivedOwn) return fail(c, new Error("Symbol is not derived from another symbol."));
  c.dispatch({ type: "SET_UPDATE_FIELDS_OPEN", open: true });
}

/** `SYMBOL_EDITOR_CONTROL::ShowLibraryTable` for `showRelatedLibFieldsTable`: the fields table with the scope "Related Symbols" (the target's root and everything derived from it). */
export function showFieldsTable(c: SymCtx, scope: "library" | "related"): void {
  c.dispatch({ type: "SET_FIELDS_TABLE_OPEN", open: true, scope });
}

/**
 * `SYMBOL_EDITOR_CONTROL::AddSymbolToSchematic`: the open symbol, at the unit being edited, becomes the schematic's symbol being placed.
 * A symbol the schematic cannot see yet (a project-library entry nothing follows) is published first -- `Update Symbol on Board` -- since a
 * placed instance resolves its drawing from the library entry by name.
 */
export async function symbolForSchematic(c: SymCtx): Promise<{ libId: string; referencePrefix: string; unit: number } | null> {
  const st = c.api.getState();
  const sym = st.symbol;
  if (!st.libId || !sym) {
    info(c, "Open a symbol first.");
    return null;
  }
  if (!sym.published) {
    if (!(await c.api.cmd({ op: "update_symbol_on_board", lib_id: st.libId }))) return null;
    info(c, `${st.libId}: placed instances now follow this library entry`);
  }
  return { libId: st.libId, referencePrefix: sym.reference_prefix, unit: Math.max(1, st.activeUnit) };
}
