// What the library editors' Save, Save As, Save Library As and Revert ask the server (`crates/cli/src/library_save_api.rs`): which libraries of the tables can be
// written, which of the design's symbols and footprints differ from their library's file, what the file holds (for Revert), and the two writes -- Save into the
// path a table row names, Save Library As into a new file the same request puts in the board's table. Every write is the server's; the page never names a file
// the tables do not.
import type { LibraryFootprint, LibrarySymbol } from "./types";
import { ApiError } from "./client";
import { librariesChanged } from "./libraryTablesClient";

export type SaveKind = "symbol" | "footprint";

/** One library of the board's tables as Save As lists it. */
export interface WritableLibrary {
  name: string;
  path: string;
  scope: "global" | "project";
  description: string;
  /** `IsSymbolLibWritable` / `IsFootprintLibWritable`. */
  writable: boolean;
  hidden: boolean;
}

async function get<T extends { error?: string }>(url: string): Promise<T> {
  const r = await fetch(url, { cache: "no-store" });
  if (!r.ok) throw new ApiError(`${url}: HTTP ${r.status}`);
  const j = (await r.json()) as T;
  if (j.error) throw new ApiError(j.error);
  return j;
}

async function post<T>(url: string, body: unknown): Promise<T> {
  const r = await fetch(url, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  if (!r.ok) throw new ApiError(`${url}: HTTP ${r.status}`);
  return (await r.json()) as T;
}

export async function fetchWritableLibraries(kind: SaveKind): Promise<WritableLibrary[]> {
  return (await get<{ libraries: WritableLibrary[]; error?: string }>(`/api/library/writable?kind=${kind}`)).libraries;
}

/** The design's items of the table libraries that differ from the library's file (`IsSymbolModified`), and their libraries. */
export async function fetchModified(kind: SaveKind): Promise<{ items: string[]; libraries: string[] }> {
  const r = await get<{ items: string[]; libraries: string[]; error?: string }>(`/api/library/modified?kind=${kind}`);
  return { items: r.items, libraries: r.libraries };
}

/** The library file's symbol `id` (`Lib:Name`), or null when the file has none (it was never saved). */
export async function fetchSavedSymbol(id: string): Promise<LibrarySymbol | null> {
  return (await get<{ symbol: LibrarySymbol | null; error?: string }>(`/api/library/saved?kind=symbol&id=${encodeURIComponent(id)}`)).symbol;
}

/** Every design symbol of library `lib` with what its file holds of it (null: nothing). */
export async function fetchSavedLibrary(lib: string): Promise<Record<string, LibrarySymbol | null>> {
  return (await get<{ symbols: Record<string, LibrarySymbol | null>; error?: string }>(`/api/library/saved?kind=symbol&lib=${encodeURIComponent(lib)}`)).symbols;
}

export async function fetchSavedFootprint(id: string): Promise<LibraryFootprint | null> {
  return (await get<{ footprint: LibraryFootprint | null; error?: string }>(`/api/library/saved?kind=footprint&id=${encodeURIComponent(id)}`)).footprint;
}

export type SaveAnswer =
  | { ok: true; path: string; written: string[]; removed?: string[]; saved?: string[]; deleted?: string[]; message: string }
  | { ok?: undefined; error: string; detail?: string; read_only?: boolean };

/**
 * Save: a symbol library's modified symbols, or one footprint, into the file the table names, and what Delete and Rename took out of the library taken out of
 * the file (UI-LEFT-2 LL1); `{kind: "footprint", lib}` deletes the library's deleted footprints alone (`DeleteFootprintFromLibrary` writes at once).
 */
export async function saveToLibrary(req: { kind: "symbol"; lib: string } | { kind: "footprint"; id: string } | { kind: "footprint"; lib: string }): Promise<SaveAnswer> {
  const a = await post<SaveAnswer>("/api/library/save", req);
  if (a.ok && (a.written.length > 0 || (a.removed?.length ?? 0) > 0)) librariesChanged();
  return a;
}

export type RevertAnswer = { ok: true; reverted: string[]; gone: string[]; message: string } | { ok?: undefined; nothing: true; message: string } | { ok?: undefined; nothing?: undefined; error: string };

/**
 * Revert (`RevertSymbol`, `RevertLibrary`, `RevertFootprint`) and the reload of a library changed on disk (`UpdateLibraryBuffer`, `ReloadFootprint`): the server puts
 * the library's file back into the design as one undoable step and clears the modified flag (`POST /api/library/revert`).
 */
export async function revertInLibrary(req: { kind: "symbol"; id: string } | { kind: "symbol"; lib: string } | { kind: "footprint"; id: string }): Promise<RevertAnswer> {
  return post<RevertAnswer>("/api/library/revert", req);
}

/** `GetDerivedSymbolNames` over the library's buffer: the bare names of the symbols Delete takes with `id` (the design's and the library file's). */
export async function fetchDerivedNames(id: string): Promise<string[]> {
  return (await get<{ names: string[]; error?: string }>(`/api/library/derived?id=${encodeURIComponent(id)}`)).names;
}

/** The watched library of an editor's item as it is now (`crates/cli/src/library_watch.rs`); `path` null: nothing to watch. */
export interface WatchAnswer {
  path: string | null;
  stamp?: string | null;
  own?: boolean;
  steady?: boolean;
  modified?: boolean;
  dirty?: boolean;
}

export async function fetchWatch(kind: SaveKind, id: string): Promise<WatchAnswer> {
  return get<WatchAnswer & { error?: string }>(`/api/library/watch?kind=${kind}&id=${encodeURIComponent(id)}`);
}

export interface SaveLibraryAsRequest {
  kind: SaveKind;
  lib: string;
  /** The new file (symbols) or folder (footprints); may hold variables and is relative to the board's folder. */
  path: string;
  /** Symbols: "Add new project library table entry" or "Update existing library table entry". */
  table?: "project" | "replace";
  overwrite?: boolean;
  nickname?: string;
  descr?: string;
}

export type SaveLibraryAsAnswer =
  | { ok: true; path: string; nickname: string; uri?: string; copied?: string[]; message: string }
  | { ok?: undefined; error: string; detail?: string; confirm?: string; path?: string; nickname?: string };

export async function saveLibraryAs(req: SaveLibraryAsRequest): Promise<SaveLibraryAsAnswer> {
  const a = await post<SaveLibraryAsAnswer>("/api/library/save_as", req);
  if (a.ok || a.nickname) librariesChanged();
  return a;
}

/** One library's item names as its file has them now (no cache: Save As asks whether a name is taken). */
export async function fetchLibraryItemsNow(kind: SaveKind, lib: string): Promise<string[]> {
  try {
    return (await get<{ names: string[]; error?: string }>(`/api/library/items?kind=${kind}&lib=${encodeURIComponent(lib)}`)).names;
  } catch {
    return []; // a library made in the dialog and not read yet, or the project library: nothing in a file
  }
}
