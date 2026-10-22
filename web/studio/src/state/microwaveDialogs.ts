// The microwave tools' prompts (GAPS.md item 9, package P19): the text entry dialog every tool asks its size, angle, length and value with (`WX_TEXT_ENTRY_DIALOG`). A tool is a run of questions, so each is a promise the tool awaits (the C++ calls `ShowQuasiModal`
// and goes on when it returns): plain module state with subscribers, like `state/pcbItemDialogs.ts`; `components/MicrowaveDialogs.tsx` renders it.
import { useSyncExternalStore } from "react";

/** `WX_TEXT_ENTRY_DIALOG( frame, message, title, value )`: a message above a text box starting at `initial`. */
export interface TextPrompt {
  kind: "text";
  /** Which question this is, so a dialog that stays mounted starts afresh for the next. */
  id: number;
  title: string;
  label: string;
  initial: string;
  /** The text the box takes (a validator): OK is off while it says no. */
  accept?: (text: string) => boolean;
  /** What a text the box does not take is told (shown under the box). */
  hint?: string;
  resolve: (text: string | null) => void;
}

export type MicrowavePrompt = TextPrompt;

let current: MicrowavePrompt | null = null;
let sequence = 0;
const listeners = new Set<() => void>();

function cancel(p: MicrowavePrompt): void {
  p.resolve(null);
}

function set(next: MicrowavePrompt | null): void {
  // a question asked while another is open takes its place: the one it replaces is cancelled
  const old = current;
  current = next;
  if (old) cancel(old);
  for (const l of listeners) l();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getMicrowavePrompt(): MicrowavePrompt | null {
  return current;
}

export function useMicrowavePrompt(): MicrowavePrompt | null {
  return useSyncExternalStore(subscribe, getMicrowavePrompt, getMicrowavePrompt);
}

/** The text entry dialog. Resolves to the text of OK, or `null` for Cancel (`ShowQuasiModal() != wxID_OK`). */
export function askText(prompt: Omit<TextPrompt, "kind" | "id" | "resolve">): Promise<string | null> {
  return new Promise((resolve) => set({ ...prompt, kind: "text", id: ++sequence, resolve }));
}

/** OK (`text`) or Cancel (`null`) of the text entry dialog. */
export function answerText(text: string | null): void {
  const p = current;
  if (!p) return;
  current = null;
  for (const l of listeners) l();
  p.resolve(text);
}
