// Which footprint text Text Properties is open on (UI-LEFT P15: `DIALOG_TEXT_PROPERTIES` on a `PCB_TEXT` whose parent is a footprint, `REF~t<k>`). Plain module
// state with subscribers, as `state/pcbItemDialogs.ts`; `components/FootprintTextDialog.tsx` renders it.
import { useSyncExternalStore } from "react";

let current: string | null = null;
const listeners = new Set<() => void>();

function set(next: string | null): void {
  current = next;
  for (const l of listeners) l();
}

export const getFpTextDialog = (): string | null => current;
export function useFpTextDialog(): string | null {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    getFpTextDialog,
    getFpTextDialog
  );
}
export const openFpTextDialog = (id: string): void => set(id);
export const closeFpTextDialog = (): void => set(null);
