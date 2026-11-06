// The model behind the Symbol Chooser and the Footprint Chooser (`SYMBOL_CHOOSER_FRAME`, `FOOTPRINT_CHOOSER_FRAME`, both a `LIB_TREE` over a
// `LIB_TREE_MODEL_ADAPTER`): what they list, in what order, what a search keeps, what is selected when they open. Pure, so every rule is unit tested
// without a browser or a server. The components are components/chooser/LibChooser.tsx and the two dialogs; the data is api/libraryChooserClient.ts.
//
// Ported from common/lib_tree_model.cpp (`LIB_TREE_NODE::Compare`, `UpdateScore`, `AssignIntrinsicRanks`), common/lib_tree_model_adapter.cpp
// (`UpdateSearchString`, `showResults`, `GetChildren`: a node with no score is hidden), common/eda_pattern_match.cpp (`EDA_COMBINED_MATCHER::ScoreTerms`,
// CTX_LIBITEM: the regular-expression, wildcard, relational and substring matchers), eeschema/lib_symbol.cpp (`cacheSearchTerms`), common/footprint_info.cpp (`FOOTPRINT_INFO::GetSearchTerms`),
// eeschema/symbol_chooser_frame.cpp and pcbnew/footprint_chooser_frame.cpp (`AddSymbolToHistory`: 8 recent; `filterFootprint`), commit 8303b2ad.
//
// The installed libraries are searched on the server (crates/cli/src/library_search.rs scores the same way: the two tests share their cases); the
// groups the browser already holds -- recently used, already placed, the project's own -- are scored here.

// ---------------------------------------------------------------------------------------------------------------------------------- matching

/** One searchable text of an item with its weight (`SEARCH_TERM`), normalized the way `ScoreTerms` does: lower case, trimmed, at most 1000 characters. */
export interface SearchTerm {
  text: string;
  weight: number;
}

export function searchTerm(text: string, weight: number): SearchTerm {
  let t = text.trim().toLowerCase();
  if ([...t].length > 1000) t = [...t].slice(0, 1000).join("");
  return { text: t, weight };
}

/** `EDA_PATTERN_MATCH_RELATIONAL`'s relations; `any` is a word whose value is not typed yet (`voltage>`). */
type Relation = "lt" | "le" | "eq" | "ge" | "gt" | "any";

/** `EDA_PATTERN_MATCH_RELATIONAL`: the key, the relation and the value (its unit's multiplier applied). */
export interface Relational {
  key: string;
  relation: Relation;
  value: number;
}

/**
 * One word of the query (`EDA_COMBINED_MATCHER`, CTX_LIBITEM): the lower-case word (`UpdateSearchString` lowers every word) and the matchers whose `SetPattern`
 * took it, in KiCad's order -- `EDA_PATTERN_MATCH_REGEX` (a word written `/re/` or `^re$`), `EDA_PATTERN_MATCH_WILDCARD` (`*` / `?`, searched anywhere in a
 * term; for a word with neither it finds what the substring finds, so it is kept only for a word that has one), `EDA_PATTERN_MATCH_RELATIONAL`
 * (`voltage>3.3`) and `EDA_PATTERN_MATCH_SUBSTR`, which takes every word.
 */
export interface Matcher {
  pattern: string;
  /** The pieces between `*`; `null` is `?`. Empty for a plain word. */
  segments: (string | null)[][];
  leadingStar: boolean;
  wild: boolean;
  /**
   * `EDA_PATTERN_MATCH_REGEX`: compiled with JavaScript's RegExp (the `u` flag), as kicad-port/edaPatternMatch.ts does -- wxRegEx compiles `wxRE_ADVANCED`
   * (the server's search runs `eda_model::wx_regex`); the usual classes, quantifiers, groups and anchors read the same. A pattern RegExp refuses is a matcher
   * not added, as wxRegEx's refusal is.
   */
  regex: RegExp | null;
  relational: Relational | null;
}

