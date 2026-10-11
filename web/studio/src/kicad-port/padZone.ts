// The zone fields of Footprint Properties and Pad Properties on a footprint placed on the board
// (`dialog_footprint_properties.cpp`'s Clearances tab, `dialog_pad_properties.cpp`'s "Zone connection", "Relief gap", "Spoke
// width", "Spoke angle" and "Clearance"): what a part carries, as form state, and the `Cmd` each form commits. Pure -- no
// React -- so the dialog stays a thin view of it. `null` is "inherited" throughout, as in KiCad (`ZONE_CONNECTION::INHERITED`,
// an unset `std::optional`).
import type { Cmd, PadConnection, Part, Um } from "../api/types";

/** One pad's own zone facts, as the dialog edits them. */
export interface PadZoneForm {
  connection: PadConnection | null;
  gap: Um | null;
  spokeWidth: Um | null;
  /** Millidegrees, KiCad's sign convention (as in the `.kicad_pcb`). */
  spokeAngleMdeg: number | null;
  clearance: Um | null;
}

export const NO_PAD_ZONE_FACTS: PadZoneForm = { connection: null, gap: null, spokeWidth: null, spokeAngleMdeg: null, clearance: null };

/** The footprint's own facts. */
export interface FootprintZoneForm {
  connection: PadConnection | null;
  clearance: Um | null;
}

/** The pad numbers of a placed part, each once, in the order they are first met (a number can be shared by several pads, which all go on one pin; a numberless pad -- a mounting hole -- has nothing to address). */
export function padNumbers(part: Part | undefined): string[] {
  const seen: string[] = [];
  for (const p of part?.pads ?? []) if (p.num !== "" && !seen.includes(p.num)) seen.push(p.num);
  return seen;
}

/** What the pads numbered `number` carry of their own: the first one that sets anything, else nothing. */
export function padZoneForm(part: Part | undefined, number: string): PadZoneForm {
  const f = part?.zone?.pads.find((p) => p.num === number);
  if (!f) return { ...NO_PAD_ZONE_FACTS };
  return { connection: f.connection ?? null, gap: f.gap ?? null, spokeWidth: f.spoke_width ?? null, spokeAngleMdeg: f.spoke_angle_mdeg ?? null, clearance: f.clearance ?? null };
}

export function footprintZoneForm(part: Part | undefined): FootprintZoneForm {
  return { connection: part?.zone?.connection ?? null, clearance: part?.zone?.clearance ?? null };
}

/** `Cmd::SetPadZoneOverrides`: a whole-panel commit for the pads numbered `pad` of `part`. An angle is kept within one turn, a length at zero or more. */
export function setPadZoneCmd(part: string, pad: string, f: PadZoneForm): Cmd {
  const len = (v: Um | null) => (v == null ? null : Math.max(0, Math.round(v)));
  return {
    op: "set_pad_zone_overrides",
    part,
    pad,
    zone_connection: f.connection,
    thermal_gap: len(f.gap),
    thermal_spoke_width: len(f.spokeWidth),
    thermal_spoke_angle_mdeg: f.spokeAngleMdeg == null ? null : ((Math.round(f.spokeAngleMdeg) % 360_000) + 360_000) % 360_000,
    clearance: len(f.clearance),
  };
}

/** `Cmd::SetFootprintZoneConnection`. */
export function setFootprintZoneCmd(part: string, f: FootprintZoneForm): Cmd {
  return { op: "set_footprint_zone_connection", part, zone_connection: f.connection, clearance: f.clearance == null ? null : Math.max(0, Math.round(f.clearance)) };
}

/** Does this pad's form set anything of its own? (A pad that sets nothing inherits everything.) */
export function padZoneIsSet(f: PadZoneForm): boolean {
  return f.connection != null || f.gap != null || f.spokeWidth != null || f.spokeAngleMdeg != null || f.clearance != null;
}

/**
 * The commands a dialog's OK sends for its zone fields: the footprint's (`set_footprint_zone_connection`) when its form differs from what the part carries,
 * and one `set_pad_zone_overrides` for every pad number whose form was opened and differs. None for a form nothing was changed on.
 */
export function zoneCmds(part: Part, footprint: FootprintZoneForm, pads: Readonly<Record<string, PadZoneForm>>): Cmd[] {
  const cmds: Cmd[] = [];
  const was = footprintZoneForm(part);
  if (footprint.connection !== was.connection || footprint.clearance !== was.clearance) cmds.push(setFootprintZoneCmd(part.ref, footprint));
  for (const [number, form] of Object.entries(pads)) if (JSON.stringify(form) !== JSON.stringify(padZoneForm(part, number))) cmds.push(setPadZoneCmd(part.ref, number, form));
  return cmds;
}

