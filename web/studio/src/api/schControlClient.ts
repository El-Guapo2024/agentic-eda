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

/**
 * The Hierarchy Navigator's read of the tree: the sheets as `fetchHierarchy` lists them, whether the root is KiCad 10's virtual root (its sheets are the project's
 * top-level ones), and with `net` each sheet's `net`: whether its screen has that net (`HIERARCHY_PANE::UpdateNetHighlight`).
 */
export async function fetchHierarchyTree(net?: string | null): Promise<{ sheets: Array<HierarchyEntry & { net?: boolean }>; virtualRoot: boolean }> {
  const j = await getJson<{ ok: boolean; sheets: Array<HierarchyEntry & { net?: boolean }>; virtual_root?: boolean }>(`/api/sch/hierarchy${net ? `?net=${encodeURIComponent(net)}` : ""}`);
  return { sheets: j.sheets, virtualRoot: !!j.virtual_root };
}

/** Every sheet that has content, depth first, the root first (`SCH_SHEET_LIST::BuildSheetList`); `kicad-port/sheetPages.ts` sorts it by page. */
export async function fetchHierarchy(): Promise<HierarchyEntry[]> {
  const j = await getJson<{ ok: boolean; sheets: HierarchyEntry[] }>("/api/sch/hierarchy");
  return j.sheets;
}

/**
 * One placed symbol as one sheet path shows it (`GET /api/sch/symbols`, `SCH_SHEET_LIST::GetSymbols`): `id` and `unit` tell it apart on its screen (`file`), `reference`
 * and `path_unit` are the path's own (`GetRef( &path )`, `GetUnitSelection( &path )`; a screen shown by several sheets lists its symbols once per path).
 */
export interface SymbolPlacement {
  sheet: string;
  file: string;
  id: string;
  unit: number;
  reference: string;
  path_unit: number;
  lib_id: string;
  value: string;
}

/** Every placed symbol of the hierarchy, a row per sheet path that shows it, in walk order (Change Symbols' and Update Symbols' matching, UI-LEFT S9). */
export async function fetchSymbolPlacements(): Promise<SymbolPlacement[]> {
  const j = await getJson<{ ok: boolean; symbols: SymbolPlacement[] }>("/api/sch/symbols");
  return j.symbols;
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

export function postSymbolSvg(req: { lib_id: string; unit: number; body_style: number; theme?: string; include_hidden_pins?: boolean; include_hidden_fields?: boolean; black_and_white?: boolean }): Promise<SymbolSvgReply> {
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

/** `variant`: the design variant the BOM is for (KiCad 25ec2335 the table's current variant, kicad-cli's `sch export bom --variant`); absent for the default. */
export function postBomFile(body: { spec: unknown; fmt: unknown; path?: string; variant?: string }): Promise<BomFileReply> {
  return postJson("/api/sch/bom", body);
}

/** The derived `.kicad_sch` files (`GET /api/schematic.kicad_sch`): the root sheet first (named `board.kicad_sch`), then one file per sheet screen. */
export async function fetchSchematicFiles(): Promise<Array<{ name: string; text: string }>> {
  const j = await getJson<{ files?: Array<{ name: string; text: string }>; error?: string }>("/api/schematic.kicad_sch");
  if (!j.files || j.files.length === 0) throw new ApiError("there is no schematic to save");
  return j.files;
}