/** `EDA_PATTERN_MATCH_RELATIONAL::m_units`, by the lower-case unit. */
const UNITS: Readonly<Record<string, number>> = {
  p: 1e-12,
  n: 1e-9,
  u: 1e-6,
  m: 1e-3,
  "": 1,
  k: 1e3,
  meg: 1e6,
  g: 1e9,
  t: 1e12,
  ki: 1024,
  mi: 1048576,
  gi: 1073741824,
  ti: 1099511627776,
};

/** `\w` as wxRegEx's advanced syntax (and `eda_model::wx_regex`) reads it: a letter or digit of any script, or `_`. */
const W = "[\\p{Alphabetic}\\p{N}_]";
/** `EDA_PATTERN_MATCH_RELATIONAL::SetPattern`'s and `FindOne`'s expressions. */
const RELATIONAL_WORD = new RegExp(`^(${W}+)(<|<=|=|>=|>)([-+]?[\\d.]*)(${W}*)$`, "u");
const RELATIONAL_FIELD = new RegExp(`(${W}+)[=:]([-+]?[\\d.]+)(${W}*)`, "u");

/** `wxString::ToCDouble`: the whole text a number (its characters are `[-+]?[\d.]+` already). */
function toCDouble(text: string): number | null {
  if (text === "") return null;
  const v = Number(text);
  return Number.isFinite(v) ? v : null;
}

/** `EDA_PATTERN_MATCH_RELATIONAL::SetPattern`: the key lowered, an empty value matching any, a value that is no number or a unit not in the table refusing the word. */
export function makeRelational(word: string): Relational | null {
  const m = RELATIONAL_WORD.exec(word);
  if (!m) return null;
  const [, key = "", rel = "", val = "", unit = ""] = m;
  const relations: Record<string, Relation> = { "<": "lt", "<=": "le", "=": "eq", ">=": "ge", ">": "gt" };
  let relation = relations[rel];
  if (!relation) return null;
  let value = 0;
  if (val === "") relation = "any";
  else {
    const v = toCDouble(val);
    if (v === null) return null;
    value = v;
  }
  const mult = UNITS[unit.toLowerCase()];
  if (mult === undefined) return null;
  return { key: key.toLowerCase(), relation, value: value * mult };
}

/** `EDA_PATTERN_MATCH_RELATIONAL::FindOne`: the first `key=value[unit]` (or `key:value`) of one token, its key this one's, its value (scaled by a known unit) in the relation. */
function findOneRelational(r: Relational, token: string): number | null {
  if (!token.includes("=") && !token.includes(":")) return null;
  const m = RELATIONAL_FIELD.exec(token);
  if (!m) return null;
  const [, key = "", val = "", unit = ""] = m;
  if (key.toLowerCase() !== r.key) return null;
  let parsed = toCDouble(val);
  if (parsed === null) return null;
  const mult = UNITS[unit.toLowerCase()];
  if (mult !== undefined) parsed *= mult;
  const hit =
    r.relation === "lt" ? parsed < r.value : r.relation === "le" ? parsed <= r.value : r.relation === "eq" ? parsed === r.value : r.relation === "ge" ? parsed >= r.value : r.relation === "gt" ? parsed > r.value : true;
  return hit ? m.index : null;
}

/** `EDA_PATTERN_MATCH_RELATIONAL::Find`: the text split on blanks (`wxTOKEN_STRTOK`), the first token with a match; its place in the text. */
export function findRelational(r: Relational, text: string): number | null {
  let offset = 0;
  for (const token of text.split(/[ \t\r\n]/)) {
    if (token !== "") {
      const at = findOneRelational(r, token);
      if (at !== null) return offset + at;
    }
    offset += token.length + 1;
  }
  return null;
}

/** `EDA_PATTERN_MATCH_REGEX::SetPattern`: `^...$` as it is, `/...` less the slashes ("Requiring a '/' on the end means they get no feedback while they type"), else none. */
export function explicitRegex(word: string): RegExp | null {
  let pattern: string;
  if (word.startsWith("^") && word.endsWith("$")) pattern = word;
  else if (word.startsWith("/")) {
    pattern = word.slice(1);
    if (pattern.endsWith("/")) pattern = pattern.slice(0, -1);
  } else return null; // "For now regular expressions must be explicit"
  try {
    return new RegExp(pattern, "u");
  } catch {
    return null;
  }
}

