// Browser-side halves of the schematic control actions that need the server: the sheet tree and the derived `.kicad_sch` text. Kept apart from
// `client.ts` so the schematic control port does not grow that shared file.
import { ApiError } from "./client";

/** One sheet of the hierarchy (`GET /api/sch/hierarchy`): the placement ids from the root down (`[]` is the root), the placement's name and file, and its page (empty when none was set). */
export interface HierarchyEntry {
  path: string[];
  name: string;
  file: string;
  page: string;
}

async function getJson<T extends { ok?: boolean; message?: string; error?: string }>(url: string): Promise<T> {
  const r = await fetch(url, { cache: "no-store" });
  if (!r.ok) throw new ApiError(`${url}: HTTP ${r.status}`);
  const j = (await r.json()) as T;
  if (j.error) throw new ApiError(j.error);
  if (j.ok === false) throw new ApiError(j.message ?? `${url} failed`);
  return j;
}

export async function postJson<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  if (!r.ok) throw new ApiError(`${url}: HTTP ${r.status}`);
  return (await r.json()) as T;
}

/** Every sheet that has content, depth first, the root first (`SCH_SHEET_LIST::BuildSheetList`); `kicad-port/sheetPages.ts` sorts it by page. */
export async function fetchHierarchy(): Promise<HierarchyEntry[]> {
  const j = await getJson<{ ok: boolean; sheets: HierarchyEntry[] }>("/api/sch/hierarchy");
  return j.sheets;
}

/** The derived `.kicad_sch` files (`GET /api/schematic.kicad_sch`): the root sheet first (named `board.kicad_sch`), then one file per sheet screen. */
export async function fetchSchematicFiles(): Promise<Array<{ name: string; text: string }>> {
  const j = await getJson<{ files?: Array<{ name: string; text: string }>; error?: string }>("/api/schematic.kicad_sch");
  if (!j.files || j.files.length === 0) throw new ApiError("there is no schematic to save");
  return j.files;
}
