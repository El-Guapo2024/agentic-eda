// Board Setup > Design Rules > Tuning Profiles' track grid calculator: the button of a Track width, Diff pair gap or Delay cell, which works out that cell
// (and the delay) from the profile's target impedance and the board's stackup -- `PANEL_SETUP_TUNING_PROFILE_INFO::calculateTrackParametersForCell`
// (pcbnew/dialogs/panel_setup_tuning_profile_info.cpp, KiCad 25ec2335), which KiCad runs from the `GRID_CELL_RUN_FUNCTION_EDITOR` of those three columns.
// The page is components/boardSetup/TuningProfilesPage.tsx; the calculators are kicad-port/transline.ts.
//
// Ported from panel_setup_tuning_profile_info.cpp (KiCad 25ec2335):
//   calculateTrackParametersForCell     microstrip on an outer layer, stripline inside; single or differential by the profile's type; the column picks
//                                       width / gap / delay; "Error: %s", "Could not compute track width" / "differential pair gap" / "track propagation delay"
//   getMicrostripBoardParameters        the signal copper, the reference (below F.Cu, above B.Cu), the dielectrics between, the solder mask
//   getStriplineBoardParameters         the top and bottom references and the dielectrics between each and the signal
//   calculateAverageDielectricConstants the thickness-weighted epsilon R and loss tangent of every sublayer, and their total height (m)
//   getDielectricLayers, getStackupLayerId, getSolderMaskParameters, getTargetImpedance, calculateSkinDepth
//   calculateSingleMicrostrip, calculateSingleStripline, calculateDifferentialMicrostrip, calculateDifferentialStripline -- the parameters each hands its
//                                       calculator, the result statuses it checks and their messages, the conversion to nanometres and attoseconds per mm
//
// Differences from KiCad, each forced by what the studio holds:
//   - The profile here has no "Frequency" or "Model solder mask" (the backend does not carry `m_Frequency` / `m_ModelSolderMask`); they are inputs that default
//     to what a new KiCad profile page shows: 1 GHz and ticked.
//   - The stackup has no dielectric model, so a layer is always `DIELECTRIC_MODEL::CONSTANT` (KiCad's Djordjevic-Sarkar refit of a layer is not reached;
//     the calculators' own overlay is ported in transline.ts). A value the stackup leaves out is the one KiCad's parser would leave: a dielectric's epsilon R
//     4.5 and loss tangent 0.02 (`BOARD_STACKUP_ITEM` of `BS_ITEM_TYPE_DIELECTRIC`), 1 and 0 for a sublayer after the first (`DIELECTRIC_PRMS`), a mask's
//     3.3 and 0, copper 0.035 mm and a mask 0.01 mm thick.
//   - KiCad's panel keeps one calculator of each kind for as long as the page is open, so a parameter set once stays set; every one the panel reads is set
//     again on each call except the solder mask's, which stays on after "Model solder mask" is unticked. Here each call starts afresh, as KiCad's first does.
//
// Dependency-free (types only), so it runs under `npm run test:unit`.

import type { ProfileLayerEntry, Stackup, StackupLayer, TuningProfileType } from "../api/types";
import { stackupItemType, sublayerCount, sublayerOf } from "./physicalStackup";
import { CoupledMicrostrip, CoupledStripline, MU0, Microstrip, Stripline, type TranslineResults } from "./transline";

/** `CalculationType`: the column whose button was pressed. */
export type CalculationType = "width" | "gap" | "delay";

/** `RHO`: the resistivity of copper (ohm metre). */
export const RHO = 1.72e-8;

/** What a new KiCad profile page shows: "1" "GHz" and "Model solder mask" ticked (panel_setup_tuning_profile_info_base.cpp). */
export const DEFAULT_FREQUENCY_HZ = 1e9;
export const DEFAULT_MODEL_SOLDER_MASK = true;

// ---------------------------------------------------------------------------------------------------- layers and the stackup

/** `PCB_LAYER_ID` of a layer by its canonical name: F_Cu 0, B_Cu 2, In1_Cu 4, In2_Cu 6, ..; the masks; -1 (`UNDEFINED_LAYER`) otherwise. */
export function pcbLayerId(name: string): number {
  if (name === "F.Cu") return 0;
  if (name === "B.Cu") return 2;
  if (name === "F.Mask") return 1;
  if (name === "B.Mask") return 3;
  const m = /^In([1-9]\d*)\.Cu$/.exec(name);
  return m ? 2 + 2 * Number(m[1]) : -1;
}

