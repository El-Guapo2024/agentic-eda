// What the Footprint Editor's library actions (`pcbnew.ModuleEditor.*`, `FOOTPRINT_EDITOR_CONTROL`) do, as plain async functions over the
// editor's store; each cites the KiCad function it ports (pcbnew/tools/footprint_editor_control.cpp, pcbnew/footprint_libraries_utils.cpp,
// pcbnew/load_select_footprint.cpp). The action registry (`libraryEditorActions.ts`) and the library tree's context menu call them; every
// edit is an `/api/cmd` verb with undo.
//
// The studio's library is the project library (`design.footprint_library`, entries keyed by `Lib:Name` or a bare name); a footprint the tree
// shows that only the intent, a `.kicad_mod` file or the builtin table defines is read-only until it is opened (which makes an editable
// copy). Copy, Export, Duplicate read any footprint; Delete and Rename need a project-library entry.
import type { Dispatch } from "react";
import type { Cmd } from "../api/types";
import { fetchFootprintLibraryNames, fetchState, postCmd } from "../api/client";
import { fetchAnyFootprint, parseFootprintText, pickTextFile } from "../api/libraryClient";
import { addedPart, insertedMessage, outlineCentre } from "../kicad-port/fpInsert";
import { duplicateIdCount, repairMessage } from "../kicad-port/fpRepair";
import { pastedFootprint, storableFootprint } from "../kicad-port/fpLibFields";
import { ensureUniqueLibId, footprintNameError, joinLibName, pastedFootprintName, PROJECT_LIBRARY, splitLibName } from "../kicad-port/libraryNames";
import { askConfirm, askText } from "../components/library/libraryDialogs";
import { fetchLibraryItemsNow, fetchWritableLibraries, saveToLibrary } from "../api/librarySaveClient";
import { deleteFootprintQuestion, footprintDeletedStatus, footprintLibraryReadOnly, isTableLibrary } from "../kicad-port/librarySave";
import { setLibraryStatusText } from "../state/libraryStatus";
import { notifyLibraryChanged } from "../components/library/useLibraryNames";
import type { FootprintEditorApi, FpAction } from "../state/footprintEditorStore";
import type { Action as StudioAction, StudioApi } from "../state/store";
import { PLACEMENT_IN_PROGRESS, placeFootprintFromEditor } from "./placeFootprintTool";

export interface FpCtx {
  api: FootprintEditorApi;
  dispatch: Dispatch<FpAction>;
}

const info = (c: FpCtx, message: string) => c.dispatch({ type: "TOAST", message, kind: "info" });
const fail = (c: FpCtx, e: unknown) => c.dispatch({ type: "TOAST", message: e instanceof Error ? e.message : String(e), kind: "error" });

/** `GetTargetFPID`: the footprint the library tree has selected, else the open one. */
export function targetName(c: FpCtx): string | null {
  const st = c.api.getState();
  return st.treeSelection ?? st.name;
}

async function names(): Promise<{ all: string[]; project: Set<string> }> {
  const r = await fetchFootprintLibraryNames();
  return { all: r.names, project: new Set(r.project ?? []) };
}

async function runBatch(c: FpCtx, cmds: Cmd[]): Promise<boolean> {
  const ok = await c.api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
  notifyLibraryChanged();
  return ok;
}

function select(c: FpCtx, name: string | null) {
  c.dispatch({ type: "SET_TREE_SELECTION", name });
}

/** `FOOTPRINT_EDITOR_CONTROL::EditFootprint`: `LoadFootprintFromLibrary( GetSelectedLibId() )`. */
export async function editFootprint(c: FpCtx): Promise<void> {
  const name = targetName(c);
  if (!name) return info(c, "Select a footprint in the library tree first.");
  try {
    await c.api.openFootprint(name);
    notifyLibraryChanged();
  } catch (e) {
    fail(c, e);
  }
}

