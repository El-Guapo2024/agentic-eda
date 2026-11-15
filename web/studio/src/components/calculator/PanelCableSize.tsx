// Calculator Tools > Power, current and isolation > Cable Size (`PANEL_CABLE_SIZE`, pcb_calculator/calculator_panels/panel_cable_size.cpp and its
// _base at KiCad 25ec2335): "Wire properties" (standard AWG size, diameter, area, conductor resistivity and temperature coefficient with their
// "..." lists, linear resistance, frequency for 100% skin depth, ampacity, the current density slider) and "Application" (cable temperature,
// current, length, DC resistance, voltage drop, dissipated power). Every box is live: typing in one rewrites the others (kicad-port/calc/cableSize.ts,
// which holds the boxes as KiCad's panel does). The settings are KiCad's `cable_size.*`, read when the window opens and written at each change.
import { useReducer, useRef } from "react";
import { CableSizeModel, CABLE_SIZES, CURRENT_DENSITY_MAX, CURRENT_DENSITY_MIN, STANDARD_CABLE_CONDUCTOR_LIST, STANDARD_CABLE_TEMP_COEF_LIST, type CableField, type CableUnit } from "../../kicad-port/calc/cableSize";
import {
  UNIT_SELECTOR_FREQUENCY,
  UNIT_SELECTOR_LEN,
  UNIT_SELECTOR_LEN_CABLE,
  UNIT_SELECTOR_LINEAR_RESISTANCE,
  UNIT_SELECTOR_POWER,
  UNIT_SELECTOR_VOLTAGE,
  type UnitSelectorDef,
} from "../../kicad-port/calc/unitSelectors";
import { getPcbCalculator, updatePcbCalculatorSettings } from "../../state/pcbCalculator";
import { useSingleChoice } from "./SingleChoiceDialog";
import { CalcBox, CalcInput, CalcPanel, UnitChoice } from "./calcKit";