const F_Cu = 0;
const B_Cu = 2;
const F_Mask = 1;
const B_Mask = 3;

/** layer_ids.h `IsCopperLayerLowerThan`: `aLayerA` is nearer the bottom of the board than `aLayerB`. */
export function isCopperLayerLowerThan(aLayerA: number, aLayerB: number): boolean {
  if (aLayerA === aLayerB) return false;
  if (aLayerA === B_Cu) return true;
  if (aLayerB === B_Cu) return false;
  return aLayerA > aLayerB;
}

/** One sublayer as a `BOARD_STACKUP_ITEM` reads it: thickness in IU (nm), epsilon R, loss tangent. */
interface Sublayer {
  thickness: number;
  epsilonR: number;
  lossTangent: number;
}

/** The parts of a `BOARD_STACKUP_ITEM` the panel reads. */
interface StackupItem {
  brdLayerId: number;
  isDielectric: boolean;
  subs: Sublayer[];
}

/** A stackup layer as KiCad holds it, with the defaults its parser leaves for what the file does not say (see the header). */
function stackupItemOf(l: StackupLayer): StackupItem {
  const type = stackupItemType(l.name);
  const subs: Sublayer[] = [];
  for (let i = 0; i < sublayerCount(l); i++) {
    const s = sublayerOf(l, i);
    let thicknessDefault = 0;
    let erDefault = 1.0;
    let tanDefault = 0.0;
    if (type === "copper" && i === 0) thicknessDefault = 35_000;
    if (type === "soldermask") {
      thicknessDefault = 10_000;
      erDefault = 3.3;
    }
    if (type === "dielectric" && i === 0) {
      erDefault = 4.5;
      tanDefault = 0.02;
    }
    const t = s.thickness_mm;
    subs.push({
      thickness: t === null || t === undefined ? thicknessDefault : Math.round(t * 1e6),
      epsilonR: s.epsilon_r ?? erDefault,
      lossTangent: s.loss_tangent ?? tanDefault,
    });
  }
  // a dielectric is on no board layer (`UNDEFINED_LAYER`); a silkscreen or a paste layer is on one the panel never looks for
  const brdLayerId = type === "dielectric" ? -1 : type === "copper" || type === "soldermask" ? pcbLayerId(l.name) : -2;
  return { brdLayerId, isDielectric: type === "dielectric", subs };
}

/** `EDA_IU_SCALE::IUTomm` of pcbIUScale (an `int` of nanometres). */
const IUTomm = (iu: number) => toInt(iu) / 1e6;

/** `static_cast<int>` of a double as x86-64 does it (`cvttsd2si`): truncated, and INT_MIN for NaN or out of range. */
export function toInt(v: number): number {
  if (!Number.isFinite(v) || v >= 2147483648 || v <= -2147483649) return -2147483648;
  return Math.trunc(v);
}

/** `getStackupLayerId`: the index of the stackup item of the layer, -1 when none. */
function getStackupLayerId(aLayerList: StackupItem[], aPcbLayerId: number): number {
  const i = aLayerList.findIndex((l) => l.brdLayerId === aPcbLayerId);
  return i;
}

/** `DIELECTRIC_INFO`: height (m), epsilon R, loss tangent. */
export interface DielectricInfo {
  Height: number;
  E_r: number;
  Loss_Tangent: number;
}

/** `calculateAverageDielectricConstants`: every sublayer of the dielectrics weighted by its thickness; zeros when they add up to no height. */
function calculateAverageDielectricConstants(aStackupLayerList: StackupItem[], ids: number[]): DielectricInfo {
  let totalHeight = 0.0;
  let e_r = 0.0;
  let lossTangent = 0.0;
  for (const i of ids) {
    const layer = aStackupLayerList[i]!;
    for (const sub of layer.subs) {
      totalHeight += IUTomm(sub.thickness);
      e_r += sub.epsilonR * IUTomm(sub.thickness);
      lossTangent += sub.lossTangent * IUTomm(sub.thickness);
    }
  }
  if (totalHeight <= 0.0) return { Height: 0.0, E_r: 0.0, Loss_Tangent: 0.0 };
  e_r = e_r / totalHeight;
  lossTangent = lossTangent / totalHeight;
  totalHeight /= 1000.0;
  return { Height: totalHeight, E_r: e_r, Loss_Tangent: lossTangent };
}

