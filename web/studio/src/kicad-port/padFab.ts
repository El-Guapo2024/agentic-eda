// The pad's fabrication property and pad-to-die length and delay as Pad Properties shows and sets them (`DIALOG_PAD_PROPERTIES` at KiCad 8303b2ad:
// `m_choiceFabProperty` and `getSelectedProperty`, `m_padToDieOpt` / `m_padToDie`, `m_padToDieDelayOpt` / `m_padToDieDelay`), over the words the board file writes
// (`PCB_IO_KICAD_SEXPR::format( const PAD* )`: `(property pad_prop_bga)` .., `(die_length ..)`, `(die_delay ..)` in picoseconds).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Nm, PadEditCmd } from "../api/types";

/** `m_choiceFabPropertyChoices`, in the dialog's order (`getSelectedProperty`), with the word the file writes for each. */
export const FAB_PROPERTY_CHOICES: readonly [string, string][] = [
  ["pad_prop_none", "None"],
  ["pad_prop_bga", "BGA pad"],
  ["pad_prop_fiducial_loc", "Fiducial, local to footprint"],
  ["pad_prop_fiducial_glob", "Fiducial, global to board"],
  ["pad_prop_testpoint", "Test point pad"],
  ["pad_prop_heatsink", "Heatsink pad"],
  ["pad_prop_mechanical", "Mechanical"],
  ["pad_prop_castellated", "Castellated pad (TH only)"],
  ["pad_prop_pressfit", "Press-fit (round TH only)"],
];

/** `IU_PER_PS`: KiCad keeps a delay in attoseconds. */
export const AS_PER_PS = 1e6;

/** What Pad Properties shows of a pad: its property (`pad_prop_none` for none), and its pad-to-die length (nm) and delay (attoseconds), null when not specified. */
export interface PadFabForm {
  property: string;
  dieLength: Nm | null;
  dieDelay: number | null;
}

export function padFabForm(pad: { property?: string | null; die_length?: Nm | null; die_delay?: number | null }): PadFabForm {
  return { property: pad.property || "pad_prop_none", dieLength: pad.die_length || null, dieDelay: pad.die_delay || null };
}

/**
 * `transferDataToPad`'s part of them: `SetProperty( getSelectedProperty() )`, `SetPadToDieLength( m_padToDieOpt ? value : 0 )`, `SetPadToDieDelay( .. )` --
 * the keys of the pad's edit that change. A pad of no copper connection (`hasConnection` false: an NPTH) has neither length nor delay.
 */
export function padFabPatch(before: PadFabForm, form: PadFabForm, hasConnection: boolean): Partial<PadEditCmd> {
  const patch: Partial<PadEditCmd> = {};
  if (form.property !== before.property) patch.property = form.property;
  const len = hasConnection ? (form.dieLength ?? 0) : 0;
  const delay = hasConnection ? (form.dieDelay ?? 0) : 0;
  if (len !== (before.dieLength ?? 0)) patch.die_length = len;
  if (delay !== (before.dieDelay ?? 0)) patch.die_delay = Math.round(delay);
  return patch;
}
