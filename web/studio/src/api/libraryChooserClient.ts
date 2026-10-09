// What the Symbol and Footprint Choosers ask the server (`crates/cli/src/library_search.rs`, `library_api.rs`): one installed library's items with their
// descriptions when it is opened, the search over every installed library, the one selected item's drawing, and the project's own symbols. The installed
// libraries are 220 MB of symbols and 179 MB of footprints, so nothing here asks for a library whole: items as names and descriptions, a drawing for
// the selected item only. Each answer is kept for the page's life (the server keeps its own cache too).
import type { ChooserItem, ChooserKind } from "../kicad-port/libChooser";
import { splitId } from "../kicad-port/libChooser";
import { withDefaults } from "../kicad-port/libraryDefaults";
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
}

function filterQuery(f: ChooserFilter | undefined): string {
  if (!f) return "";
  let q = "";
  if (f.pins && f.pins > 0) q += `&pins=${f.pins}`;
  if (f.fpFilters && f.fpFilters.length > 0) q += `&fp_filters=${encodeURIComponent(f.fpFilters.join(" "))}`;
  if (f.excludePower) q += "&power=exclude";
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
}

function toItem(lib: string, w: WireItem): ChooserItem {
  return { id: `${lib}:${w.name}`, lib, name: w.name, description: w.description ?? "", units: w.units, pins: w.pins ?? w.pads, power: w.power, score: w.score };
}

const entriesCache = new Map<string, Promise<ChooserItem[]>>();

/** One installed library's items (`GET /api/library/entries`), asked once per library and filter. */
export function fetchEntries(kind: ChooserKind, lib: string, filter?: ChooserFilter): Promise<ChooserItem[]> {
  const key = `${kind}\u0001${lib}\u0001${filterQuery(filter)}`;
  const hit = entriesCache.get(key);
  if (hit) return hit;
  const p = get<{ entries: WireItem[]; error?: string }>(`/api/library/entries?kind=${kind}&lib=${encodeURIComponent(lib)}${filterQuery(filter)}`).then((r) => r.entries.map((w) => toItem(lib, w)));
  entriesCache.set(key, p);
  p.catch(() => {
    if (entriesCache.get(key) === p) entriesCache.delete(key);
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
}

/** The search over every installed library (`GET /api/library/search`): the best matches, grouped by library, best first. */
export async function searchInstalled(kind: ChooserKind, query: string, filter?: ChooserFilter, signal?: AbortSignal): Promise<SearchAnswer> {
  const r = await get<{ status: "ready" | "indexing"; indexed: number; total: number; matches: number; truncated: boolean; libraries: { name: string; description?: string; score: number; items: WireItem[] }[]; error?: string }>(
    `/api/library/search?kind=${kind}&q=${encodeURIComponent(query)}${filterQuery(filter)}`,
    signal
  );
  return {
    status: r.status,
    indexed: r.indexed,
    total: r.total,
    matches: r.matches,
    truncated: r.truncated,
    libraries: r.libraries.map((l) => ({ name: l.name, description: l.description, score: l.score, items: l.items.map((w) => toItem(l.name, w)) })),
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
  const r = await get<{ entries: { id: string; source: ProjectSymbol["source"]; placed: boolean; description: string; reference: string; units: number; pins: number; power: boolean; keywords: string }[]; error?: string }>("/api/library/project");
  return r.entries.map((e) => {
    const { lib, name } = splitId(e.id);
    return { id: e.id, lib, name, description: e.description, keywords: e.keywords, reference: e.reference, units: e.units, pins: e.pins, power: e.power, source: e.source, placed: e.placed };
  });
}