/** `getDielectricLayers`: the dielectrics strictly between the two stackup indices. */
function getDielectricLayers(aStackupLayerList: StackupItem[], aSignalLayerId: number, aReferenceLayerId: number, out: number[]): void {
  for (let i = Math.min(aSignalLayerId, aReferenceLayerId) + 1; i < Math.max(aSignalLayerId, aReferenceLayerId); ++i) {
    const layer = aStackupLayerList[i];
    // `aStackupLayerList.at( i )` throws past the end; an index of -1 (a reference not in the stackup) starts the loop at 0, which is in range
    if (!layer) throw new RangeError("vector::at");
    if (!layer.isDielectric) continue;
    out.push(i);
  }
}

/** `getSolderMaskParameters`: the mask over F.Cu or B.Cu (thickness in m), zeros for another layer or none. */
function getSolderMaskParameters(aStackupLayerList: StackupItem[], aSignalLayerId: number): DielectricInfo {
  let maskLayerId: number;
  if (aSignalLayerId === F_Cu) maskLayerId = F_Mask;
  else if (aSignalLayerId === B_Cu) maskLayerId = B_Mask;
  else return { Height: 0.0, E_r: 0.0, Loss_Tangent: 0.0 };
  const layerIdx = getStackupLayerId(aStackupLayerList, maskLayerId);
  if (layerIdx < 0) return { Height: 0.0, E_r: 0.0, Loss_Tangent: 0.0 };
  const layer = aStackupLayerList[layerIdx]!;
  return { Height: IUTomm(layer.subs[0]!.thickness) / 1000.0, E_r: layer.subs[0]!.epsilonR, Loss_Tangent: layer.subs[0]!.lossTangent };
}

// ---------------------------------------------------------------------------------------------------- the panel

/** One cell's press: the profile's values, the board, the row and the column. */
export interface TrackCellInput {
  profileType: TuningProfileType;
  /** The "Target impedance" field (ohms); `NaN` for text that is no number, which KiCad reads as -1. */
  targetImpedance: number;
  /** Hz; KiCad's "Frequency" field, default `DEFAULT_FREQUENCY_HZ`. */
  frequencyHz?: number;
  /** KiCad's "Model solder mask", default `DEFAULT_MODEL_SOLDER_MASK`. */
  modelSolderMask?: boolean;
  /** `m_layerNames`: the board's copper layers, by the names the grid's layer cells hold. */
  copperLayers: readonly string[];
  /** `BOARD::GetStackupOrDefault()`. */
  stackup: Stackup;
  row: Pick<ProfileLayerEntry, "signal_layer" | "top_reference_layer" | "bottom_reference_layer" | "width_nm" | "diff_pair_gap_nm">;
  column: CalculationType;
}

/** `CALCULATION_BOARD_PARAMETERS`: permittivity, thicknesses in metres. */
export interface CalculationBoardParameters {
  SignalLayer: number;
  DielectricConstant: number;
  TopDielectricLayerThickness: number;
  BottomDielectricLayerThickness: number;
  SignalLayerThickness: number;
  LossTangent: number;
  SolderMaskDielectricConstant: number;
  SolderMaskThickness: number;
  SolderMaskLossTangent: number;
}

/** `CALCULATION_RESULT`: the width, gap and delay in IU (nm, nm, as/mm), or the error message. */
export interface CalculationResult {
  OK: boolean;
  Width: number;
  DiffPairGap: number;
  Delay: number;
  ErrorMsg: string;
}

const err = (msg: string): CalculationResult => ({ OK: false, Width: 0, DiffPairGap: 0, Delay: 0, ErrorMsg: msg });

/** What pressing the cell's button does: nothing (the signal layer is not one of the board's), KiCad's error box, or the cells it writes. */
export type TrackCellOutcome = { kind: "none" } | { kind: "error"; message: string } | { kind: "ok"; patch: Partial<Pick<ProfileLayerEntry, "width_nm" | "diff_pair_gap_nm" | "delay_as_per_mm">> };

