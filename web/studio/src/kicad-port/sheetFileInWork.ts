// Where the derived schematic writes a screen (`eda_kicad::sheet_file_in_work`, crates/kicad/src/lib.rs): the screen's key -- its path from the root's folder,
// as the placing sheet names it -- with each `..` as `__up__` and an absolute key under `__root__/`, so a sheet above the root's folder is inside the work folder
// too. The files `/api/schematic.kicad_sch` hands back are named so (r4 B4-M1: Save Current Sheet Copy As looked a sheet's file up by its key).

/** `sheet_file_in_work`: the name the screen `file` (its key) is written at. */
export function sheetFileInWork(file: string): string {
  const parts: string[] = [];
  if (file.startsWith("/")) parts.push("__root__");
  for (const c of file.split("/")) {
    if (c === "" || c === ".") continue;
    parts.push(c === ".." ? "__up__" : c);
  }
  return parts.join("/");
}

/** The file name a copy of the screen `file` is saved under: its last part (`../shared/amp.kicad_sch` is `amp.kicad_sch`). */
export function sheetFileBaseName(file: string): string {
  return file.split("/").filter((c) => c !== "").pop() ?? file;
}
