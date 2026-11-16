// The installed KiCad libraries for the two library trees (`crates/cli/src/library_index.rs`): the library NAMES first, one library's item names when the
// person expands it, and every library's names only when the tree's search box needs them. KiCad.app's footprint libraries (155) and symbol libraries
// (223, 220 MB) are far too large to ship whole, so each answer is asked once per page load and kept (the server keeps its own cache too): a collapsed
// library costs nothing, an expanded one costs one small request, and expanding it again -- or leaving the tab and coming back -- costs none.
import { clearLoadError, reportLoadErrors } from "../state/libraryLoadErrors";
import { ApiError } from "./client";

export type LibraryKind = "footprint" | "symbol";

export interface InstalledLibrary {
  name: string;
  /** How many items it holds, when the server knew without opening it (footprint libraries: a directory listing). */
  count?: number;
  /** What KiCad's library table says it is (the choosers' Description column of a library row). */
  description?: string;
  /** The table the library is in: KiCad's own (`global`) or the board's (`project`). */
  scope?: "global" | "project";
  /** A symbol library that is a folder (`.kicad_symdir`): KiCad 25ec2335 offers it Save Library As Packed, a file library Save Library As Unpacked. */
  unpacked?: boolean;
  /** An HTTP library's categories (`SCH_IO_HTTP_LIB::GetSubLibraryNames`, `GetSubLibraryDescription`), which the Symbol Chooser lists as libraries of their own (UI-LEFT-3 L3-3). */
  sublibraries?: { name: string; description: string }[];
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

const resetListeners = new Set<() => void>();

/**
 * Forget every answer: the library tables changed (a library was added, a path edited, a variable defined), so what the server lists is not what was asked
 * before. The next ask goes to the server; the trees that are on screen are told (`onInstalledLibrariesReset`) to ask again.
 */
export function resetInstalledLibraries(): void {
  libraryLists.clear();
  itemLists.clear();
  everything.clear();
  knownLibraries.clear();
  knownItems.clear();
  knownEverything.clear();
  for (const l of [...resetListeners]) l();
}

/** Called when the answers were forgotten (`resetInstalledLibraries`); returns the unsubscribe. */
export function onInstalledLibrariesReset(listener: () => void): () => void {
  resetListeners.add(listener);
  return () => resetListeners.delete(listener);
}

/** The installed libraries' names (and counts where cheap), sorted; empty when KiCad is not installed. */
export function fetchInstalledLibraries(kind: LibraryKind): Promise<InstalledLibrary[]> {
  return once(libraryLists, kind, async () => {
    const libs = (await get<{ libraries: InstalledLibrary[]; error?: string }>(`/api/library/index?kind=${kind}`)).libraries;
    knownLibraries.set(kind, libs);
    return libs;
  });
}

type ItemsAnswer = { names: string[]; load_error?: string; error?: string; verdict?: "pending" };

const itemListeners = new Set<(kind: LibraryKind, lib: string, names: string[]) => void>();

/** Told when a library's item names changed after they were first given (KiCad's parser's verdict landed); returns the unsubscribe. */
export function onInstalledItemsUpdated(listener: (kind: LibraryKind, lib: string, names: string[]) => void): () => void {
  itemListeners.add(listener);
  return () => itemListeners.delete(listener);
}

/** How often, and how long, a library whose KiCad verdict is still to come is asked again. */
export const VERDICT_POLL_MS = 1000;
export const VERDICT_POLL_LIMIT_MS = 180000;

function takeItems(kind: LibraryKind, lib: string, r: ItemsAnswer): string[] {
  // a folder library with a file that did not read: its names are listed, and the status bar warns (`GetLibraryLoadErrors`); a footprint library's file the
  // reader refuses is left out and said the same way (`FP_CACHE::Load`, UI-LEFT-2 LL6)
  if (r.load_error) reportLoadErrors([{ lib, error: r.load_error }]);
  else clearLoadError(lib);
  knownItems.set(`${kind}/${lib}`, r.names);
  return r.names;
}

/**
 * A library whose KiCad verdict is still to come (`"verdict": "pending"`: the server's parts indexer asks kicad-cli on a thread of its own, never on the request
 * loop): asked again until the verdict has landed, then its names are the kept answer and the trees are told (`onInstalledItemsUpdated`).
 */
async function followVerdict(kind: LibraryKind, lib: string, first: string[]): Promise<void> {
  const key = `${kind}/${lib}`;
  const t0 = Date.now();
  while (Date.now() - t0 < VERDICT_POLL_LIMIT_MS) {
    await new Promise((resolve) => setTimeout(resolve, VERDICT_POLL_MS));
    if (!itemLists.has(key)) return; // forgotten (the tables changed): the next ask starts over
    let r: ItemsAnswer;
    try {
      r = await get<ItemsAnswer>(`/api/library/items?kind=${kind}&lib=${encodeURIComponent(lib)}`);
    } catch {
      continue;
    }
    if (r.verdict === "pending") continue;
    const names = takeItems(kind, lib, r);
    itemLists.set(key, Promise.resolve(names));
    if (names.join("\u0001") !== first.join("\u0001")) for (const l of [...itemListeners]) l(kind, lib, names);
    return;
  }
}

/** The bare item names of one installed library (`Lib` -> `["R", "C", ...]`; the tree key is `Lib:Name`). */
export function fetchInstalledItems(kind: LibraryKind, lib: string): Promise<string[]> {
  return once(itemLists, `${kind}/${lib}`, async () => {
    const r = await get<ItemsAnswer>(`/api/library/items?kind=${kind}&lib=${encodeURIComponent(lib)}`);
    const names = takeItems(kind, lib, r);
    if (r.verdict === "pending") void followVerdict(kind, lib, names);
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
