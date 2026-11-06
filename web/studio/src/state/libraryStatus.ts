// The library editors' `SetStatusText( msg )` (EDA_BASE_FRAME's status bar, field 0): what a library action reports where KiCad reports it -- the status bar,
// not a message box -- "Footprint '%s' deleted from library '%s'" (`DeleteFootprintFromLibrary`), "Footprint '%s' added to '%s'" / "replaced in" (`SaveFootprintAs`),
// "Symbol %s saved to library '%s'." (`ExportSymbol`). The text stays until the next one, as KiCad's does. Plain module state with no React, so the actions
// (which run outside any component) can set it; the status bar reads it with `useLibraryStatusText` on the two library editors' tabs (UI-LEFT-2 LL6).
import { useSyncExternalStore } from "react";

let text = "";
const listeners = new Set<() => void>();

export function setLibraryStatusText(message: string): void {
  text = message;
  for (const l of [...listeners]) l();
}

export function getLibraryStatusText(): string {
  return text;
}

export function useLibraryStatusText(): string {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => text
  );
}
