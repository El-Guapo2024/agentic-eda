// The two questions the Design Blocks actions ask, as promises (the actions are plain async functions run from the action registry, outside any React tree, so a question is a
// request on this module-level bus that `components/designBlocks/DesignBlockDialogs.tsx` shows and answers):
//
//   askBlockProperties   `DIALOG_DESIGN_BLOCK_PROPERTIES`: the name, the keywords, the description and the default fields of a block. OK with the values, null for Cancel. The
//                        dialog does the checks it does (`TransferDataFromWindow`: the characters a name may not have; `TransferDataFromGrid`: no field twice).
//   askBlockConfirm      `OKOrCancelDialog` / `IsOK`: "Design block 'X' already exists in library 'L'." with "Overwrite existing design block?" and a button called Overwrite.
//
// With no host mounted a question is answered "no" at once rather than left hanging.
import { useSyncExternalStore } from "react";
import type { BlockField } from "../api/designBlockClient";

export interface BlockValues {
  name: string;
  description: string;
  keywords: string;
  fields: BlockField[];
}

export interface PropertiesQuestion {
  /** The library the block is, or is to be, in (the dialog says so). */
  lib: string;
  initial: BlockValues;
  /** `aDisableName`: an update keeps the block's name. */
  nameLocked: boolean;
  okLabel?: string;
}

export interface ConfirmQuestion {
  title: string;
  message: string;
  /** The question under the message (`OKOrCancelDialog`'s detail). */
  detail?: string;
  okLabel: string;
}

type Request = { id: number } & ({ kind: "properties"; q: PropertiesQuestion; resolve: (v: BlockValues | null) => void } | { kind: "confirm"; q: ConfirmQuestion; resolve: (v: boolean) => void });

let next = 1;
let queue: Request[] = [];
let mounted = 0;
const listeners = new Set<() => void>();

function emit(): void {
  for (const l of [...listeners]) l();
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useBlockQuestions(): Request | undefined {
  return useSyncExternalStore(subscribe, () => queue[0], () => queue[0]);
}

/** The host announces itself while it is on screen. */
export function hostMounted(): () => void {
  mounted += 1;
  return () => {
    mounted -= 1;
    if (mounted === 0) {
      // nobody can answer: every question left is "no"
      for (const r of queue) {
        if (r.kind === "properties") r.resolve(null);
        else r.resolve(false);
      }
      queue = [];
      emit();
    }
  };
}

export function askBlockProperties(q: PropertiesQuestion): Promise<BlockValues | null> {
  return new Promise((resolve) => {
    if (mounted === 0) return resolve(null);
    queue = [...queue, { id: next++, kind: "properties", q, resolve }];
    emit();
  });
}

export function askBlockConfirm(q: ConfirmQuestion): Promise<boolean> {
  return new Promise((resolve) => {
    if (mounted === 0) return resolve(false);
    queue = [...queue, { id: next++, kind: "confirm", q, resolve }];
    emit();
  });
}

/** The question on screen is answered. */
export function answered(id: number): void {
  queue = queue.filter((r) => r.id !== id);
  emit();
}
