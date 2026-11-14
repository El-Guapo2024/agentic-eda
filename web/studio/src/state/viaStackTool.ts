// KiCad 10's microvia stack in the board editor (UI-LEFT-3 X3-1): the toolbar's preset choice (`BOARD_DESIGN_SETTINGS::m_viaStackIndex`, kept for the session as KiCad
// keeps it: no file has it), Place Microvia Stack's loop (`DRAWING_TOOL::PlaceMicroviaStack`: the settings it places, the hops a staggered stack has so far, the
// preview) and the Microvia Stack dialog (`DIALOG_MICROVIA_STACK`: on a placed stack, `PCB_VIA_STACK::ShowPropertiesDialog`, or on the tool's settings when the board
// has no preset). Plain module state with subscribers (like `state/kicad10Dialogs.ts`), so the action runner, the canvas and the dialog share it without the reducer.
import { useSyncExternalStore } from "react";
import type { BoardViaStack } from "../api/pcbItemTypes";
import type { PreviewMember } from "../kicad-port/viaStack";
import type { Pt } from "../kicad-port/snapGeom";

export interface ViaStackPlacement {
  /** The settings placed (`PCB_VIA_STACK settings`). */
  settings: BoardViaStack;
  /** The span's copper layers in stack order (`spanLayers`). */
  span: string[];
  /** The hops placed so far (`hops`): a staggered stack's. */
  hops: Pt[];
  /** `lastDir`: where the next hop starts, after a click. */
  lastDir: Pt;
  /** The members the preview draws (`previewItems`). */
  preview: PreviewMember[];
}

export interface ViaStackToolState {
  /** `GetViaStackIndex()`: the toolbar's preset. */
  index: number;
  placement: ViaStackPlacement | null;
  /** The Microvia Stack dialog: on a placed stack (`id`), or on the tool's settings (`id` null). */
  dialog: { id: string | null; stack: BoardViaStack } | null;
}

let current: ViaStackToolState = { index: 0, placement: null, dialog: null };
const listeners = new Set<() => void>();

function set(next: ViaStackToolState): void {
  current = next;
  for (const l of listeners) l();
}

export function getViaStackTool(): ViaStackToolState {
  return current;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useViaStackTool(): ViaStackToolState {
  return useSyncExternalStore(subscribe, getViaStackTool, getViaStackTool);
}

export function setViaStackIndex(index: number): void {
  set({ ...current, index });
}

export function setViaStackPlacement(placement: ViaStackPlacement | null): void {
  set({ ...current, placement });
}

export function openViaStackDialog(id: string | null, stack: BoardViaStack): void {
  set({ ...current, dialog: { id, stack } });
}

export function closeViaStackDialog(): void {
  set({ ...current, dialog: null });
}
