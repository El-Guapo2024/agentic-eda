// The image the schematic's Place Image tool holds (`SCH_DRAWING_TOOLS::PlaceImage`): after the file is chosen the image follows the cursor by its centre until a click puts it down.
// SchematicView draws it at the grid-snapped cursor; the tool (actions/schImageActions.ts) sets and clears it. A store of its own, read with `useSyncExternalStore`, like the paste in the hand
// (state/schPasteStore.ts), so `store.tsx` -- shared by every editor -- keeps no schematic-only field.
import { useSyncExternalStore } from "react";

export interface SchImageHand {
  name: string;
  /** The file, base64: what the `add_graphic` command carries. */
  data: string;
  /** The decoded bitmap the cursor draws. */
  img: HTMLImageElement;
  px: [number, number];
  ppi: number;
}

let current: SchImageHand | null = null;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());
const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
};

export const getSchImageHand = (): SchImageHand | null => current;

export function setSchImageHand(next: SchImageHand | null): void {
  current = next;
  emit();
}

export const useSchImageHand = (): SchImageHand | null => useSyncExternalStore(subscribe, getSchImageHand, getSchImageHand);
