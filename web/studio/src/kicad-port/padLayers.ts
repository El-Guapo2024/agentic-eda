// Pad Properties' layers (UI-LEFT F6): the copper choice `m_rbCopperLayersSel` and the technical layers' check boxes, ported from `DIALOG_PAD_PROPERTIES`
// (pcbnew/dialogs/dialog_pad_properties.cpp at KiCad 8303b2ad): `UpdateLayersDropdown` (the copper choices of each pad type), `updatePadLayersList` (what a pad's layer set
// shows), `transferDataToPad` (the set the choices give, and the unconnected-layer mode of a plated hole's choice) -- with `PCB_IO_KICAD_SEXPR::formatLayers`' wildcards
// for the set as a library pad's `(layers ..)` holds it (no board: every copper layer, or F.Cu and B.Cu, is `*.Cu`; both sides of a pair `*.Mask` ...).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { PadDlgType } from "./padType";

/** The technical layers the dialog has a box for, in its order (`m_layerFrontAdhesive` .. `m_layerUserDwgs`). */
export const PAD_TECH_LAYERS: readonly string[] = ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Eco1.User", "Eco2.User", "Dwgs.User"];

/** The unconnected-layer mode of a plated hole's copper choice (`UNCONNECTED_LAYER_MODE`, as the IR names it). */
export type CopperMode = "keep_all" | "remove_except_start_and_end" | "remove_all";

/** `UpdateLayersDropdown`: the copper choices of a pad type, in order. */
export function copperChoices(type: PadDlgType, fCu = "F.Cu", bCu = "B.Cu"): string[] {
  switch (type) {
    case "pth":
      return ["All copper layers", `${fCu}, ${bCu} and connected layers`, "Connected layers only", "None"];
    case "npth":
      return [`${fCu} and ${bCu}`, fCu, bCu, "None"];
    case "smd":
    case "conn":
      return [fCu, bCu];
    case "aperture":
      return ["None"];
  }
}

/** The layers a library pad's `(layers ..)` names, its wildcards spread (`*.Cu` every copper layer -- here F.Cu and B.Cu stand for them --, `*.Mask` both masks ...). */
export function spreadLayers(layers: readonly string[]): Set<string> {
  const out = new Set<string>();
  for (const l of layers) {
    if (l === "*.Cu" || l === "F&B.Cu") {
      out.add("F.Cu");
      out.add("B.Cu");
      if (l === "*.Cu") out.add("*.Cu");
    } else if (l.startsWith("*.")) {
      out.add(`F.${l.slice(2)}`);
      out.add(`B.${l.slice(2)}`);
    } else out.add(l);
  }
  return out;
}

/** The dialog's layer controls: the copper choice (an index into `copperChoices`) and the boxes of `PAD_TECH_LAYERS` that are ticked. */
export interface PadLayerForm {
  copper: number;
  tech: Set<string>;
}

/**
 * `updatePadLayersList( layer_mask, remove_unconnected, keep_top_bottom )` on a pad of type `type` whose layers are `layers` (none: the type's default mask) and whose
 * plated hole has the unconnected-layer mode `mode`.
 */
export function padLayerForm(type: PadDlgType, layers: readonly string[] | null, mode: CopperMode): PadLayerForm {
  const defaults: Record<PadDlgType, string[]> = { pth: ["*.Cu", "*.Mask"], smd: ["F.Cu", "F.Paste", "F.Mask"], conn: ["F.Cu", "F.Mask"], npth: ["*.Cu", "*.Mask"], aperture: ["F.Paste"] };
  let set = spreadLayers(layers && layers.length > 0 ? layers : defaults[type]);
  const hasCopper = set.has("F.Cu") || set.has("B.Cu") || [...set].some((l) => l.endsWith(".Cu"));
  let copper = 0;
  switch (type) {
    case "pth":
      copper = !hasCopper ? 3 : mode === "keep_all" ? 0 : mode === "remove_except_start_and_end" ? 1 : 2;
      break;
    case "smd":
    case "conn":
      // "The SMD/CONN UI has no "no copper" radio choice, so fall back to defaults if the stored mask has no copper layers."
      if (!hasCopper) set = spreadLayers(defaults[type]);
      copper = set.has("F.Cu") ? 0 : 1;
      break;
    case "npth":
      copper = set.has("F.Cu") && set.has("B.Cu") ? 0 : set.has("F.Cu") ? 1 : set.has("B.Cu") ? 2 : 3;
      break;
    case "aperture":
      copper = 0;
      break;
  }
  return { copper, tech: new Set(PAD_TECH_LAYERS.filter((l) => set.has(l))) };
}