/**
 * `FOOTPRINT_EDITOR_CONTROL::CutCopyFootprint`: the selected footprint is kept as `m_copiedFootprint` (a copy of the loaded one when it is the
 * loaded one), and a cut then runs `DeleteFootprint`.
 */
export async function copyFootprint(c: FpCtx, cut: boolean): Promise<void> {
  const name = targetName(c);
  if (!name) return info(c, "Select a footprint in the library tree first.");
  try {
    const { footprint } = await fetchAnyFootprint(name);
    c.dispatch({ type: "SET_COPIED_FOOTPRINT", footprint });
    info(c, `${name} ${cut ? "cut" : "copied"}`);
  } catch (e) {
    return fail(c, e);
  }
  if (cut) await deleteFootprint(c);
}

/**
 * `FOOTPRINT_EDITOR_CONTROL::PasteFootprint`: the copied footprint lands in the target's library under its own name, `_copy` appended
 * while that name is taken, and is loaded. Only the name changes (`SetFPID`): the Value keeps what it showed (kicad-port/fpLibFields.ts `pastedFootprint`).
 */
export async function pasteFootprint(c: FpCtx): Promise<void> {
  const copied = c.api.getState().copiedFootprint;
  if (!copied) return info(c, "Nothing was copied.");
  try {
    const { all } = await names();
    const lib = splitLibName(targetName(c) ?? "").lib;
    const name = pastedFootprintName(joinLibName(lib, splitLibName(copied.name).item), all);
    if (!(await runBatch(c, [{ op: "put_library_footprint", footprint: { ...storableFootprint(pastedFootprint(copied, name)), published: false } }]))) return;
    await c.api.openFootprint(name);
    select(c, name);
  } catch (e) {
    fail(c, e);
  }
}

/** `FOOTPRINT_EDITOR_CONTROL::DuplicateFootprint` -> `FOOTPRINT_EDIT_FRAME::DuplicateFootprint`: a copy under `name_1`, `name_2`, ..., loaded. */
export async function duplicateFootprint(c: FpCtx): Promise<void> {
  const name = targetName(c);
  if (!name) return info(c, "Select a footprint in the library tree first.");
  try {
    const { footprint } = await fetchAnyFootprint(name);
    const { all } = await names();
    const copyName = ensureUniqueLibId(name, all);
    if (!(await runBatch(c, [{ op: "put_library_footprint", footprint: { ...storableFootprint(footprint), name: copyName, published: false } }]))) return;
    await c.api.openFootprint(copyName);
    select(c, copyName);
  } catch (e) {
    fail(c, e);
  }
}

/**
 * `FOOTPRINT_EDITOR_CONTROL::DeleteFootprint` -> `DeleteFootprintFromLibrary( fpid, true )`: a library that cannot be written is "Library '%s' is read only." (the
 * info bar); after "Delete footprint '%s' from library '%s'?" the footprint is removed from the library -- for a library of the tables, its file deleted from the
 * `.pretty` folder at once, as KiCad's `FootprintDelete` does (UI-LEFT-2 LL1: the design's mark, then the library's save of it) -- the status bar says
 * "Footprint '%s' deleted from library '%s'", and if it was the loaded one the canvas is emptied (`Clear_Pcb`).
 */
export async function deleteFootprint(c: FpCtx): Promise<void> {
  const name = targetName(c);
  if (!name) return info(c, "Select a footprint in the library tree first.");
  try {
    const { project } = await names();
    const { lib, item } = splitLibName(name);
    const tableLib = isTableLibrary(lib);
    if (!tableLib && !project.has(name)) return fail(c, new Error(`${name} is not in the project library (the intent, a .kicad_mod file or the built-in table defines it), so there is nothing to delete.`));
    if (tableLib) {
      const row = (await fetchWritableLibraries("footprint")).find((l) => l.name === lib);
      if (!row) return; // `GetFullURI` found no library: `return false`
      if (!row.writable) return fail(c, new Error(footprintLibraryReadOnly(lib)));
    }
    if (!(await askConfirm({ title: "Confirmation", message: deleteFootprintQuestion(item, lib || PROJECT_LIBRARY), okLabel: "Yes", cancelLabel: "No" }))) return;
    const wasOpen = c.api.getState().name === name;
    if (!(await runBatch(c, [{ op: "delete_library_footprint", name }]))) return;
    if (tableLib) {
      const r = await saveToLibrary({ kind: "footprint", lib });
      if (!r.ok) return fail(c, new Error(r.error));
    }
    if (wasOpen) c.api.closeFootprint();
    select(c, null);
    setLibraryStatusText(footprintDeletedStatus(item, lib || PROJECT_LIBRARY));
  } catch (e) {
    fail(c, e);
  }
}