export function makeMatcher(word: string): Matcher {
  const pattern = word.toLowerCase();
  const wild = pattern.includes("*") || pattern.includes("?");
  const segments = wild
    ? pattern
        .split("*")
        .filter((s) => s !== "")
        .map((s) => [...s].map((c) => (c === "?" ? null : c)))
    : [];
  return { pattern, segments, leadingStar: wild && pattern.startsWith("*"), wild, regex: explicitRegex(pattern), relational: makeRelational(pattern) };
}

function findWild(m: Matcher, text: string): number | null {
  if (m.segments.length === 0) return 0; // only stars: `.*` matches the empty start
  const chars = [...text];
  let from = 0;
  let first: number | null = null;
  for (const seg of m.segments) {
    let at = -1;
    for (let s = from; s + seg.length <= chars.length; s++) {
      if (seg.every((c, k) => c === null || chars[s + k] === c)) {
        at = s;
        break;
      }
    }
    if (at < 0) return null;
    if (first === null) first = at;
    from = at + seg.length;
  }
  return m.leadingStar ? 0 : (first ?? 0);
}

/** Where the word first matches in `text` (0 only when from the first character), or null: `EDA_COMBINED_MATCHER::Find`, the earliest of its matchers. */
export function findIn(m: Matcher, text: string): number | null {
  const found: number[] = [];
  if (m.regex) {
    const r = m.regex.exec(text);
    if (r) found.push(r.index);
  }
  if (m.wild) {
    const w = findWild(m, text);
    if (w !== null) found.push(w);
  }
  if (m.relational) {
    const r = findRelational(m.relational, text);
    if (r !== null) found.push(r);
  }
  const plain = text.indexOf(m.pattern);
  if (plain >= 0) found.push(plain);
  return found.length === 0 ? null : Math.min(...found);
}

/** `EDA_COMBINED_MATCHER::ScoreTerms`: an exact term is worth 8 times its weight, a match at its start twice, a match anywhere once. */
export function scoreTerms(m: Matcher, terms: readonly SearchTerm[]): number {
  let score = 0;
  for (const t of terms) {
    if (m.pattern === t.text) score += 8 * t.weight;
    else {
      const at = findIn(m, t.text);
      if (at !== null) score += (at === 0 ? 2 : 1) * t.weight;
    }
  }
  return score;
}

/** The words of a query, at most 100 (`UpdateSearchString`'s `MAX_TERMS`). */
export function parseQuery(query: string): Matcher[] {
  return query
    .split(/\s+/)
    .filter((w) => w !== "")
    .slice(0, 100)
    .map(makeMatcher);
}

/** `LIB_TREE_NODE_ITEM::UpdateScore`: 1 plus the score of every word, or 0 when one of them scores nothing. */
export function scoreItemTerms(matchers: readonly Matcher[], terms: readonly SearchTerm[]): number {
  let total = 1;
  for (const m of matchers) {
    const s = scoreTerms(m, terms);
    if (s === 0) return 0;
    total += s;
  }
  return total;
}

/** `StrNumCmp( a, b, true )`: runs of digits compare as numbers, anything else by upper-case character, the shorter of two equal prefixes first. */
export function naturalCompare(a: string, b: string): number {
  const x = [...a];
  const y = [...b];
  let i = 0;
  let j = 0;
  const isDigit = (c: string | undefined) => c !== undefined && c >= "0" && c <= "9";
  while (i < x.length && j < y.length) {
    if (isDigit(x[i]) && isDigit(y[j])) {
      let n1 = 0n;
      let n2 = 0n;
      while (isDigit(x[i])) n1 = n1 * 10n + BigInt(x[i++]!);
      while (isDigit(y[j])) n2 = n2 * 10n + BigInt(y[j++]!);
      if (n1 !== n2) return n1 < n2 ? -1 : 1;
      continue;
    }
    const u1 = x[i]!.toUpperCase();
    const u2 = y[j]!.toUpperCase();
    if (u1 !== u2) return u1 < u2 ? -1 : 1;
    i++;
    j++;
  }
  if (i < x.length) return 1;
  if (j < y.length) return -1;
  return 0;
}

