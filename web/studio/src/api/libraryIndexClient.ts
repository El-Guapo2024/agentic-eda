// The installed KiCad libraries for the two library trees (`crates/cli/src/library_index.rs`): the library NAMES first, one library's item names when the
// person expands it, and every library's names only when the tree's search box needs them. KiCad.app's footprint libraries (155) and symbol libraries
// (223, 220 MB) are far too large to ship whole, so each answer is asked once per page load and kept (the server keeps its own cache too): a collapsed
// library costs nothing, an expanded one costs one small request, and expanding it again -- or leaving the tab and coming back -- costs none.
import { ApiError } from "./client";

export type LibraryKind = "footprint" | "symbol";

export interface InstalledLibrary {
  name: string;
  /** How many items it holds, when the server knew without opening it (footprint libraries: a directory listing). */
  count?: number;
}

async function get<T extends { error?: string }>(url: string): Promise<T> {
  const r = await fetch(url, { cache: "no-store" });
  if (!r.ok) throw new ApiError(`${url}: HTTP ${r.status}`);
  const j = (await r.json()) as T;
  if (j.error) throw new ApiError(j.error);
  return j;
}

/** Ask once and keep the answer; a failed ask is forgotten so the next call retries. */
function once<K, V>(cache: Map<K, Promise<V>>, key: K, load: () => Promise<V>): Promise<V> {
  const hit = cache.get(key);
  if (hit) return hit;
  const p = load();
  cache.set(key, p);
  p.catch(() => {
    if (cache.get(key) === p) cache.delete(key);
  });
  return p;
}

const libraryLists = new Map<LibraryKind, Promise<InstalledLibrary[]>>();
const itemLists = new Map<string, Promise<string[]>>();
const everything = new Map<LibraryKind, Promise<Record<string, string[]>>>();
// The same answers once they are in, readable synchronously: a tree that remounts (the person left the tab and came back) draws what it already
// knew on its first render instead of flashing every library closed again.
const knownLibraries = new Map<LibraryKind, InstalledLibrary[]>();
const knownItems = new Map<string, string[]>();
const knownEverything = new Map<LibraryKind, Record<string, string[]>>();

/** The installed libraries' names (and counts where cheap), sorted; empty when KiCad is not installed. */
export function fetchInstalledLibraries(kind: LibraryKind): Promise<InstalledLibrary[]> {
  return once(libraryLists, kind, async () => {
    const libs = (await get<{ libraries: InstalledLibrary[]; error?: string }>(`/api/library/index?kind=${kind}`)).libraries;
    knownLibraries.set(kind, libs);
    return libs;
  });
}

/** The bare item names of one installed library (`Lib` -> `["R", "C", ...]`; the tree key is `Lib:Name`). */
export function fetchInstalledItems(kind: LibraryKind, lib: string): Promise<string[]> {
  return once(itemLists, `${kind}/${lib}`, async () => {
    const names = (await get<{ names: string[]; error?: string }>(`/api/library/items?kind=${kind}&lib=${encodeURIComponent(lib)}`)).names;
    knownItems.set(`${kind}/${lib}`, names);
    return names;
  });
}

/** Every installed library's item names, for the search box. The server builds this on a thread of its own and answers `pending` until it is done. */
export function fetchAllInstalledItems(kind: LibraryKind, pollMs = 600): Promise<Record<string, string[]>> {
  return once(everything, kind, async () => {
    for (;;) {
      const r = await get<{ status: "pending" | "ready"; libraries?: Record<string, string[]>; error?: string }>(`/api/library/all?kind=${kind}`);
      if (r.status === "ready" && r.libraries) {
        knownEverything.set(kind, r.libraries);
        return r.libraries;
      }
      await new Promise((resolve) => setTimeout(resolve, pollMs));
    }
  });
}

/** What the page has already been told, or `undefined` (synchronous: for a tree's first render). */
export function knownInstalledLibraries(kind: LibraryKind): InstalledLibrary[] | undefined {
  return knownLibraries.get(kind);
}
/** Whether every library's names are in already (the search index was built). */
export function knownSearchIndex(kind: LibraryKind): boolean {
  return knownEverything.has(kind);
}
export function knownInstalledItems(kind: LibraryKind): Map<string, string[]> {
  const out = new Map<string, string[]>();
  for (const [key, names] of knownItems) if (key.startsWith(`${kind}/`)) out.set(key.slice(kind.length + 1), names);
  for (const [lib, names] of Object.entries(knownEverything.get(kind) ?? {})) out.set(lib, names);
  return out;
}

/** The tree key of an installed item: `Lib:Name`. */
export function installedKey(lib: string, item: string): string {
  return `${lib}:${item}`;
}
