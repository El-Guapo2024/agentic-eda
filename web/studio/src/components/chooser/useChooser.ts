// The state of one chooser dialog (`LIB_TREE` + its `LIB_TREE_MODEL_ADAPTER`): the search text, what it finds, which libraries are open, what is selected.
// Shared by the Symbol Chooser and the Footprint Chooser; the rules are kicad-port/libChooser.ts's, the data comes from api/libraryChooserClient.ts.
//
//   - The search runs 200 ms after the last key (`LIB_TREE::onQueryText`'s debounce timer). The installed libraries are searched on the server, which reads
//     them lazily: while it has not finished the answer says so and is asked again (`indexing`), the matches growing as libraries are read.
//   - A library's items are fetched when it is opened, once, as names and descriptions: a collapsed library costs nothing.
//   - After a search the best match is selected and its library open (`showResults`); with no search, the item last used.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fetchEntries, searchInstalled, warmUpSearch, type ChooserFilter, type SearchAnswer } from "../../api/libraryChooserClient";
import { fetchInstalledLibraries, type InstalledLibrary } from "../../api/libraryIndexClient";
import {
  composeGroups,
  initialSelection,
  keyOfItem,
  moveSelection,
  parseQuery,
  splitId,
  visibleRows,
  type ChooserGroup,
  type ChooserItem,
  type ChooserKind,
  type ChooserRow,
} from "../../kicad-port/libChooser";
import { useLibraryTree } from "../../state/libraryTree";

export interface ChooserOptions {
  kind: ChooserKind;
  /** The dialog is open: nothing is fetched while it is not. */
  active: boolean;
  recent: readonly ChooserItem[];
  placed?: readonly ChooserItem[];
  project?: readonly ChooserItem[];
  extra?: { label: string; items: readonly ChooserItem[] };
  /** What the server narrows the installed libraries by. */
  filter?: ChooserFilter;
  /** The same narrowing for the items the browser holds (recent, placed, project, extra). */
  keep?: (item: ChooserItem) => boolean;
  /** The item to open on (`Lib:Name`): the one last used, or the symbol's current footprint. */
  preselect: string | null;
}

export interface SearchStatus {
  /** There is a search text. */
  searching: boolean;
  /** The server is still reading libraries for it. */
  indexing: boolean;
  indexed: number;
  total: number;
  matches: number;
  truncated: boolean;
  error: string | null;
}

export interface Chooser {
  query: string;
  setQuery: (q: string) => void;
  groups: ChooserGroup[];
  rows: ChooserRow[];
  selectedKey: string | null;
  selectedRow: ChooserRow | undefined;
  select: (key: string | null) => void;
  /** Arrow keys: the selection moves; this counts as the person choosing, so the next search result does not take the selection back. */
  move: (delta: number) => void;
  toggleGroup: (lib: string) => void;
  toggleItem: (key: string) => void;
  openGroup: (lib: string, open: boolean) => void;
  status: SearchStatus;
  loading: ReadonlySet<string>;
  /** How many installed libraries there are (0 where KiCad is not installed). */
  libraries: number;
}

const DEBOUNCE_MS = 200;

/** The libraries that were open when a chooser was last closed (`m_LibTree.open_libs`), per kind, for this page's life. */
const rememberedOpen: Record<ChooserKind, Set<string>> = { symbol: new Set(), footprint: new Set() };

