// What the Symbol and Footprint Choosers ask the server (`crates/cli/src/library_search.rs`, `library_api.rs`): one installed library's items with their
// descriptions when it is opened, the search over every installed library, the one selected item's drawing, and the project's own symbols. The installed
// libraries are 220 MB of symbols and 179 MB of footprints, so nothing here asks for a library whole: items as names and descriptions, a drawing for
// the selected item only. Each answer is kept for the page's life (the server keeps its own cache too).
import type { ChooserItem, ChooserKind } from "../kicad-port/libChooser";
import { splitId } from "../kicad-port/libChooser";
import { withDefaults } from "../kicad-port/libraryDefaults";
import { clearLoadError, refreshLoadErrors, reportLoadErrors } from "../state/libraryLoadErrors";
import { ApiError } from "./client";
import type { LibraryFootprint, LibrarySymbol } from "./types";

async function get<T extends { error?: string }>(url: string, signal?: AbortSignal): Promise<T> {
  const r = await fetch(url, { cache: "no-store", signal });
  if (!r.ok) throw new ApiError(`${url}: HTTP ${r.status}`);
  const j = (await r.json()) as T;
  if (j.error) throw new ApiError(j.error);
  return j;
}

/** What narrows the items: a footprint chooser's pin count and footprint filters (`FOOTPRINT_CHOOSER_FRAME::filterFootprint`); a symbol chooser leaves power symbols out. */
export interface ChooserFilter {
  /** Only footprints with exactly this many numbered pads. */
  pins?: number;
  /** Only footprints one of these wildcard patterns matches (the symbol's `ki_fp_filters`). */
  fpFilters?: string[];
  excludePower?: boolean;
  /** Place Power's chooser: only power symbols (`SYMBOL_LIBRARY_FILTER::FilterPowerSymbols( true )`). */
  onlyPower?: boolean;
}

function filterQuery(f: ChooserFilter | undefined): string {
  if (!f) return "";
  let q = "";
  if (f.pins && f.pins > 0) q += `&pins=${f.pins}`;
  if (f.fpFilters && f.fpFilters.length > 0) q += `&fp_filters=${encodeURIComponent(f.fpFilters.join(" "))}`;
  if (f.excludePower) q += "&power=exclude";
  else if (f.onlyPower) q += "&power=only";
  return q;
}

interface WireItem {
  name: string;
  description?: string;
  units?: number;
  pins?: number;
  pads?: number;
  power?: boolean;
  score?: number;
  /** A symbol's chooser fields (`LIB_SYMBOL::GetChooserFields`): the values of the library tree's extra columns. */
  fields?: Record<string, string>;
  /** An HTTP library's part: its category (its `LIB_ID`'s sub-library name). */
  sublib?: string;
}

function toItem(lib: string, w: WireItem): ChooserItem {
  return { id: `${lib}:${w.name}`, lib, name: w.name, description: w.description ?? "", units: w.units, pins: w.pins ?? w.pads, power: w.power, score: w.score, ...(w.fields ? { fields: w.fields } : {}), ...(w.sublib !== undefined ? { sublib: w.sublib } : {}) };
}

const entriesCache = new Map<string, Promise<ChooserItem[]>>();

/** One installed library's items (`GET /api/library/entries`), asked once per library and filter. */
export function fetchEntries(kind: ChooserKind, lib: string, filter?: ChooserFilter): Promise<ChooserItem[]> {
  const key = `${kind}\u0001${lib}\u0001${filterQuery(filter)}`;
  const hit = entriesCache.get(key);
  if (hit) return hit;
  const p = get<{ entries: WireItem[]; load_error?: string; error?: string }>(`/api/library/entries?kind=${kind}&lib=${encodeURIComponent(lib)}${filterQuery(filter)}`).then((r) => {
    // a library read with errors lists the symbols that read; the warning says the rest (`GetLibraryLoadErrors`)
    if (kind === "symbol") {
      if (r.load_error) reportLoadErrors([{ lib, error: r.load_error }]);
      else clearLoadError(lib);
    }
    return r.entries.map((w) => toItem(lib, w));
  });
  entriesCache.set(key, p);
  p.catch(() => {
    if (entriesCache.get(key) === p) entriesCache.delete(key);
  });
  return p;
}

const allEntriesCache = new Map<string, Promise<ReadonlyMap<string, ChooserItem[]>>>();

/**
 * Every installed library's items with their descriptions and fields (`GET /api/library/entries_all`), for the library editors' trees (UI-LEFT-3 L3-5: KiCad's
 * `LIB_TREE_MODEL_ADAPTER` is filled with them when the tree is built). The server reads the libraries in the background: `onProgress` gets the ones read so
 * far until it has read them all. Asked once per kind for the page's life.
 */
