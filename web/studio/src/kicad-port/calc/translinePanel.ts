// The Calculator Tools' Transmission Lines panel (KiCad 25ec2335 pcb_calculator): what happens between its text boxes and the calculators of
// kicad-port/transline.ts. components/calculator/PanelTransline.tsx draws it.
//
// Ported from (KiCad 25ec2335, pcb_calculator/):
//   transline_ident.{h,cpp}                 TRANSLINE_TYPE_ID, TRANSLINE_PRM, TRANSLINE_IDENT (each type's parameters: keyword, label, tooltip, default, unit
//                                           selector; its result lines), ReadConfig / WriteConfig
//   transline_dlg_funct.cpp                 TranslineTypeSelection, TransfDlgDataToTranslineParams, OnTranslineSelection, OnTransLineResetButtonClick,
//                                           OnTranslineEpsilonR_Button / TanD / Rho (the lists of common_data.cpp)
//   calculator_panels/panel_transline.cpp   the constructor's defaults, updateSoldermaskEnables, SaveSettings / LoadSettings, UpdateSpecFrequencyEnable,
//                                           OnTranslineAnalyse / OnTranslineSynthetize
//   params_read_write.cpp                   GetPrmValue / SetPrmValue / SetPrmBgColor / SetResult / IsPrmSelected (the glue the TRANSLINE wrappers call)
//   transline/transline.{h,cpp}             TRANSLINE: Init, getProperties, checkProperties, analyze, synthesize, setErrorLevel, pushSoldermaskParameters
//   transline/{microstrip,c_microstrip,stripline,c_stripline,coplanar,rectwaveguide,coax,twistedpair}.cpp
//                                           each wrapper's getProperties, calcAnalyze / calcSynthesize, showAnalyze / showSynthesize, show_results
//   common_data.cpp                         StandardRelativeDielectricConstantList, StandardLossTangentList, StandardResistivityList
//   pcb_calculator_utils.cpp                findMatch (the "..." list's preselected row)
//
// The panel's controls are shared by all nine types (KiCad reuses the same text boxes, choosers and result labels and relabels them), so this keeps
// them the same way: `slots` are the controls (text, unit, background), each type's TRANSLINE_PRM list holds its own values. A background colour
// set by an analysis stays on its control when another type is chosen, as in KiCad. Not ported: the line type's picture (`m_BitmapName`) and the
// Zodd / Zeven picture of the coupled lines; the "..." lists are a choice list in the panel rather than wxGetSingleChoiceIndex's dialog.
//
// Pure: no React, no DOM.

import { entryValue, findMatch as commonFindMatch, standardLossTangentList, standardRelativeDielectricConstantList, standardResistivityList } from "./commonData";
import { printfG } from "./cformat";
import { Coax, Coplanar, CoupledMicrostrip, CoupledStripline, Microstrip, MU0, RectWaveguide, Stripline, TwistedPair, type TCP, type TranslineCalculationBase, type TranslineResults, type TranslineStatus } from "../transline";
import type { PcbCalculatorSettings, TranslineName } from "./calcSettings";
import { UNIT_MICRON, UNIT_SELECTOR_ANGLE, UNIT_SELECTOR_FREQUENCY, UNIT_SELECTOR_LEN, UNIT_SELECTOR_RESISTOR, doubleFromString, unitScale, type UnitSelectorDef } from "./unitSelectors";

/** `PRMS_ID` without `_PRM` (DUMMY skips a line in the dialog). */
export type PrmsId =
  | "EPSILONR"
  | "TAND"
  | "RHO"
  | "H"
  | "TWISTEDPAIR_TWIST"
  | "H_T"
  | "STRIPLINE_A"
  | "T"
  | "ROUGH"
  | "MUR"
  | "TWISTEDPAIR_EPSILONR_ENV"
  | "MURC"
  | "FREQUENCY"
  | "Z0"
  | "Z0_E"
  | "Z0_O"
  | "ANG_L"
  | "PHYS_WIDTH"
  | "PHYS_DIAM_IN"
  | "PHYS_S"
  | "PHYS_DIAM_OUT"
  | "PHYS_LEN"
  | "DUMMY";

/** The UI parameters in `PRMS_ID` order (`TRANSLINE::getProperties` reads `0 .. DUMMY_PRM - 1`). */
const UI_PRMS: readonly Exclude<PrmsId, "DUMMY">[] = ["EPSILONR", "TAND", "RHO", "H", "TWISTEDPAIR_TWIST", "H_T", "STRIPLINE_A", "T", "ROUGH", "MUR", "TWISTEDPAIR_EPSILONR_ENV", "MURC", "FREQUENCY", "Z0", "Z0_E", "Z0_O", "ANG_L", "PHYS_WIDTH", "PHYS_DIAM_IN", "PHYS_S", "PHYS_DIAM_OUT", "PHYS_LEN"];

/** `PRM_TYPE`: which group of controls the parameter goes in. */
export type PrmType = "SUBS" | "PHYS" | "ELEC" | "FREQUENCY";

/** `TRANSLINE_PRM`'s constant part. */
export interface TranslinePrmDef {
  type: PrmType;
  id: PrmsId;
  /** `m_KeyWord`: the key in `trans_line.<name>.values / units`. */
  key: string;
  /** `m_DlgLabel`. */
  label: string;
  tooltip: string;
  /** `m_DefaultValue`, in the unit the chooser shows first. */
  value: number;
  /** `m_ConvUnit`: a unit chooser is shown and scales the value. */
  convUnit: boolean;
}

/** `TRANSLINE_IDENT`'s constant part. */
export interface TranslineTypeDef {
  /** The `trans_line` settings key (`m_TLine->m_Name`). */
  name: TranslineName;
  /** The radio box's label. */
  label: string;
  /** `m_Messages`: the result lines' labels. */
  messages: readonly string[];
  /** `m_HasPrmSelection`: the radio buttons choosing which physical parameter Synthesize solves. */
  hasPrmSelection: boolean;
  prms: readonly TranslinePrmDef[];
}

const P = (type: PrmType, id: PrmsId, key = "", label = "", tooltip = "", value = 0.0, convUnit = false): TranslinePrmDef => ({ type, id, key, label, tooltip, value, convUnit });
const DUMMY = P("ELEC", "DUMMY");
const coverHeightToolTip = "Height from substrate top to grounded cover. Leave at the large default value for no cover.";
const effEr = "Effective εr:";

/** The four parameters every type starts with (default values are for FR4 and copper; the frequency is in GHz). */
const COMMON_PRMS: readonly TranslinePrmDef[] = [
  P("SUBS", "EPSILONR", "Er", "εr", "Substrate relative permittivity (dielectric constant)", 4.5, false),
  P("SUBS", "TAND", "TanD", "tan δ", "Dielectric loss (dissipation factor)", 2e-2, false),
  P("SUBS", "RHO", "Rho", "ρ", "Electrical resistivity or specific electrical resistance of conductor (ohm*meter)", 1.72e-8, false),
  P("FREQUENCY", "FREQUENCY", "Frequency", "Frequency", "Frequency of the input signal", 1.0, true),
];