/** The grid row's cells and the page's settings as the panel's functions read them. */
class Panel {
  readonly stackup: StackupItem[];
  constructor(private readonly input: TrackCellInput) {
    this.stackup = input.stackup.layers.map(stackupItemOf);
  }

  private get frequency(): number {
    return this.input.frequencyHz ?? DEFAULT_FREQUENCY_HZ;
  }

  private get modelSolderMask(): boolean {
    return this.input.modelSolderMask ?? DEFAULT_MODEL_SOLDER_MASK;
  }

  /** `m_layerNamesToIDs.contains( name )`. */
  contains(name: string | null | undefined): name is string {
    return !!name && this.input.copperLayers.includes(name) && pcbLayerId(name) >= 0;
  }

  /** `getTargetImpedance`: -1 for text that is no number. */
  getTargetImpedance(): number {
    const z = this.input.targetImpedance;
    return Number.isFinite(z) ? z : -1;
  }

  /** `GetUnitValue` of a width cell: an empty cell is 0. */
  private unitValue(v: number): number {
    return Number.isFinite(v) ? toInt(v) : 0;
  }

  /** `GetOptionalUnitValue`: an empty cell is none. */
  private optionalUnitValue(v: number): number | undefined {
    return Number.isFinite(v) ? toInt(v) : undefined;
  }

  /** `getMicrostripBoardParameters`. */
  getMicrostripBoardParameters(): [CalculationBoardParameters | null, CalculationResult] {
    const list = this.stackup;
    const r = this.input.row;
    if (!this.contains(r.signal_layer)) return [null, err("Signal layer not found in stackup")];
    const signalLayer = pcbLayerId(r.signal_layer);
    if (signalLayer !== F_Cu && signalLayer !== B_Cu) return [null, err("Internal error: Microstrip can only be on an outer copper layer")];
    const signalLayerStackupId = getStackupLayerId(list, signalLayer);
    if (signalLayerStackupId === -1) return [null, err("Signal layer not found in stackup")];
    const signalLayerThickness = IUTomm(list[signalLayerStackupId]!.subs[0]!.thickness) / 1000.0;
    if (signalLayerThickness <= 0) return [null, err("Signal layer thickness must be greater than 0")];
    const referenceLayerName = signalLayer === F_Cu ? r.bottom_reference_layer : r.top_reference_layer;
    if (!this.contains(referenceLayerName)) return [null, err("Reference layer not found in stackup")];
    const referenceLayer = pcbLayerId(referenceLayerName);
    const referenceLayerStackupId = getStackupLayerId(list, referenceLayer);
    if (signalLayerStackupId === referenceLayerStackupId) return [null, err("Reference layer must be different to signal layer")];
    const ids: number[] = [];
    getDielectricLayers(list, signalLayerStackupId, referenceLayerStackupId, ids);
    const dielectricInfo = calculateAverageDielectricConstants(list, ids);
    if (dielectricInfo.Height <= 0.0) return [null, err("Dielectric height must be greater than 0")];
    const solderMaskInfo = getSolderMaskParameters(list, signalLayer);
    return [
      {
        SignalLayer: signalLayer,
        DielectricConstant: dielectricInfo.E_r,
        TopDielectricLayerThickness: dielectricInfo.Height,
        BottomDielectricLayerThickness: 0.0,
        SignalLayerThickness: signalLayerThickness,
        LossTangent: dielectricInfo.Loss_Tangent,
        SolderMaskDielectricConstant: solderMaskInfo.E_r,
        SolderMaskThickness: solderMaskInfo.Height,
        SolderMaskLossTangent: solderMaskInfo.Loss_Tangent,
      },
      { ...err(""), OK: true },
    ];
  }