/** `EDA_PATTERN_MATCH_WILDCARD_ANCHORED`: `*` and `?` over the whole text, both lower case already. */
export function wildcardFull(pattern: string, text: string): boolean {
  const p = [...pattern];
  const t = [...text];
  let pi = 0;
  let ti = 0;
  let star = -1;
  let mark = 0;
  while (ti < t.length) {
    if (pi < p.length && p[pi] !== "*" && (p[pi] === "?" || p[pi] === t[ti])) {
      pi++;
      ti++;
    } else if (pi < p.length && p[pi] === "*") {
      star = pi++;
      mark = ti;
    } else if (star >= 0) {
      pi = star + 1;
      ti = ++mark;
    } else return false;
  }
  return p.slice(pi).every((c) => c === "*");
}

// ---------------------------------------------------------------------------------------------------------------------------------------- items

/** What the tree lists for one symbol, footprint or unplaced part. */
export interface ChooserItem {
  /** `Lib:Name`. The Place Footprint chooser's unplaced parts are `ref:R1`. */
  id: string;
  lib: string;
  name: string;
  description: string;
  keywords?: string;
  /** The symbol's Reference prefix ("U", "R"). */
  reference?: string;
  /** The symbol's Value field. */
  value?: string;
  /** The symbol's default footprint. */
  footprint?: string;
  /** How many units a symbol has (a row per unit is offered when more than one). */
  units?: number;
  /** A symbol's pins, a footprint's numbered pads. */
  pins?: number;
  power?: boolean;
  /** The score of the last search (1 plus the words' scores); absent when there was none. */
  score?: number;
}

export type ChooserKind = "symbol" | "footprint";

/** `Lib:Name` -> its two halves; a bare name has no library. */
export function splitId(id: string): { lib: string; name: string } {
  const at = id.indexOf(":");
  return at < 0 ? { lib: "", name: id } : { lib: id.slice(0, at), name: id.slice(at + 1) };
}

/** `LIB_SYMBOL::cacheSearchTerms` and the shown columns (Description and Value, 4 each); `FOOTPRINT_INFO::GetSearchTerms` for a footprint. */
export function itemTerms(kind: ChooserKind, item: ChooserItem): SearchTerm[] {
  const keywords = item.keywords ?? "";
  const terms = [searchTerm(item.lib, 4), searchTerm(item.name, 8), searchTerm(`${item.lib}:${item.name}`, 16)];
  for (const k of keywords.split(/\s+/)) if (k !== "") terms.push(searchTerm(k, 4));
  terms.push(searchTerm(keywords, 1), searchTerm(item.description, 1));
  if (kind === "symbol") {
    if (item.footprint) terms.push(searchTerm(item.footprint, 1));
    terms.push(searchTerm(item.description, 4), searchTerm(item.value ?? "", 4));
  }
  return terms;
}

/** The items a search keeps, each with its score, in the tree's order within a score (name, naturally); the whole list when the query is empty. */
export function scoreItems(kind: ChooserKind, items: readonly ChooserItem[], query: string): ChooserItem[] {
  const matchers = parseQuery(query);
  const out: ChooserItem[] = [];
  for (const item of items) {
    const score = scoreItemTerms(matchers, itemTerms(kind, item));
    if (score > 0) out.push({ ...item, score });
  }
  return out;
}

/** `AddSymbolToHistory`: newest first, no duplicate, at most `max` (8). */
export function addRecent<T extends { id: string }>(list: readonly T[], entry: T, max = 8): T[] {
  return [entry, ...list.filter((e) => e.id !== entry.id)].slice(0, max);
}

// ---------------------------------------------------------------------------------------------------------------------------------------- tree