const H = P("SUBS", "H", "H", "H", "Height of substrate", 0.2, true);
const T = P("SUBS", "T", "T", "T", "Strip thickness", 0.035, true);
const W = P("PHYS", "PHYS_WIDTH", "W", "W", "Line width", 0.2, true);
const S = P("PHYS", "PHYS_S", "S", "S", "Gap width", 0.2, true);
const L = P("PHYS", "PHYS_LEN", "L", "L", "Line length", 50.0, true);
const Z0 = P("ELEC", "Z0", "Z0", "Z0", "Characteristic impedance", 50.0, true);
const ANG_L = P("ELEC", "ANG_L", "Ang_l", "Ang_l", "Electrical length", 0.0, true);
const MURC_C = P("SUBS", "MURC", "mu Rel C", "μ(conductor)", "Relative permeability (mu) of conductor", 1, false);
const MURC_CL = P("SUBS", "MURC", "mu rel C", "μ(conductor)", "Relative permeability (mu) of conductor", 1, false);
const MUR_I = P("SUBS", "MUR", "mu Rel I", "μ(insulator)", "Relative permeability (mu) of insulator", 1, false);
const ZEVEN = P("ELEC", "Z0_E", "Zeven", "Zeven", "Even mode impedance (lines driven by common voltages)", 50.0, true);
const ZODD = P("ELEC", "Z0_O", "Zodd", "Zodd", "Odd mode impedance (lines driven by opposite (differential) voltages)", 50.0, true);
const DIN = P("PHYS", "PHYS_DIAM_IN", "Din", "Din", "Inner diameter (conductor)", 1.0, true);
const DOUT = P("PHYS", "PHYS_DIAM_OUT", "Dout", "Dout", "Outer diameter (insulator)", 8.0, true);
const CPW_MESSAGES = [effEr, "Unit propagation delay:", "Conductor losses:", "Dielectric losses:", "Skin depth:"];

/** `TRANSLINE_IDENT::TRANSLINE_IDENT` for each `TRANSLINE_TYPE_ID`, in the radio box's order (MICROSTRIP_TYPE .. TWISTEDPAIR_TYPE). */
export const TRANSLINE_TYPES: readonly TranslineTypeDef[] = [
  {
    name: "MicroStrip",
    label: "Microstrip Line",
    messages: CPW_MESSAGES,
    hasPrmSelection: false,
    prms: [
      ...COMMON_PRMS,
      H,
      P("SUBS", "H_T", "H_t", "H(top)", coverHeightToolTip, 1e20, true),
      T,
      P("SUBS", "ROUGH", "Rough", "Roughness", "Conductor roughness", 0.0, true),
      P("SUBS", "MUR", "mu Rel S", "μr (substrate)", "Relative permeability (mu) of substrate", 1, false),
      P("SUBS", "MURC", "mu Rel C", "μr (conductor)", "Relative permeability (mu) of conductor", 1, false),
      W,
      L,
      Z0,
      DUMMY,
      ANG_L,
    ],
  },
  {
    name: "Coupled_MicroStrip",
    label: "Coupled Microstrip Line",
    messages: [
      "Effective εr (even):",
      "Effective εr (odd):",
      "Unit propagation delay (even):",
      "Unit propagation delay (odd):",
      "Conductor losses (even):",
      "Conductor losses (odd):",
      "Dielectric losses (even):",
      "Dielectric losses (odd):",
      "Skin depth:",
      "Differential Impedance (Zd):",
    ],
    hasPrmSelection: true,
    prms: [
      ...COMMON_PRMS,
      H,
      P("SUBS", "H_T", "H_t", "H_t", coverHeightToolTip, 1e20, true),
      T,
      P("SUBS", "ROUGH", "Rough", "Roughness", "Conductor roughness", 0.0, true),
      MURC_CL,
      W,
      S,
      L,
      ZEVEN,
      ZODD,
      ANG_L,
    ],
  },
  {
    name: "StripLine",
    label: "Stripline",
    messages: CPW_MESSAGES,
    hasPrmSelection: false,
    prms: [...COMMON_PRMS, H, P("SUBS", "STRIPLINE_A", "a", "a", "Distance between strip and top metal", 0.2, true), T, MURC_C, W, L, Z0, DUMMY, ANG_L],
  },
  {
    name: "Coupled_Stripline",
    label: "Coupled Stripline",
    messages: [
      "Effective εr (even):",
      "Effective εr (odd):",
      "Unit propagation delay (even):",
      "Unit propagation delay (odd):",
      "Skin depth:",
      "Differential Impedance (Zd):",
      "Common-mode Impedance (Zcomm):",
      "Coupling coefficient (kc):",
    ],
    hasPrmSelection: true,
    prms: [...COMMON_PRMS, H, P("SUBS", "STRIPLINE_A", "a", "a", "Distance from track to closest ground plane (default: centered)", 0.0, true), T, MURC_CL, W, S, L, ZEVEN, ZODD, ANG_L],
  },
  { name: "CoPlanar", label: "Coplanar wave guide", messages: CPW_MESSAGES, hasPrmSelection: true, prms: [...COMMON_PRMS, H, T, MURC_C, W, S, L, Z0, DUMMY, ANG_L] },
  { name: "GrCoPlanar", label: "Coplanar wave guide w/ ground plane", messages: CPW_MESSAGES, hasPrmSelection: true, prms: [...COMMON_PRMS, H, T, MURC_C, W, S, L, Z0, DUMMY, ANG_L] },
  {
    name: "RectWaveGuide",
    label: "Rectangular Waveguide",
    messages: ["ZF(H10) = Ey / Hx:", effEr, "Conductor losses:", "Dielectric losses:", "TE-modes:", "TM-modes:"],
    hasPrmSelection: true,
    prms: [
      ...COMMON_PRMS,
      MUR_I,
      MURC_C,
      P("PHYS", "PHYS_WIDTH", "a", "a", "Width of waveguide", 10.0, true),
      P("PHYS", "PHYS_S", "b", "b", "Height of waveguide", 5.0, true),
      P("PHYS", "PHYS_LEN", "L", "L", "Waveguide length", 50.0, true),
      Z0,
      DUMMY,
      ANG_L,
    ],
  },
  { name: "Coax", label: "Coaxial Line", messages: [effEr, "Conductor losses:", "Dielectric losses:", "TE-modes:", "TM-modes:"], hasPrmSelection: true, prms: [...COMMON_PRMS, MUR_I, MURC_C, DIN, DOUT, L, Z0, DUMMY, ANG_L] },
  {
    name: "TwistedPair",
    label: "Twisted Pair",
    messages: [effEr, "Conductor losses:", "Dielectric losses:", "Skin depth:"],
    hasPrmSelection: true,
    prms: [
      ...COMMON_PRMS,
      P("SUBS", "TWISTEDPAIR_TWIST", "Twists", "Twists", "Number of twists per length", 0.0, false),
      MURC_C,
      P("SUBS", "TWISTEDPAIR_EPSILONR_ENV", "ErEnv", "εr(environment)", "Relative permittivity of environment", 1, false),
      DIN,
      DOUT,
      P("PHYS", "PHYS_LEN", "L", "L", "Cable length", 50.0, true),
      Z0,
      DUMMY,
      ANG_L,
    ],
  },
];