export function fetchAllEntries(kind: ChooserKind, onProgress?: (libs: ReadonlyMap<string, ChooserItem[]>) => void): Promise<ReadonlyMap<string, ChooserItem[]>> {
  const hit = allEntriesCache.get(kind);
  if (hit) return hit;
  const p = (async () => {
    for (;;) {
      const r = await get<{ status: "ready" | "indexing"; libraries: Record<string, WireItem[]>; error?: string }>(`/api/library/entries_all?kind=${kind}`);
      const libs = new Map(Object.entries(r.libraries).map(([lib, items]) => [lib, items.map((w) => toItem(lib, w))] as const));
      onProgress?.(libs);
      if (r.status === "ready") return libs;
      await new Promise((res) => setTimeout(res, 1000));
    }
  })();
  allEntriesCache.set(kind, p);
  p.catch(() => {
    if (allEntriesCache.get(kind) === p) allEntriesCache.delete(kind);
  });
  return p;
}

export interface SearchLibrary {
  name: string;
  description?: string;
  score: number;
  items: ChooserItem[];
}

export interface SearchAnswer {
  /** `indexing` while the server is still reading the libraries: `indexed` of `total` are searched. */
  status: "ready" | "indexing";
  indexed: number;
  total: number;
  matches: number;
  truncated: boolean;
  libraries: SearchLibrary[];
  /** With `onlyPower`: the libraries read so far that hold a power symbol (the power chooser's tree shows only those). */
  powerLibraries?: string[];
}

/** The search over every installed library (`GET /api/library/search`): the best matches, grouped by library, best first. */
/**
 * The server's search of the installed libraries. `columns`: the tree's shown columns (Select Columns...), whose fields are search terms of weight 4
 * (`LIB_TREE_NODE::RebuildSearchTerms`; UI-LEFT-2 LL4); left out, the server takes the default, Item and Description.
 */
export async function searchInstalled(kind: ChooserKind, query: string, filter?: ChooserFilter, signal?: AbortSignal, columns?: readonly string[]): Promise<SearchAnswer> {
  const r = await get<{
    status: "ready" | "indexing";
    indexed: number;
    total: number;
    matches: number;
    truncated: boolean;
    libraries: { name: string; description?: string; score: number; items: WireItem[] }[];
    power_libraries?: string[];
    load_errors?: { lib: string; error: string }[];
    error?: string;
  }>(`/api/library/search?kind=${kind}&q=${encodeURIComponent(query)}${filterQuery(filter)}${columns && columns.length > 0 ? `&cols=${encodeURIComponent(columns.join("\t"))}` : ""}`, signal);
  if (r.load_errors) reportLoadErrors(r.load_errors);
  return {
    status: r.status,
    indexed: r.indexed,
    total: r.total,
    matches: r.matches,
    truncated: r.truncated,
    libraries: r.libraries.map((l) => ({ name: l.name, description: l.description, score: l.score, items: l.items.map((w) => toItem(l.name, w)) })),
    ...(r.power_libraries ? { powerLibraries: r.power_libraries } : {}),
  };
}

/** Start the server's indexing of every library of `kind` (a search with no text), so the first real search finds it done. */
export function warmUpSearch(kind: ChooserKind): void {
  void searchInstalled(kind, "").catch(() => {
    /* no server answer: the chooser asks again when it searches */
  });
}

/** An installed symbol with the fields the description pane and the footprint line show, and its drawing. */
export interface SymbolDetails {
  id: string;
  description: string;
  keywords: string;
  reference: string;
  value: string;
  /** The default footprint (`Lib:Name`), empty for most symbols. */
  footprint: string;
  datasheet: string;
  /** `ki_fp_filters`, space separated. */
  fp_filters: string;
  units: number;
  pins: number;
  power: boolean;
  extends: string | null;
  alternate_body_style: boolean;
  symbol: LibrarySymbol;
}

export interface FootprintDetails {
  id: string;
  description: string;
  tags: string;
  pads: number;
  numbered_pads: number;
  footprint: LibraryFootprint;
}

const MAX_KEPT = 120;

function remember<T>(cache: Map<string, Promise<T>>, key: string, load: () => Promise<T>): Promise<T> {
  const hit = cache.get(key);
  if (hit) return hit;
  const p = load();
  cache.set(key, p);
  if (cache.size > MAX_KEPT) cache.delete(cache.keys().next().value as string);
  p.catch(() => {
    if (cache.get(key) === p) cache.delete(key);
  });
  return p;
}

const symbolDetails = new Map<string, Promise<SymbolDetails>>();
const footprintDetails = new Map<string, Promise<FootprintDetails>>();

/** Forget the items and drawings read so far: the library tables changed, so a nickname may lead to another file. */
export function resetChooserCaches(): void {
  entriesCache.clear();
  symbolDetails.clear();
  footprintDetails.clear();
  footprintSelections.clear();
  // the libraries are others now: which of them load with errors is asked again
  void refreshLoadErrors();
}

