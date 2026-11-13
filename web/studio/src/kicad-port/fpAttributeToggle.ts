// The board editor's footprint attribute toggles (UI-LEFT-3 P3-4): the selection tool's context menu "Attributes" submenu -- Exclude from Bill of
// Materials, Exclude from Simulation, Exclude from Position Files -- on the selected footprints, in the board's current variant.
//
// Ports KiCad 25ec2335 pcbnew/tools/edit_tool.cpp:
//   EDIT_TOOL::ToggleFootprintAttribute         `RequestSelection( FootprintFilter )` (footprints only); the new state is true when any of them lacks the
//                                               attribute in the variant, else false; each footprint takes it -- in its variant (`FOOTPRINT_VARIANT`) when one
//                                               is current, else as its own attribute -- in one commit ("Toggle Attribute")
//   EDIT_TOOL::Init's excludeFromBOMCond ..     the check mark: every footprint of the selection has the attribute (`checked > 0 && unchecked == 0`)
//   FOOTPRINT::GetExcludedFromBOMForVariant ..  an empty variant name (or the default's) is the footprint's own attribute
//   pcbnew/tools/pcb_actions.cpp                the three actions (`toggleExcludeFromBOM`, `toggleExcludeFromSim`, `toggleExcludeFromPosFiles`)
// KiCad 25ec2335 has no DNP toggle among them (DNP is Footprint Properties' and the symbol's).
//
// Where the studio keeps the attribute (one design for board and schematic): Exclude from BOM and from Position Files are the footprint's own attributes
// (`edit_board_footprint`, whose BOM flag the symbol shares: `Design::sync_symbol_bom_flags`); Exclude from Simulation is the symbol's (`set_symbol_attrs`;
// the studio's footprint has no flag of its own, KiCad's `FP_EXCLUDE_FROM_SIM` is copied from it). A variant's attributes are the symbol's record of the
// variant (`set_symbol_variant`, PARITY-pcb section 110), so a footprint with no symbol keeps its default-variant attributes in a variant.
//
// Pure: no store, no DOM.

import type { Cmd, Part } from "../api/types";
import { editAttrsCmd } from "./fpFields";

export type FpAttribute = "bom" | "sim" | "pos";

/** The actions (KiCad 25ec2335 `PCB_ACTIONS::toggleExcludeFromBOM` / `toggleExcludeFromSim` / `toggleExcludeFromPosFiles`) and the attribute each toggles. */
export const FP_ATTRIBUTE_ACTIONS: Readonly<Record<string, FpAttribute>> = {
  "pcbnew.InteractiveEdit.toggleExcludeFromBOM": "bom",
  "pcbnew.InteractiveEdit.toggleExcludeFromSim": "sim",
  "pcbnew.InteractiveEdit.toggleExcludeFromPosFiles": "pos",
};

const VARIANT_KEY: Record<FpAttribute, "exclude_from_bom" | "exclude_from_sim" | "exclude_from_pos_files"> = { bom: "exclude_from_bom", sim: "exclude_from_sim", pos: "exclude_from_pos_files" };

/** The footprint has no symbol: "Not in schematic" (`FP_BOARD_ONLY`), the variants and the simulation flag being the symbol's. */
const hasSymbol = (p: Part): boolean => !p.attrs?.board_only;

/** `GetExcludedFromBOMForVariant( variant )` and kin: the variant's attribute when a variant is current (and the footprint has one), else its own. */
export function fpAttributeIn(part: Part, attr: FpAttribute, variant: string): boolean {
  if (variant !== "" && part.variant && hasSymbol(part)) return part.variant[VARIANT_KEY[attr]];
  const a = part.attrs;
  return attr === "bom" ? !!a?.exclude_from_bom : attr === "pos" ? !!a?.exclude_from_pos_files : !!a?.exclude_from_sim;
}

/** `excludeFromBOMCond` and kin: checked when the selection has footprints and every one of them has the attribute. */
export function fpAttributeChecked(parts: readonly Part[], attr: FpAttribute, variant: string): boolean {
  return parts.length > 0 && parts.every((p) => fpAttributeIn(p, attr, variant));
}

/**
 * `ToggleFootprintAttribute` over `parts` (the footprints `RequestSelection( FootprintFilter )` kept): the commands of its one commit, empty when there are no
 * footprints. The new state is true when any footprint lacks the attribute (`new_state = true; break;`).
 */
export function toggleFpAttributeCmds(parts: readonly Part[], attr: FpAttribute, variant: string): Cmd[] {
  const placed = parts.filter((p) => p.placed);
  if (placed.length === 0) return [];
  const on = placed.some((p) => !fpAttributeIn(p, attr, variant));
  const cmds: Cmd[] = [];
  // in a variant: `variant->SetExcludedFromBOM( new_state )` ... on the symbol's record of the variant
  const inVariant = variant !== "" ? placed.filter(hasSymbol).map((p) => p.ref) : [];
  if (inVariant.length > 0) cmds.push({ op: "set_symbol_variant", variant, ids: inVariant, [VARIANT_KEY[attr]]: on });
  // the footprint's own attribute (`fp->SetExcludedFromBOM( new_state )`): the default variant, or a footprint with no symbol in a variant
  const own = placed.filter((p) => !inVariant.includes(p.ref));
  if (attr === "sim") {
    const ids = own.filter(hasSymbol).filter((p) => fpAttributeIn(p, "sim", "") !== on).map((p) => p.ref);
    if (ids.length > 0) cmds.push({ op: "set_symbol_attrs", ids, exclude_from_sim: on });
  } else {
    for (const p of own) if (fpAttributeIn(p, attr, "") !== on) cmds.push(editAttrsCmd(p, attr === "bom" ? { exclude_from_bom: on } : { exclude_from_pos_files: on }));
  }
  return cmds;
}