export const TRANSLINE_TYPE_COUNT = TRANSLINE_TYPES.length;
export const MICROSTRIP_TYPE = 0;
export const C_MICROSTRIP_TYPE = 1;
export const STRIPLINE_TYPE = 2;
export const C_STRIPLINE_TYPE = 3;
export const CPW_TYPE = 4;
export const GROUNDED_CPW_TYPE = 5;
export const RECTWAVEGUIDE_TYPE = 6;
export const COAX_TYPE = 7;
export const TWISTEDPAIR_TYPE = 8;

/** The dielectric model chooser (`m_dielectricModelChoice`). */
export const DIELECTRIC_MODEL_CHOICES = ["Constant (εr, tan δ)", "Djordjevic-Sarkar causal"] as const;

/** common_data.cpp's rows of the "..." buttons beside εr, tan δ and ρ (kicad-port/calc/commonData.ts). */
export const STANDARD_RELATIVE_DIELECTRIC_CONSTANTS: readonly string[] = standardRelativeDielectricConstantList();
export const STANDARD_LOSS_TANGENTS: readonly string[] = standardLossTangentList();
export const STANDARD_RESISTIVITIES: readonly string[] = standardResistivityList();

/** pcb_calculator_utils.cpp `findMatch`: the row the text box holds, or the index past the list (KiCad's appended empty row) when none is. */
export function findMatch(aList: readonly string[], aValue: string): number {
  return commonFindMatch(aList, aValue)[1];
}

/** The value a "..." row puts in the text box (`list.Item( index ).BeforeFirst( ' ' )`). */
export const listValue = entryValue;

/** printf's `%g` (`wxString::Format( "%g" )`, kicad-port/calc/cformat.ts). */
export const formatG = printfG;

// ------------------------------------------------------------------------------------------------------------------ the controls

/** The panel's controls a parameter can sit in: 9 substrate rows, 3 physical, 3 electrical, the frequency. */
export type SlotKey = `subs${0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8}` | `phys${0 | 1 | 2}` | `elec${0 | 1 | 2}` | "freq0";

export const SUBS_SLOTS: readonly SlotKey[] = ["subs0", "subs1", "subs2", "subs3", "subs4", "subs5", "subs6", "subs7", "subs8"];
export const PHYS_SLOTS: readonly SlotKey[] = ["phys0", "phys1", "phys2"];
export const ELEC_SLOTS: readonly SlotKey[] = ["elec0", "elec1", "elec2"];

/** Each control's unit chooser (panel_transline_base.cpp): none for εr, tan δ and ρ; lengths; ohms for the impedances, angle for the length. */
export const SLOT_UNITS: Record<SlotKey, UnitSelectorDef | null> = {
  subs0: null,
  subs1: null,
  subs2: null,
  subs3: UNIT_SELECTOR_LEN,
  subs4: UNIT_SELECTOR_LEN,
  subs5: UNIT_SELECTOR_LEN,
  subs6: UNIT_SELECTOR_LEN,
  subs7: UNIT_SELECTOR_LEN,
  subs8: UNIT_SELECTOR_LEN,
  phys0: UNIT_SELECTOR_LEN,
  phys1: UNIT_SELECTOR_LEN,
  phys2: UNIT_SELECTOR_LEN,
  elec0: UNIT_SELECTOR_RESISTOR,
  elec1: UNIT_SELECTOR_RESISTOR,
  elec2: UNIT_SELECTOR_ANGLE,
  freq0: UNIT_SELECTOR_FREQUENCY,
};

export type SlotStatus = "ok" | "warning" | "error";

/** One control as the panel shows it. */
export interface TranslineSlot {
  /** The label (`m_DlgLabel + ':'`, empty for a row the type leaves unused). */
  label: string;
  tooltip: string;
  text: string;
  enabled: boolean;
  /** The unit chooser's selection, and whether it is shown (`m_ConvUnit`). */
  unit: number;
  unitShown: boolean;
  /** The background `setErrorLevel` gave the text box. */
  status: SlotStatus;
  /** The index of the type's parameter in this control, or null. */
  prm: number | null;
}

/** `TRANSLINE_PRM`'s changing part. */
interface PrmState {
  value: number;
  unitSelection: number;
  normalized: number;
  /** `m_UnitCtrl` is set (the type was shown and the parameter has a chooser). */
  hasUnitCtrl: boolean;
  slot: SlotKey | null;
}

/** `TRANSLINE`'s panel-level parameters (`SetExtraParameter`; `TRANSLINE::Init`'s defaults). */
interface Extras {
  DIELECTRIC_MODEL: number;
  EPSILONR_SPEC_FREQ: number;
  SOLDERMASK_PRESENT: number;
  SOLDERMASK_THICKNESS: number;
  SOLDERMASK_EPSILONR: number;
  SOLDERMASK_TAND: number;
  SOLDERMASK_FILLS_GAPS: number;
}

const initExtras = (): Extras => ({ DIELECTRIC_MODEL: 0.0, EPSILONR_SPEC_FREQ: 1.0e9, SOLDERMASK_PRESENT: 0.0, SOLDERMASK_THICKNESS: 20.0e-6, SOLDERMASK_EPSILONR: 3.5, SOLDERMASK_TAND: 0.025, SOLDERMASK_FILLS_GAPS: 1.0 });

const STATUS_OF: Record<TranslineStatus, SlotStatus> = { OK: "ok", WARNING: "warning", TS_ERROR: "error" };

function makeCalc(type: number): TranslineCalculationBase {
  switch (type) {
    case MICROSTRIP_TYPE:
      return new Microstrip();
    case C_MICROSTRIP_TYPE:
      return new CoupledMicrostrip();
    case STRIPLINE_TYPE:
      return new Stripline();
    case C_STRIPLINE_TYPE:
      return new CoupledStripline();
    case CPW_TYPE:
    case GROUNDED_CPW_TYPE:
      return new Coplanar();
    case RECTWAVEGUIDE_TYPE:
      return new RectWaveguide();
    case COAX_TYPE:
      return new Coax();
    default:
      return new TwistedPair();
  }
}

/** `results[X]` of the C++ (`operator[]` gives a zero, OK entry for a parameter the map lacks). */
function res(m: TranslineResults, p: TCP): [number, TranslineStatus] {
  return m.get(p) ?? [0, "OK"];
}

/** The panel's soldermask group: shown for microstrip, coupled microstrip, CPW and CBCPW; "Mask fills gaps" for the two CPWs only. */
export function soldermaskShown(type: number): { group: boolean; fillsGaps: boolean } {
  const group = type === MICROSTRIP_TYPE || type === C_MICROSTRIP_TYPE || type === CPW_TYPE || type === GROUNDED_CPW_TYPE;
  return { group, fillsGaps: group && (type === CPW_TYPE || type === GROUNDED_CPW_TYPE) };
}

/**
 * `PANEL_TRANSLINE` with its nine `TRANSLINE_IDENT`s and their TRANSLINE wrappers and calculators. The UI keeps one model while it is shown and draws
 * its fields; every method is one of the panel's event handlers.
 */
