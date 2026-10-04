// File > Save As... (`common.Control.saveAs`, ACTIONS::saveAs, "Save current document to another location").
//
// KiCad writes the editor's own document (`.kicad_pcb` / `.kicad_sch`) under a new name. Here the master is
// `design.json` (KiCad's files are derived from it, like every export), so Save As hands the browser the derived
// KiCad file(s) to save -- the web's Save dialog -- named after the board.

/** A board name as a file stem: characters a file name cannot hold become "_", leading dots go (no hidden file), "board" when nothing is left. */
export function fileStem(name: string): string {
  const stem = name
    .trim()
    // eslint-disable-next-line no-control-regex
    .replace(/[\\/:*?"<>|\u0000-\u001f]/g, "_")
    .replace(/^\.+/, "");
  return stem === "" ? "board" : stem;
}

/**
 * The names the schematic's files are saved under: the root sheet takes the board's name (the backend names it
 * `board.kicad_sch`), every sub-sheet keeps its own file name -- the root's `(sheet ...)` blocks refer to those by name.
 */
export function schematicSaveNames(serverNames: readonly string[], boardName: string): string[] {
  return serverNames.map((n, i) => (i === 0 ? `${fileStem(boardName)}.kicad_sch` : n));
}