/**
 * `transferDataToPad`'s layer set from the controls, written as `formatLayers` writes a library pad's (`*.Cu` for the copper of a plated hole and for F.Cu with B.Cu, `*.Adhes`,
 * `*.Paste`, `*.SilkS`, `*.Mask` for a pair, then each other layer in `PCB_LAYER_ID` order), and the unconnected-layer mode a plated hole's choice gives (`KEEP_ALL` otherwise).
 */
export function layersOfForm(type: PadDlgType, form: PadLayerForm): { layers: string[]; mode: CopperMode } {
  const set = new Set<string>();
  let mode: CopperMode = "keep_all";
  switch (type) {
    case "pth":
      if (form.copper <= 2) {
        set.add("F.Cu");
        set.add("B.Cu");
      }
      mode = form.copper === 1 ? "remove_except_start_and_end" : form.copper === 2 ? "remove_all" : "keep_all";
      break;
    case "npth":
      if (form.copper === 0 || form.copper === 1) set.add("F.Cu");
      if (form.copper === 0 || form.copper === 2) set.add("B.Cu");
      break;
    case "smd":
    case "conn":
      set.add(form.copper === 1 ? "B.Cu" : "F.Cu");
      break;
    case "aperture":
      break;
  }
  for (const l of form.tech) set.add(l);
  const out: string[] = [];
  if (set.has("F.Cu") && set.has("B.Cu")) {
    out.push("*.Cu");
    set.delete("F.Cu");
    set.delete("B.Cu");
  }
  for (const pair of ["Adhes", "Paste", "SilkS", "Mask"]) {
    if (set.has(`F.${pair}`) && set.has(`B.${pair}`)) {
      out.push(`*.${pair}`);
      set.delete(`F.${pair}`);
      set.delete(`B.${pair}`);
    }
  }
  // `PCB_LAYER_ID` order: F.Cu 0, F.Mask 1, B.Cu 2, B.Mask 3, F.SilkS 5, B.SilkS 7, F.Adhes 9, B.Adhes 11, F.Paste 13, B.Paste 15, Dwgs.User 17, Eco1.User 21, Eco2.User 23
  const ORDER = ["F.Cu", "F.Mask", "B.Cu", "B.Mask", "F.SilkS", "B.SilkS", "F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User"];
  for (const l of ORDER) if (set.has(l)) out.push(l);
  return { layers: out, mode };
}

/** `OnUpdateUI`: the copper choice is disabled for an aperture pad (`m_padType->GetSelection() != APERTURE_DLG_TYPE`). */
export const copperChoiceEnabled = (type: PadDlgType): boolean => type !== "aperture";

// ------------------------------------------------------------------------------------------------------------------------------- fabrication

/** `m_choiceFabProperty`'s choices in KiCad's words, with `PAD_PROP`'s file token (`format( const PAD* )`: `(property pad_prop_..)`; none for `PAD_PROP::NONE`). */
export const FAB_PROPERTY_CHOICES: readonly { token: string | null; label: string }[] = [
  { token: null, label: "None" },
  { token: "pad_prop_bga", label: "BGA pad" },
  { token: "pad_prop_fiducial_loc", label: "Fiducial, local to footprint" },
  { token: "pad_prop_fiducial_glob", label: "Fiducial, global to board" },
  { token: "pad_prop_testpoint", label: "Test point pad" },
  { token: "pad_prop_heatsink", label: "Heatsink pad" },
  { token: "pad_prop_mechanical", label: "Mechanical" },
  { token: "pad_prop_castellated", label: "Castellated pad (TH only)" },
  { token: "pad_prop_pressfit", label: "Press-fit (round TH only)" },
];