export class TranslinePanelModel {
  current = MICROSTRIP_TYPE;
  readonly slots = {} as Record<SlotKey, TranslineSlot>;
  /** The result lines (`m_left_messageN` / `m_MessageN`, ten of them). */
  readonly messages: { label: string; value: string }[] = Array.from({ length: 10 }, () => ({ label: "", value: "" }));
  /** `m_radioBtnPrm1` (1, selected first: wxRB_GROUP) or `m_radioBtnPrm2` (2). */
  radio: 1 | 2 = 1;
  dielectricModel = 0;
  specFrequencyText = "1";
  specFrequencyUnit = 0;
  soldermaskPresent = false;
  soldermaskThicknessText = "20";
  soldermaskEpsilonRText = "3.5";
  soldermaskTanDText = "0.025";
  soldermaskFillsGaps = true;
  /** The calculator's result maps after the last Analyze / Synthesize (for the tests). */
  lastAnalysis: TranslineResults | null = null;
  lastSynthesis: TranslineResults | null = null;

  private readonly prms: PrmState[][];
  private readonly extras: Extras[];
  private readonly calcs: TranslineCalculationBase[];
  /** Whether a type has ever been shown since the model was made (`m_TLine` is only current once chosen). */
  private hasCurrent = false;

  constructor() {
    for (const k of [...SUBS_SLOTS, ...PHYS_SLOTS, ...ELEC_SLOTS, "freq0" as const]) {
      this.slots[k] = { label: "", tooltip: "", text: "", enabled: false, unit: 0, unitShown: false, status: "ok", prm: null };
    }
    this.prms = TRANSLINE_TYPES.map((t) => t.prms.map((p) => ({ value: p.value, unitSelection: 0, normalized: 0, hasUnitCtrl: false, slot: null })));
    this.extras = TRANSLINE_TYPES.map(() => initExtras());
    this.calcs = TRANSLINE_TYPES.map((_, i) => makeCalc(i));
  }

  get typeDef(): TranslineTypeDef {
    return TRANSLINE_TYPES[this.current]!;
  }

  /** `UpdateSpecFrequencyEnable`: the spec frequency is typed only for the Djordjevic-Sarkar model. */
  get specFrequencyEnabled(): boolean {
    return this.dielectricModel === 1;
  }

  /** `updateSoldermaskEnables`. */
  get soldermaskEnables(): { fields: boolean; fillsGaps: boolean } {
    const shown = soldermaskShown(this.current);
    const present = shown.group && this.soldermaskPresent;
    return { fields: present, fillsGaps: present && shown.fillsGaps };
  }

  /** `TranslineTypeSelection`: relabel the controls for a type and show its values (`%g` of each `m_Value`). */
  private typeSelection(aType: number): void {
    this.current = aType < 0 || aType >= TRANSLINE_TYPE_COUNT || !Number.isInteger(aType) ? MICROSTRIP_TYPE : aType;
    this.hasCurrent = true;
    const def = this.typeDef;
    for (let jj = 0; jj < 10; jj++) this.messages[jj] = { label: def.messages[jj] ?? "", value: "" };
    const next: Record<PrmType, number> = { SUBS: 0, PHYS: 0, ELEC: 0, FREQUENCY: 0 };
    const groups: Record<PrmType, readonly SlotKey[]> = { SUBS: SUBS_SLOTS, PHYS: PHYS_SLOTS, ELEC: ELEC_SLOTS, FREQUENCY: ["freq0"] };
    const used = new Set<SlotKey>();
    def.prms.forEach((p, ii) => {
      const key = groups[p.type][next[p.type]++]!;
      used.add(key);
      const slot = this.slots[key];
      const st = this.prms[this.current]![ii]!;
      slot.tooltip = p.tooltip;
      slot.label = p.label !== "" ? `${p.label}:` : "";
      slot.prm = ii;
      st.slot = key;
      if (p.id !== "DUMMY") {
        slot.text = formatG(st.value);
        slot.enabled = true;
      } else {
        slot.text = "";
        slot.enabled = false;
      }
      if (p.convUnit) st.hasUnitCtrl = SLOT_UNITS[key] !== null;
      if (SLOT_UNITS[key]) {
        slot.unitShown = p.convUnit;
        slot.unit = st.unitSelection;
      }
    });
    for (const [key, slot] of Object.entries(this.slots) as [SlotKey, TranslineSlot][]) {
      if (used.has(key)) continue;
      slot.label = "";
      slot.tooltip = "";
      slot.text = "";
      slot.enabled = false;
      slot.prm = null;
      if (SLOT_UNITS[key]) {
        slot.unitShown = false;
        slot.unit = 0;
      }
    }
  }

  /** `TransfDlgDataToTranslineParams`: the texts typed into the type's values (`DoubleFromString`, scaled by the chooser), and the panel-level fields. */
  private transfer(): void {
    const def = this.typeDef;
    def.prms.forEach((_p, ii) => {
      const st = this.prms[this.current]![ii]!;
      const slot = this.slots[st.slot!];
      let value = doubleFromString(slot.text);
      st.value = value;
      if (st.hasUnitCtrl) {
        st.unitSelection = slot.unit;
        value *= unitScale(SLOT_UNITS[st.slot!]!, slot.unit);
      }
      st.normalized = value;
    });
    const x = this.extras[this.current]!;
    x.DIELECTRIC_MODEL = this.dielectricModel;
    const fSpec = doubleFromString(this.specFrequencyText) * unitScale(UNIT_SELECTOR_FREQUENCY, this.specFrequencyUnit);
    if (Number.isFinite(fSpec) && fSpec > 0.0) x.EPSILONR_SPEC_FREQ = fSpec;
    x.SOLDERMASK_PRESENT = this.soldermaskPresent ? 1.0 : 0.0;
    x.SOLDERMASK_THICKNESS = doubleFromString(this.soldermaskThicknessText) * 1.0e-6;
    x.SOLDERMASK_EPSILONR = doubleFromString(this.soldermaskEpsilonRText);
    x.SOLDERMASK_TAND = doubleFromString(this.soldermaskTanDText);
    x.SOLDERMASK_FILLS_GAPS = this.soldermaskFillsGaps ? 1.0 : 0.0;
  }

  /** `OnTranslineSelection`: keep what was typed for the type shown, then show the new one. */
  selectType(aType: number): void {
    if (this.hasCurrent) this.transfer();
    this.typeSelection(aType);
  }

  /** `OnTransLineResetButtonClick`: the type's parameters back to their defaults (and the default units where there is a chooser). */
  reset(): void {
    this.typeDef.prms.forEach((p, ii) => {
      const st = this.prms[this.current]![ii]!;
      st.value = p.value;
      if (st.hasUnitCtrl) st.unitSelection = 0;
    });
    this.typeSelection(this.current);
  }

  /** `OnTranslineAnalyse`. */
  analyze(): void {
    this.transfer();
    this.run(false);
  }

  /** `OnTranslineSynthetize`. */
  synthesize(): void {
    this.transfer();
    this.run(true);
  }

