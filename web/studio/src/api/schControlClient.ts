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

/** One of KiCad's legacy BOM generator scripts (`GET /api/sch/bom_plugins`, `BOM_GENERATOR_HANDLER`): its name in the list, the header it carries, the command it runs. */
export interface BomPlugin {
  name: string;
  file: string;
  kind: string;
  info: string;
  command: string;
  ext: string;
}

export async function fetchBomPlugins(): Promise<{ dir: string; plugins: BomPlugin[] }> {
  const j = await getJson<{ ok: boolean; dir: string; plugins: BomPlugin[] }>("/api/sch/bom_plugins");
  return { dir: j.dir, plugins: j.plugins };
}

/** `POST /api/sch/bom_legacy`: what `DIALOG_BOM`'s Generate did -- the files written, the generator's own messages and, when it made one, the output text. */
export interface LegacyBomReply {
  ok: boolean;
  message?: string;
  files?: string[];
  messages?: string;
  command?: string;
  engine?: string;
  output?: { name: string; text: string };
}

export function postBomLegacy(plugin: string): Promise<LegacyBomReply> {
  return postJson("/api/sch/bom_legacy", { plugin });
}

/** `POST /api/sch/export_symbols`: the library symbols the schematic uses, as the text of one `.kicad_sym`. */
export interface ExportSymbolsReply {
  ok: boolean;
  message?: string;
  text?: string;
  ids?: string[];
  skipped?: string[];
  clashes?: string[];
}

export function postExportSymbols(includePower: boolean): Promise<ExportSymbolsReply> {
  return postJson("/api/sch/export_symbols", { include_power: includePower });
}

/** `POST /api/sym/svg`: `kicad-cli sym export svg` on one symbol; `svg` is the unit and body style asked for. */
export interface SymbolSvgReply {
  ok: boolean;
  message?: string;
  files?: string[];
  engine?: string;
  svg?: { name: string; text: string };
}

export function postSymbolSvg(req: { lib_id: string; unit: number; body_style: number }): Promise<SymbolSvgReply> {
  return postJson("/api/sym/svg", req);
}

/** `POST /api/sch/bom`: the Fields Table's Export tab through `kicad-cli sch export bom`; `text` is the file that was written. */
export interface BomFileReply {
  ok: boolean;
  message?: string;
  files?: string[];
  engine?: string;
  text?: string;
}

export function postBomFile(body: { spec: unknown; fmt: unknown; path?: string }): Promise<BomFileReply> {
  return postJson("/api/sch/bom", body);
}

/** The derived `.kicad_sch` files (`GET /api/schematic.kicad_sch`): the root sheet first (named `board.kicad_sch`), then one file per sheet screen. */
export async function fetchSchematicFiles(): Promise<Array<{ name: string; text: string }>> {
  const j = await getJson<{ files?: Array<{ name: string; text: string }>; error?: string }>("/api/schematic.kicad_sch");
  if (!j.files || j.files.length === 0) throw new ApiError("there is no schematic to save");
  return j.files;
}
