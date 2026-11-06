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
 * What is said of a drop beyond placing it: the sheets whose files were not dropped, each with the file that places it (what `LoadSheetFromFile` makes of a file read without its sheets:
 * they come in empty), and, for a drop of several files, that each is placed in turn (`DoWithAcceptedFiles`). `null` when there is nothing to say.
 */
export function droppedSaid(r: DropAnswer & { paths?: string[] }): string | null {
  const parts: string[] = [];
  const missing = r.missing ?? [];
  if (missing.length > 0) {
    const list = missing.map((m) => `${m.file} (placed by ${m.placed_by})`).join(", ");
    parts.push(`Not in the drop: ${list}. ${missing.length === 1 ? "Its sheet comes" : "Their sheets come"} in empty.`);
  }
  const n = r.paths?.length ?? 0;
  if (n > 1) parts.push(`${n} schematics dropped: each is placed in turn.`);
  return parts.length > 0 ? parts.join(" ") : null;
}

/**
 * The files of a drop still to place (`EDA_BASE_FRAME::DoWithAcceptedFiles`: `RunAction( importSheet, file )` for each accepted file in turn): the first is in the hand, the
 * rest wait here until it is placed or dropped (UI-LEFT S17).
 */
let queued: string[] = [];

export function queueDropped(paths: readonly string[]): void {
  queued = [...paths];
}

/** The next file of the drop to place, taken off the queue; null when none is left. */
export function nextDropped(): string | null {
  return queued.shift() ?? null;
}

export function droppedLeft(): number {
  return queued.length;
}