/**
 * `FOOTPRINT_EDITOR_CONTROL::RenameFootprint`: "Change Footprint Name" ("Footprint must have a name."); a name the library has (`FootprintExists`: the design's
 * copy or the library's file) is replaced only after "Footprint '%s' already exists in library '%s'." with Overwrite. The loaded footprint only changes its
 * name (`SetFPID`, and a Value that was the name): its Save writes it under the new name and deletes the old file (`SaveFootprint`'s `nameChanged`); any other
 * footprint of a library of the tables is saved under the new name and its old file deleted at once (`SaveFootprintInLibrary`, `DeleteFootprint`). UI-LEFT-2 LL1.
 */
export async function renameFootprint(c: FpCtx): Promise<void> {
  const name = targetName(c);
  if (!name) return info(c, "Select a footprint in the library tree first.");
  try {
    const r = await fetchFootprintLibraryNames();
    const project = new Set(r.project ?? []);
    const { lib, item } = splitLibName(name);
    const tableLib = isTableLibrary(lib);
    if (!tableLib && !project.has(name)) return fail(c, new Error(`${name} is not in the project library (the intent, a .kicad_mod file or the built-in table defines it); duplicate it under a new name instead.`));
    const entered = await askText({
      title: "Change Footprint Name",
      label: "New name:",
      initial: item,
      okLabel: "Rename",
      validate: (v) => (v === "" ? "Footprint must have a name." : footprintNameError(joinLibName(lib, v))),
    });
    if (entered === null) return;
    const newName = joinLibName(lib, entered);
    if (newName === name) return;
    const deleted = new Set(r.deleted ?? []);
    const inFile = tableLib ? (await fetchLibraryItemsNow("footprint", lib)).map((n) => joinLibName(lib, n)).filter((n) => !deleted.has(n)) : [];
    let overwrite = false;
    if (r.names.includes(newName) || inFile.includes(newName)) {
      if (!(await askConfirm({ title: "Confirmation", message: `Footprint '${entered}' already exists in library '${lib || PROJECT_LIBRARY}'.`, okLabel: "Overwrite" }))) return;
      overwrite = true;
    }
    const wasOpen = c.api.getState().name === name;
    if (!(await runBatch(c, [{ op: "rename_library_footprint", name, new_name: newName, overwrite }]))) return;
    if (tableLib && !wasOpen) {
      // a footprint that is not loaded: `SaveFootprintInLibrary` under the new name, then `DeleteFootprint( libraryName, oldName )`
      const saved = await saveToLibrary({ kind: "footprint", id: newName });
      if (!saved.ok) fail(c, new Error(`Error renaming footprint\n${saved.error}`));
    }
    if (wasOpen) await c.api.openFootprint(newName);
    select(c, newName);
  } catch (e) {
    fail(c, e);
  }
}

/**
 * `FOOTPRINT_EDIT_FRAME::ImportFootprint`: choose a `.kicad_mod`, read it and load it. KiCad loads it into the editor unsaved under the
 * name in the file; here every edit is stored at once, so it is stored in the target's library under that name, `_1`, `_2`... if the
 * name is taken (an import never replaces a footprint), and loaded. `dropped` is the file a drop handed over (`DdImportFootprint`:
 * `ImportFootprint( aName )` with the name given, no chooser): its name and text, a browser having no path to give.
 */
