// A schematic file dropped on the sheet (`eeschema.EditorControl.ddAppendFile`; `SCH_EDIT_TOOL::DdAppendFile` runs Import Sheet with the file): the pure half. KiCad hands the action the path
// of one file and finds the sheets it places beside it; a browser hands the page the text of every file of the drop, which the server puts side by side (crates/cli/src/sheet_drop_api.rs) and
// answers with the file to import, the sheets that were not dropped and the files that are left out. Unit-tested in sheetDrop.test.ts.

/** A dropped schematic: its name and the text the page read. */
export interface DroppedFile {
  name: string;
  text: string;
}

/** The files a `ddAppendFile` event carries: `{ files: [..] }`, a list, or one `{ name, text }` (what the other drop actions take). */
export function droppedFilesOf(param: unknown): DroppedFile[] {
  const one = (v: unknown): DroppedFile[] => {
    if (!v || typeof v !== "object") return [];
    const o = v as { name?: unknown; text?: unknown };
    return typeof o.name === "string" && typeof o.text === "string" ? [{ name: o.name, text: o.text }] : [];
  };
  if (Array.isArray(param)) return param.flatMap(one);
  if (param && typeof param === "object" && Array.isArray((param as { files?: unknown }).files)) return (param as { files: unknown[] }).files.flatMap(one);
  return one(param);
}

/** What the server says of the drop. */
export interface DropAnswer {
  root?: string;
  missing?: Array<{ file: string; placed_by: string }>;
  left_out?: string[];
}

/**
 * What is said of a drop beyond opening Import Sheet on it: the sheets whose files were not dropped, each with the file that places it (what `LoadSheetFromFile` makes of a file read
 * without its sheets: they come in empty), and the files of the drop the imported one does not place (KiCad would run Import Sheet for each in turn; one is imported at a time here).
 * `null` when there is nothing to say.
 */
export function droppedSaid(r: DropAnswer): string | null {
  const parts: string[] = [];
  const missing = r.missing ?? [];
  if (missing.length > 0) {
    const list = missing.map((m) => `${m.file} (placed by ${m.placed_by})`).join(", ");
    parts.push(`Not in the drop: ${list}. ${missing.length === 1 ? "Its sheet comes" : "Their sheets come"} in empty.`);
  }
  const left = r.left_out ?? [];
  if (left.length > 0) parts.push(`Only ${r.root ?? "the first file"} is imported; ${left.join(", ")} ${left.length === 1 ? "is" : "are"} not placed by it (drop ${left.length === 1 ? "it on its own" : "them on their own"}).`);
  return parts.length > 0 ? parts.join(" ") : null;
}