  /** `getStriplineBoardParameters`. */
  getStriplineBoardParameters(): [CalculationBoardParameters | null, CalculationResult] {
    const list = this.stackup;
    const r = this.input.row;
    if (!this.contains(r.signal_layer)) return [null, err("Signal layer not found in stackup")];
    const signalLayer = pcbLayerId(r.signal_layer);
    const signalLayerStackupId = getStackupLayerId(list, signalLayer);
    if (signalLayerStackupId === -1) return [null, err("Signal layer not found in stackup")];
    const signalLayerThickness = IUTomm(list[signalLayerStackupId]!.subs[0]!.thickness) / 1000.0;
    if (signalLayerThickness <= 0) return [null, err("Signal layer thickness must be greater than 0")];
    if (!this.contains(r.top_reference_layer)) return [null, err("Top reference layer not found in stackup")];
    const topReferenceLayer = pcbLayerId(r.top_reference_layer);
    const topReferenceLayerStackupId = getStackupLayerId(list, topReferenceLayer);
    if (!isCopperLayerLowerThan(signalLayer, topReferenceLayer)) return [null, err("Top reference layer must be above signal layer in board stackup")];
    if (!this.contains(r.bottom_reference_layer)) return [null, err("Bottom reference layer not found in stackup")];
    const bottomReferenceLayer = pcbLayerId(r.bottom_reference_layer);
    const bottomReferenceLayerStackupId = getStackupLayerId(list, bottomReferenceLayer);
    if (!isCopperLayerLowerThan(bottomReferenceLayer, signalLayer)) return [null, err("Bottom reference layer must be below signal layer in board stackup")];
    const top: number[] = [];
    const bottom: number[] = [];
    getDielectricLayers(list, signalLayerStackupId, topReferenceLayerStackupId, top);
    getDielectricLayers(list, signalLayerStackupId, bottomReferenceLayerStackupId, bottom);
    const all = [...top, ...bottom];
    const topDielectricInfo = calculateAverageDielectricConstants(list, top);
    const bottomDielectricInfo = calculateAverageDielectricConstants(list, bottom);
    const allDielectricInfo = calculateAverageDielectricConstants(list, all);
    if (topDielectricInfo.Height <= 0.0 && bottomDielectricInfo.Height <= 0.0) return [null, err("Dielectric heights must be greater than 0")];
    return [
      {
        SignalLayer: signalLayer,
        DielectricConstant: allDielectricInfo.E_r,
        TopDielectricLayerThickness: topDielectricInfo.Height,
        BottomDielectricLayerThickness: bottomDielectricInfo.Height,
        SignalLayerThickness: signalLayerThickness,
        LossTangent: allDielectricInfo.Loss_Tangent,
        SolderMaskDielectricConstant: 0,
        SolderMaskThickness: 0,
        SolderMaskLossTangent: 0,
      },
      { ...err(""), OK: true },
    ];
  }

  /** `calculateSingleMicrostrip`. */
  calculateSingleMicrostrip(aCalculationType: CalculationType): CalculationResult {
    const calc = new Microstrip();
    const targetZ = this.getTargetImpedance();
    if (targetZ <= 0) return err("Target impedance must be greater than 0");
    const [bp, result] = this.getMicrostripBoardParameters();
    if (!result.OK || !bp) return result;
    if (aCalculationType === "width") calc.SetParameter("PHYS_WIDTH", 0.001);
    else if (aCalculationType === "delay") calc.SetParameter("PHYS_WIDTH", IUTomm(this.unitValue(this.input.row.width_nm)) / 1000.0);
    const frequency = this.frequency;
    calc.SetParameter("SIGMA", 1.0 / RHO);
    calc.SetParameter("EPSILON_EFF", 1.0);
    calc.SetParameter("SKIN_DEPTH", calculateSkinDepth(frequency, 1.0, 1.0 / RHO));
    calc.SetParameter("EPSILONR", bp.DielectricConstant);
    calc.SetParameter("H_T", 1e20);
    calc.SetParameter("H", bp.TopDielectricLayerThickness);
    calc.SetParameter("T", bp.SignalLayerThickness);
    calc.SetParameter("Z0", targetZ);
    calc.SetParameter("FREQUENCY", frequency);
    calc.SetParameter("ROUGH", 0);
    calc.SetParameter("TAND", bp.LossTangent);
    calc.SetParameter("PHYS_LEN", 10);
    calc.SetParameter("MUR", 1);
    calc.SetParameter("MURC", 1);
    calc.SetParameter("ANG_L", 1);
    if (this.modelSolderMask) {
      calc.SetParameter("SOLDERMASK_PRESENT", 1.0);
      calc.SetParameter("SOLDERMASK_THICKNESS", bp.SolderMaskThickness);
      calc.SetParameter("SOLDERMASK_EPSILONR", bp.SolderMaskDielectricConstant);
      calc.SetParameter("SOLDERMASK_TAND", bp.SolderMaskLossTangent);
    }
    if (aCalculationType === "width") calc.Synthesize("DEFAULT");
    else calc.Analyse();
    const results = aCalculationType === "width" ? calc.GetSynthesisResults() : calc.GetAnalysisResults();
    return singleResults(results);
  }

