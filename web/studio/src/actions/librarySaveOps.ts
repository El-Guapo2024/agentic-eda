// Save, Save All, Revert and Save Library As of the Symbol Editor and the Footprint Editor into the libraries of the library tables, as plain async functions over
// the editors' stores (the action registry, `editorFrameActions.ts`, and the trees' context menus call them). The rules are kicad-port/librarySave.ts; the writes are
// the server's (api/librarySaveClient.ts -> crates/cli/src/library_save_api.rs), into the path a table row names. Revert is an `/api/cmd` batch (one undo step).
//
// KiCad: `SYMBOL_EDIT_FRAME::Save`, `saveCurrentSymbol`, `SaveAll` / `saveAllLibraries`, `Revert`, `SaveLibraryAs` (eeschema/symbol_editor/symbol_editor.cpp);
// `FOOTPRINT_EDITOR_CONTROL::Save`, `SaveAs`, `Revert` and `FOOTPRINT_EDIT_FRAME::SaveFootprint`, `RevertFootprint` (pcbnew/tools/footprint_editor_control.cpp,
// pcbnew/footprint_libraries_utils.cpp), commit 8303b2ad.
import { updateLinkedFootprint } from "./footprintLibraryOps";
import { fetchModified, fetchWritableLibraries, revertInLibrary, saveToLibrary } from "../api/librarySaveClient";
import { librariesChanged } from "../api/libraryTablesClient";
import { setLibraryStatusText } from "../state/libraryStatus";
import { askConfirm } from "../components/library/libraryDialogs";
import { notifyLibraryChanged } from "../components/library/useLibraryNames";
import { footprintSaveTarget, isTableLibrary, libraryOfId, MUST_SAVE_ELSEWHERE, notWritableMessage, REVERT_DETAIL, revertQuestion, saveAllPlan, symbolSaveLibrary, symbolTarget } from "../kicad-port/librarySave";
import { splitLibName } from "../kicad-port/libraryNames";
import { getDockLayout } from "../state/dockLayoutStore";
import type { FootprintEditorApi } from "../state/footprintEditorStore";
import { getLibraryTree } from "../state/libraryTree";
import { openSaveLibraryAs } from "../state/librarySaveDialogs";
import type { SymbolEditorApi } from "../state/symbolEditorStore";

type Toast = (message: string, kind?: "info" | "error") => void;

/** The project library's items are the design's own: every edit is stored as it is made (said in the status bar, where KiCad says what a save did). */
const SYMBOL_SAVED_IN_DESIGN = "Symbol saved: every edit of the project library is stored in the design as you make it (File > Export writes a .kicad_sym).";
const FOOTPRINT_SAVED_IN_DESIGN = "Footprint saved: every edit of the project library is stored in the design as you make it (File > Export writes a .kicad_mod).";

/**
 * `saveLibrary( lib, false )`, and for a library that cannot be written the question `Save` asks ("Symbol library '%s' is not writable.", "You must save to a different
 * location.", OK / Cancel) whose OK is Save Library As. True when the library was saved.
 */
async function saveSymbolLibraryNamed(lib: string, toast: Toast, askReadOnly = true): Promise<boolean> {
  const r = await saveToLibrary({ kind: "symbol", lib });
  if (r.ok) {
    // `saveLibrary` formats "Symbol library file '%s' saved." and shows nothing: the tree's ` *` going is the word (UI-LEFT-2 LL6)
    notifyLibraryChanged();
    return true;
  }
  if (r.read_only) {
    if (askReadOnly && (await askConfirm({ title: "Warning", message: `${r.error}\n\n${r.detail ?? MUST_SAVE_ELSEWHERE}`, okLabel: "OK", cancelLabel: "Cancel" }))) openSaveLibraryAs("symbol", lib);
    return false;
  }
  toast(r.detail ? `${r.error}\n${r.detail}` : r.error, "error");
  return false;
}

