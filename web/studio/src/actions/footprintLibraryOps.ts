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
import { fetchFootprintLibraryNames } from "../api/client";
import { fetchAnyFootprint, parseFootprintText, pickTextFile } from "../api/libraryClient";
import { duplicateIdCount, repairMessage } from "../kicad-port/fpRepair";
import { ensureUniqueLibId, footprintNameError, joinLibName, pastedFootprintName, PROJECT_LIBRARY, splitLibName } from "../kicad-port/libraryNames";
import { askConfirm, askText } from "../components/library/libraryDialogs";
import { notifyLibraryChanged } from "../components/library/useLibraryNames";
import type { FootprintEditorApi, FpAction } from "../state/footprintEditorStore";

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
 * while that name is taken, and is loaded.
 */
export async function pasteFootprint(c: FpCtx): Promise<void> {
  const copied = c.api.getState().copiedFootprint;
  if (!copied) return info(c, "Nothing was copied.");
  try {
    const { all } = await names();
    const lib = splitLibName(targetName(c) ?? "").lib;
    const name = pastedFootprintName(joinLibName(lib, splitLibName(copied.name).item), all);
    if (!(await runBatch(c, [{ op: "put_library_footprint", footprint: { ...copied, name, published: false } }]))) return;
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
    if (!(await runBatch(c, [{ op: "put_library_footprint", footprint: { ...footprint, name: copyName, published: false } }]))) return;
    await c.api.openFootprint(copyName);
    select(c, copyName);
  } catch (e) {
    fail(c, e);
  }
}

/**
 * `FOOTPRINT_EDITOR_CONTROL::DeleteFootprint` -> `DeleteFootprintFromLibrary( fpid, true )`: after "Delete footprint '%s' from library '%s'?" the
 * footprint is removed from the library, and if it was the loaded one the canvas is emptied.
 */
export async function deleteFootprint(c: FpCtx): Promise<void> {
  const name = targetName(c);
  if (!name) return info(c, "Select a footprint in the library tree first.");
  try {
    const { project } = await names();
    const { lib, item } = splitLibName(name);
    if (!project.has(name)) return fail(c, new Error(`${name} is not in the project library (the intent, a .kicad_mod file or the built-in table defines it), so there is nothing to delete.`));
    if (!(await askConfirm({ title: "Delete Footprint", message: `Delete footprint '${item}' from library '${lib || PROJECT_LIBRARY}'?`, okLabel: "Delete" }))) return;
    const wasOpen = c.api.getState().name === name;
    if (!(await runBatch(c, [{ op: "delete_library_footprint", name }]))) return;
    if (wasOpen) c.api.closeFootprint();
    select(c, null);
    info(c, `Footprint '${item}' deleted from library '${lib || PROJECT_LIBRARY}'`);
  } catch (e) {
    fail(c, e);
  }
}

/** `FOOTPRINT_EDITOR_CONTROL::RenameFootprint`: "Change Footprint Name"; a name that is taken is replaced only after an "Overwrite" confirmation. */
export async function renameFootprint(c: FpCtx): Promise<void> {
  const name = targetName(c);
  if (!name) return info(c, "Select a footprint in the library tree first.");
  try {
    const { all, project } = await names();
    if (!project.has(name)) return fail(c, new Error(`${name} is not in the project library (the intent, a .kicad_mod file or the built-in table defines it); duplicate it under a new name instead.`));
    const { lib, item } = splitLibName(name);
    const entered = await askText({ title: "Change Footprint Name", label: "New name:", initial: item, okLabel: "Rename", validate: (v) => footprintNameError(joinLibName(lib, v)) });
    if (entered === null) return;
    const newName = joinLibName(lib, entered);
    if (newName === name) return;
    let overwrite = false;
    if (all.includes(newName)) {
      if (!(await askConfirm({ title: "Confirmation", message: `Footprint '${entered}' already exists in library '${lib || PROJECT_LIBRARY}'.`, okLabel: "Overwrite" }))) return;
      overwrite = true;
    }
    const wasOpen = c.api.getState().name === name;
    if (!(await runBatch(c, [{ op: "rename_library_footprint", name, new_name: newName, overwrite }]))) return;
    if (wasOpen) await c.api.openFootprint(newName);
    select(c, newName);
  } catch (e) {
    fail(c, e);
  }
}

/**
 * `FOOTPRINT_EDIT_FRAME::ImportFootprint`: choose a `.kicad_mod`, read it and load it. KiCad loads it into the editor unsaved under the
 * name in the file; here every edit is stored at once, so it is stored in the target's library under that name, `_1`, `_2`... if the
 * name is taken (an import never replaces a footprint), and loaded.
 */
export async function importFootprint(c: FpCtx): Promise<void> {
  try {
    const file = await pickTextFile(".kicad_mod");
    if (!file) return;
    const parsed = await parseFootprintText(file.text);
    const { all } = await names();
    const lib = splitLibName(targetName(c) ?? "").lib;
    const name = ensureUniqueLibId(joinLibName(lib, splitLibName(parsed.footprint.name).item || file.name.replace(/\.kicad_mod$/i, "")), all);
    if (!(await runBatch(c, [{ op: "put_library_footprint", footprint: { ...parsed.footprint, name, published: false } }]))) return;
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
 * `FOOTPRINT_EDIT_FRAME::SaveFootprintToBoard` (`pcbnew.ModuleEditor.saveFootprintToBoard`, "Insert footprint into PCB"): the edited footprint goes
 * to the board. KiCad exchanges the board footprint it was loaded from, or inserts a new one; the studio's board holds only the footprints of its
 * netlist's parts, so this is the explicit "Update Footprint on Board": every part naming the footprint now takes its pads from the library entry.
 * `parts` are the board's parts as `{ ref, footprint }`.
 */
export async function saveFootprintToBoard(c: FpCtx, parts: readonly { ref: string; footprint: string | null }[]): Promise<void> {
  const st = c.api.getState();
  if (!st.name) return info(c, "Open a footprint first.");
  const users = parts.filter((p) => p.footprint === st.name).map((p) => p.ref);
  if (users.length === 0) {
    return fail(c, new Error(`No part on the board uses ${st.name}. The studio's board holds only the footprints of its netlist's parts, so there is nothing to update and a free footprint cannot be inserted.`));
  }
  if (!(await c.api.cmd({ op: "update_footprint_on_board", name: st.name }))) return;
  info(c, `${st.name}: ${users.length === 1 ? users[0] : `${users.length} parts`} on the board now follow${users.length === 1 ? "s" : ""} this footprint`);
}