  /** `calculateSingleStripline`. */
  calculateSingleStripline(aCalculationType: CalculationType): CalculationResult {
    const calc = new Stripline();
    const targetZ = this.getTargetImpedance();
    if (targetZ <= 0) return err("Target impedance must be greater than 0");
    const [bp, result] = this.getStriplineBoardParameters();
    if (!result.OK || !bp) return result;
    if (aCalculationType === "width") calc.SetParameter("PHYS_WIDTH", 0.001);
    else if (aCalculationType === "delay") calc.SetParameter("PHYS_WIDTH", IUTomm(this.unitValue(this.input.row.width_nm)) / 1000.0);
    calc.SetParameter("SKIN_DEPTH", calculateSkinDepth(1.0, 1.0, 1.0 / RHO));
    calc.SetParameter("EPSILONR", bp.DielectricConstant);
    calc.SetParameter("T", bp.SignalLayerThickness);
    calc.SetParameter("STRIPLINE_A", bp.TopDielectricLayerThickness);
    calc.SetParameter("H", bp.TopDielectricLayerThickness + bp.SignalLayerThickness + bp.BottomDielectricLayerThickness);
    calc.SetParameter("Z0", targetZ);
    calc.SetParameter("PHYS_LEN", 1.0);
    calc.SetParameter("FREQUENCY", this.frequency);
    calc.SetParameter("TAND", bp.LossTangent);
    calc.SetParameter("ANG_L", 1.0);
    calc.SetParameter("SIGMA", 1.0 / RHO);
    calc.SetParameter("MURC", 1);
    if (aCalculationType === "width") calc.Synthesize("DEFAULT");
    else calc.Analyse();
    const results = aCalculationType === "width" ? calc.GetSynthesisResults() : calc.GetAnalysisResults();
    return singleResults(results);
  }

  /** The width / gap cells the differential calculations read, and their messages. */
  private diffGeometry(aCalculationType: CalculationType): CalculationResult | { width: number; gap: number } {
    const widthOpt = this.optionalUnitValue(this.input.row.width_nm);
    const gapOpt = this.optionalUnitValue(this.input.row.diff_pair_gap_nm);
    let width = 0.0;
    let gap = 0.0;
    if (aCalculationType === "width") {
      if (gapOpt === undefined || gapOpt <= 0) return err("Diff pair gap must be greater than 0 to calculate width");
      gap = IUTomm(gapOpt) / 1000.0;
    } else if (aCalculationType === "gap") {
      if (widthOpt === undefined || widthOpt <= 0) return err("Width must be greater than 0 to calculate diff pair gap");
      width = IUTomm(widthOpt) / 1000.0;
    } else {
      if (widthOpt === undefined || gapOpt === undefined || widthOpt <= 0 || gapOpt <= 0) return err("Width and diff pair gap must be greater than 0 to calculate delay");
      width = IUTomm(widthOpt) / 1000.0;
      gap = IUTomm(gapOpt) / 1000.0;
    }
    return { width, gap };
  }

