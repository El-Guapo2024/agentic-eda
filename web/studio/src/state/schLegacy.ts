// Whether the schematic is a legacy one (`SCH_SCREENS::HasNoFullyDefinedLibIds`: it has symbols and none names a library), which alone enables Tools > Remap Legacy
// Library Symbols... (`SCH_EDIT_FRAME::setupUIConditions`' `remapSymbolsCondition`: "The remapping can only be performed on legacy projects."; UI-LEFT-2 LL7).
// The server answers it over every sheet (`GET /api/sch/legacy`); asked again, a little after, whenever the schematic the page holds changes.
import { useEffect, useSyncExternalStore } from "react";

let legacy = false;
const listeners = new Set<() => void>();
let timer: ReturnType<typeof setTimeout> | undefined;
let asked: unknown = undefined;

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

function set(v: boolean): void {
  if (v === legacy) return;
  legacy = v;
  for (const l of listeners) l();
}

/** The answer for `schematic` (any value that changes when the schematic does): asked once per change, 300 ms after it. */
export function useSchLegacy(schematic: unknown): boolean {
  useEffect(() => {
    if (schematic === asked) return;
    asked = schematic;
    clearTimeout(timer);
    if (!schematic) return set(false);
    timer = setTimeout(() => {
      fetch("/api/sch/legacy")
        .then((r) => r.json() as Promise<{ legacy?: boolean }>)
        .then((j) => set(!!j.legacy))
        .catch(() => set(false));
    }, 300);
  }, [schematic]);
  return useSyncExternalStore(subscribe, () => legacy, () => legacy);
}

/** The action this condition enables. */
export const REMAP_SYMBOLS_ACTION = "eeschema.EditorControl.remapSymbols";
