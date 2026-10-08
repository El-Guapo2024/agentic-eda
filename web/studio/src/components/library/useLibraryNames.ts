// The name list behind a library tree: every footprint / symbol the editor can open, and which of them are entries of the
// project library itself -- polled like the editors poll their open document; `notifyLibraryChanged()` (called by the library
// actions after a verb) refreshes every mounted list at once instead of waiting for the next tick -- plus KiCad's INSTALLED libraries:
// the library names the moment the tree mounts, one library's item names when it is opened (`load`), every library's names only when the
// search box needs them (`ensureSearchIndex`). Everything installed is read from the server once and kept (api/libraryIndexClient.ts).
import { useCallback, useEffect, useRef, useState } from "react";
import { fetchFootprintLibraryNames, fetchSymbolEditorNames } from "../../api/client";
import { fetchAllInstalledItems, fetchInstalledItems, fetchInstalledLibraries, knownInstalledItems, knownInstalledLibraries, knownSearchIndex, type LibraryKind } from "../../api/libraryIndexClient";
import type { InstalledLib, TreeItem } from "../../kicad-port/libraryTreeModel";

const listeners = new Set<() => void>();

/** Tell every mounted library tree to re-read its names now. */
export function notifyLibraryChanged(): void {
  for (const l of [...listeners]) l();
}

export type { LibraryKind };

export interface LibraryNames {
  /** What the editor already lists: the project's entries, what the board's model resolves, the built-in table. */
  items: TreeItem[];
  refresh: () => Promise<void>;
  /** KiCad's installed libraries (names, counts where cheap). */
  installed: InstalledLib[];
  /** The item names of every installed library loaded so far, by library (bare names). */
  loaded: ReadonlyMap<string, readonly string[]>;
  /** Libraries whose items are being fetched right now. */
  loading: ReadonlySet<string>;
  /** Load one installed library's items (no-op when loaded or on its way). */
  load: (lib: string) => void;
  /** The search box needs every library's names: "loading" while the server builds them. */
  searchIndex: "idle" | "loading" | "ready";
  ensureSearchIndex: () => void;
}

export function useLibraryNames(kind: LibraryKind): LibraryNames {
  const [items, setItems] = useState<TreeItem[]>([]);
  const [installed, setInstalled] = useState<InstalledLib[]>(() => knownInstalledLibraries(kind) ?? []);
  const [loaded, setLoaded] = useState<ReadonlyMap<string, readonly string[]>>(() => knownInstalledItems(kind));
  const [loading, setLoading] = useState<ReadonlySet<string>>(new Set());
  const [searchIndex, setSearchIndex] = useState<"idle" | "loading" | "ready">(() => (knownSearchIndex(kind) ? "ready" : "idle"));
  // Read through refs by the stable callbacks below (an effect in the tree calls `load` on every render that opens a group).
  const loadedRef = useRef(loaded);
  loadedRef.current = loaded;
  const loadingRef = useRef(new Set<string>());
  const searchStarted = useRef(searchIndex !== "idle");
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    try {
      const r = kind === "footprint" ? await fetchFootprintLibraryNames() : await fetchSymbolEditorNames();
      const project = new Set(r.project ?? []);
      setItems(r.names.map((name) => ({ name, project: project.has(name) })));
    } catch {
      /* the backend is busy or restarting: keep the list we have */
    }
  }, [kind]);

  useEffect(() => {
    void refresh();
    const id = setInterval(() => void refresh(), 1500);
    const l = () => void refresh();
    listeners.add(l);
    return () => {
      clearInterval(id);
      listeners.delete(l);
    };
  }, [refresh]);

  // The installed libraries' names: one cheap request, kept for the page's lifetime.
  useEffect(() => {
    let cancelled = false;
    fetchInstalledLibraries(kind)
      .then((libs) => {
        if (!cancelled) setInstalled(libs);
      })
      .catch(() => {
        /* no server answer or no KiCad install: the tree shows the project's entries alone */
      });
    return () => {
      cancelled = true;
    };
  }, [kind]);

  const load = useCallback(
    (lib: string) => {
      if (loadedRef.current.has(lib) || loadingRef.current.has(lib)) return;
      loadingRef.current.add(lib);
      setLoading(new Set(loadingRef.current));
      fetchInstalledItems(kind, lib)
        .then((names) => {
          if (alive.current) setLoaded((prev) => new Map(prev).set(lib, names));
        })
        .catch(() => {
          /* the library vanished or the server is busy: the group stays empty and can be opened again */
        })
        .finally(() => {
          loadingRef.current.delete(lib);
          if (alive.current) setLoading(new Set(loadingRef.current));
        });
    },
    [kind]
  );
  const ensureSearchIndex = useCallback(() => {
    if (searchStarted.current) return;
    searchStarted.current = true;
    setSearchIndex("loading");
    fetchAllInstalledItems(kind)
      .then((all) => {
        if (!alive.current) return;
        setLoaded((prev) => new Map([...prev, ...Object.entries(all)]));
        setSearchIndex("ready");
      })
      .catch(() => {
        searchStarted.current = false;
        if (alive.current) setSearchIndex("idle");
      });
  }, [kind]);

  return { items, refresh, installed, loaded, loading, load, searchIndex, ensureSearchIndex };
}