  /** `calculateDifferentialMicrostrip`. */
  calculateDifferentialMicrostrip(aCalculationType: CalculationType): CalculationResult {
    const calc = new CoupledMicrostrip();
    const targetZ = this.getTargetImpedance();
    if (targetZ <= 0) return err("Target impedance must be greater than 0");
    const [bp, result] = this.getMicrostripBoardParameters();
    if (!result.OK || !bp) return result;
    const geom = this.diffGeometry(aCalculationType);
    if ("OK" in geom) return geom;
    calc.SetParameter("Z0_E", targetZ / 2.0);
    calc.SetParameter("Z0_O", targetZ / 2.0);
    calc.SetParameter("Z_DIFF", targetZ);
    calc.SetParameter("PHYS_WIDTH", geom.width);
    calc.SetParameter("PHYS_S", geom.gap);
    calc.SetParameter("EPSILONR", bp.DielectricConstant);
    calc.SetParameter("PHYS_LEN", 10);
    calc.SetParameter("H", bp.TopDielectricLayerThickness);
    calc.SetParameter("T", bp.SignalLayerThickness);
    calc.SetParameter("H_T", 1e20);
    calc.SetParameter("FREQUENCY", this.frequency);
    calc.SetParameter("MURC", 1);
    calc.SetParameter("SKIN_DEPTH", calculateSkinDepth(1.0, 1.0, 1.0 / RHO));
    calc.SetParameter("SIGMA", 1.0 / RHO);
    calc.SetParameter("ROUGH", 0);
    calc.SetParameter("TAND", bp.LossTangent);
    calc.SetParameter("ANG_L", 1);
    if (this.modelSolderMask) {
      calc.SetParameter("SOLDERMASK_PRESENT", 1.0);
      calc.SetParameter("SOLDERMASK_THICKNESS", bp.SolderMaskThickness);
      calc.SetParameter("SOLDERMASK_EPSILONR", bp.SolderMaskDielectricConstant);
      calc.SetParameter("SOLDERMASK_TAND", bp.SolderMaskLossTangent);
    }
    if (aCalculationType === "width") calc.Synthesize("FIX_SPACING");
    else if (aCalculationType === "gap") calc.Synthesize("FIX_WIDTH");
    else calc.Analyse();
    return differentialResults(aCalculationType === "delay" ? calc.GetAnalysisResults() : calc.GetSynthesisResults());
  }

  /** `calculateDifferentialStripline`. */
  calculateDifferentialStripline(aCalculationType: CalculationType): CalculationResult {
    const calc = new CoupledStripline();
    const targetZ = this.getTargetImpedance();
    if (targetZ <= 0) return err("Target impedance must be greater than 0");
    const [bp, result] = this.getStriplineBoardParameters();
    if (!result.OK || !bp) return result;
    const geom = this.diffGeometry(aCalculationType);
    if ("OK" in geom) return geom;
    calc.SetParameter("Z0_E", targetZ / 2.0);
    calc.SetParameter("Z0_O", targetZ / 2.0);
    calc.SetParameter("Z_DIFF", targetZ);
    calc.SetParameter("PHYS_WIDTH", geom.width);
    calc.SetParameter("PHYS_S", geom.gap);
    calc.SetParameter("T", bp.SignalLayerThickness);
    const totalH = bp.TopDielectricLayerThickness + bp.SignalLayerThickness + bp.BottomDielectricLayerThickness;
    // STRIPLINE_A is the strip's midplane from the ground: Top + T/2 (the centred fast path when that is h/2)
    const striplineA = bp.TopDielectricLayerThickness + 0.5 * bp.SignalLayerThickness;
    calc.SetParameter("H", totalH);
    calc.SetParameter("STRIPLINE_A", CoupledStripline.IsCenteredOffset(striplineA, totalH) ? 0.0 : striplineA);
    calc.SetParameter("EPSILONR", bp.DielectricConstant);
    calc.SetParameter("SKIN_DEPTH", calculateSkinDepth(1.0, 1.0, 1.0 / RHO));
    calc.SetParameter("PHYS_LEN", 1.0);
    calc.SetParameter("FREQUENCY", this.frequency);
    calc.SetParameter("ANG_L", 1.0);
    calc.SetParameter("SIGMA", 1.0 / RHO);
    calc.SetParameter("MURC", 1);
    if (aCalculationType === "width") calc.Synthesize("FIX_SPACING");
    else if (aCalculationType === "gap") calc.Synthesize("FIX_WIDTH");
    else calc.Analyse();
    return differentialResults(aCalculationType === "delay" ? calc.GetAnalysisResults() : calc.GetSynthesisResults());
  }
}

/** `calculateSkinDepth`. */
export function calculateSkinDepth(aFreq: number, aMurc: number, aSigma: number): number {
  return 1.0 / Math.sqrt(Math.PI * aFreq * aMurc * MU0 * aSigma);
}

/** `results[ aParam ]`: a parameter the map lacks reads as ( 0, OK ). */
function at(results: TranslineResults, p: Parameters<TranslineResults["get"]>[0]): [number, string] {
  return results.get(p) ?? [0, "OK"];
}

