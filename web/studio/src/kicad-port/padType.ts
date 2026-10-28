// The pad type of a library pad, as the Footprint Editor's Pad Properties and Pad Table set it (pcbnew/dialogs/dialog_pad_properties.cpp,
// dialog_fp_edit_pad_table.cpp, pcbnew/pad.cpp):
//
//   DIALOG_PAD_PROPERTIES::PadTypeSelected       the dialog's "Pad type" choice: the type's default layers, hole and number
//   DIALOG_PAD_PROPERTIES::transferDataToPad     an NPTH pad has no number; an SMD or connector pad no hole
//   DIALOG_PAD_PROPERTIES::OnUpdateUI            the number and the hole are enabled by the type
//   PAD::SetAttribute                            the Pad Table's Type column: the copper layers plumped up or trimmed, the hole, the number
//   PAD::IsAperturePad / CanHaveNumber           an SMD pad with no copper layer is an aperture, and has no number
//
// The IR has three pad kinds (`PadKind`): an edge connector pad (`PAD_ATTRIB::CONN`) is an SMD pad with `kicad.connector` set (the writer then writes
// `connect`), an aperture pad an SMD pad with no copper layer -- KiCad's own model (`code_type[]`: "Aperture pad :type SMD with no copper layers").
//
// Pure: no store, no DOM. Unit-tested in padType.test.ts.
import type { LibraryPad, PadKind } from "../api/types";

/** The dialog's `m_padType` choices, in KiCad's order (`PTH_DLG_TYPE` .. `APERTURE_DLG_TYPE`). */
export type PadDlgType = "pth" | "smd" | "conn" | "npth" | "aperture";

/** `m_padTypeChoices` (dialog_pad_properties_base.cpp), word for word. */
export const PAD_DLG_TYPES: { value: PadDlgType; label: string }[] = [
  { value: "pth", label: "Through-hole" },
  { value: "smd", label: "SMD" },
  { value: "conn", label: "Edge Connector" },
  { value: "npth", label: "NPTH, Mechanical" },
  { value: "aperture", label: "SMD Aperture" },
];

/**
 * `PAD::PTHMask`, `SMDMask`, `ConnSMDMask`, `UnplatedHoleMask`, `ApertureMask` as the file writes them (`formatLayers`: every copper layer, or F.Cu and B.Cu, is
 * `*.Cu`; both masks `*.Mask`): what `updatePadLayersList( std::nullopt, .. )` puts on a pad whose type was just chosen.
 */
export const PAD_TYPE_LAYERS: Record<PadDlgType, string[]> = {
  pth: ["*.Cu", "*.Mask"],
  smd: ["F.Cu", "F.Paste", "F.Mask"],
  conn: ["F.Cu", "F.Mask"],
  npth: ["*.Cu", "*.Mask"],
  aperture: ["F.Paste"],
};

/** `PAD::DEFAULT_PAD_DRILL_DIAMETER_MM` (0.8 mm), nm. */
export const DEFAULT_PAD_DRILL_NM = 800_000;

/** A copper layer token of a pad's layer list: `F.Cu`, `In1.Cu`, `B.Cu`, `*.Cu`, `F&B.Cu`. */
export const isCopperToken = (layer: string): boolean => layer.endsWith(".Cu");

type TypeFields = Pick<LibraryPad, "kind" | "layers" | "kicad">;

/** `PAD::IsAperturePad`: an SMD pad on no copper layer. A pad whose layers are not set yet (an empty list, which the writer gives the kind's defaults) is not one. */
export function isAperturePad(pad: Pick<LibraryPad, "kind" | "layers">): boolean {
  return pad.kind === "smd" && pad.layers.length > 0 && !pad.layers.some(isCopperToken);
}

/** Whether the pad is an edge connector pad (`PAD_ATTRIB::CONN`): `kicad.connector` on an SMD pad. */
export function isConnectorPad(pad: TypeFields): boolean {
  return pad.kind === "smd" && pad.kicad?.connector === true;
}

/** `TransferDataToWindow`'s type selection: an aperture first (`GetAttribute() == SMD && IsAperturePad()`), then the attribute. */
export function padDlgTypeOf(pad: TypeFields): PadDlgType {
  if (pad.kind === "through_hole") return "pth";
  if (pad.kind === "non_plated_hole") return "npth";
  if (isConnectorPad(pad)) return "conn";
  return isAperturePad(pad) ? "aperture" : "smd";
}

/** `code_type[]`: the pad attribute a dialog type stands for (CONN and the aperture are SMD pads here). */
export function kindOfDlgType(type: PadDlgType): PadKind {
  return type === "pth" ? "through_hole" : type === "npth" ? "non_plated_hole" : "smd";
}

/** `PadTypeSelected` / `OnUpdateUI`: whether the type has a hole and a connection (a number and a net). */
export function padTypeHas(type: PadDlgType): { hole: boolean; connection: boolean } {
  return { hole: type === "pth" || type === "npth", connection: type === "pth" || type === "smd" || type === "conn" };
}