  // ------------------------------------------------------------------------------------------------------------------ the TRANSLINE glue

  /** `PANEL_TRANSLINE::GetPrmValue`: the normalized value of the type's parameter, 1.0 for one it does not have. */
  private getPrmValue(id: PrmsId): number {
    const i = this.typeDef.prms.findIndex((p) => p.id === id);
    return i >= 0 ? this.prms[this.current]![i]!.normalized : 1.0;
  }

  /** `SetPrmValue`: a result into a parameter and its text box (`%g` of the value in the chooser's unit). */
  private setPrmValue(id: PrmsId, aValue: number): void {
    const i = this.typeDef.prms.findIndex((p) => p.id === id);
    if (i < 0) return;
    const p = this.typeDef.prms[i]!;
    const st = this.prms[this.current]![i]!;
    st.normalized = aValue;
    const toUser = st.hasUnitCtrl && p.convUnit ? 1.0 / unitScale(SLOT_UNITS[st.slot!]!, this.slots[st.slot!].unit) : 1.0;
    st.value = st.normalized * toUser;
    this.slots[st.slot!].text = formatG(st.value);
  }

  /** `setErrorLevel` -> `SetPrmBgColor`: the background of the parameter's text box, when the type has the parameter. */
  private setErrorLevel(id: PrmsId, s: SlotStatus): void {
    const i = this.typeDef.prms.findIndex((p) => p.id === id);
    if (i < 0) return;
    this.slots[this.prms[this.current]![i]!.slot!].status = s;
  }

  /** `IsPrmSelected`. */
  private isSelected(id: PrmsId): boolean {
    if (id === "PHYS_WIDTH" || id === "PHYS_DIAM_IN") return this.radio === 1;
    if (id === "PHYS_S" || id === "PHYS_DIAM_OUT") return this.radio === 2;
    return false;
  }

  /** `SetResultInDialog`: `"%g " + unit`, or the text itself. */
  private setResult(line: number, value: number | string, unit = ""): void {
    const l = Math.max(0, Math.min(9, line));
    this.messages[l] = { ...this.messages[l]!, value: typeof value === "string" ? value : `${formatG(value)} ${unit}` };
  }

  /** `TRANSLINE::analyze` / `synthesize`: getProperties, checkProperties, calcAnalyze / calcSynthesize, showAnalyze / showSynthesize, show_results. */
  private run(synth: boolean): void {
    const m = {} as Record<Exclude<PrmsId, "DUMMY">, number>;
    // TRANSLINE::getProperties
    for (const id of UI_PRMS) {
      m[id] = this.getPrmValue(id);
      this.setErrorLevel(id, "ok");
    }
    const SIGMA = 1.0 / this.getPrmValue("RHO");
    const SKIN_DEPTH = 1.0 / Math.sqrt(Math.PI * m.FREQUENCY * m.MURC * MU0 * SIGMA);
    this.checkProperties(m);
    const x = this.extras[this.current]!;
    const calc = this.calcs[this.current]!;
    const set = (p: TCP, v: number) => calc.SetParameter(p, v);
    const mask = (fills: boolean) => {
      set("SOLDERMASK_PRESENT", x.SOLDERMASK_PRESENT);
      set("SOLDERMASK_THICKNESS", x.SOLDERMASK_THICKNESS);
      set("SOLDERMASK_EPSILONR", x.SOLDERMASK_EPSILONR);
      set("SOLDERMASK_TAND", x.SOLDERMASK_TAND);
      if (fills) set("SOLDERMASK_FILLS_GAPS", x.SOLDERMASK_FILLS_GAPS);
    };
    const model = () => {
      set("DIELECTRIC_MODEL_SEL", x.DIELECTRIC_MODEL);
      set("EPSILONR_SPEC_FREQ", x.EPSILONR_SPEC_FREQ);
    };
    const t = this.current;
    // each wrapper's getProperties
    switch (t) {
      case MICROSTRIP_TYPE:
        set("SIGMA", SIGMA);
        set("EPSILON_EFF", 1.0);
        set("SKIN_DEPTH", SKIN_DEPTH);
        set("EPSILONR", m.EPSILONR);
        model();
        mask(false);
        for (const [p, v] of [["H_T", m.H_T], ["H", m.H], ["PHYS_WIDTH", m.PHYS_WIDTH], ["T", m.T], ["Z0", m.Z0], ["FREQUENCY", m.FREQUENCY], ["ROUGH", m.ROUGH], ["TAND", m.TAND], ["PHYS_LEN", m.PHYS_LEN], ["MUR", m.MUR], ["MURC", m.MURC], ["ANG_L", m.ANG_L]] as [TCP, number][]) set(p, v);
        break;
      case C_MICROSTRIP_TYPE:
        set("Z0_E", m.Z0_E);
        set("Z0_O", m.Z0_O);
        set("EPSILONR", m.EPSILONR);
        model();
        mask(false);
        for (const [p, v] of [["PHYS_WIDTH", m.PHYS_WIDTH], ["PHYS_LEN", m.PHYS_LEN], ["PHYS_S", m.PHYS_S], ["H", m.H], ["T", m.T], ["H_T", m.H_T], ["FREQUENCY", m.FREQUENCY], ["MURC", m.MURC], ["SKIN_DEPTH", SKIN_DEPTH], ["SIGMA", SIGMA], ["ROUGH", m.ROUGH], ["TAND", m.TAND], ["ANG_L", m.ANG_L]] as [TCP, number][]) set(p, v);
        break;
      case STRIPLINE_TYPE:
        set("SKIN_DEPTH", SKIN_DEPTH);
        set("EPSILONR", m.EPSILONR);
        model();
        for (const [p, v] of [["T", m.T], ["STRIPLINE_A", m.STRIPLINE_A], ["H", m.H], ["Z0", m.Z0], ["PHYS_LEN", m.PHYS_LEN], ["FREQUENCY", m.FREQUENCY], ["TAND", m.TAND], ["PHYS_WIDTH", m.PHYS_WIDTH], ["ANG_L", m.ANG_L], ["SIGMA", SIGMA], ["MURC", m.MURC]] as [TCP, number][]) set(p, v);
        break;
      case C_STRIPLINE_TYPE:
        set("Z0_E", m.Z0_E);
        set("Z0_O", m.Z0_O);
        set("EPSILONR", m.EPSILONR);
        model();
        for (const [p, v] of [["PHYS_WIDTH", m.PHYS_WIDTH], ["PHYS_LEN", m.PHYS_LEN], ["PHYS_S", m.PHYS_S], ["H", m.H], ["STRIPLINE_A", m.STRIPLINE_A], ["T", m.T], ["FREQUENCY", m.FREQUENCY], ["MUR", m.MUR], ["MURC", m.MURC], ["SKIN_DEPTH", SKIN_DEPTH], ["SIGMA", SIGMA], ["TAND", m.TAND], ["ROUGH", m.ROUGH], ["ANG_L", m.ANG_L]] as [TCP, number][]) set(p, v);
        break;
      case CPW_TYPE:
      case GROUNDED_CPW_TYPE:
        set("EPSILONR", m.EPSILONR);
        model();
        mask(true);
        for (const [p, v] of [["TAND", m.TAND], ["SIGMA", SIGMA], ["MURC", m.MURC], ["PHYS_WIDTH", m.PHYS_WIDTH], ["PHYS_S", m.PHYS_S], ["H", m.H], ["T", m.T], ["PHYS_LEN", m.PHYS_LEN], ["FREQUENCY", m.FREQUENCY], ["Z0", m.Z0], ["ANG_L", m.ANG_L], ["SKIN_DEPTH", SKIN_DEPTH]] as [TCP, number][]) set(p, v);
        set("CPW_BACKMETAL", t === GROUNDED_CPW_TYPE ? 1.0 : 0.0);
        if (this.isSelected("PHYS_WIDTH")) calc.SetSynthesizeTarget("PHYS_WIDTH");
        else if (this.isSelected("PHYS_S")) calc.SetSynthesizeTarget("PHYS_S");
        break;
      case RECTWAVEGUIDE_TYPE:
        set("EPSILONR", m.EPSILONR);
        model();
        for (const [p, v] of [["TAND", m.TAND], ["SIGMA", SIGMA], ["MUR", m.MUR], ["MURC", m.MURC], ["PHYS_WIDTH", m.PHYS_WIDTH], ["PHYS_S", m.PHYS_S], ["PHYS_LEN", m.PHYS_LEN], ["FREQUENCY", m.FREQUENCY], ["Z0", m.Z0], ["ANG_L", m.ANG_L]] as [TCP, number][]) set(p, v);
        if (this.isSelected("PHYS_WIDTH")) calc.SetSynthesizeTarget("PHYS_WIDTH");
        break;
      case COAX_TYPE:
        set("SIGMA", SIGMA);
        set("SKIN_DEPTH", SKIN_DEPTH);
        set("EPSILONR", m.EPSILONR);
        model();
        for (const [p, v] of [["TAND", m.TAND], ["PHYS_DIAM_IN", m.PHYS_DIAM_IN], ["PHYS_DIAM_OUT", m.PHYS_DIAM_OUT], ["PHYS_LEN", m.PHYS_LEN], ["MUR", m.MUR], ["MURC", m.MURC], ["FREQUENCY", m.FREQUENCY], ["Z0", m.Z0], ["ANG_L", m.ANG_L]] as [TCP, number][]) set(p, v);
        if (this.isSelected("PHYS_DIAM_IN")) calc.SetSynthesizeTarget("PHYS_DIAM_IN");
        else if (this.isSelected("PHYS_DIAM_OUT")) calc.SetSynthesizeTarget("PHYS_DIAM_OUT");
        break;
      default:
        set("EPSILONR", m.EPSILONR);
        model();
        for (const [p, v] of [["TAND", m.TAND], ["SIGMA", SIGMA], ["MURC", m.MURC], ["PHYS_DIAM_IN", m.PHYS_DIAM_IN], ["PHYS_DIAM_OUT", m.PHYS_DIAM_OUT], ["PHYS_LEN", m.PHYS_LEN], ["FREQUENCY", m.FREQUENCY], ["Z0", m.Z0], ["ANG_L", m.ANG_L], ["TWISTEDPAIR_TWIST", m.TWISTEDPAIR_TWIST], ["TWISTEDPAIR_EPSILONR_ENV", m.TWISTEDPAIR_EPSILONR_ENV]] as [TCP, number][]) set(p, v);
        if (this.isSelected("PHYS_DIAM_IN")) calc.SetSynthesizeTarget("PHYS_DIAM_IN");
        else if (this.isSelected("PHYS_DIAM_OUT")) calc.SetSynthesizeTarget("PHYS_DIAM_OUT");
        break;
    }
    // calcAnalyze / calcSynthesize
    if (synth) calc.Synthesize("DEFAULT");
    else calc.Analyse();
    this.lastSynthesis = new Map(calc.GetSynthesisResults());
    this.lastAnalysis = new Map(calc.GetAnalysisResults());
    if (synth) this.showSynthesize(calc);
    else this.showAnalyze(calc);
    this.showResults(calc);
  }

