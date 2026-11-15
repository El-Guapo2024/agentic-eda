// The Cable Size calculator (pcb_calculator/calculator_panels/panel_cable_size.{h,cpp} at KiCad 25ec2335, `PANEL_CABLE_SIZE`): a round wire's
// diameter, area, linear resistance, the frequency at which the skin depth is the radius, and its ampacity at a current density, with what it does
// in an application (DC resistance over a length, voltage drop and dissipated power at a current). Every box can be typed in: each change
// (`wxEVT_COMMAND_TEXT_UPDATED`) finds the wire radius it means and rewrites every other box (`updateAll` / `printAll`).
//
// The model is the panel itself, controls included: `CableSizeModel` holds the boxes' texts and the unit choosers' selections, and `setText` is
// wxTextCtrl::SetValue, which sends the text event to the box's handler unless `m_updatingUI` is set (what `printAll` sets while it writes). The
// handlers are KiCad's, statement for statement, so `LoadSettings`' chain of events (a resistivity setting of "" turning into "1.72e-8") plays out
// as in KiCad. Numbers are written `wxString( "" ) << double`, which is "%g".
//
// Pure: no React, no DOM. The panel is components/calculator/PanelCableSize.tsx.
import { printfG } from "./cformat";
import type { PcbCalculatorSettings } from "./calcSettings";
import {
  UNIT_SELECTOR_FREQUENCY,
  UNIT_SELECTOR_LEN,
  UNIT_SELECTOR_LEN_CABLE,
  UNIT_SELECTOR_LINEAR_RESISTANCE,
  UNIT_SELECTOR_POWER,
  UNIT_SELECTOR_VOLTAGE,
  doubleFromString,
  toDouble,
  unitScale,
} from "./unitSelectors";

const VACCUM_PERMEABILITY = 1.256637e-6;
const RELATIVE_PERMEABILITY = 1;
/** The default current density in ampere by mm2 (3A/mm2 is common to make transformers). */
const AMP_DENSITY_BY_MM2 = 3.0;
const M2_to_MM2 = 1e6;
/** `m_slCurrentDensity`: wxSlider( value 3, min 3, max 12 ). */
export const CURRENT_DENSITY_MIN = 3;
export const CURRENT_DENSITY_MAX = 12;

/** `buildCableList`: the AWG sizes and their radius in metres. */
export const CABLE_SIZES: readonly { name: string; radius: number }[] = [
  ["AWG0000", 0.005842],
  ["AWG000", 0.00520192],
  ["AWG00", 0.00463296],
  ["AWG0", 0.00412623],
  ["AWG1", 0.00367411],
  ["AWG2", 0.00327152],
  ["AWG3", 0.00291338],
  ["AWG4", 0.00259461],
  ["AWG5", 0.00231013],
  ["AWG6", 0.0020574],
  ["AWG7", 0.00183261],
  ["AWG8", 0.00163195],
  ["AWG9", 0.00145288],
  ["AWG10", 0.00129413],
  ["AWG11", 0.00115189],
  ["AWG12", 0.00102616],
  ["AWG13", 0.0009144],
  ["AWG14", 0.00081407],
  ["AWG15", 0.00072517],
  ["AWG16", 0.00064516],
  ["AWG17", 0.00057531],
  ["AWG18", 0.00051181],
  ["AWG19", 0.00045593],
  ["AWG20", 0.0004046],
  ["AWG21", 0.00036195],
  ["AWG22", 0.00032258],
  ["AWG23", 0.00028702],
  ["AWG24", 0.00025527],
  ["AWG25", 0.00022773],
  ["AWG26", 0.00020193],
  ["AWG27", 0.00018034],
  ["AWG28", 0.00016002],
  ["AWG29", 0.00014351],
  ["AWG30", 0.000127],
].map(([name, radius]) => ({ name: name as string, radius: radius as number }));

