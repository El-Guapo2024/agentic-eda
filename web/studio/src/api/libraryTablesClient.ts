// What the library table dialogs ask the server (`crates/cli/src/library_tables.rs`): the tables of both kinds and the variables, the Project tab's OK, the checks of
// the rows as they stand in the grid, Add Existing Library and New Library, the board's own path variables (Configure Paths), and the folder listing the browse buttons use
// in place of a native file dialog. The global tables are KiCad's and are only read -- there is no call here that could write one.
import type { TableKind, TableRow, TableScope, VariableValue } from "../kicad-port/libraryTable";
import { ApiError } from "./client";
import { resetChooserCaches } from "./libraryChooserClient";
import { resetInstalledLibraries } from "./libraryIndexClient";

/** One table as the server read it: the file, whether it is there, whether the studio may write it, and its rows (a chained table's own rows below its row). */
export interface TableScopeView {
  scope: TableScope;
  path: string;
  exists: boolean;
  writable: boolean;
  /** The global table only: where the table the studio uses came from (`config`: KiCad's own; `stock`: the one KiCad installs; `folder`: the library files; `missing`). */
  origin?: "config" | "stock" | "folder" | "missing";
  /** The global table only: what the person should know about it. */
  note?: string;
  /** Why the file could not be read. */
  error: string | null;
  rows: TableRow[];
}

export interface KindView {
  kind: TableKind;
  /** `sym-lib-table` / `fp-lib-table`. */
  file: string;
  /** The symbol table has a Show column; the footprint table does not. */
  supports_visibility: boolean;
  /** The library formats the Library Format column offers. */
  types: string[];
  /** `New_Library`, or the first free `New_Library_<n>`. */
  new_library_name: string;
  global: TableScopeView;
  project: TableScopeView;
}

/** One of KiCad's own variables (the Configure Paths rows the person cannot change here). */
export interface GlobalVariable {
  name: string;
  value: string;
  /** Defined in the real environment, which takes precedence over KiCad's settings. */
  external: boolean;
  /** Still the value KiCad gave it. */
  default: boolean;
  predefined: boolean;
}

export interface LibraryTablesView {
  project_dir: string;
  config_dir: string | null;
  version: string | null;
  major: number;
  tables: Record<TableKind, KindView>;
  variables: {
    global: GlobalVariable[];
    project: VariableValue[];
    /** Used by a library path and defined by nobody, with what the environment says. */
    undefined: { name: string; value: string; external: boolean }[];
    reserved: string[];
    file: string;
  };
}

/** A request the server refused: the message, and for an edit of a table or of the variables, the row and the column to put the cursor in. */
export interface Refusal {
  error: string;
  row?: number;
  column?: string;
}

export type Answer = ({ ok: true } & LibraryTablesView) | ({ ok?: false } & Refusal);

async function send<T>(url: string, body?: unknown): Promise<T> {
  const r = await fetch(url, body === undefined ? { cache: "no-store" } : { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  if (!r.ok) throw new ApiError(`${url}: HTTP ${r.status}`);
  return (await r.json()) as T;
}

export async function fetchLibraryTables(): Promise<LibraryTablesView> {
  const v = await send<LibraryTablesView & { error?: string }>("/api/library_tables");
  if (v.error) throw new ApiError(v.error);
  return v;
}

/** The library tables changed: every list the page kept of what the server has is stale, and the trees on screen ask again. */
export function librariesChanged(): void {
  resetChooserCaches();
  resetInstalledLibraries();
}

function settle(a: Answer): Answer {
  if ("ok" in a && a.ok) librariesChanged();
  return a;
}

/** The Project tab's OK: the rows go through KiCad's checks and, if they pass, into the board's `sym-lib-table` / `fp-lib-table`. */
export async function saveLibraryTable(kind: TableKind, rows: readonly TableRow[]): Promise<Answer> {
  return settle(await send<Answer>("/api/library_tables", { kind, rows: rows.map(wireRow) }));
}

function wireRow(r: TableRow) {
  return { nickname: r.nickname, uri: r.uri, type: r.type, options: r.options, descr: r.descr, enabled: r.enabled, visible: r.visible };
}

export interface RowCheck {
  error: string;
  path: string;
  guess: string | null;
}

/** `CheckTableRow` for each row as it stands in the grid: where its path leads and why it cannot be read. */
export async function checkLibraryRows(kind: TableKind, rows: readonly TableRow[]): Promise<RowCheck[]> {
  const r = await send<{ results?: RowCheck[]; error?: string }>("/api/library_tables/check", { kind, rows: rows.map(wireRow) });
  if (r.error || !r.results) throw new ApiError(r.error ?? "no answer");
  return r.results;
}

/** A path the way a table holds it (`NormalizePath`): `${KICAD10_SYMBOL_DIR}/Device.kicad_sym`, `${KIPRJMOD}/libs/Mine.pretty`. */
export async function normalizeLibraryPath(path: string): Promise<{ uri: string; path: string }> {
  const r = await send<{ uri?: string; path?: string; error?: string }>(`/api/library_tables/normalize?path=${encodeURIComponent(path)}`);
  if (r.error || r.uri === undefined) throw new ApiError(r.error ?? "no answer");
  return { uri: r.uri, path: r.path ?? path };
}

export interface AddRequest {
  kind: TableKind;
  /** The file or folder (variables allowed, relative to the board's folder). */
  path: string;
  /** New Library: make an empty one. */
  create?: boolean;
  nickname?: string;
  descr?: string;
  /** Add a library table (a row of type `Table`) rather than a library. */
  table?: boolean;
}

export type AddAnswer = ({ ok: true; added: { kind: TableKind; nickname: string; uri: string; path: string; created: boolean } } & LibraryTablesView) | ({ ok?: false } & Refusal);

/** Add Existing Library, and New Library when `create` is set: a row in the board's table, never in the global one. */
export async function addLibrary(req: AddRequest): Promise<AddAnswer> {
  const a = await send<AddAnswer>("/api/library_tables/add", req);
  if ("ok" in a && a.ok) librariesChanged();
  return a;
}

/** Configure Paths' OK: the board's own variables (KiCad's cannot be changed here). */
export async function savePathVariables(variables: readonly VariableValue[]): Promise<Answer> {
  return settle(await send<Answer>("/api/path_variables", { variables }));
}

export interface FolderEntry {
  name: string;
  dir: boolean;
  /** A `.pretty` folder: it is the footprint library. */
  library: boolean;
}

export interface FolderListing {
  path: string;
  parent: string | null;
  home: string | null;
  project: string;
  entries: FolderEntry[];
  truncated: boolean;
}

/** What the browse buttons list: the folders of `path` and the files `what` asks for (`symbol`: .kicad_sym, `table`: a library table; folders only for `footprint` and `folder`). */
export async function browseFolder(path: string, what: "symbol" | "footprint" | "table" | "folder"): Promise<FolderListing> {
  const r = await send<FolderListing & { error?: string }>(`/api/fs/list?path=${encodeURIComponent(path)}&what=${what}`);
  if (r.error) throw new ApiError(r.error);
  return r;
}