  /** `TRANSLINE::checkProperties`: the inputs out of range get the warning background. */
  private checkProperties(m: Record<Exclude<PrmsId, "DUMMY">, number>): void {
    const fin = Number.isFinite;
    const warn = (id: PrmsId) => this.setErrorLevel(id, "warning");
    if (!fin(m.EPSILONR) || m.EPSILONR <= 0) warn("EPSILONR");
    if (!fin(m.TAND) || m.TAND < 0) warn("TAND");
    if (!fin(m.RHO) || m.RHO < 0) warn("RHO");
    if (!fin(m.H) || m.H < 0) warn("H");
    if (!fin(m.TWISTEDPAIR_TWIST) || m.TWISTEDPAIR_TWIST < 0) warn("TWISTEDPAIR_TWIST");
    if (!fin(m.STRIPLINE_A) || m.STRIPLINE_A <= 0) warn("STRIPLINE_A");
    if (!fin(m.H_T) || m.H_T <= 0 || (fin(m.H) && m.H > 0 && fin(m.T) && m.H_T < m.T + 0.1 * m.H)) warn("H_T");
    if (fin(m.STRIPLINE_A) && m.STRIPLINE_A > 0 && fin(m.H) && m.H > 0) {
      const aRatio = m.STRIPLINE_A / m.H;
      if (aRatio < 0.2 || aRatio > 0.8) warn("STRIPLINE_A");
    }
    if (fin(m.T) && m.T > 0 && fin(m.H) && m.H > 0 && m.T / m.H > 0.2) warn("T");
    if (!fin(m.MUR) || m.MUR < 0) warn("MUR");
    if (!fin(m.TWISTEDPAIR_EPSILONR_ENV) || m.TWISTEDPAIR_EPSILONR_ENV <= 0) warn("TWISTEDPAIR_EPSILONR_ENV");
    if (!fin(m.MURC) || m.MURC < 0) warn("MURC");
    if (!fin(m.FREQUENCY) || m.FREQUENCY <= 0) warn("FREQUENCY");
  }

  private level(id: PrmsId, r: [number, TranslineStatus]): void {
    this.setErrorLevel(id, STATUS_OF[r[1]]);
  }

