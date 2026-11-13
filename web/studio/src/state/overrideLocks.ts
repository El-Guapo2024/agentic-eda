// The "Override locks" check box of the board editor's and the schematic editor's top toolbar: its state and readers are kicad-port/overrideLocks.ts
// (pure, read by the kicad-port modules); this adds the hook the toolbar box renders with.
import { useSyncExternalStore } from "react";
import { getOverrideLocks, subscribeOverrideLocks, type OverrideLocksFrame } from "../kicad-port/overrideLocks";

export { getOverrideLocks, setOverrideLocks, LOCKED_ITEMS_FILTERED, type OverrideLocksFrame } from "../kicad-port/overrideLocks";

export function useOverrideLocks(frame: OverrideLocksFrame): boolean {
  return useSyncExternalStore(subscribeOverrideLocks, () => getOverrideLocks(frame));
}
