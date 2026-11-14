// Board Setup > Design Rules > Microvia Stacks: the pure parts of KiCad 25ec2335 pcbnew/dialogs/panel_setup_via_stacks.cpp (`rebuildGrid`, `nameIsFree`, `onAdd`)
// and dialog_microvia_stack.cpp (`TransferDataFromWindow`'s checks, `updateEnableState`, `showNetclassViaSize`), over `VIA_STACK_PRESET`
// (include/board_design_settings.h). The page is components/boardSetup/ViaStacksPage.tsx (UI-LEFT-3 B3-8).
import type { NetClass, ViaStackPreset } from "../api/types";

/** `MAX_MICROVIA_STACK_PITCH_MM` (board_design_settings_defaults.h): 25 mm. */
export const MAX_MICROVIA_STACK_PITCH_NM = 25_000_000;
/** `GEOMETRY_MIN_SIZE` (pcb_track.h): 1 um. */
export const GEOMETRY_MIN_SIZE_NM = 1_000;

/** `VIA_STACK_PRESET()`: F.Cu to In1.Cu, stacked, no sizes, copper-filled, not capped, no pitch (`PANEL_SETUP_VIA_STACKS::onAdd`'s new preset). */
export function newViaStackPreset(name: string): ViaStackPreset {
  return { name, start_layer: "F.Cu", end_layer: "In1.Cu", staggered: false, via_size_nm: 0, via_drill_nm: 0, use_netclass: false, filled: true, capped: false, pitch_nm: 0 };
}

/** `nameIsFree`: no preset but the row `ignore` has the name, compared without regard to case (`CmpNoCase`). */
export function nameIsFree(presets: readonly ViaStackPreset[], name: string, ignore: number): boolean {
  return !presets.some((p, row) => row !== ignore && p.name.toLowerCase() === name.toLowerCase());
}

/** `onAdd`'s and `onCellChanged`'s message for a name already taken. */
export const nameTakenMessage = (name: string): string => `A preset named "${name}" already exists.`;

/** `rebuildGrid`'s cells of a row: Name, Type, Layers ("F.Cu -> In1.Cu" by the board's layer names), Diameter / Hole ("netclass" or the two sizes), Fill, Pitch. */
export function presetRow(p: ViaStackPreset, layerName: (l: string) => string, length: (nm: number, withUnit: boolean) => string): string[] {
  return [
    p.name,
    p.staggered ? "Staggered" : "Stacked",
    `${layerName(p.start_layer)} -> ${layerName(p.end_layer)}`,
    p.use_netclass ? "netclass" : `${length(p.via_size_nm, false)} / ${length(p.via_drill_nm, true)}`,
    p.filled ? "yes" : "no",
    p.staggered ? length(p.pitch_nm, true) : "-",
  ];
}

/**
 * `DIALOG_MICROVIA_STACK::TransferDataFromWindow`'s checks, in its order and with its messages word for word (`PCB_VIA_STACK::IsSpanValid`,
 * `PCB_VIA::ValidateViaParameters` for the sizes when the net class does not give them); `copper` is the board's copper layers, `pitchLimit` the 25 mm limit as
 * the dialog writes it in the user's units. Null when the dialog may close.
 */
export function microviaStackProblem(p: ViaStackPreset, copper: readonly string[], pitchLimit: string): string | null {
  if (p.start_layer === p.end_layer) return "The start and end layers must be different.";
  if (!copper.includes(p.start_layer) || !copper.includes(p.end_layer)) return "Select a start and an end layer present on this board.";
  if (p.staggered && !(p.pitch_nm > 0)) return "Staggered stacks need a pitch greater than zero.";
  if (p.pitch_nm > MAX_MICROVIA_STACK_PITCH_NM) return `The pitch must be ${pitchLimit} or less.`;
  if (!p.use_netclass) {
    if (!(p.via_size_nm >= GEOMETRY_MIN_SIZE_NM)) return "Via diameter is too small.";
    if (!(p.via_drill_nm >= GEOMETRY_MIN_SIZE_NM)) return "Via drill is too small.";
    if (p.via_size_nm <= p.via_drill_nm) return "Via hole size must be smaller than via diameter";
  }
  return null;
}

/** `TransferDataFromWindow`'s writes: a stacked stack is always filled ("Stacked stacks land on filled copper"), and with "Use netclass values" no sizes are kept. */
export function settledPreset(p: ViaStackPreset): ViaStackPreset {
  return { ...p, filled: p.staggered ? p.filled : true, via_size_nm: p.use_netclass ? 0 : p.via_size_nm, via_drill_nm: p.use_netclass ? 0 : p.via_drill_nm };
}

/** `PCB_VIA_STACK::GetNetclassViaSize( board, 0 )` of the preset dialog's stack (no net: the Default class): the class's microvia sizes, else the board's via size. */
export function netclassViaSize(defaultClass: NetClass | undefined, boardVia: { diameter: number; drill: number }): { size: number; drill: number } {
  return { size: defaultClass?.microvia_diameter ?? boardVia.diameter, drill: defaultClass?.microvia_drill ?? boardVia.drill };
}

/** The page's checks before Apply (`TransferDataFromWindow` of the page): the names given and free, and each preset one the dialog would close on. */
export function viaStacksProblem(presets: readonly ViaStackPreset[], copper: readonly string[], pitchLimit: string): { row: number; message: string } | null {
  for (let row = 0; row < presets.length; row++) {
    const p = presets[row]!;
    if (p.name.trim() === "") return { row, message: "A preset needs a name." };
    // the second of two alike is the one named (the first keeps the name, as the project's reader keeps it)
    if (!nameIsFree(presets.slice(0, row), p.name, -1)) return { row, message: nameTakenMessage(p.name) };
    const why = microviaStackProblem(p, copper, pitchLimit);
    if (why) return { row, message: why };
  }
  return null;
}