/** A library pad's kept record (`LibraryPadKeep`) as far as the dialog reads it. */
export interface PadKeepFab {
  property?: string | null;
  tokens?: string[];
}

const kept = (tokens: readonly string[] | undefined, tag: string): number | null => {
  const t = (tokens ?? []).find((x) => x.startsWith(`(${tag} `));
  const v = t ? Number(t.slice(tag.length + 2, -1).trim()) : NaN;
  return Number.isFinite(v) ? v : null;
};

/** `PAD::GetPadToDieLength` (nm) and `GetPadToDieDelay` (ps) of the pad's kept `(die_length ..)` / `(die_delay ..)`; `null` for none (KiCad's 0). The file's version
 *  (20240108, which the library writer writes and kicad-cli reads with `parseBoardUnits`) has the delay's picoseconds as the number, `IU_PER_PS` being `IU_PER_MM`. */
export function padToDieOf(keep: PadKeepFab | undefined): { length: number | null; delay: number | null } {
  const mm = kept(keep?.tokens, "die_length");
  const ps = kept(keep?.tokens, "die_delay");
  return { length: mm === null || mm === 0 ? null : Math.round(mm * 1e6), delay: ps === null || ps === 0 ? null : ps };
}

/** A number as `formatInternalUnits` writes one: up to six decimals, no trailing zeros. */
function fileNumber(v: number): string {
  return Number(v.toFixed(6)).toString();
}

/**
 * `transferDataToPad`'s `SetProperty( getSelectedProperty() )`, `SetPadToDieLength` and `SetPadToDieDelay` on the pad's kept record: the property token (none for "None"), the
 * `(die_length ..)` and `(die_delay ..)` tokens put where they were (or at the start) and taken away when the box is off (KiCad's 0, which the writer leaves out).
 */
export function withPadFab<K extends PadKeepFab>(keep: K | undefined, property: string | null, lengthNm: number | null, delayPs: number | null): K {
  const tokens = [...(keep?.tokens ?? [])];
  const put = (tag: string, value: string | null) => {
    const i = tokens.findIndex((t) => t.startsWith(`(${tag} `));
    if (value === null) {
      if (i >= 0) tokens.splice(i, 1);
    } else if (i >= 0) tokens[i] = `(${tag} ${value})`;
    else tokens.unshift(`(${tag} ${value})`);
  };
  put("die_delay", delayPs === null || delayPs === 0 ? null : fileNumber(delayPs));
  put("die_length", lengthNm === null || lengthNm === 0 ? null : fileNumber(lengthNm / 1e6));
  const out = { ...(keep ?? {}), tokens } as K;
  if (property) out.property = property;
  else delete out.property;
  if (tokens.length === 0) delete out.tokens;
  return out;
}

/**
 * `OnDrillShapeSelected` (E7-F6): the hole's shape changes and its size stays -- the controls keep their values, so a round hole of diameter d becomes an oblong d x y (`y`
 * the Y the dialog last had, the round hole's own d at first) and an oblong w x h a round hole of diameter w.
 */
export function holeShapeSelected(drill: number | null, slot: [number, number] | null, shape: "round" | "oblong", lastY: number | null): { drill: number | null; drill_slot: [number, number] | null } {
  if (shape === "round") return { drill: slot ? slot[0] : drill, drill_slot: null };
  const x = slot ? slot[0] : (drill ?? 0);
  return { drill: null, drill_slot: slot ?? [x, lastY ?? x] };
}