/** A node of the tree that holds items: a library, or one of the pseudo-libraries ("-- Recently Used --", "-- Already Placed --"). */
export interface ChooserGroup {
  /** The library's nickname, or the pseudo-library's label. The key of the group. */
  lib: string;
  pseudo?: "recent" | "placed" | "extra";
  description?: string;
  /** Not an installed library: the project's own, or a table the design carries. */
  project?: boolean;
  pinned: boolean;
  items: ChooserItem[];
  /** How many items the library holds, when known without loading it. */
  count?: number;
  /** Its items are not loaded yet (an installed library nobody opened): the row can be opened to load them. */
  lazy: boolean;
  /** The best score of its items, 0 when there was no search. */
  score: number;
}

export const RECENT_LABEL = "-- Recently Used --";
export const PLACED_LABEL = "-- Already Placed --";

/**
 * `LIB_TREE_NODE::Compare` over groups: recently used first, then the other pseudo-libraries, then pinned libraries, then the best score (while a search
 * is on), then the name. Without a search the project's own libraries come before the installed ones, as they do in the library editors' trees.
 */
export function orderGroups(groups: readonly ChooserGroup[], searching: boolean): ChooserGroup[] {
  const rank = (g: ChooserGroup) => (g.pseudo === "recent" ? 0 : g.pseudo === "placed" ? 1 : g.pseudo === "extra" ? 2 : 3);
  return [...groups].sort((a, b) => {
    if (rank(a) !== rank(b)) return rank(a) - rank(b);
    if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
    if (searching && a.score !== b.score) return b.score - a.score;
    if (!searching && Boolean(a.project) !== Boolean(b.project)) return a.project ? -1 : 1;
    return naturalCompare(a.lib, b.lib);
  });
}

export interface ComposeInput {
  kind: ChooserKind;
  query: string;
  /** The recently used items, newest first. */
  recent: readonly ChooserItem[];
  /** Symbols with an instance on the schematic. */
  placed?: readonly ChooserItem[];
  /** The project's own symbols/footprints, and what the design uses: grouped by library here. */
  project?: readonly ChooserItem[];
  /** An extra pseudo-library at the top (Place Footprint: the unplaced parts). */
  extra?: { label: string; items: readonly ChooserItem[] };
  /** The installed libraries; `lazy` until their items are loaded. */
  installed: readonly { name: string; description?: string; count?: number }[];
  /** The items of the installed libraries opened so far (no search). */
  loaded: ReadonlyMap<string, readonly ChooserItem[]>;
  /** The server's answer to the search: the libraries with a match and their best items. Null while there is no search. */
  results: readonly { name: string; description?: string; score: number; items: readonly ChooserItem[] }[] | null;
  pinned: ReadonlySet<string>;
}

/**
 * Every group the tree shows. With no query: recently used, already placed, the extra group, the project's libraries and every installed library
 * (its items once loaded). With a query: the groups of the browser, scored and with the items that scored; the installed libraries from the server's
 * answer, in order of their best item; a group with no item left is hidden (`GetChildren` drops a node whose score is 0).
 */