/** common_data.cpp `StandardCableConductorList` (the "..." beside the resistivity; lined up with the coefficients). */
export const STANDARD_CABLE_CONDUCTOR_LIST: readonly string[] = ["1.72e-8 \tCu, Copper", "2.62e-8 \tAl, Aluminum", "100e-8 \tNiCr, Nichrome", "9.71e-8 \tFe, Iron", "5.6e-8 \tW, Tungsten"];
/** common_data.cpp `StandardCableTempCoefList`. */
export const STANDARD_CABLE_TEMP_COEF_LIST: readonly string[] = ["3.93e-3 \tCu, Copper", "4.29e-3 \tAl, Aluminum", "0.4e-3 \tNiCr, Nichrome", "5e-3 \tFe, Iron", "4.5e-3 \tW, Tungsten"];

/** The editable boxes. */
export type CableField =
  | "diameter"
  | "area"
  | "resistivity"
  | "thermCoef"
  | "linResistance"
  | "frequency"
  | "ampacity"
  | "conductorTemp"
  | "current"
  | "length"
  | "resistanceDc"
  | "vDrop"
  | "power";

/** The unit choosers. */
export type CableUnit = "diameter" | "linResistance" | "frequency" | "length" | "vDrop" | "power";

const UNIT_DEFS = {
  diameter: UNIT_SELECTOR_LEN,
  linResistance: UNIT_SELECTOR_LINEAR_RESISTANCE,
  frequency: UNIT_SELECTOR_FREQUENCY,
  length: UNIT_SELECTOR_LEN_CABLE,
  vDrop: UNIT_SELECTOR_VOLTAGE,
  power: UNIT_SELECTOR_POWER,
} as const;

export class CableSizeModel {
  texts: Record<CableField, string> = {
    diameter: "",
    area: "",
    resistivity: "",
    thermCoef: "",
    linResistance: "",
    frequency: "",
    ampacity: "",
    conductorTemp: "",
    current: "",
    length: "",
    resistanceDc: "",
    vDrop: "",
    power: "",
  };
  units: Record<CableUnit, number> = { diameter: 0, linResistance: 0, frequency: 0, length: 0, vDrop: 0, power: 0 };
  /** `m_sizeChoice`'s selection, -1 for none (the base sets 0, AWG0000, though the wire starts at 1 mm). */
  sizeChoice = 0;
  /** `m_slCurrentDensity`'s value. */
  currentDensity = 3;
  /** The resistivity box's tooltip (`printAll`). */
  resistivityTip = "";

  private updatingUI = false;
  private updating: Partial<Record<CableField, boolean>> = {};

  // Normalized values (m, ohm, Hz, A, V, W).
  diameter = 0.001;
  area = 0;
  conductorTemperature = 20;
  current = 1.0;
  length = 1.0;
  conductorMaterialResitivity = 1.72e-8;
  conductorMaterialResitivityRef = 1.72e-8;
  conductorMaterialThermalCoef = 3.93e-3;
  ampByMm2 = AMP_DENSITY_BY_MM2;
  linearResistance = 0;
  maxFrequency = 0;
  ampacity = 0;
  resistanceDc = 0;
  voltageDrop = 0;
  dissipatedPower = 0;

  /** The constructor: `updateAll( m_diameter / 2 )`. */
  constructor() {
    this.updateAll(this.diameter / 2);
  }

  private scale(u: CableUnit): number {
    return unitScale(UNIT_DEFS[u], this.units[u]);
  }

  private m2ByAmpere(): number {
    return 1 / this.ampByMm2 / M2_to_MM2;
  }

  /** wxTextCtrl::SetValue: the text, then its text event (unless `printAll` is writing). The user typing goes through here too. */
  setText(field: CableField, text: string): void {
    this.texts[field] = text;
    if (!this.updatingUI) this.onText(field);
  }

  /** wxTextCtrl::ChangeValue: the text without the event. */
  changeText(field: CableField, text: string): void {
    this.texts[field] = text;
  }

  /** A unit chooser changed (`OnUpdateUnit`); SetSelection from the settings sends no event (`setUnit( .., false )`). */
  setUnit(u: CableUnit, index: number, event = true): void {
    this.units[u] = index;
    if (event) this.printAll();
  }

  /** `OnCableSizeChange`. */
  selectSize(index: number): void {
    this.sizeChoice = index;
    if (!this.updatingUI) {
      const e = CABLE_SIZES[index];
      if (e) this.updateAll(e.radius);
    }
  }

