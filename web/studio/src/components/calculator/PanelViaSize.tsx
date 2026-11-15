// The Via Size panel (`PANEL_VIA_SIZE`, pcb_calculator/calculator_panels/panel_via_size.cpp and its _base, KiCad 25ec2335): the Parameters (finished
// hole diameter, plating thickness, via length, via pad diameter, clearance hole diameter, Z0, applied current, plating resistivity and substrate
// relative permittivity with their "..." lists, temperature rise, pulse rise time), the Results recalculated on every change (`OnViaCalculate` is
// bound to each box and unit chooser), the warning when the pad is not smaller than the clearance hole (`onUpdateViaCalcErrorText`), and Reset to
// Defaults. Every box and unit is a setting (`via_size.*`, kept as typed). The arithmetic is kicad-port/calc/viaSize.ts. KiCad's top-view bitmap
// of a via (`m_viaBitmap`) is not drawn.
import { printfG } from "../../kicad-port/calc/cformat";
import { entryValue, standardRelativeDielectricConstantList, standardResistivityList } from "../../kicad-port/calc/commonData";
import { calculateVia } from "../../kicad-port/calc/viaSize";
import type { PcbCalculatorSettings } from "../../kicad-port/calc/calcSettings";
import { UNIT_SELECTOR_LEN, UNIT_SELECTOR_RESISTOR, doubleFromString, unitScale } from "../../kicad-port/calc/unitSelectors";
import { updatePcbCalculatorSettings, usePcbCalculatorSettings } from "../../state/pcbCalculator";
import { CalcBox, CalcGrid, CalcInput, CalcPanel, UnitChoice } from "./calcKit";
import { useSingleChoice } from "./SingleChoiceDialog";

type Via = PcbCalculatorSettings["via_size"];
type TextKey = { [K in keyof Via]: Via[K] extends string ? K : never }[keyof Via];
type UnitKey = { [K in keyof Via]: Via[K] extends number ? K : never }[keyof Via];

/** `OnViaResetButtonClick` (each value through "%g", the units mm and Ω). */
const RESET: Via = {
  hole_diameter: printfG(0.4),
  hole_diameter_units: 0,
  thickness: printfG(0.035),
  thickness_units: 0,
  length: printfG(1.6),
  length_units: 0,
  pad_diameter: printfG(0.6),
  pad_diameter_units: 0,
  clearance_diameter: printfG(1.0),
  clearance_diameter_units: 0,
  characteristic_impedance: printfG(50.0),
  characteristic_impedance_units: 0,
  applied_current: printfG(1.0),
  plating_resistivity: printfG(1.72e-8),
  permittivity: printfG(4.5),
  temp_rise: printfG(10.0),
  pulse_rise_time: printfG(1.0),
};

const num = (t: string) => Math.abs(doubleFromString(t));