  /** Each wrapper's `showAnalyze`. */
  private showAnalyze(calc: TranslineCalculationBase): void {
    const r = calc.GetAnalysisResults();
    const g = (p: TCP) => res(r, p);
    switch (this.current) {
      case MICROSTRIP_TYPE:
        this.setPrmValue("Z0", g("Z0")[0]);
        this.setPrmValue("ANG_L", g("ANG_L")[0]);
        this.microstripLines(g);
        this.levels(g, [["Z0", "Z0"], ["ANG_L", "ANG_L"], ["PHYS_LEN", "PHYS_LEN"], ["PHYS_WIDTH", "PHYS_WIDTH"]]);
        break;
      case C_MICROSTRIP_TYPE:
      case C_STRIPLINE_TYPE:
        this.setPrmValue("Z0_E", g("Z0_E")[0]);
        this.setPrmValue("Z0_O", g("Z0_O")[0]);
        this.setPrmValue("ANG_L", g("ANG_L")[0]);
        this.coupledLines(g);
        this.levels(g, [["Z0_E", "Z0_E"], ["Z0_O", "Z0_O"], ["ANG_L", "ANG_L"], ["PHYS_WIDTH", "PHYS_WIDTH"], ["PHYS_LEN", "PHYS_LEN"], ["PHYS_S", "PHYS_S"]]);
        break;
      case STRIPLINE_TYPE:
        this.striplineLines(g);
        this.setPrmValue("Z0", g("Z0")[0]);
        this.setPrmValue("ANG_L", g("ANG_L")[0]);
        this.levels(g, [["Z0", "Z0"], ["ANG_L", "ANG_L"], ["PHYS_LEN", "PHYS_LEN"], ["PHYS_WIDTH", "PHYS_WIDTH"], ["STRIPLINE_A", "STRIPLINE_A"], ["T", "T"], ["H", "T"]]);
        break;
      case CPW_TYPE:
      case GROUNDED_CPW_TYPE:
      case RECTWAVEGUIDE_TYPE:
        this.setPrmValue("Z0", g("Z0")[0]);
        this.setPrmValue("ANG_L", g("ANG_L")[0]);
        this.levels(g, [["Z0", "Z0"], ["ANG_L", "ANG_L"], ["PHYS_LEN", "PHYS_LEN"], ["PHYS_WIDTH", "PHYS_WIDTH"], ["PHYS_S", "PHYS_S"]]);
        break;
      default:
        // COAX_UI, TWISTEDPAIR_UI
        this.setPrmValue("Z0", g("Z0")[0]);
        this.setPrmValue("ANG_L", g("ANG_L")[0]);
        this.levels(g, [["Z0", "Z0"], ["ANG_L", "ANG_L"], ["PHYS_LEN", "PHYS_LEN"], ["PHYS_DIAM_IN", "PHYS_DIAM_IN"], ["PHYS_DIAM_OUT", "PHYS_DIAM_OUT"]]);
        break;
    }
  }

  /** Each wrapper's `showSynthesize` (the coupled lines read the analysis map, as KiCad's do). */
  private showSynthesize(calc: TranslineCalculationBase): void {
    const t = this.current;
    const r = t === C_MICROSTRIP_TYPE || t === C_STRIPLINE_TYPE ? calc.GetAnalysisResults() : calc.GetSynthesisResults();
    const g = (p: TCP) => res(r, p);
    switch (t) {
      case MICROSTRIP_TYPE:
        this.setPrmValue("PHYS_WIDTH", g("PHYS_WIDTH")[0]);
        this.setPrmValue("PHYS_LEN", g("PHYS_LEN")[0]);
        this.setPrmValue("Z0", g("Z0")[0]);
        this.setPrmValue("ANG_L", g("ANG_L")[0]);
        this.microstripLines(g);
        this.levels(g, [["Z0", "Z0"], ["ANG_L", "ANG_L"], ["PHYS_LEN", "PHYS_LEN"], ["PHYS_WIDTH", "PHYS_WIDTH"]]);
        break;
      case C_MICROSTRIP_TYPE:
      case C_STRIPLINE_TYPE:
        this.setPrmValue("PHYS_WIDTH", g("PHYS_WIDTH")[0]);
        this.setPrmValue("PHYS_S", g("PHYS_S")[0]);
        this.setPrmValue("PHYS_LEN", g("PHYS_LEN")[0]);
        this.coupledLines(g);
        this.levels(g, [["Z0_E", "Z0_E"], ["Z0_O", "Z0_O"], ["ANG_L", "ANG_L"], ["PHYS_WIDTH", "PHYS_WIDTH"], ["PHYS_LEN", "PHYS_LEN"], ["PHYS_S", "PHYS_S"]]);
        break;
      case STRIPLINE_TYPE:
        this.striplineLines(g);
        this.setPrmValue("PHYS_LEN", g("PHYS_LEN")[0]);
        this.setPrmValue("PHYS_WIDTH", g("PHYS_WIDTH")[0]);
        this.levels(g, [["Z0", "Z0"], ["ANG_L", "ANG_L"], ["PHYS_LEN", "PHYS_LEN"], ["PHYS_WIDTH", "PHYS_WIDTH"], ["STRIPLINE_A", "STRIPLINE_A"], ["T", "T"], ["H", "T"]]);
        break;
      case CPW_TYPE:
      case GROUNDED_CPW_TYPE:
      case RECTWAVEGUIDE_TYPE:
        if (this.isSelected("PHYS_WIDTH")) this.setPrmValue("PHYS_WIDTH", g("PHYS_WIDTH")[0]);
        if (this.isSelected("PHYS_S")) this.setPrmValue("PHYS_S", g("PHYS_S")[0]);
        this.setPrmValue("PHYS_LEN", g("PHYS_LEN")[0]);
        this.levels(g, [["Z0", "Z0"], ["ANG_L", "ANG_L"], ["PHYS_LEN", "PHYS_LEN"], ["PHYS_WIDTH", "PHYS_WIDTH"], ["PHYS_S", "PHYS_S"]]);
        break;
      default:
        if (this.isSelected("PHYS_DIAM_IN")) this.setPrmValue("PHYS_DIAM_IN", g("PHYS_DIAM_IN")[0]);
        else if (this.isSelected("PHYS_DIAM_OUT")) this.setPrmValue("PHYS_DIAM_OUT", g("PHYS_DIAM_OUT")[0]);
        this.setPrmValue("PHYS_LEN", g("PHYS_LEN")[0]);
        this.levels(g, [["Z0", "Z0"], ["ANG_L", "ANG_L"], ["PHYS_LEN", "PHYS_LEN"], ["PHYS_DIAM_IN", "PHYS_DIAM_IN"], ["PHYS_DIAM_OUT", "PHYS_DIAM_OUT"]]);
        break;
    }
  }