/** `EDA_UNIT_UTILS::UI::FromUserUnit( pcbIUScale, EDA_UNITS::MM, m * 1000 )` cast to int: metres to nanometres. */
const mToIU = (m: number) => toInt(m * 1000.0 * 1e6);
/** `FromUserUnit( pcbIUScale, EDA_UNITS::PS_PER_CM, v )` cast to int: ps/cm to attoseconds per millimetre. */
const psPerCmToIU = (v: number) => toInt((v / 10) * 1e6);

function singleResults(results: TranslineResults): CalculationResult {
  if (at(results, "PHYS_WIDTH")[1] !== "OK") return err("Width calculation failed");
  if (at(results, "UNIT_PROP_DELAY")[1] !== "OK") return err("Delay calculation failed");
  return { OK: true, Width: mToIU(at(results, "PHYS_WIDTH")[0]), DiffPairGap: 0, Delay: psPerCmToIU(at(results, "UNIT_PROP_DELAY")[0]), ErrorMsg: "" };
}

function differentialResults(results: TranslineResults): CalculationResult {
  if (at(results, "PHYS_WIDTH")[1] !== "OK") return err("Width calculation failed");
  if (at(results, "PHYS_S")[1] !== "OK") return err("Diff pair gap calculation failed");
  if (at(results, "UNIT_PROP_DELAY_ODD")[1] !== "OK") return err("Delay calculation failed");
  return { OK: true, Width: mToIU(at(results, "PHYS_WIDTH")[0]), DiffPairGap: mToIU(at(results, "PHYS_S")[0]), Delay: psPerCmToIU(at(results, "UNIT_PROP_DELAY_ODD")[0]), ErrorMsg: "" };
}

/** The board parameters a row works with (microstrip on an outer layer, else stripline), or the error that stops it: for tests and diagnostics. */
export function boardParametersFor(input: TrackCellInput): [CalculationBoardParameters | null, CalculationResult] {
  const panel = new Panel(input);
  const id = pcbLayerId(input.row.signal_layer);
  return id === F_Cu || id === B_Cu ? panel.getMicrostripBoardParameters() : panel.getStriplineBoardParameters();
}

/**
 * `calculateTrackParametersForCell`: the press of a Track width, Diff pair gap or Delay cell's button. Nothing when the row's signal layer is not one of the
 * board's copper layers; else KiCad's message ("Error: %s" of the calculation's, or "Could not compute track width" / "Could not compute differential pair
 * gap" / "Could not compute track propagation delay" for a result that is not above 0); else the cells it writes -- the width (Track width) or the gap
 * (Diff pair gap, differential profiles), and the delay always.
 */
export function calculateTrackParametersForCell(input: TrackCellInput): TrackCellOutcome {
  const panel = new Panel(input);
  if (!panel.contains(input.row.signal_layer)) return { kind: "none" };
  const signalLayer = pcbLayerId(input.row.signal_layer);
  const isMicrostrip = signalLayer === F_Cu || signalLayer === B_Cu;
  const calculationType = input.column;
  const differential = input.profileType === "differential";
  let result: CalculationResult;
  if (differential) result = isMicrostrip ? panel.calculateDifferentialMicrostrip(calculationType) : panel.calculateDifferentialStripline(calculationType);
  else result = isMicrostrip ? panel.calculateSingleMicrostrip(calculationType) : panel.calculateSingleStripline(calculationType);
  if (!result.OK) return { kind: "error", message: `Error: ${result.ErrorMsg}` };
  if (!(result.Width > 0)) return { kind: "error", message: "Could not compute track width" };
  if (differential && !(result.DiffPairGap > 0)) return { kind: "error", message: "Could not compute differential pair gap" };
  if (!(result.Delay > 0)) return { kind: "error", message: "Could not compute track propagation delay" };
  const patch: Partial<Pick<ProfileLayerEntry, "width_nm" | "diff_pair_gap_nm" | "delay_as_per_mm">> = {};
  if (calculationType === "width") patch.width_nm = result.Width;
  else if (differential && calculationType === "gap") patch.diff_pair_gap_nm = result.DiffPairGap;
  patch.delay_as_per_mm = result.Delay;
  return { kind: "ok", patch };
}
