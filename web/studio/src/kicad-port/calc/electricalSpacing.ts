// The Electrical Spacing calculator (pcb_calculator/calculator_panels/panel_electrical_spacing*.cpp at KiCad 25ec2335): a notebook of two pages,
//   "IPC 2221"   `PANEL_ELECTRICAL_SPACING_IPC2221`: the minimum spacing table of IPC-2221C (Dec 2023) by voltage class and conductor case, in the unit
//                chosen, its last row worked out for the voltage typed (`ElectricalSpacingUpdateData`);
//   "IEC 60664"  `PANEL_ELECTRICAL_SPACING_IEC60664`: the rated impulse voltage (`CalculateTransientImpulse`) and the clearance, creepage and minimum
//                groove width (`CalculateClearanceCreepage`) of IEC 60664-1 (kicad-port/calc/iec60664.ts).
// The panel UI is components/calculator/PanelElectricalSpacing.tsx.
//
// Pure: no React, no DOM.
import { IEC60664, INSULATION_TYPE, MATERIAL_GROUP, OV_CATEGORY, POLLUTION_DEGREE, type InsulationType, type MaterialGroup, type OvCategory, type PollutionDegree } from "./iec60664";
import { printfG as formatG, wxToDouble } from "./cformat";
import { UNIT_MM, doubleFromString } from "./unitSelectors";

/** `VALUE_COUNT` columns (B1 .. A8) and `CLASS_COUNT` rows (nine voltage classes and the per-volt row for > 500 V). */
export const IPC2221_VALUE_COUNT = 8;
export const IPC2221_CLASS_COUNT = 10;

/** The grid's column labels. */
export const IPC2221_COLUMNS = ["B1", "B2", "B3", "B4", "B5", "A6", "A7", "A8"] as const;

/** The grid's row labels (`SetRowLabelValue`). */
export const IPC2221_ROWS = ["0 .. 15 V", "16 .. 30 V", "31 .. 50 V", "51 .. 100 V", "101 .. 150 V", "151 .. 170 V", "171 .. 250 V", "251 .. 300 V", "301 .. 500 V", " > 500 V"] as const;

/** `m_stHelp`, the legend under the grid. */
export const IPC2221_LEGEND =
  "*  B1 - Internal Conductors\n*  B2 - External Conductors, uncoated, sea level to 3050 m \n*  B3 - External Conductors, uncoated, over 3050 m or a vacuum\n*  B4 - External Conductors, with permanent polymer coating (any elevation)\n*  B5 - External Conductors, with conformal (any elevation or in a vacuum)\n*  A6 - External Component lead termination, with conformal coating (any elevation or in a vacuum)\n*  A7 - External Component lead/termination, uncoated, sea level to 3050 m \n*  A8 - External Component lead/termination, uncoated, over 3050m or in a vacuum";

const mm = (row: number[]): number[] => row.map((v) => v * UNIT_MM);

/** `clist`: metres. The last row is the per-volt increment added to the 301 .. 500 V row above 500 V, not a spacing. */
export const IPC2221_CLIST: readonly (readonly number[])[] = [
  mm([0.05, 0.1, 0.1, 0.075, 0.075, 0.13, 0.13, 0.13]), // 0 ... 15
  mm([0.05, 0.1, 0.1, 0.075, 0.075, 0.13, 0.25, 0.25]), // 16 ... 30
  mm([0.1, 0.64, 0.64, 0.3, 0.13, 0.13, 0.4, 0.8]), // 31 ... 50
  mm([0.1, 0.64, 1.5, 0.3, 0.13, 0.13, 0.5, 1]), // 51 ... 100
  mm([0.2, 0.64, 3.2, 0.8, 0.4, 0.4, 0.8, 1.6]), // 101 ... 150
  mm([0.2, 1.25, 3.2, 0.8, 0.4, 0.4, 0.8, 1.6]), // 151 ... 170
  mm([0.2, 1.25, 6.4, 0.8, 0.4, 0.4, 0.8, 1.6]), // 171 ... 250
  mm([0.2, 1.25, 12.5, 0.8, 0.4, 0.4, 0.8, 1.6]), // 251 ... 300
  mm([0.25, 2.5, 12.5, 1.6, 0.8, 0.8, 1.5, 3]), // 301 ... 500
  mm([0.0025, 0.005, 0.025, 0.00305, 0.00305, 0.00305, 0.00305, 0.0061]), // >500
];