/** `SYMBOL_EDIT_FRAME::Save` (Ctrl+S): the library the tree has selected, else the open symbol's (`saveCurrentSymbol`). */
export async function saveSymbol(api: SymbolEditorApi, toast: Toast): Promise<void> {
  const st = api.getState();
  const lib = symbolSaveLibrary(!getDockLayout().treeCollapsed, getLibraryTree("symbol").selectedLibs, st.treeSelection, st.libId);
  if (lib === null) return; // `saveCurrentSymbol` with no symbol: nothing to save
  if (!isTableLibrary(lib)) return setLibraryStatusText(SYMBOL_SAVED_IN_DESIGN);
  await saveSymbolLibraryNamed(lib, toast);
}

/** `SYMBOL_EDIT_FRAME::SaveAll` -> `saveAllLibraries( false )`: every modified library of the tables (kicad-port/librarySave.ts `saveAllPlan`). */
export async function saveAllSymbols(toast: Toast): Promise<void> {
  const [{ libraries }, writable] = await Promise.all([fetchModified("symbol"), fetchWritableLibraries("symbol")]);
  const plan = saveAllPlan(libraries, new Map(writable.map((l) => [l.name, l.writable])));
  if (plan.save.length === 0 && plan.readOnly.length === 0) return; // `saveAllLibraries` with nothing modified: nothing to say
  for (const lib of plan.save) await saveSymbolLibraryNamed(lib, toast, false);
  if (plan.askAboutReadOnly) {
    const lib = plan.readOnly[0]!;
    if (await askConfirm({ title: "Warning", message: `${notWritableMessage(lib)}\n\n${MUST_SAVE_ELSEWHERE}`, okLabel: "OK", cancelLabel: "Cancel" })) openSaveLibraryAs("symbol", lib);
  } else {
    for (const lib of plan.readOnly) toast(`${notWritableMessage(lib)}  ${MUST_SAVE_ELSEWHERE}`, "error");
  }
}

/** `SYMBOL_EDIT_FRAME::SaveLibraryAs` (Ctrl+Shift+S): the library of `GetTargetLibId()`, the tree's selection or the open symbol's. */
export function saveSymbolLibraryAs(api: SymbolEditorApi, toast: Toast): void {
  const st = api.getState();
  const t = symbolTarget(st.treeSelection, getLibraryTree("symbol").selectedLibs, st.libId);
  if (t.kind === "none") return toast("Select a library in the library tree first.");
  openSaveLibraryAs("symbol", t.kind === "library" ? t.lib : libraryOfId(t.id));
}

/**
 * `SYMBOL_EDIT_FRAME::Revert( true )`: the tree's target symbol, or library, goes back to what the library's file holds after `ConfirmRevertDialog`. Only a modified
 * symbol is reverted (the action's `symbolModifiedCondition`). The server makes it one undoable step (`POST /api/library/revert`): a symbol the file never had goes
 * back to the symbol as it was made (`RevertSymbol`: the buffer's `m_original`), a library drops the symbols its file never had and gets back what Delete and
 * Rename took out (`RevertLibrary`: the buffer and its deleted list).
 */
export async function revertSymbol(api: SymbolEditorApi, toast: Toast): Promise<void> {
  const st = api.getState();
  const t = symbolTarget(st.treeSelection, getLibraryTree("symbol").selectedLibs, st.libId);
  if (t.kind === "none") return;
  const lib = t.kind === "library" ? t.lib : libraryOfId(t.id);
  if (!isTableLibrary(lib)) return toast("The project library has no saved file to revert to: Undo steps back through its edits.");
  const { items, libraries } = await fetchModified("symbol");
  const touched = t.kind === "library" ? libraries.includes(lib) : items.includes(t.id);
  if (!touched) return toast(`${t.kind === "library" ? lib : splitLibName(t.id).item} has no changes to revert.`);
  const name = t.kind === "library" ? lib : splitLibName(t.id).item;
  if (!(await askConfirm({ title: "Revert", message: `${revertQuestion(name)}\n\n${REVERT_DETAIL}`, okLabel: "Revert", cancelLabel: "Cancel" }))) return;
  const r = await revertInLibrary(t.kind === "library" ? { kind: "symbol", lib } : { kind: "symbol", id: t.id });
  notifyLibraryChanged();
  if (!r.ok) return toast(r.nothing ? r.message : r.error, r.nothing ? "info" : "error");
  // the open symbol is shown again as it was saved, or leaves the canvas when the library never had it (`reload_currentSymbol`)
  const open = st.libId;
  if (open && r.gone.includes(open)) api.closeSymbol();
  else await api.refresh();
}

