// The microwave tools' prompts (GAPS.md item 9, package P19): the text entry dialog every tool asks its size, angle, length and value with (`WX_TEXT_ENTRY_DIALOG`) and the
// Complex Shape dialog of the polygonal shape (`MWAVE_POLYGONAL_SHAPE_DLG`). A tool is a run of questions, so each is a promise the tool awaits (the C++ calls `ShowQuasiModal`
// and goes on when it returns): plain module state with subscribers, like `state/pcbItemDialogs.ts`; `components/MicrowaveDialogs.tsx` renders it.
import { useSyncExternalStore } from "react";
import type { MicrowaveShapeKind } from "../api/microwaveTypes";
import type { LengthUnit } from "./units";

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

/** What the Complex Shape dialog answered (OK): the Shape box, the Size boxes as typed and as they were first shown, and the file read, if any. */
export interface ShapeAnswer {
  kind: MicrowaveShapeKind;
  sizeX: string;
  sizeY: string;
  shownX: string;
  shownY: string;
  /** The text of the shape description file (`null`: none was read). */
  file: string | null;
}

export interface ShapePrompt {
  kind: "shape";
  id: number;
  units: LengthUnit;
  resolve: (answer: ShapeAnswer | null) => void;
}

export type MicrowavePrompt = TextPrompt | ShapePrompt;

let current: MicrowavePrompt | null = null;
let sequence = 0;
const listeners = new Set<() => void>();

function cancel(p: MicrowavePrompt): void {
  if (p.kind === "text") p.resolve(null);
  else p.resolve(null);
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

/** The Complex Shape dialog. Resolves to what OK answered, or `null` for Cancel. */
export function askShape(units: LengthUnit): Promise<ShapeAnswer | null> {
  return new Promise((resolve) => set({ kind: "shape", id: ++sequence, units, resolve }));
}

/** OK (`text`) or Cancel (`null`) of the text entry dialog. */
export function answerText(text: string | null): void {
  const p = current;
  if (!p || p.kind !== "text") return;
  current = null;
  for (const l of listeners) l();
  p.resolve(text);
}

/** OK (`answer`) or Cancel (`null`) of the Complex Shape dialog. */
export function answerShape(answer: ShapeAnswer | null): void {
  const p = current;
  if (!p || p.kind !== "shape") return;
  current = null;
  for (const l of listeners) l();
  p.resolve(answer);
}
