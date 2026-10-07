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
import { fetchSymbolEditorNames } from "../api/client";
import { fetchAnySymbol, fetchSymbolKicadSymText, parseSymbolText, pickTextFile, readClipboardText, saveCanvasPng, saveTextFile, writeClipboardText } from "../api/libraryClient";
import { ensureUniqueLibId, joinLibName, PROJECT_LIBRARY, splitLibName, symbolLibIdError } from "../kicad-port/libraryNames";
import { PROJECT_SYMBOL_LIB } from "../kicad-port/symEditActions";
import { clipboardTextForSymbols, looksLikeSymbolText } from "../kicad-port/symClipboard";
import { askConfirm, askText } from "../components/library/libraryDialogs";
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

async function names(): Promise<{ all: string[]; project: Set<string> }> {
  const r = await fetchSymbolEditorNames();
  return { all: r.names, project: new Set(r.project ?? []) };
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
    for (const id of ids) {
      symbols.push((await fetchAnySymbol(id)).symbol);
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
    const taken = [...all];
    const cmds: Cmd[] = [];
    const created: string[] = [];
    for (const sym of symbols) {
      const item = splitLibName(sym.lib_id).item;
      const libId = ensureUniqueLibId(joinLibName(lib, item), taken); // `ensureUniqueName`
      taken.push(libId);
      created.push(libId);
      cmds.push({ op: "put_library_symbol", symbol: { ...sym, lib_id: libId, published: false } });
    }
    if (!(await runBatch(c, cmds))) return;
    await c.api.openSymbol(created[0]!);
    select(c, [created[0]!]);
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
    const { project } = await names();
    const deletable = ids.filter((id) => project.has(id));
    const refused = ids.filter((id) => !project.has(id));
    if (refused.length > 0) fail(c, new Error(`${refused.join(", ")} ${refused.length === 1 ? "is" : "are"} not in the project library (defined by a library file or the built-in table), so there is nothing to delete.`));
    if (deletable.length === 0) return;
    const open = c.api.getState().libId;
    const cmds: Cmd[] = deletable.map((lib_id): Cmd => ({ op: "delete_library_symbol", lib_id }));
    if (!(await runBatch(c, cmds))) return;
    if (open && deletable.includes(open)) c.api.closeSymbol();
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
export async function performImport(c: SymCtx, lib: string, chosen: LibrarySymbol[], resolutions: Record<string, "skip" | "overwrite">): Promise<void> {
  try {
    const { all } = await names();
    const taken = new Set(all);
    const cmds: Cmd[] = [];
    const imported: string[] = [];
    let skipped = 0;
    for (const sym of chosen) {
      const libId = joinLibName(lib, splitLibName(sym.lib_id).item);
      const exists = taken.has(libId);
      if (exists && (resolutions[sym.lib_id] ?? "overwrite") === "skip") {
        skipped++;
        continue;
      }
      cmds.push({ op: "put_library_symbol", symbol: { ...sym, lib_id: libId, published: false }, overwrite: exists });
      imported.push(libId);
    }
    if (cmds.length === 0) return info(c, skipped > 0 ? `Skipped ${skipped} symbol(s).` : "Nothing to import.");
    if (!(await runBatch(c, cmds))) return;
    await c.api.openSymbol(imported[0]!);
    select(c, [imported[0]!]);
    info(c, skipped > 0 ? `Imported ${imported.length} symbol(s), skipped ${skipped}.` : `Imported ${imported.length} symbol(s).`);
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

/**
 * The dialog's OK: `SYMBOL_SAVE_AS_HANDLER::DoSave` -- the symbol is stored under `newLibId`; a name the library already has is overwritten
 * only after the caller's confirmation (`ID_OVERWRITE_CONFLICTS`). `openCopy` loads the copy afterwards.
 */
export async function performSaveAs(c: SymCtx, req: { libId: string; openCopy: boolean }, newLibId: string, overwrite: boolean): Promise<boolean> {
  try {
    const { symbol } = await fetchAnySymbol(req.libId);
    if (!(await runBatch(c, [{ op: "put_library_symbol", symbol: { ...symbol, lib_id: newLibId, published: false }, overwrite }]))) return false;
    if (req.openCopy) await c.api.openSymbol(newLibId);
    select(c, [newLibId]);
    return true;
  } catch (e) {
    fail(c, e);
    return false;
  }
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
