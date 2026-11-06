// Page Settings' drawing sheet file on the server (`crates/cli/src/drawing_sheet_api.rs`): a picked `.kicad_wks` read with the name it is kept under, where the
// dialog's browser starts, and a file from the person's computer written into the board's folder.

/** A picked sheet: the name to keep (relative to the board's folder when it is in it), where it is, and its text; `error` is KiCad's "Error loading drawing sheet". */
export interface PickedSheet {
  ok: boolean;
  path?: string;
  file?: string;
  text?: string | null;
  error?: string;
}

async function getJson<T>(url: string, body?: unknown): Promise<T> {
  const r = await fetch(url, body === undefined ? { cache: "no-store" } : { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  if (!r.ok) return { ok: false, error: `${url}: HTTP ${r.status}` } as T;
  return (await r.json()) as T;
}

/** `GET /api/drawing_sheet?path=`: the sheet a name or path points at. */
export function pickDrawingSheet(path: string): Promise<PickedSheet> {
  return getJson<PickedSheet>(`/api/drawing_sheet?path=${encodeURIComponent(path)}`);
}

/** `GET /api/drawing_sheet/folders`: the board's folder and KiCad's template folder (null when it is not on this machine). */
export function drawingSheetFolders(): Promise<{ project: string; templates: string | null }> {
  return getJson(`/api/drawing_sheet/folders`);
}

/** `POST /api/drawing_sheet/upload`: a `.kicad_wks` from the person's computer, into the board's folder. */
export function uploadDrawingSheet(name: string, text: string, overwrite = false): Promise<PickedSheet> {
  return getJson<PickedSheet>(`/api/drawing_sheet/upload`, { name, text, overwrite });
}
