// Rescue Symbols' dialog rules (`DIALOG_RESCUE_EACH`, eeschema/dialogs/dialog_rescue_each.cpp, and `RESCUER::RescueProject`, eeschema/project_rescue.cpp, at
// KiCad 8303b2ad), apart from the React of components/schControl/RescueSymbolsDialog.tsx: the words, which candidates the Accept column keeps, and what is
// said when there is nothing to rescue or nothing was kept.

/** `DIALOG_RESCUE_EACH`'s info message. */
export const RESCUE_INFO =
  'This schematic was made using older symbol libraries which may break the schematic.  Some symbols may need to be linked to a different symbol name.  Some symbols may need to be "rescued" (copied and renamed) into a new library.\n\nThe following changes are recommended to update the project.';

/** `RESCUER::RescueProject` run on demand with no candidate. */
export const NOTHING_TO_RESCUE = "This project has nothing to rescue.";

/** `RESCUER::RescueProject` when the dialog kept none (Skip Symbol Rescue, or every Accept box cleared). */
export const NOTHING_RESCUED = "No symbols were rescued.";

/** `DIALOG_SYMBOL_REMAP`'s explanation. */
export const REMAP_INFO =
  'This schematic currently uses the project symbol library list look up method for loading library symbols.  KiCad will attempt to map the existing symbols to use the new symbol library table.  Remapping will change some project files and schematics may not be compatible with older versions of KiCad.  All files that are changed will be backed up to the "rescue-backup" folder in the project folder should you need to revert any changes.  If you choose to skip this step, you will be responsible for manually remapping the symbols.';

/** `PopulateInstanceList`'s title. */
export function instancesTitle(count: number): string {
  return `Instances of this symbol (${count} items):`;
}

/** `TransferDataFromWindow`: the requested ids of the rows whose Accept box is checked, in the list's order. Every row starts checked (`PopulateConflictList`). */
export function chosenCandidates(requested: readonly string[], unchecked: ReadonlySet<string>): string[] {
  return requested.filter((r) => !unchecked.has(r));
}

/** The message the rescue ends with, or none when it rescued (`RescueProject`'s two message dialogs). */
export function rescueOutcome(candidates: number, chosen: number): string | null {
  if (candidates === 0) return NOTHING_TO_RESCUE;
  if (chosen === 0) return NOTHING_RESCUED;
  return null;
}
