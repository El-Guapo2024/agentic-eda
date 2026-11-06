// The list of symbol libraries that loaded with errors (`LIBRARY_MANAGER_ADAPTER::GetLibraryLoadErrors`, KiCad 25ec2335), the pure half of
// state/libraryLoadErrors.ts: how what the server answers is merged, and the status bar's words. Unit tested in libraryLoadErrors.test.ts.

export interface LibraryLoadError {
  /** The library's nickname. */
  lib: string;
  /** KiCad's message (`LIB_STATUS::error`). */
  error: string;
}

/** One list from what was known and what an answer says: a library's newer message replaces its older one, a library once each, in name order. */
export function mergeLoadErrors(known: readonly LibraryLoadError[], reported: readonly LibraryLoadError[]): LibraryLoadError[] {
  const by = new Map(known.map((e) => [e.lib, e]));
  for (const e of reported) by.set(e.lib, e);
  return [...by.values()].sort((a, b) => (a.lib < b.lib ? -1 : a.lib > b.lib ? 1 : 0));
}

/** The status bar's and the chooser's line for the list. */
export function loadErrorSummary(errors: readonly LibraryLoadError[]): string {
  if (errors.length === 0) return "";
  return errors.length === 1 ? `Library '${errors[0]?.lib}' loaded with errors` : `${errors.length} libraries loaded with errors`;
}

/** The whole text of the list, a library's message under its name, as the warning's tooltip and its message box show it. */
export function loadErrorDetails(errors: readonly LibraryLoadError[]): string {
  return errors.map((e) => `Library '${e.lib}' loaded with errors:\n\n${e.error}`).join("\n\n");
}
