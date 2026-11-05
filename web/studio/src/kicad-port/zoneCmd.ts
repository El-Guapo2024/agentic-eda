// The `edit_zone` verb as the studio builds it from a zone on the board (crates/ops/src/lib.rs `Cmd::EditZone`, which replaces the zone's settings wholesale):
// shared by the Properties panel (`pcbProperties.ts`, `ZONE_DESC`) and Swap (`pcbSwap.ts`, `EDIT_TOOL::Swap`'s `ZONE::SetLayer`). Pure; no React, no DOM.
import type { Cmd, Zone } from "../api/types";

/**
 * `ZONE_DESC`'s settings, as `edit_zone` takes them: every field of the zone with `patch` laid over it (the verb replaces the zone's settings wholesale). The
 * layer set (`layers`) is sent only when `patch` names it: without it the verb keeps a zone on several layers as it is when `layer` is one of them.
 */
export function zoneEditCmd(z: Zone, patch: Partial<Zone>): Cmd {
  const s = { ...z, ...patch };
  return {
    op: "edit_zone",
    id: z.id,
    net: s.net,
    layer: s.layer,
    ...(patch.layers ? { layers: patch.layers } : {}),
    clearance: s.clearance,
    min_thickness: s.min_thickness,
    thermal_gap: s.thermal_gap,
    thermal_spoke_width: s.thermal_spoke_width,
    pad_connection: s.pad_connection,
    priority: s.priority,
    island_removal_mode: s.island_removal_mode,
    min_island_area: s.min_island_area,
    fill_mode: s.fill_mode,
    hatch_thickness: s.hatch_thickness,
    hatch_gap: s.hatch_gap,
    hatch_orientation_mdeg: s.hatch_orientation_mdeg,
    hatch_smoothing_level: s.hatch_smoothing_level,
    hatch_smoothing_value: s.hatch_smoothing_value,
    hatch_hole_min_area: s.hatch_hole_min_area,
    hatch_border_algorithm: s.hatch_border_algorithm,
    smoothing: s.smoothing,
    corner_radius: s.corner_radius,
    is_rule_area: s.is_rule_area,
    keepout_tracks: s.keepout_tracks,
    keepout_vias: s.keepout_vias,
    keepout_pads: s.keepout_pads,
    keepout_copper_pour: s.keepout_copper_pour,
    keepout_footprints: s.keepout_footprints,
    ...(s.placement ? { placement: s.placement } : {}),
  };
}
