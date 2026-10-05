// The name list behind a library tree: every footprint / symbol the editor can open, and which of them are entries of the
// project library itself. Polled like the editors poll their open document; `notifyLibraryChanged()` (called by the library
// actions after a verb) refreshes every mounted list at once instead of waiting for the next tick.
import { useCallback, useEffect, useState } from "react";
import { fetchFootprintLibraryNames, fetchSymbolEditorNames } from "../../api/client";
import type { TreeItem } from "./LibraryTree";

const listeners = new Set<() => void>();

/** Tell every mounted library tree to re-read its names now. */
export function notifyLibraryChanged(): void {
  for (const l of [...listeners]) l();
}

export type LibraryKind = "footprint" | "symbol";

export function useLibraryNames(kind: LibraryKind): { items: TreeItem[]; refresh: () => Promise<void> } {
  const [items, setItems] = useState<TreeItem[]>([]);

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

  return { items, refresh };
}