/** The `kicad` keep with the connector flag set or cleared (an empty keep is left out). */
function withConnector(kicad: LibraryPad["kicad"], connector: boolean): LibraryPad["kicad"] {
  const next: Record<string, unknown> = { ...(kicad ?? {}) };
  if (connector) next.connector = true;
  else delete next.connector;
  return Object.keys(next).length > 0 ? next : undefined;
}

/**
 * `DIALOG_PAD_PROPERTIES::PadTypeSelected` then `transferDataToPad` on the dialog's pad `form`, `current` being the pad the dialog was opened on (`m_currentPad`;
 * null for Default Pad Properties):
 * - the layers are the new type's defaults ("the old pad's layer set is no longer meaningful", `updatePadLayersList( std::nullopt, .. )`);
 * - a type with no hole has none (`m_holeX.ChangeValue( 0 )`); a type with one, when the dialog's hole is 0, takes the current pad's
 *   (`m_currentPad->GetDrillSize()`, which is 0 for an SMD pad) or, with no current pad, `DEFAULT_PAD_DRILL_DIAMETER_MM`;
 * - a type with no connection has no number (`m_padNumCtrl->ChangeValue( wxEmptyString )`); one with a connection whose number is empty takes the
 *   current pad's back.
 * A hole of 0 is no hole here (`drill: null`), which `padValuesOK` then refuses for a through-hole type ("Through hole pad has no hole.").
 */
export function padTypeSelected(form: LibraryPad, type: PadDlgType, current: LibraryPad | null): LibraryPad {
  const has = padTypeHas(type);
  let next: LibraryPad = { ...form, kind: kindOfDlgType(type), layers: [...PAD_TYPE_LAYERS[type]], kicad: withConnector(form.kicad, type === "conn") };
  if (!has.hole) next = { ...next, drill: null, drill_slot: null };
  else if (!(form.drill && form.drill > 0) && !form.drill_slot) {
    if (current) {
      const sized = current.kind !== "smd";
      next = { ...next, drill: sized ? current.drill : null, drill_slot: sized ? current.drill_slot : null };
    } else next = { ...next, drill: DEFAULT_PAD_DRILL_NM, drill_slot: null };
  }
  if (!has.connection) next = { ...next, number: "" };
  else if (next.number === "" && current) next = { ...next, number: current.number };
  return next;
}

/**
 * `PAD::SetAttribute( aAttribute )` (the Pad Table's Type column, `OnCellChanged( COL_TYPE )`), when the type really changes:
 * - plated through-hole: `layerMask |= LSET::AllCuMask()` (the copper becomes `*.Cu`, the other layers stay);
 * - SMD or connector: no more than one copper layer (B.Cu if the pad was on it, else its first) and no hole;
 * - NPTH: no number (and no net).
 * The Pad Table has a "Connector" type of its own; the aperture is not a type it sets ("Aperture derived by copper-less layers").
 * Where KiCad leaves a through-hole pad that had no hole with a 0 hole (which DRC then reports), the pad here takes `DEFAULT_PAD_DRILL_DIAMETER_MM`:
 * the IR refuses a plated or non-plated pad with no hole.
 */
export function setPadAttribute(pad: LibraryPad, type: Exclude<PadDlgType, "aperture">): LibraryPad {
  const kind = kindOfDlgType(type);
  const connector = type === "conn";
  if (kind === pad.kind && isConnectorPad(pad) === connector) return pad;
  // A pad with no layers set yet has the writer's defaults for its kind.
  const layers = pad.layers.length > 0 ? pad.layers : pad.kind === "smd" ? PAD_TYPE_LAYERS.smd : PAD_TYPE_LAYERS.pth;
  const copper = layers.filter(isCopperToken);
  const rest = layers.filter((l) => !isCopperToken(l));
  let next: LibraryPad = { ...pad, kind, kicad: withConnector(pad.kicad, connector) };
  switch (type) {
    case "pth":
      next = { ...next, layers: ["*.Cu", ...rest] };
      break;
    case "smd":
    case "conn": {
      // `copperLayers.count() > 1`: a wildcard is every copper layer, B.Cu among them.
      const several = copper.length > 1 || copper.some((l) => l.startsWith("*") || l.includes("&"));
      const one = copper.some((l) => l === "B.Cu" || l.startsWith("*") || l.includes("&")) ? "B.Cu" : copper[0]!;
      next = { ...next, layers: several ? [one, ...rest] : [...layers], drill: null, drill_slot: null };
      break;
    }
    case "npth":
      next = { ...next, number: "", layers: [...layers] };
      break;
  }
  if (kind !== "smd" && next.drill == null && next.drill_slot == null) next = { ...next, drill: DEFAULT_PAD_DRILL_NM };
  return next;
}
