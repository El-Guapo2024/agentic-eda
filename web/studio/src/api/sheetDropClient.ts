// The browser half of a schematic file dropped on the sheet (`eeschema.EditorControl.ddAppendFile`; crates/cli/src/sheet_drop_api.rs): the files of the drop are posted as the text the page
// read, because a browser hands the page a file's contents and not its path, and the server puts them side by side so that Import Sheet can read the file with the sheets it places.

/** `POST /api/sch/dropped_sheets`'s reply. */
export interface DroppedSheets {
  ok: boolean;
  message?: string;
  /** The file to import, on the server. */
  path?: string;
  /** The dropped file that no other places: the one imported. */
  root?: string;
  /** Every file written, relative to the folder of the one imported. */
  files?: string[];
  /** The sheets whose files were not dropped, with the file that places each (the check `LoadSheetFromFile` makes of a file read without its sheets). */
  missing?: Array<{ file: string; placed_by: string }>;
  /** Files of the drop the imported one does not place: not imported. */
  left_out?: string[];
}

export async function putDroppedSheets(files: Array<{ name: string; text: string }>): Promise<DroppedSheets> {
  const r = await fetch("/api/sch/dropped_sheets", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ files }) });
  return (await r.json()) as DroppedSheets;
}