const footprintSelections = new Map<string, string[]>();

/** What the footprint selector lists: `[]` while the server reads the libraries (it answers `indexing`), else the footprints. */
export interface FootprintSelection {
  status: "ready" | "indexing";
  footprints: string[];
}

/**
 * The footprints the Symbol Chooser's footprint selector offers for a symbol (`GET /api/library/fp_select`, pcbnew's `filterFootprints`): of the symbol's pin
 * count and footprint filters, at most 400. `zero`: the symbol has no footprint filters (`m_zero_filter`), so with no pin count either there is nothing to list.
 * A finished answer is kept for the page's life (until the tables change).
 */
export async function fetchFootprintSelection(pins: number, fpFilters: readonly string[], signal?: AbortSignal): Promise<FootprintSelection> {
  const zero = fpFilters.length === 0;
  const key = `${pins}\u0001${fpFilters.join(" ")}`;
  const hit = footprintSelections.get(key);
  if (hit) return { status: "ready", footprints: hit };
  let q = `zero=${zero ? 1 : 0}`;
  if (pins > 0) q += `&pins=${pins}`;
  if (!zero) q += `&fp_filters=${encodeURIComponent(fpFilters.join(" "))}`;
  const r = await get<{ status: "ready" | "indexing"; footprints: string[]; error?: string }>(`/api/library/fp_select?${q}`, signal);
  if (r.status === "ready") footprintSelections.set(key, r.footprints);
  return { status: r.status, footprints: r.footprints };
}

/** The selected installed symbol (`GET /api/library/details`): cut out of its library file on the server, only this one. */
export function fetchSymbolDetails(id: string): Promise<SymbolDetails> {
  return remember(symbolDetails, id, async () => {
    const d = await get<SymbolDetails & { error?: string }>(`/api/library/details?kind=symbol&id=${encodeURIComponent(id)}`);
    return { ...d, extends: d.extends ?? null, symbol: withDefaults(d.symbol) };
  });
}

function withFootprintDefaults(f: LibraryFootprint): LibraryFootprint {
  return { ...f, pads: f.pads ?? [], graphics: f.graphics ?? [], texts: f.texts ?? [], fields: f.fields ?? [] };
}

/** The selected installed footprint: its pads, graphics and courtyard for the preview, and its description and tags. */
export function fetchFootprintDetails(id: string): Promise<FootprintDetails> {
  return remember(footprintDetails, id, async () => {
    const d = await get<FootprintDetails & { error?: string }>(`/api/library/details?kind=footprint&id=${encodeURIComponent(id)}`);
    return { ...d, footprint: withFootprintDefaults(d.footprint) };
  });
}

/** Any symbol of the project (its own library, what the design uses, the built-in table): the drawing for the preview. */
export async function fetchProjectSymbolDrawing(id: string): Promise<LibrarySymbol> {
  const r = await get<{ symbol: LibrarySymbol; error?: string }>(`/api/library/symbol?lib_id=${encodeURIComponent(id)}`);
  return withDefaults(r.symbol);
}

/** Any footprint the board knows (the project library, the model, the built-in table): the drawing for the preview. */
export async function fetchProjectFootprintDrawing(name: string): Promise<LibraryFootprint> {
  const r = await get<{ footprint: LibraryFootprint; error?: string }>(`/api/library/footprint?name=${encodeURIComponent(name)}`);
  return withFootprintDefaults(r.footprint);
}

/** One symbol of the project's own libraries or of what the design uses, from `GET /api/library/project`. */
export interface ProjectSymbol extends ChooserItem {
  /** `project`: an entry of the project symbol library (editable); `design`: resolved by the board's model; `builtin`: the stand-in table. */
  source: "project" | "design" | "builtin";
  placed: boolean;
}

export async function fetchProjectSymbols(): Promise<ProjectSymbol[]> {
  const r = await get<{ entries: { id: string; source: ProjectSymbol["source"]; placed: boolean; description: string; reference: string; units: number; pins: number; power: boolean; keywords: string; fields?: Record<string, string> }[]; error?: string }>("/api/library/project");
  return r.entries.map((e) => {
    const { lib, name } = splitId(e.id);
    // the chooser fields (`GetChooserFields`): the extra columns of the Already Placed and project rows, and their search words (UI-LEFT-2 LL4)
    const fields = e.fields ?? undefined;
    return { id: e.id, lib, name, description: e.description, keywords: e.keywords, reference: e.reference, units: e.units, pins: e.pins, power: e.power, source: e.source, placed: e.placed, ...(fields ? { fields, value: fields.Value, footprint: fields.Footprint } : {}) };
  });
}