export function composeGroups(input: ComposeInput): ChooserGroup[] {
  const { kind, query } = input;
  const searching = parseQuery(query).length > 0;
  const groups: ChooserGroup[] = [];
  const pseudo = (label: string, tag: "recent" | "placed" | "extra", items: readonly ChooserItem[]) => {
    let kept = searching ? scoreItems(kind, items, query) : [...items];
    if (items.length === 0 || (searching && kept.length === 0)) return;
    // The recently used are listed by recency and the extra group in the order it was given; the placed ones by name (`std::sort` by `GetLibItemName`).
    if (tag === "placed") kept = [...kept].sort((a, b) => naturalCompare(a.name, b.name));
    // A search sorts every group by score, the order of the list within equal scores (a stable sort).
    if (searching) kept = [...kept].sort((a, b) => (b.score ?? 0) - (a.score ?? 0));
    groups.push({ lib: label, pseudo: tag, pinned: false, items: kept, lazy: false, score: Math.max(0, ...kept.map((i) => i.score ?? 0)) });
  };
  pseudo(RECENT_LABEL, "recent", input.recent);
  if (input.placed) pseudo(PLACED_LABEL, "placed", input.placed);
  if (input.extra) pseudo(input.extra.label, "extra", input.extra.items);

  const byLib = new Map<string, ChooserItem[]>();
  for (const it of input.project ?? []) byLib.set(it.lib, [...(byLib.get(it.lib) ?? []), it]);
  for (const [lib, list] of byLib) {
    const kept = searching ? scoreItems(kind, list, query) : [...list];
    if (searching && kept.length === 0) continue;
    kept.sort((a, b) => (searching && a.score !== b.score ? (b.score ?? 0) - (a.score ?? 0) : naturalCompare(a.name, b.name)));
    groups.push({ lib, project: true, pinned: input.pinned.has(lib), items: kept, lazy: false, score: Math.max(0, ...kept.map((i) => i.score ?? 0)) });
  }

  const taken = new Set(groups.map((g) => g.lib));
  if (searching) {
    for (const r of input.results ?? []) {
      if (taken.has(r.name) || r.items.length === 0) continue;
      groups.push({ lib: r.name, description: r.description, pinned: input.pinned.has(r.name), items: [...r.items], lazy: false, score: r.score });
    }
  } else {
    for (const lib of input.installed) {
      if (taken.has(lib.name)) continue;
      const items = input.loaded.get(lib.name);
      groups.push({ lib: lib.name, description: lib.description, pinned: input.pinned.has(lib.name), items: items ? [...items] : [], count: items ? items.length : lib.count, lazy: !items, score: 0 });
    }
  }
  return orderGroups(groups, searching);
}

// ------------------------------------------------------------------------------------------------------------------------------------- rows

export type ChooserRow =
  | { kind: "group"; key: string; group: ChooserGroup; open: boolean }
  | { kind: "item"; key: string; group: ChooserGroup; item: ChooserItem; expandable: boolean; open: boolean }
  | { kind: "unit"; key: string; group: ChooserGroup; item: ChooserItem; unit: number };

export const groupKey = (g: { lib: string }) => `g:${g.lib}`;
export const itemKey = (g: { lib: string }, item: { id: string }) => `i:${g.lib}\u0001${item.id}`;
export const unitKey = (g: { lib: string }, item: { id: string }, unit: number) => `u:${g.lib}\u0001${item.id}\u0001${unit}`;

/** Which groups and symbols are open. A search opens every group that has items (and a library with a match, `showResults` expanding the ancestors of the matches). */
export interface OpenState {
  groups: ReadonlySet<string>;
  items: ReadonlySet<string>;
  /** A search is on: every group with items is open. */
  searching: boolean;
}

/** The rows the tree draws, top to bottom: a group row, its item rows when it is open, and under a multi-unit symbol that is open one row per unit. */
export function visibleRows(groups: readonly ChooserGroup[], open: OpenState, kind: ChooserKind = "symbol"): ChooserRow[] {
  const rows: ChooserRow[] = [];
  for (const group of groups) {
    const isOpen = open.groups.has(group.lib) || (open.searching && group.items.length > 0);
    rows.push({ kind: "group", key: groupKey(group), group, open: isOpen });
    if (!isOpen) continue;
    for (const item of group.items) {
      const units = kind === "symbol" ? (item.units ?? 1) : 1;
      const expandable = units > 1;
      const itemOpen = expandable && open.items.has(itemKey(group, item));
      rows.push({ kind: "item", key: itemKey(group, item), group, item, expandable, open: itemOpen });
      if (itemOpen) for (let u = 1; u <= units; u++) rows.push({ kind: "unit", key: unitKey(group, item, u), group, item, unit: u });
    }
  }
  return rows;
}

/** `GetPrevItem` / `GetNextItem`: the key `delta` rows from `from`, kept inside the list; the first row when nothing is selected. */
export function moveSelection(rows: readonly ChooserRow[], from: string | null, delta: number): string | null {
  if (rows.length === 0) return null;
  const at = from === null ? -1 : rows.findIndex((r) => r.key === from);
  if (at < 0) return rows[delta < 0 ? rows.length - 1 : 0]!.key;
  return rows[Math.max(0, Math.min(rows.length - 1, at + delta))]!.key;
}