/**
 * `FOOTPRINT_EDITOR_CONTROL::Save` -> `SaveFootprint`: the loaded footprint, when it is the tree's target, into its library's folder (the project library's are the
 * design's own). A footprint not in any library is Save As in KiCad; here the project library is its library.
 */
export async function saveFootprint(api: FootprintEditorApi, toast: Toast): Promise<void> {
  const st = api.getState();
  // `SaveFootprint`: a footprint loaded from the board is saved to the board -- that footprint only (`SaveFootprintToBoard( false )`, UI-LEFT F3)
  if (st.footprint?.board_link) {
    await updateLinkedFootprint(api, (m, kind) => toast(m, kind));
    return;
  }
  const id = footprintSaveTarget(st.treeSelection, getLibraryTree("footprint").selectedLibs, st.name);
  if (!id) return;
  // `if( padTool->InPadEditMode() ) m_toolManager->RunAction( PCB_ACTIONS::recombinePad )`: a pad being edited as shapes is a pad again first
  if (st.footprint?.pad_edit) await api.editPadShapes();
  if (!isTableLibrary(libraryOfId(id))) return setLibraryStatusText(FOOTPRINT_SAVED_IN_DESIGN);
  // `SaveFootprint`: the old file of a renamed footprint deleted, then `SaveFootprintInLibrary` (which says nothing: UI-LEFT-2 LL6)
  const r = await saveToLibrary({ kind: "footprint", id });
  if (r.ok) notifyLibraryChanged();
  else toast(r.error, "error");
}

/** `FOOTPRINT_EDIT_FRAME::RevertFootprint`: a modified footprint (`IsContentModified`) goes back to its library's file after `ConfirmRevertDialog`, one undoable step. */
export async function revertFootprint(api: FootprintEditorApi, toast: Toast): Promise<void> {
  const id = api.getState().name;
  if (!id) return;
  if (!isTableLibrary(libraryOfId(id))) return toast("The project library has no saved file to revert to: Undo steps back through its edits.");
  if (!(await fetchModified("footprint")).items.includes(id)) return toast(`${splitLibName(id).item} has no changes to revert.`);
  if (!(await askConfirm({ title: "Revert", message: `${revertQuestion(splitLibName(id).item)}\n\n${REVERT_DETAIL}`, okLabel: "Revert", cancelLabel: "Cancel" }))) return;
  const r = await revertInLibrary({ kind: "footprint", id });
  notifyLibraryChanged();
  if (!r.ok) return toast(r.nothing ? r.message : r.error, r.nothing ? "info" : "error");
  await api.refresh();
}

/**
 * The reload of a library changed on disk (UI-LEFT-2 LL3; state/libraryWatch.ts): the Symbol Editor's `MAIL_REFRESH_SYMBOL` -> `UpdateLibraryBuffer( libName )`
 * (the whole library's buffer read again, the open symbol shown as the file has it now, or the canvas emptied when the file no longer has it), the Footprint
 * Editor's `LoadFootprint` -> `ReloadFootprint`. One undoable step, as Revert.
 */
export async function reloadFromLibrary(kind: "symbol" | "footprint", id: string, done: (gone: boolean) => Promise<void>, toast: Toast): Promise<void> {
  const r = await revertInLibrary(kind === "symbol" ? { kind, lib: libraryOfId(id) } : { kind, id });
  // the file changed: what the trees list of it is read again
  librariesChanged();
  notifyLibraryChanged();
  if (r.ok) return done(r.gone.includes(id));
  if (!r.nothing) toast(r.error, "error"); // `DisplayError( this, ioe.What() )`
}