export function PanelViaSize() {
  const v = usePcbCalculatorSettings().via_size;
  const [dialog, ask] = useSingleChoice("via-list");
  const set = (patch: Partial<Via>) => updatePcbCalculatorSettings((s) => void Object.assign(s.via_size, patch));

  // `OnViaCalculate`.
  const len = (t: string, u: number) => num(t) * unitScale(UNIT_SELECTOR_LEN, u);
  const r = calculateVia({
    finishedHoleDia: len(v.hole_diameter, v.hole_diameter_units),
    platingThickness: len(v.thickness, v.thickness_units),
    viaLength: len(v.length, v.length_units),
    padDia: len(v.pad_diameter, v.pad_diameter_units),
    clearanceDia: len(v.clearance_diameter, v.clearance_diameter_units),
    charImpedance: num(v.characteristic_impedance) * unitScale(UNIT_SELECTOR_RESISTOR, v.characteristic_impedance_units),
    appliedCurrent: num(v.applied_current),
    platingResistivity: num(v.plating_resistivity),
    relativePermitivity: num(v.permittivity),
    temperatureDiff: num(v.temp_rise),
    pulseRiseTime: num(v.pulse_rise_time),
  });
  // `onUpdateViaCalcErrorText`.
  const warning = len(v.clearance_diameter, v.clearance_diameter_units) <= len(v.pad_diameter, v.pad_diameter_units);

  const text = (key: TextKey, label: string) => <CalcInput testid={`via-${key}`} label={label} value={v[key]} onChange={(t) => set({ [key]: t })} />;
  const lenUnit = (key: UnitKey, label: string) => <UnitChoice testid={`via-${key}`} label={label} def={UNIT_SELECTOR_LEN} value={v[key]} onChange={(i) => set({ [key]: i })} />;
  const tip = (label: string, title: string) => <span title={title}>{label}</span>;

  // `OnViaRho_Button` / `OnViaEpsilonR_Button`: `wxGetSingleChoice( .. ).BeforeFirst( ' ' )`, nothing preselected.
  const pick = async (caption: string, list: string[], key: TextKey) => {
    const i = await ask(caption, list, 0);
    if (i >= 0) {
      const value = entryValue(list[i]!);
      if (value !== "") set({ [key]: value });
    }
  };

  const result = (key: keyof typeof r, label: string, unit: string, unitTitle?: string, labelTitle?: string) => [
    <span title={labelTitle}>{label}</span>,
    <span data-testid={`via-result-${key}`}>{printfG(r[key])}</span>,
    <span title={unitTitle}>{unit}</span>,
  ];

  return (
    <CalcPanel testid="calc-panel-via_size" style={{ flexDirection: "row", alignItems: "flex-start", flexWrap: "wrap" }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <CalcBox title="Parameters">
          <CalcGrid
            rows={[
              ["Finished hole diameter (D):", text("hole_diameter", "Finished hole diameter"), lenUnit("hole_diameter_units", "Finished hole diameter unit")],
              ["Plating thickness (T):", text("thickness", "Plating thickness"), lenUnit("thickness_units", "Plating thickness unit")],
              [tip("Via length:", "Via length is the board thickness for through hole vias"), text("length", "Via length"), lenUnit("length_units", "Via length unit")],
              [tip("Via pad diameter:", "Diameter of pad surrounding via (annular ring)"), text("pad_diameter", "Via pad diameter"), lenUnit("pad_diameter_units", "Via pad diameter unit")],
              [tip("Clearance hole diameter:", "Diameter of clearance hole in ground plane(s)"), text("clearance_diameter", "Clearance hole diameter"), lenUnit("clearance_diameter_units", "Clearance hole diameter unit")],
              [
                tip("Z0:", "Characteristic impedance of conductor"),
                text("characteristic_impedance", "Z0"),
                <UnitChoice testid="via-characteristic_impedance_units" label="Z0 unit" def={UNIT_SELECTOR_RESISTOR} value={v.characteristic_impedance_units} onChange={(i) => set({ characteristic_impedance_units: i })} />,
              ],
              ["Applied current:", text("applied_current", "Applied current"), "A"],
              [
                tip("Plating resistivity:", "Specific resistance in ohms * meters"),
                <span style={{ display: "flex", gap: 2 }}>
                  {text("plating_resistivity", "Plating resistivity")}
                  <button type="button" data-testid="via-resistivity-list" onClick={() => void pick("Electrical Resistivity in Ohm*m", standardResistivityList(), "plating_resistivity")}>
                    ...
                  </button>
                </span>,
                "Ω⋅m",
              ],
              [
                tip("Substrate relative permittivity:", "Relative dielectric constant (epsilon r)"),
                <span style={{ display: "flex", gap: 2 }}>
                  {text("permittivity", "Substrate relative permittivity")}
                  <button type="button" data-testid="via-permittivity-list" onClick={() => void pick("Relative Dielectric Constants", standardRelativeDielectricConstantList(), "permittivity")}>
                    ...
                  </button>
                </span>,
                null,
              ],
              [tip("Temperature rise:", "Maximum acceptable rise in temperature"), text("temp_rise", "Temperature rise"), "°C"],
              [tip("Pulse rise time:", "Pulse rise time to calculate reactance"), text("pulse_rise_time", "Pulse rise time"), <span title="nanoseconds">ns</span>],
            ]}
          />
        </CalcBox>
        {warning && (
          <div data-testid="via-warning" style={{ whiteSpace: "pre-line" }}>
            {"Warning:\nVia pad diameter >= Clearance hole diameter.\nSome parameters cannot be calculated for a via inside a copper zone."}
          </div>
        )}
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <CalcBox title="Results">
          <CalcGrid
            rows={[
              result("viaResistance", "Resistance:", "Ω"),
              result("voltageDrop", "Voltage drop:", "V"),
              result("powerLoss", "Power loss:", "W"),
              result("thermalResistance", "Thermal resistance:", "°C/W", undefined, "Using thermal conductivity value 401 Watts/(meter-Kelvin)"),
              result("estimatedAmpacity", "Estimated ampacity:", "A", undefined, "Based on temperature rise"),
              result("capacitance", "Capacitance:", "pF", "pico-Farad"),
              result("timeDegradation", "Rise time degradation:", "ps", "picoseconds", "Rise time degradation for given Z0 and calculated capacitance"),
              result("inductance", "Inductance:", "nH", "nano-Henry"),
              result("reactance", "Reactance:", "Ω", undefined, "Inductive reactance for given rise time and calculated inductance"),
            ]}
          />
        </CalcBox>
        <div>
          <button type="button" data-testid="via-reset" onClick={() => set({ ...RESET })}>
            Reset to Defaults
          </button>
        </div>
      </div>
      {dialog}
    </CalcPanel>
  );
}