export async function importFootprint(c: FpCtx, dropped?: { name: string; text: string }): Promise<void> {
  try {
    const file = dropped ?? (await pickTextFile(".kicad_mod"));
    if (!file) return;
    const parsed = await parseFootprintText(file.text);
    const { all } = await names();
    const lib = splitLibName(targetName(c) ?? "").lib;
    const name = ensureUniqueLibId(joinLibName(lib, splitLibName(parsed.footprint.name).item || file.name.replace(/\.kicad_mod$/i, "")), all);
    if (!(await runBatch(c, [{ op: "put_library_footprint", footprint: { ...storableFootprint(parsed.footprint), name, published: false } }]))) return;
    await c.api.openFootprint(name);
    select(c, name);
    info(c, parsed.warnings.length ? `Imported ${name}. ${parsed.warnings.join("; ")}` : `Imported ${name}`);
  } catch (e) {
    fail(c, e);
  }
}

/**
 * `FOOTPRINT_EDITOR_CONTROL::Properties` (`pcbnew.ModuleEditor.footprintProperties`): the properties of the selected footprint. The one
 * dialog here edits the open footprint, so a different footprint is opened first.
 */
export async function footprintProperties(c: FpCtx): Promise<void> {
  const name = targetName(c);
  if (!name) return info(c, "Select a footprint in the library tree first.");
  try {
    if (c.api.getState().name !== name) await c.api.openFootprint(name);
    c.dispatch({ type: "SET_FOOTPRINT_PROPERTIES_OPEN", open: true });
  } catch (e) {
    fail(c, e);
  }
}

/** `FOOTPRINT_EDITOR_CONTROL::RepairFootprint`: duplicate ids are replaced; the answer is "%d potential problems repaired." or "No footprint problems found.". */
export async function repairFootprint(c: FpCtx): Promise<void> {
  const st = c.api.getState();
  if (!st.name || !st.footprint) return info(c, "Open a footprint first.");
  const duplicates = duplicateIdCount(st.footprint);
  const msg = repairMessage(duplicates);
  if (duplicates > 0 && !(await c.api.cmd({ op: "repair_footprint", name: st.name }))) return;
  info(c, msg.details ? `${msg.title} ${msg.details}` : msg.title);
}

/**
 * `FOOTPRINT_EDIT_FRAME::SaveFootprintToBoard` (`pcbnew.ModuleEditor.saveFootprintToBoard`, "Insert footprint into PCB"): the edited footprint goes to the board. KiCad exchanges
 * the board footprint it was loaded from, or -- when there is none -- inserts it as a new one. The studio's board holds the footprints of its parts, so where parts name the footprint
 * this is the explicit "Update Footprint on Board" (every part naming it takes its pads from the library entry); where none does the footprint is inserted as a part of its own
 * (`insertFootprint`). `parts` are the board's parts as `{ ref, footprint }`.
 */
export async function saveFootprintToBoard(c: FpCtx, parts: readonly { ref: string; footprint: string | null }[], studio?: Dispatch<StudioAction>, studioApi?: StudioApi): Promise<void> {
  const st = c.api.getState();
  if (!st.name) return info(c, "Open a footprint first.");
  // a footprint loaded from the board goes back on that footprint only (`sourceFootprint`, found by the link): the exchange
  if (st.footprint?.board_link) return void (await updateLinkedFootprint(c.api, (m, kind) => (kind === "error" ? fail(c, new Error(m)) : info(c, m))));
  const users = parts.filter((p) => p.footprint === st.name).map((p) => p.ref);
  if (users.length === 0) return studio && studioApi ? carryInsertedFootprint(c, st.name, studio, studioApi) : insertFootprint(c, st.name, studio);
  if (!(await c.api.cmd({ op: "update_footprint_on_board", name: st.name }))) return;
  info(c, `${st.name}: ${users.length === 1 ? users[0] : `${users.length} parts`} on the board now follow${users.length === 1 ? "s" : ""} this footprint`);
}