export interface Ipc2221Table {
  /** What the voltage box shows afterwards (`txt.Printf( "%g", voltage )`). */
  voltageText: string;
  /** `IPC2221_CLASS_COUNT` rows of `IPC2221_VALUE_COUNT` cells, as the grid shows them. */
  cells: string[][];
}

/**
 * `PANEL_ELECTRICAL_SPACING_IPC2221::ElectricalSpacingUpdateData`: an empty voltage is 500 V, anything below 500 V is raised to 500 V (the box is
 * rewritten with it), a text that is not a number gives NaN ("nan" in the box and the last row, as KiCad prints it); each cell `%g` of the spacing
 * divided by the unit's scale.
 */
export function ipc2221Table(voltageText: string, unitScale: number): Ipc2221Table {
  let voltage = 500.0;
  if (voltageText !== "") voltage = doubleFromString(voltageText);
  if (voltage < 500.0) voltage = 500.0;
  const cells: string[][] = [];
  for (let ii = 0; ii < IPC2221_CLASS_COUNT - 1; ii++) cells.push(IPC2221_CLIST[ii]!.map((v) => formatG(v / unitScale)));
  const last: string[] = [];
  for (let jj = 0; jj < IPC2221_VALUE_COUNT; jj++) {
    let spacing = IPC2221_CLIST[IPC2221_CLASS_COUNT - 2]![jj]!;
    const spacingExtra = IPC2221_CLIST[IPC2221_CLASS_COUNT - 1]![jj]!;
    spacing += spacingExtra * (voltage - 500.0);
    last.push(formatG(spacing / unitScale));
  }
  cells.push(last);
  return { voltageText: formatG(voltage), cells };
}

// ---------------------------------------------------------------------------------------------------------------------------------------- IEC 60664

/** The choosers' strings (`m_OVCchoiceChoices`, `m_insulationTypeChoices`, `m_pollutionDegreeChoices`, `m_materialGroupChoices`). */
export const IEC_OVC_CHOICES = ["OVC I", "OVC II", "OVC III", "OVC IV"] as const;
export const IEC_INSULATION_CHOICES = ["Functional", "Basic", "Reinforced"] as const;
export const IEC_POLLUTION_CHOICES = ["PD1", "PD2", "PD3", "PD4"] as const;
export const IEC_MATERIAL_CHOICES = ["I", "II", "IIIa", "IIIb"] as const;

const ovcOf = (sel: number): OvCategory => (sel === 0 ? OV_CATEGORY.OV_I : sel === 1 ? OV_CATEGORY.OV_II : sel === 2 ? OV_CATEGORY.OV_III : OV_CATEGORY.OV_IV);
const insulOf = (sel: number): InsulationType => (sel === 0 ? INSULATION_TYPE.FUNCTIONAL : sel === 1 ? INSULATION_TYPE.BASIC : INSULATION_TYPE.REINFORCED);
const pdOf = (sel: number): PollutionDegree => (sel === 0 ? POLLUTION_DEGREE.PD1 : sel === 1 ? POLLUTION_DEGREE.PD2 : sel === 2 ? POLLUTION_DEGREE.PD3 : POLLUTION_DEGREE.PD4);
const mgOf = (sel: number): MaterialGroup => (sel === 0 ? MATERIAL_GROUP.MG_I : sel === 1 ? MATERIAL_GROUP.MG_II : sel === 2 ? MATERIAL_GROUP.MG_IIIa : MATERIAL_GROUP.MG_IIIb);

/**
 * `CalculateTransientImpulse`: the rated impulse withstand voltage in kV for the rated voltage typed and the overvoltage category chosen, or "Error"
 * out of the table. Null when the rated voltage does not read as a number (`ToDouble` fails): KiCad then leaves the box as it was.
 */
export function iecImpulseVoltageText(ratedVoltageText: string, ovcSel: number): string | null {
  const value = wxToDouble(ratedVoltageText);
  if (value === null) return null;
  const cal = new IEC60664();
  cal.SetOvervoltageCategory(ovcOf(ovcSel));
  cal.SetRatedVoltage(value);
  const result = cal.GetRatedImpulseWithstandVoltage();
  return result >= 0 ? formatG(result / 1000) : "Error";
}

export interface IecSpacingInput {
  rmsVoltage: string;
  transientOvervoltage: string;
  peakVoltage: string;
  altitude: string;
  insulationSel: number;
  pollutionSel: number;
  materialSel: number;
  pcbMaterial: boolean;
}

export interface IecSpacingResult {
  clearance: string;
  creepage: string;
  minGrooveWidth: string;
}