export function useChooser(o: ChooserOptions): Chooser {
  const { kind, active } = o;
  const pinned = useLibraryTree(kind).pinned;
  const [query, setQueryState] = useState("");
  const [debounced, setDebounced] = useState("");
  const [installed, setInstalled] = useState<InstalledLibrary[]>([]);
  const [loaded, setLoaded] = useState<ReadonlyMap<string, readonly ChooserItem[]>>(new Map());
  const [loading, setLoading] = useState<ReadonlySet<string>>(new Set());
  const [results, setResults] = useState<SearchAnswer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [openGroups, setOpenGroups] = useState<ReadonlySet<string>>(new Set());
  const [openItems, setOpenItems] = useState<ReadonlySet<string>>(new Set());
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const moved = useRef(false);

  const filterKey = JSON.stringify(o.filter ?? {});
  const searching = parseQuery(debounced).length > 0;

  // Open: start from a clean slate, ask for the library names and start the server reading them.
  const preselect = o.preselect;
  useEffect(() => {
    if (!active) return;
    setQueryState("");
    setDebounced("");
    setResults(null);
    setError(null);
    setOpenItems(new Set());
    setSelectedKey(null);
    moved.current = false;
    const start = new Set(rememberedOpen[kind]);
    if (preselect) start.add(splitId(preselect).lib);
    setOpenGroups(start);
    let cancelled = false;
    fetchInstalledLibraries(kind)
      .then((libs) => !cancelled && setInstalled(libs))
      .catch(() => !cancelled && setInstalled([]));
    warmUpSearch(kind);
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, kind]);

  // Closing: the libraries that were open are open the next time (`m_LibTree.open_libs`).
  const openRef = useRef(openGroups);
  openRef.current = openGroups;
  useEffect(
    () => () => {
      rememberedOpen[kind] = new Set([...openRef.current].filter((l) => !l.startsWith("--")));
    },
    [kind]
  );

  // The search text, after the debounce.
  const setQuery = useCallback((q: string) => {
    setQueryState(q);
    moved.current = false;
  }, []);
  useEffect(() => {
    if (!active) return;
    const t = setTimeout(() => setDebounced(query), query === "" ? 0 : DEBOUNCE_MS);
    return () => clearTimeout(t);
  }, [active, query]);

  // The search itself: the server's answer, asked again while it is still indexing.
  useEffect(() => {
    if (!active) return;
    if (!searching) {
      setResults(null);
      return;
    }
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const ctl = new AbortController();
    const run = async () => {
      try {
        const answer = await searchInstalled(kind, debounced, o.filter, ctl.signal);
        if (cancelled) return;
        setResults(answer);
        setError(null);
        if (answer.status === "indexing") timer = setTimeout(() => void run(), 500);
      } catch (e) {
        if (cancelled || (e instanceof DOMException && e.name === "AbortError")) return;
        setError(e instanceof Error ? e.message : String(e));
      }
    };
    void run();
    return () => {
      cancelled = true;
      clearTimeout(timer);
      ctl.abort();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, kind, debounced, searching, filterKey]);

  // A narrower filter changes what a library holds: forget what was loaded.
  useEffect(() => {
    setLoaded(new Map());
    setLoading(new Set());
  }, [filterKey, kind]);

  // An open installed library gets its items.
  const installedNames = useMemo(() => new Set(installed.map((l) => l.name)), [installed]);
  useEffect(() => {
    if (!active || searching) return;
    for (const lib of openGroups) {
      if (!installedNames.has(lib) || loaded.has(lib) || loading.has(lib)) continue;
      setLoading((prev) => new Set(prev).add(lib));
      fetchEntries(kind, lib, o.filter)
        .then((items) => setLoaded((prev) => new Map(prev).set(lib, items)))
        .catch((e) => setError(e instanceof Error ? e.message : String(e)))
        .finally(() =>
          setLoading((prev) => {
            const next = new Set(prev);
            next.delete(lib);
            return next;
          })
        );
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, searching, openGroups, installedNames, loaded, filterKey]);

  const keep = o.keep;
  const kept = useCallback((items: readonly ChooserItem[] | undefined) => (items ? (keep ? items.filter(keep) : [...items]) : undefined), [keep]);
  const groups = useMemo(
    () =>
      composeGroups({
        kind,
        query: searching ? debounced : "",
        recent: kept(o.recent) ?? [],
        placed: kept(o.placed),
        project: kept(o.project),
        extra: o.extra ? { label: o.extra.label, items: kept(o.extra.items) ?? [] } : undefined,
        installed,
        loaded,
        results: results ? results.libraries : null,
        pinned,
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [kind, searching, debounced, o.recent, o.placed, o.project, o.extra, installed, loaded, results, pinned, kept]
  );
  const rows = useMemo(() => visibleRows(groups, { groups: openGroups, items: openItems, searching }, kind), [groups, openGroups, openItems, searching, kind]);

  // What is selected when the list changes under a new search: the best match (`showResults`), unless the person has moved since typing.
  const seen = useRef("");
  useEffect(() => {
    if (!active) return;
    if (searching) {
      if (results === null || moved.current) return;
      const sig = `${debounced}\u0001${results.indexed}\u0001${results.libraries.length}`;
      if (sig === seen.current) return;
      seen.current = sig;
      setSelectedKey(initialSelection(groups, debounced, null).select);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, searching, results, debounced, groups]);

  // Back to no search: the item last used, in its library, once that is loaded.
  const preselected = useRef<string | null>(null);
  useEffect(() => {
    if (!active) {
      preselected.current = null;
      return;
    }
    if (searching || !preselect || preselected.current === preselect) return;
    const sel = initialSelection(groups, "", preselect);
    if (sel.select) {
      preselected.current = preselect;
      setSelectedKey(sel.select);
      if (sel.open.length > 0) setOpenGroups((prev) => new Set([...prev, ...sel.open]));
    } else if (keyOfItem(groups, preselect)) {
      preselected.current = preselect;
      setSelectedKey(keyOfItem(groups, preselect));
    }
  }, [active, searching, preselect, groups]);

  // Nothing preselected and a single library: its first item.
  useEffect(() => {
    if (!active || searching || preselect || selectedKey !== null || moved.current) return;
    const sel = initialSelection(groups, "", null);
    if (sel.select) {
      setSelectedKey(sel.select);
      if (sel.open.length > 0) setOpenGroups((prev) => new Set([...prev, ...sel.open]));
    }
  }, [active, searching, preselect, selectedKey, groups]);

  const select = useCallback((key: string | null) => {
    moved.current = true;
    setSelectedKey(key);
  }, []);
  const move = useCallback(
    (delta: number) => {
      moved.current = true;
      setSelectedKey((cur) => moveSelection(rows, cur, delta));
    },
    [rows]
  );
  const openGroup = useCallback((lib: string, open: boolean) => {
    setOpenGroups((prev) => {
      const next = new Set(prev);
      if (open) next.add(lib);
      else next.delete(lib);
      return next;
    });
  }, []);
  const toggleGroup = useCallback(
    (lib: string) => {
      const row = rows.find((r) => r.kind === "group" && r.group.lib === lib);
      openGroup(lib, !(row && row.kind === "group" && row.open));
    },
    [rows, openGroup]
  );
  const toggleItem = useCallback((key: string) => {
    setOpenItems((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const selectedRow = rows.find((r) => r.key === selectedKey);
  return {
    query,
    setQuery,
    groups,
    rows,
    selectedKey,
    selectedRow,
    select,
    move,
    toggleGroup,
    toggleItem,
    openGroup,
    status: {
      searching,
      indexing: searching && (results === null || results.status === "indexing"),
      indexed: results?.indexed ?? 0,
      total: results?.total ?? 0,
      matches: results?.matches ?? 0,
      truncated: results?.truncated ?? false,
      error,
    },
    loading,
    libraries: installed.length,
  };
}