/**
 * `FOOTPRINT_EDIT_FRAME::SaveFootprintToBoard` of a footprint loaded from the board (`GetLink() != niluuid`; Save, and "Insert footprint into PCB" with a link): the board footprint the
 * copy is linked to is exchanged for it (`fp_editor save_to_board`, one step of the board, "Update Footprint"), and no other. A pad being edited as shapes is recombined first
 * (`SaveFootprint`). False when the board refused it ("Unable to find the footprint on the main board.\nCannot save." once the footprint is gone).
 */
export async function updateLinkedFootprint(api: FootprintEditorApi, say: (message: string, kind: "info" | "error") => void): Promise<boolean> {
  const st = api.getState();
  const link = st.footprint?.board_link;
  if (!st.name || !link) return false;
  if (st.footprint?.pad_edit) await api.editPadShapes();
  const reply = await postCmd({ op: "fp_editor", verb: "save_to_board", footprint: st.name }, false);
  if (!reply.ok) {
    say(reply.message, "error");
    return false;
  }
  say(`${link.reference} updated on the board.`, "info");
  return true;
}

/**
 * The insert branch of `SaveFootprintToBoard`, as KiCad runs it: a part of its own for the footprint (`Cmd::PlaceFootprint`: the next free reference of the footprint's usual
 * prefix, its name as the value) carrying the editor's copy, with the project's entry published in the same step so the part resolves to the edited footprint by its name --
 * then `pcbframe->Raise(); pcb_ToolManager->RunAction( PCB_ACTIONS::placeFootprint, newFootprint )`: the board is shown and its Place Footprint tool carries the footprint
 * from the board's cursor until a click puts it down (R and F before that click), and the tool goes on after it (actions/placeFootprintTool.ts). The insert and the drop are
 * one undo step; Escape takes the footprint away again (KiCad's `cleanup()` pops the "Insert Footprint" step). An insert while a footprint is carried is refused in KiCad's words.
 */
async function carryInsertedFootprint(c: FpCtx, name: string, studio: Dispatch<StudioAction>, studioApi: StudioApi): Promise<void> {
  const definition = c.api.getState().footprint;
  if (!definition) return fail(c, new Error(`${name} has no footprint to insert.`));
  studio({ type: "SET_TAB", tab: "pcb" });
  const build = (at: { x: number; y: number }): Cmd => ({ op: "batch", cmds: [{ op: "place_footprint", footprint: name, at, definition }, { op: "update_footprint_on_board", name }] });
  if (!placeFootprintFromEditor({ api: studioApi, dispatch: studio }, build)) fail(c, new Error(PLACEMENT_IN_PROGRESS));
}

/**
 * INACTIVE (UI-LEFT P19, replaced by `carryInsertedFootprint`, which carries the footprint as KiCad does; reached only without the board editor's API): the insert put
 * the part at the centre of the board and selected it.
 */
async function insertFootprint(c: FpCtx, name: string, studio?: Dispatch<StudioAction>): Promise<void> {
  const definition = c.api.getState().footprint;
  if (!definition) return fail(c, new Error(`${name} has no footprint to insert.`));
  let before;
  try {
    before = await fetchState();
  } catch (e) {
    return fail(c, e);
  }
  const place: Cmd = { op: "place_footprint", footprint: name, at: outlineCentre(before.outline), definition };
  if (!(await c.api.cmd({ op: "batch", cmds: [place, { op: "update_footprint_on_board", name }] }))) return;
  let added: string | null = null;
  try {
    added = addedPart(before.parts, (await fetchState()).parts);
  } catch {
    /* the part is on the board; only its name is not told */
  }
  if (studio) {
    studio({ type: "SET_TAB", tab: "pcb" });
    if (added) studio({ type: "SET_SELECTION", refs: [added] });
  }
  info(c, insertedMessage(name, added));
}