export function PanelCableSize() {
  const model = useRef<CableSizeModel | null>(null);
  if (!model.current) {
    model.current = new CableSizeModel();
    model.current.loadSettings(getPcbCalculator().settings.cable_size);
  }
  const m = model.current;
  const [, rerender] = useReducer((n: number) => n + 1, 0);
  const [choiceDialog, askChoice] = useSingleChoice("cable-choice");
  // `wxGetSingleChoice( wxEmptyString, caption, list )`: the entry chosen, or nothing on Cancel.
  const pick = (which: "resistivity" | "thermCoef") => {
    const list = which === "resistivity" ? STANDARD_CABLE_CONDUCTOR_LIST : STANDARD_CABLE_TEMP_COEF_LIST;
    void askChoice(which === "resistivity" ? "Electrical Resistivity in Ohm*m at 20 deg C" : "Temperature coefficient", list, 0).then((i) => {
      m.pickFromList(which, i >= 0 ? (list[i] ?? null) : null);
      changed();
    });
  };

  const changed = () => {
    updatePcbCalculatorSettings((s) => m.saveSettings(s.cable_size));
    rerender();
  };
  const box = (f: CableField, label: string) => (
    <CalcInput
      testid={`cable-${f}`}
      label={label}
      value={m.texts[f]}
      onChange={(v) => {
        m.setText(f, v);
        changed();
      }}
    />
  );
  const unit = (u: CableUnit, def: UnitSelectorDef, label: string) => (
    <UnitChoice
      testid={`cable-${u}-unit`}
      label={label}
      def={def}
      value={m.units[u]}
      onChange={(i) => {
        m.setUnit(u, i);
        changed();
      }}
    />
  );
  const rows = (cells: React.ReactNode[][]) =>
    cells.flatMap((r, i) => [0, 1, 2].map((j) => <div key={`${i}:${j}`} className="calc-cell">{r[j] ?? null}</div>));
  const grid = { gridTemplateColumns: "auto auto auto", justifyContent: "start" } as const;

  return (
    <CalcPanel testid="calc-panel-cable_size">
      <div style={{ display: "flex", gap: 10, flexWrap: "wrap", alignItems: "flex-start" }}>
        <CalcBox title="Wire properties">
          <div className="calc-grid" style={grid}>
            {rows([
              [
                "Standard Size:",
                <select
                  key="size"
                  className="calc-choice"
                  data-testid="cable-size"
                  aria-label="Standard Size"
                  value={m.sizeChoice}
                  onChange={(e) => {
                    m.selectSize(Number(e.target.value));
                    changed();
                  }}
                >
                  {m.sizeChoice < 0 ? <option value={-1}>{""}</option> : null}
                  {CABLE_SIZES.map((s, i) => (
                    <option key={s.name} value={i}>
                      {s.name}
                    </option>
                  ))}
                </select>,
              ],
              ["Diameter:", box("diameter", "Diameter"), unit("diameter", UNIT_SELECTOR_LEN, "Diameter unit")],
              ["Area:", box("area", "Area"), "mm²"],
              [
                <span key="l" title="Specific resistance in Ohm*m at 20 deg C">
                  Conductor resistivity:
                </span>,
                <span key="r" style={{ display: "flex", gap: 2 }} title={m.resistivityTip}>
                  {box("resistivity", "Conductor resistivity")}
                  <button type="button" data-testid="cable-resistivity-list" onClick={() => pick("resistivity")}>
                    ...
                  </button>
                </span>,
                "Ω⋅m",
              ],
              [
                <span key="l" title="Thermal coefficient at 20 deg C">
                  Temperature Coefficient:
                </span>,
                <span key="r" style={{ display: "flex", gap: 2 }}>
                  {box("thermCoef", "Temperature Coefficient")}
                  <button type="button" data-testid="cable-thermcoef-list" onClick={() => pick("thermCoef")}>
                    ...
                  </button>
                </span>,
              ],
              ["Linear resistance:", box("linResistance", "Linear resistance"), unit("linResistance", UNIT_SELECTOR_LINEAR_RESISTANCE, "Linear resistance unit")],
              ["Frequency for 100% skin depth:", box("frequency", "Frequency for 100% skin depth"), unit("frequency", UNIT_SELECTOR_FREQUENCY, "Frequency unit")],
              ["Ampacity:", box("ampacity", "Ampacity"), "A"],
              [
                "Current density",
                <span key="sl" style={{ display: "flex", alignItems: "center", gap: 4 }}>
                  <input
                    type="range"
                    data-testid="cable-current-density"
                    aria-label="Current density"
                    min={CURRENT_DENSITY_MIN}
                    max={CURRENT_DENSITY_MAX}
                    step={1}
                    value={m.currentDensity}
                    onChange={(e) => {
                      m.setCurrentDensity(Number(e.target.value));
                      changed();
                    }}
                  />
                  <span data-testid="cable-current-density-value">{m.currentDensity}</span>
                </span>,
                "A/mm²",
              ],
            ])}
          </div>
        </CalcBox>
        <CalcBox title="Application">
          <div className="calc-grid" style={grid}>
            {rows([
              [<span key="l" title="Off-Load max conductor temp. Reference: 20 deg C">Cable temperature:</span>, box("conductorTemp", "Cable temperature"), "°C"],
              ["Current:", box("current", "Current"), "A"],
              [<span key="l" title="Length includes the return path">Length:</span>, box("length", "Length"), unit("length", UNIT_SELECTOR_LEN_CABLE, "Length unit")],
              [<span key="l" title="DC Resistance of the conductor">Resistance DC:</span>, box("resistanceDc", "Resistance DC"), "Ω"],
              ["Voltage drop:", box("vDrop", "Voltage drop"), unit("vDrop", UNIT_SELECTOR_VOLTAGE, "Voltage drop unit")],
              ["Dissipated power:", box("power", "Dissipated power"), unit("power", UNIT_SELECTOR_POWER, "Dissipated power unit")],
            ])}
          </div>
        </CalcBox>
      </div>
      {choiceDialog}
    </CalcPanel>
  );
}
