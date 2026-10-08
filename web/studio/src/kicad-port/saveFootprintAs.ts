// Save As in the Footprint Editor (`common.Control.saveAs`, `ACTIONS::saveAs`): the rules of KiCad's `FOOTPRINT_EDITOR_CONTROL::SaveAs`, the
// `SAVE_AS_DIALOG` and `FOOTPRINT_EDIT_FRAME::SaveFootprintAs` (pcbnew/tools/footprint_editor_control.cpp, pcbnew/footprint_libraries_utils.cpp, commit
// 8303b2ad). Pure; the dialog is components/SaveFootprintAsDialog.tsx and the verb is `put_library_footprint`.
//
// Not here: the footprint's Value field following the new name (`updateValue`: a library footprint has no Value of its own in this model -- the board's
// instances do) and the library file behind a nickname (the project library is the one set of footprints in design.json).
import { footprintNameError, joinLibName, PROJECT_LIBRARY, splitLibName } from "./libraryNames";

/** What `SaveAs` works on: the footprint the library tree has selected, else the loaded one -- or a library row, which is "Save Library As". */
export type SaveAsTarget = { kind: "footprint"; name: string; loaded: boolean } | { kind: "library"; lib: string } | { kind: "none" };

/**
 * `GetTargetFPID()` and the three branches of `SaveAs`: a library with no footprint selected is Save Library As; the loaded footprint (the tree's selection,
 * or none) is Save Footprint As; another footprint the tree selects is Save Selected Footprint As.
 */
export function saveAsTarget(treeSelection: string | null, loadedName: string | null, selectedLibs: readonly string[]): SaveAsTarget {
  if (treeSelection === null && selectedLibs.length > 0) return { kind: "library", lib: selectedLibs[0]! };
  const name = treeSelection ?? loadedName;
  if (!name) return { kind: "none" };
  return { kind: "footprint", name, loaded: name === loadedName };
}

/** `SAVE_AS_DIALOG::GetFPName`: the typed name with the blanks at both ends trimmed. */
export function typedFootprintName(typed: string): string {
  return typed.trim();
}

/** The id the footprint is stored under: the project library's footprints are bare names, every other library's are `Lib:Name`. */
export function savedFootprintId(lib: string, typed: string): string {
  const name = typedFootprintName(typed);
  return lib === PROJECT_LIBRARY ? name : joinLibName(lib, name);
}

/**
 * The validator the dialog runs on Save (`SaveFootprintAs`' lambda, in its order): a library, a name, a legal name -- or null. The name is checked as an item
 * of its library (`Lib:Name`, also for the project library, whose stored ids are bare), so a colon typed into it is refused and not read as a library.
 */
export function saveAsError(lib: string, typed: string): string | null {
  if (lib === "") return "A library must be specified.";
  return footprintNameError(joinLibName(lib, typedFootprintName(typed)));
}

/** "Footprint %s already exists in %s." -- the question before an existing footprint is replaced (the OK button says "Overwrite"). */
export function existsMessage(lib: string, typed: string): string {
  return `Footprint ${typedFootprintName(typed)} already exists in ${lib}.`;
}

/** The status text after the save: "Footprint '%s' added to '%s'" / "Footprint '%s' replaced in '%s'". */
export function savedMessage(lib: string, typed: string, replaced: boolean): string {
  return `Footprint '${typedFootprintName(typed)}' ${replaced ? "replaced in" : "added to"} '${lib}'`;
}

/** The library and name the dialog starts with: the footprint's own (`SAVE_AS_DIALOG( this, footprintName, libraryName )`); a bare name is the project library's. */
export function saveAsDefaults(name: string): { lib: string; item: string } {
  const { lib, item } = splitLibName(name);
  return { lib: lib || PROJECT_LIBRARY, item };
}
