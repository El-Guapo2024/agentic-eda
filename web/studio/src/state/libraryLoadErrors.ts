// The symbol libraries that loaded with errors, as KiCad 10 tells them: `LIBRARY_MANAGER_ADAPTER::GetLibraryLoadErrors` (each library whose load ended in
// `LOAD_STATUS::LOAD_ERROR`, with its `LIB_STATUS::error`), which the frames put in the status bar's warnings once the libraries are loaded
// (`statusBar->AddWarningMessages( "load", adapter->GetLibraryLoadErrors() )`, eeschema/cross-probing.cpp, KiCad 25ec2335). A folder library
// (`.kicad_symdir`) with a file that does not read is one (`SCH_IO_KICAD_SEXPR_LIB_CACHE::Load`: "Unable to read file '%s'" ... "The library cannot be saved
// until these errors are fixed manually."); its good files are read all the same.
//
// Plain module state with subscribers (like `state/libraryTableDialogs.ts`): the server's `GET /api/library/load_errors` fills it, and every chooser answer that
// names a library read with errors (`/api/library/entries`' `load_error`, `/api/library/search`'s `load_errors`) adds to it, so the chooser and the status bar
// say the same.
import { useSyncExternalStore } from "react";

import { mergeLoadErrors, type LibraryLoadError } from "../kicad-port/libraryLoadErrors";

export type { LibraryLoadError };

let current: readonly LibraryLoadError[] = [];
const listeners = new Set<() => void>();

function set(next: readonly LibraryLoadError[]): void {
  current = next;
  for (const l of listeners) l();
}

/** What an answer says of the libraries it read: added to what is known (an answer about some libraries says nothing of the others). */
export function reportLoadErrors(reported: readonly LibraryLoadError[]): void {
  if (reported.length === 0) return;
  const next = mergeLoadErrors(current, reported);
  if (next.length === current.length && next.every((e, i) => e.lib === current[i]?.lib && e.error === current[i]?.error)) return;
  set(next);
}

/** A library read again without errors (a tree or a chooser opened it): no longer one. */
export function clearLoadError(lib: string): void {
  if (current.some((e) => e.lib === lib)) set(current.filter((e) => e.lib !== lib));
}

export function getLibraryLoadErrors(): readonly LibraryLoadError[] {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useLibraryLoadErrors(): readonly LibraryLoadError[] {
  return useSyncExternalStore(subscribe, getLibraryLoadErrors, getLibraryLoadErrors);
}

/** Every symbol library of the tables that loads with errors (`GET /api/library/load_errors`): the whole list, replacing what was known. */
export async function refreshLoadErrors(): Promise<void> {
  try {
    const r = await fetch("/api/library/load_errors?kind=symbol", { cache: "no-store" });
    if (!r.ok) return;
    const j = (await r.json()) as { errors?: LibraryLoadError[] };
    if (Array.isArray(j.errors)) set(mergeLoadErrors([], j.errors.map((e) => ({ lib: e.lib, error: e.error }))));
  } catch {
    /* no server answer: the next one tells */
  }
}