  /** `onUpdateCurrentDensity`. */
  setCurrentDensity(v: number): void {
    this.currentDensity = v;
    this.ampByMm2 = v;
    this.updateAll(this.diameter / 2);
  }

  /** `OnConductorResistivity_Button` / `OnConductorThermCoefChange_Button` after `wxGetSingleChoice`: the value before the first blank (nothing on Cancel). */
  pickFromList(field: "resistivity" | "thermCoef", choice: string | null): void {
    const value = (choice ?? "").split(" ")[0] ?? "";
    if (value !== "") this.changeText(field, value);
    this.onText(field);
  }

  private parsed(field: CableField, fn: (v: number) => void): void {
    if (this.updatingUI) return;
    this.updating[field] = true;
    const v = toDouble(this.texts[field]);
    if (v !== null) fn(v);
    this.updating[field] = false;
  }

  private onText(field: CableField): void {
    if (this.updatingUI) return;
    const PI = Math.PI;
    switch (field) {
      case "diameter":
        return this.parsed(field, (v) => {
          this.updateAll((v / 2) * this.scale("diameter"));
          this.sizeChoice = -1;
        });
      case "linResistance":
        return this.parsed(field, (v) => {
          this.updateAll(Math.sqrt(this.conductorMaterialResitivity / (v * this.scale("linResistance")) / PI));
          this.sizeChoice = -1;
        });
      case "area":
        return this.parsed(field, (v) => {
          this.updateAll(Math.sqrt(v / PI / M2_to_MM2));
          this.sizeChoice = -1;
        });
      case "frequency":
        return this.parsed(field, (v) => {
          this.updateAll(Math.sqrt(this.conductorMaterialResitivity / v / this.scale("frequency") / PI / VACCUM_PERMEABILITY / RELATIVE_PERMEABILITY));
          this.sizeChoice = -1;
        });
      case "ampacity":
        return this.parsed(field, (v) => {
          this.updateAll(Math.sqrt((v * this.m2ByAmpere()) / PI));
          this.sizeChoice = -1;
        });
      case "resistivity":
        // `OnConductorResistivityChange`: DoubleFromString, so "abc" is NaN and goes on.
        this.updating.resistivity = true;
        this.conductorMaterialResitivityRef = Math.abs(doubleFromString(this.texts.resistivity));
        this.updateAll(this.diameter / 2);
        this.updating.resistivity = false;
        return;
      case "thermCoef":
        this.conductorMaterialThermalCoef = Math.abs(doubleFromString(this.texts.thermCoef));
        this.updateAll(this.diameter / 2);
        return;
      case "conductorTemp": {
        // `OnConductorTempChange` sets no updating flag.
        const v = toDouble(this.texts.conductorTemp);
        if (v !== null) {
          this.conductorTemperature = v;
          this.updateAll(this.diameter / 2);
        }
        return;
      }
      case "current":
        return this.parsed(field, (v) => {
          this.current = v;
          this.updateApplication();
        });
      case "length":
        return this.parsed(field, (v) => {
          this.length = v * this.scale("length");
          this.updateApplication();
        });
      case "resistanceDc":
        return this.parsed(field, (v) => {
          this.updateAll(Math.sqrt(((this.conductorMaterialResitivity / v) * this.length) / PI));
          this.sizeChoice = -1;
        });
      case "vDrop":
        return this.parsed(field, (v) => {
          this.updateAll(Math.sqrt(((((this.conductorMaterialResitivity / v) * this.scale("vDrop")) * this.length) * this.current) / PI));
          this.sizeChoice = -1;
        });
      case "power":
        return this.parsed(field, (v) => {
          this.updateAll(Math.sqrt((((((this.conductorMaterialResitivity / v) * this.scale("power")) * this.length) * this.current) * this.current) / PI));
          this.sizeChoice = -1;
        });
    }
  }