/**
 * `CalculateClearanceCreepage`: the clearance, creepage and minimum groove width in mm (`wxString << double`, i.e. %g). Creepage and groove are "N/A"
 * under pollution degree 4 (IEC 60664 §4.6.3: the surface may be covered with permanent conductive pollution); a value out of the tables is "Error",
 * and all three are "Error" when one of the four numbers does not read (`ToDouble`).
 */
export function iecSpacing(input: IecSpacingInput): IecSpacingResult {
  const altitude = wxToDouble(input.altitude);
  const transientV = wxToDouble(input.transientOvervoltage);
  const peakV = wxToDouble(input.peakVoltage);
  const RMSV = wxToDouble(input.rmsVoltage);
  if (altitude === null || transientV === null || peakV === null || RMSV === null) return { clearance: "Error", creepage: "Error", minGrooveWidth: "Error" };
  const pd = pdOf(input.pollutionSel);
  const cal = new IEC60664();
  cal.SetPollutionDegree(pd);
  cal.SetMaterialGroup(mgOf(input.materialSel));
  cal.SetInsulationType(insulOf(input.insulationSel));
  cal.SetAltitude(altitude);
  cal.SetTransientVoltage(transientV);
  cal.SetPeakVoltage(peakV);
  cal.SetRMSVoltage(RMSV);
  cal.SetPCBMaterial(input.pcbMaterial);
  cal.Compute();
  const groove = cal.GetMinGrooveWidth();
  const creepage = cal.GetCreepageDistance();
  const clearance = cal.GetClearanceDistance();
  const pdText = (v: number) => (v >= 0 && pd < POLLUTION_DEGREE.PD4 ? formatG(v) : pd === POLLUTION_DEGREE.PD4 ? "N/A" : "Error");
  return { minGrooveWidth: pdText(groove), creepage: pdText(creepage), clearance: clearance >= 0 ? formatG(clearance) : "Error" };
}

/** iec60664_help.md, verbatim (the IEC 60664 page's Help box). */
export const IEC60664_HELP =
  "The goal of the IEC60664-1 is to provide guidance on designing insulation for products that have a connection to mains supply.\n" +
  "\n" +
  "However some cases are not covered by this calculator:\n" +
  "\n" +
  "- For frequencies higher than 30kHz, the dielectric performances are degraded. IEC60664-4 covers those cases\n" +
  "- When using a conformal coating or a potting in order to protect for pollution, if all conditions specified by IEC60664-3 are met, the clearance and creepage distances can be reduced. Soldermask is usually not considered as a conformal coating.\n" +
  "- Insulations trough liquids, compressed air or gases other than air are not in the scope of IEC60664\n" +
  "- The dimensions for creepage distance cannot be specified where permanently conductive pollution is present (pollution degree 4)\n" +
  "\n" +
  "The OVC category can be determined as follows:\n" +
  "\n" +
  "- OVC I: Equipment connected to circuits in which measures are taken to limit transient overvoltages to an appropriately low level\n" +
  "- OVC II: Equipment connected to the mains through outlets or plugs, such as household appliances and portable tools\n" +
  "- OVC III: Equipment that is part of the fixed installation, such as switches, circuit breakers, distribution boards, ACs, and permanently installed industrial equipment\n" +
  "- OVC IV: Equipment installed at the origin of the electrical installation, such as electricity meters, primary overcurrent protection devices, or equipment connected directly to the grid\n" +
  "\n" +
  "The PD category can be determined as follows:\n" +
  "\n" +
  "- PD1: Sealed or in clean laboratory (non-conductive pollution).\n" +
  "- PD2: General commercial indoor (non-conductive pollution, but temporary conductivity caused by condensation may occasionally be expected)\n" +
  "- PD3: Industrial environments, or humid areas (conductive pollution, or non-conductive pollution becomes conductive due to expected condensation)\n" +
  "- PD4: Outdoors or exposed equipment (persistent conductive pollution)\n" +
  "\n" +
  "Material Insulation Groups (For reference only, verify the final calculations with your supplier):\n" +
  "\n" +
  "- I: PTFE, PE, PA, PP, PVC, ABS (CTI >= 600V)\n" +
  "- II: PBT, PS, Non-Brominated Epoxy (400V <= CTI < 600V)\n" +
  "- IIIa: Standard FR4, CEM-1, Brominated Epoxy (175V <= CTI < 400V)\n" +
  "- IIIb: PEEK, PI, PF (100V <= CTI < 175V)\n";
