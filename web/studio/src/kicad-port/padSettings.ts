// The pad tool's "default pad" (`BOARD_DESIGN_SETTINGS::m_Pad_Master`) and the one rule that moves a pad's
// settings around (`PAD::ImportSettingsFrom`), shared by Place Pad (`PAD_PLACER::CreateItem`), Copy / Paste
// Pad Properties (`PAD_TOOL::copyPadSettings` / `pastePadProperties`), Push Pad Properties and the Default Pad
// Properties dialog. `crates/ops/src/library_editors.rs::import_pad_settings` is the same algorithm on the backend
// (Push Pad Properties runs there); keep the two identical.
import type { LibraryPad, LibraryPadShape, PadConnection } from "../api/types";
import { padCanHaveNumber } from "./fpEditActions";

/** A pad's settings: everything but the id, the number and the position (what `ImportSettingsFrom` never touches). */
export type PadSettings = Omit<LibraryPad, "id" | "number" | "at">;

/**
 * `BOARD_DESIGN_SETTINGS::SetDefaultMasterPad` on a `PAD` just built by `PAD::PAD( FOOTPRINT* )`:
 * size `DEFAULT_PAD_WIDTH_MM` x `DEFAULT_PAD_HEIGTH_MM` (2.54 x 1.27), a round rectangle whose corner radius is
 * `DEFAULT_PAD_RR_RADIUS_RATIO` (0.15) of the height, a round hole of `DEFAULT_PAD_DRILL_DIAMETER_MM` (0.8), and
 * what the constructor leaves: the pad type is plated through-hole (`m_attribute = PAD_ATTRIB::PTH`) on the
 * through-hole layer set (`PTHMask()`: every copper layer and both masks).
 */
export const DEFAULT_PAD_MASTER: PadSettings = {
  offset: { x: 0, y: 0 },
  size: [2540, 1270],
  shape: "round_rect",
  kind: "through_hole",
  drill: 800,
  drill_slot: null,
  rot: 0,
  roundrect_ratio: 0.15,
  trapezoid_delta: null,
  chamfer_ratio: null,
  chamfer_corners: { top_left: false, top_right: false, bottom_left: false, bottom_right: false },
  layers: ["*.Cu", "*.Mask"],
  clearance_override: null,
  thermal_gap_override: null,
  thermal_spoke_width_override: null,
  zone_connection: null,
  thermal_spoke_angle_mdeg: null,
};

/** A pad's settings, as the master pad holds them (`m_Pad_Master->ImportSettingsFrom( pad )`). */
export function settingsOf(pad: LibraryPad): PadSettings {
  return {
    offset: pad.offset,
    size: pad.size,
    shape: pad.shape,
    kind: pad.kind,
    drill: pad.drill,
    drill_slot: pad.drill_slot,
    rot: pad.rot,
    roundrect_ratio: pad.roundrect_ratio,
    trapezoid_delta: pad.trapezoid_delta,
    chamfer_ratio: pad.chamfer_ratio,
    chamfer_corners: pad.chamfer_corners,
    layers: [...pad.layers],
    clearance_override: pad.clearance_override,
    thermal_gap_override: pad.thermal_gap_override,
    thermal_spoke_width_override: pad.thermal_spoke_width_override,
    zone_connection: pad.zone_connection ?? null,
    thermal_spoke_angle_mdeg: pad.thermal_spoke_angle_mdeg ?? null,
  };
}

/**
 * `PAD::ImportSettingsFrom( aMasterPad )` for what the library pad models: padstack, layers, pad type, orientation
 * and the local overrides come from `master`; the number and the position stay. Then the C++'s three fix-ups: a circle
 * master squares the size from its x (`SetSize( x, x )`), an SMD master drops the hole (`SetDrillSize( 0, 0 )`), and a
 * pad that cannot have a number loses it (`if( !CanHaveNumber() ) SetNumber( wxEmptyString )`).
 */
export function importPadSettings(dst: LibraryPad, master: PadSettings): LibraryPad {
  const out: LibraryPad = { ...dst, ...settingsOf({ ...dst, ...master }) };
  if (master.shape === "circle") out.size = [out.size[0], out.size[0]];
  if (master.kind === "smd") {
    out.drill = null;
    out.drill_slot = null;
  }
  if (!padCanHaveNumber(out)) out.number = "";
  return out;
}

/**
 * `PAD_PLACER::CreateItem`: a new pad is the master's settings with a number -- `padNumber` for a pad that can have one,
 * nothing for one that cannot (`if( pad->CanHaveNumber() ) { ... pad->SetNumber( padNumber ); }`) -- at `at`.
 */
export function newPadFromMaster(master: PadSettings, number: string, at: { x: number; y: number }): LibraryPad {
  return importPadSettings({ ...master, layers: [...master.layers], number, at }, master);
}

/** The dialog's "Pad connection" (`m_ZoneConnectionChoice`): the pad's own override of how a zone connects to it; "From parent footprint" is no override. The zone's "thermal reliefs for PTH only" is a zone setting, not a pad one. */
export const PAD_CONNECTION_OPTIONS: { value: PadConnection | ""; label: string }[] = [
  { value: "", label: "From parent footprint" },
  { value: "Full", label: "Solid" },
  { value: "Thermal", label: "Thermal relief" },
  { value: "None", label: "None" },
];

/** `PADSTACK::DefaultThermalSpokeAngleForShape`, degrees: a + for an oval or (rounded) rectangle, an X for everything else. */
export function defaultSpokeAngleDeg(shape: LibraryPadShape): number {
  return shape === "oval" || shape === "rect" || shape === "round_rect" || shape === "chamfered_rect" ? 90 : 45;
}