/** What is chosen when OK is pressed on a row: an item (with the unit a unit row names, 0 for none), nothing for a group. */
export function chosenOf(row: ChooserRow | undefined): { item: ChooserItem; unit: number; group: ChooserGroup } | null {
  if (!row || row.kind === "group") return null;
  return { item: row.item, unit: row.kind === "unit" ? row.unit : 0, group: row.group };
}

export interface Selection {
  /** Groups to open. */
  open: string[];
  /** The row to select, or null. */
  select: string | null;
}

/**
 * `showResults`: with a query, the best-scoring item is selected (the first of equals in tree order) and its library opened; with none, the preselected
 * item (the most recently used one) in the real library -- not in the recent group -- and else, when there is a single library, its first item.
 */
export function initialSelection(groups: readonly ChooserGroup[], query: string, preselect: string | null): Selection {
  const searching = parseQuery(query).length > 0;
  if (searching) {
    let best: { score: number; key: string } | null = null;
    for (const g of groups)
      for (const item of g.items) {
        const score = item.score ?? 0;
        if (score > 1 && (!best || score > best.score)) best = { score, key: itemKey(g, item) };
      }
    if (best) return { open: [], select: best.key };
    const first = groups.find((g) => g.items.length > 0);
    return { open: [], select: first ? itemKey(first, first.items[0]!) : null };
  }
  if (preselect) {
    for (const g of groups) {
      if (g.pseudo) continue;
      const item = g.items.find((i) => i.id === preselect);
      if (item) return { open: [g.lib], select: itemKey(g, item) };
    }
    const recent = groups.find((g) => g.pseudo === "recent");
    const item = recent?.items.find((i) => i.id === preselect);
    if (recent && item) return { open: [recent.lib], select: itemKey(recent, item) };
  }
  const libs = groups.filter((g) => !g.pseudo);
  if (libs.length === 1 && libs[0]!.items.length > 0) return { open: [libs[0]!.lib], select: itemKey(libs[0]!, libs[0]!.items[0]!) };
  return { open: [], select: null };
}

/** The key of the row for item `id` in the first real library that holds it (what Preselect finds), or null. */
export function keyOfItem(groups: readonly ChooserGroup[], id: string): string | null {
  for (const g of groups) {
    if (g.pseudo) continue;
    const item = g.items.find((i) => i.id === id);
    if (item) return itemKey(g, item);
  }
  return null;
}

// --------------------------------------------------------------------------------------------------------------------------------- footprints

/** `PANEL_SYMBOL_CHOOSER::populateFootprintSelector`'s `FilterByFootprintFilters`: the symbol's `ki_fp_filters` (space separated) as lower-case patterns. */
export function footprintFilterPatterns(filters: readonly string[] | string | undefined): string[] {
  const list = typeof filters === "string" ? filters.split(/\s+/) : [...(filters ?? [])];
  return list.map((f) => f.trim().toLowerCase()).filter((f) => f !== "");
}

/** `FOOTPRINT_CHOOSER_FRAME::filterFootprint`: a pattern with a colon is matched against `lib:name`, any other against the name; any one match passes. */
export function passesFootprintFilters(patterns: readonly string[], lib: string, name: string): boolean {
  if (patterns.length === 0) return true;
  const bare = name.toLowerCase();
  const qualified = `${lib.toLowerCase()}:${bare}`;
  return patterns.some((p) => wildcardFull(p, p.includes(":") ? qualified : bare));
}

/** `GetFootprintDocumentationURL`: a link in the description, without the punctuation or the parenthesis that ends it; null when there is none. */
export function urlIn(description: string): string | null {
  const at = description.search(/https?:/);
  if (at < 0) return null;
  let url = "";
  let nesting = 0;
  for (const ch of description.slice(at)) {
    const code = ch.codePointAt(0) ?? 0;
    if (code <= 0x20 || code >= 0x7f || ch === '"') break;
    if (ch === "(") nesting++;
    else if (ch === ")" && --nesting < 0) break;
    url += ch;
  }
  url = url.replace(/[.,:;]+$/, "");
  return url === "" ? null : url;
}