  /** Each wrapper's `show_results` (the four without result lines of their own in showAnalyze / showSynthesize). */
  private showResults(calc: TranslineCalculationBase): void {
    const t = this.current;
    if (t !== CPW_TYPE && t !== GROUNDED_CPW_TYPE && t !== RECTWAVEGUIDE_TYPE && t !== COAX_TYPE && t !== TWISTEDPAIR_TYPE) return;
    const r = calc.GetAnalysisResults();
    const g = (p: TCP) => res(r, p)[0];
    const modes = () => {
      const c = calc as RectWaveguide | Coax;
      return [c.GetTEModes() || "none", c.GetTMModes() || "none"];
    };
    if (t === CPW_TYPE || t === GROUNDED_CPW_TYPE) {
      this.setResult(0, g("EPSILON_EFF"), "");
      this.setResult(1, g("UNIT_PROP_DELAY"), "ps/cm");
      this.setResult(2, g("LOSS_CONDUCTOR"), "dB");
      this.setResult(3, g("LOSS_DIELECTRIC"), "dB");
      this.setResult(4, g("SKIN_DEPTH") / UNIT_MICRON, "µm");
    } else if (t === RECTWAVEGUIDE_TYPE) {
      // Z0EH = ZF0 / sqrt( eps_eff ) above cutoff (KiCad's own constant here, 376.730313668), zero below
      const epsEff = g("EPSILON_EFF");
      const z0eh = epsEff > 0.0 ? 376.730313668 / Math.sqrt(epsEff) : 0.0;
      this.setResult(0, z0eh, "Ohm");
      this.setResult(1, g("EPSILON_EFF"), "");
      this.setResult(2, g("LOSS_CONDUCTOR"), "dB");
      this.setResult(3, g("LOSS_DIELECTRIC"), "dB");
      const [te, tm] = modes();
      this.setResult(4, te!);
      this.setResult(5, tm!);
    } else if (t === COAX_TYPE) {
      this.setResult(0, g("EPSILONR"), "");
      this.setResult(1, g("LOSS_CONDUCTOR"), "dB");
      this.setResult(2, g("LOSS_DIELECTRIC"), "dB");
      const [te, tm] = modes();
      this.setResult(3, te!);
      this.setResult(4, tm!);
    } else {
      this.setResult(0, g("EPSILON_EFF"), "");
      this.setResult(1, g("LOSS_CONDUCTOR"), "dB");
      this.setResult(2, g("LOSS_DIELECTRIC"), "dB");
      this.setResult(3, g("SKIN_DEPTH") / UNIT_MICRON, "µm");
    }
  }

  private levels(g: (p: TCP) => [number, TranslineStatus], pairs: [PrmsId, TCP][]): void {
    for (const [id, p] of pairs) this.level(id, g(p));
  }

  private microstripLines(g: (p: TCP) => [number, TranslineStatus]): void {
    this.setResult(0, g("EPSILON_EFF")[0], "");
    this.setResult(1, g("UNIT_PROP_DELAY")[0], "ps/cm");
    this.setResult(2, g("ATTEN_COND")[0], "dB");
    this.setResult(3, g("ATTEN_DILECTRIC")[0], "dB");
    this.setResult(4, g("SKIN_DEPTH")[0] / UNIT_MICRON, "µm");
  }

  private striplineLines(g: (p: TCP) => [number, TranslineStatus]): void {
    this.setResult(0, g("EPSILON_EFF")[0], "");
    this.setResult(1, g("UNIT_PROP_DELAY")[0], "ps/cm");
    this.setResult(2, g("LOSS_CONDUCTOR")[0], "dB");
    this.setResult(3, g("LOSS_DIELECTRIC")[0], "dB");
    this.setResult(4, g("SKIN_DEPTH")[0] / UNIT_MICRON, "µm");
  }

  private coupledLines(g: (p: TCP) => [number, TranslineStatus]): void {
    this.setResult(0, g("EPSILON_EFF_EVEN")[0], "");
    this.setResult(1, g("EPSILON_EFF_ODD")[0], "");
    this.setResult(2, g("UNIT_PROP_DELAY_EVEN")[0], "ps/cm");
    this.setResult(3, g("UNIT_PROP_DELAY_ODD")[0], "ps/cm");
    if (this.current === C_MICROSTRIP_TYPE) {
      this.setResult(4, g("ATTEN_COND_EVEN")[0], "dB");
      this.setResult(5, g("ATTEN_COND_ODD")[0], "dB");
      this.setResult(6, g("ATTEN_DILECTRIC_EVEN")[0], "dB");
      this.setResult(7, g("ATTEN_DILECTRIC_ODD")[0], "dB");
      this.setResult(8, g("SKIN_DEPTH")[0] / UNIT_MICRON, "µm");
      this.setResult(9, g("Z_DIFF")[0], "Ω");
    } else {
      this.setResult(4, g("SKIN_DEPTH")[0] / UNIT_MICRON, "µm");
      this.setResult(5, g("Z_DIFF")[0], "Ω");
      this.setResult(6, g("Z_COMM")[0], "Ω");
      this.setResult(7, g("COUPLING_K")[0], "");
    }
  }

  // ------------------------------------------------------------------------------------------------------------------ settings

  /** `PANEL_TRANSLINE::LoadSettings` with each type's `TRANSLINE_IDENT::ReadConfig`. */
  loadSettings(cfg: PcbCalculatorSettings): void {
    const tl = cfg.translines;
    let modelSel = tl.dielectric_model;
    if (modelSel < 0 || modelSel > 1) modelSel = 0;
    this.dielectricModel = modelSel;
    if (tl.spec_frequency !== "") this.specFrequencyText = tl.spec_frequency;
    this.specFrequencyUnit = tl.spec_frequency_unit;
    this.soldermaskPresent = tl.soldermask_present !== 0;
    this.soldermaskThicknessText = formatG(tl.soldermask_thickness * 1.0e6);
    this.soldermaskEpsilonRText = formatG(tl.soldermask_epsilonr);
    this.soldermaskTanDText = formatG(tl.soldermask_tand);
    this.soldermaskFillsGaps = tl.soldermask_fills_gaps !== 0;
    // ReadConfig: each parameter whose keyword is stored (its unit only when the value is: KiCad reads the value, then the unit, in one try block)
    TRANSLINE_TYPES.forEach((def, t) => {
      const values = cfg.trans_line[def.name]?.values ?? {};
      const units = cfg.trans_line[def.name]?.units ?? {};
      def.prms.forEach((p, ii) => {
        if (!(p.key in values)) return;
        const st = this.prms[t]![ii]!;
        st.value = values[p.key]!;
        if (p.key in units) st.unitSelection = units[p.key]!;
      });
    });
    this.typeSelection(tl.type);
  }

  /** `PANEL_TRANSLINE::SaveSettings` with each type's `TRANSLINE_IDENT::WriteConfig` (a value that is not a number is stored, and kept, as 0). */
  saveSettings(cfg: PcbCalculatorSettings): void {
    if (this.hasCurrent) this.transfer();
    const tl = cfg.translines;
    tl.type = this.current;
    tl.dielectric_model = this.dielectricModel;
    tl.spec_frequency = this.specFrequencyText;
    tl.spec_frequency_unit = this.specFrequencyUnit;
    tl.soldermask_present = this.soldermaskPresent ? 1 : 0;
    const thicknessUm = doubleFromString(this.soldermaskThicknessText);
    if (Number.isFinite(thicknessUm)) tl.soldermask_thickness = thicknessUm * 1.0e-6;
    const epsR = doubleFromString(this.soldermaskEpsilonRText);
    if (Number.isFinite(epsR)) tl.soldermask_epsilonr = epsR;
    const tand = doubleFromString(this.soldermaskTanDText);
    if (Number.isFinite(tand)) tl.soldermask_tand = tand;
    tl.soldermask_fills_gaps = this.soldermaskFillsGaps ? 1 : 0;
    TRANSLINE_TYPES.forEach((def, t) => {
      const entry = (cfg.trans_line[def.name] ??= { values: {}, units: {} });
      def.prms.forEach((p, ii) => {
        const st = this.prms[t]![ii]!;
        if (!Number.isFinite(st.value)) st.value = 0;
        entry.values[p.key] = st.value;
        entry.units[p.key] = st.unitSelection;
      });
    });
  }
}