  /** `printAll`: every box but the one being typed in. The temperature and coefficient boxes are never rewritten. */
  printAll(): void {
    this.updatingUI = true;
    const g = printfG;
    const u = this.updating;
    if (!u.diameter) this.texts.diameter = g(this.diameter / this.scale("diameter"));
    if (!u.area) this.texts.area = g(this.area * M2_to_MM2);
    if (!u.ampacity) this.texts.ampacity = g(this.ampacity);
    if (!u.frequency) this.texts.frequency = g(this.maxFrequency / this.scale("frequency"));
    if (!u.linResistance) this.texts.linResistance = g(this.linearResistance / this.scale("linResistance"));
    if (!u.length) this.texts.length = g(this.length / this.scale("length"));
    if (!u.resistivity) {
      this.texts.resistivity = g(this.conductorMaterialResitivityRef);
      this.resistivityTip = `Resistivity for ${g(this.conductorTemperature)} deg C is${g(this.conductorMaterialResitivity)} Ohm*m`;
    }
    if (!u.current) this.texts.current = g(this.current);
    if (!u.resistanceDc) this.texts.resistanceDc = g(this.resistanceDc);
    if (!u.vDrop) this.texts.vDrop = g(this.voltageDrop * this.scale("vDrop"));
    if (!u.power) this.texts.power = g(this.dissipatedPower * this.scale("power"));
    this.updatingUI = false;
  }

  /** `updateAll`. */
  updateAll(aRadius: number): void {
    this.diameter = aRadius * 2;
    this.area = Math.PI * aRadius * aRadius;
    this.conductorMaterialResitivity = this.conductorMaterialResitivityRef * (1 + this.conductorMaterialThermalCoef * (this.conductorTemperature - 20));
    this.linearResistance = this.conductorMaterialResitivity / this.area;
    // max frequency is when skin depth = radius
    this.maxFrequency = this.conductorMaterialResitivity / (Math.PI * aRadius * aRadius * VACCUM_PERMEABILITY * RELATIVE_PERMEABILITY);
    this.ampacity = this.area / this.m2ByAmpere();
    this.resistanceDc = this.linearResistance * this.length;
    this.voltageDrop = this.resistanceDc * this.current;
    this.dissipatedPower = this.voltageDrop * this.current;
    this.printAll();
  }

  /** `updateApplication`. */
  updateApplication(): void {
    this.resistanceDc = this.linearResistance * this.length;
    this.voltageDrop = this.resistanceDc * this.current;
    this.dissipatedPower = this.voltageDrop * this.current;
    this.printAll();
  }

  /** `LoadSettings`, with the text events its SetValue calls send. */
  loadSettings(cfg: PcbCalculatorSettings["cable_size"]): void {
    this.setUnit("diameter", cfg.diameterUnit, false);
    this.setUnit("linResistance", cfg.linResUnit, false);
    this.setUnit("frequency", cfg.frequencyUnit, false);
    this.setUnit("length", cfg.lengthUnit, false);
    this.setText("resistivity", cfg.conductorMaterialResitivity);
    this.setText("conductorTemp", cfg.conductorTemperature);
    this.setText("thermCoef", cfg.conductorThermalCoef);
    let choice = cfg.currentDensityChoice;
    if (choice < CURRENT_DENSITY_MIN || choice > CURRENT_DENSITY_MAX) choice = AMP_DENSITY_BY_MM2;
    this.currentDensity = choice;
    this.ampByMm2 = choice;
    const value = printfG(this.conductorMaterialResitivity);
    if (this.texts.resistivity === "" || value === "nan") {
      this.setText("resistivity", "1.72e-8");
      this.setText("conductorTemp", "20");
    }
    if (this.texts.thermCoef === "") this.setText("thermCoef", "3.93e-3");
    this.updateAll(this.diameter / 2);
  }

  /** `SaveSettings`. */
  saveSettings(cfg: PcbCalculatorSettings["cable_size"]): void {
    cfg.diameterUnit = this.units.diameter;
    cfg.linResUnit = this.units.linResistance;
    cfg.frequencyUnit = this.units.frequency;
    cfg.lengthUnit = this.units.length;
    cfg.conductorMaterialResitivity = printfG(this.conductorMaterialResitivityRef);
    cfg.conductorTemperature = this.texts.conductorTemp;
    cfg.conductorThermalCoef = this.texts.thermCoef;
    cfg.currentDensityChoice = this.currentDensity;
  }
}
