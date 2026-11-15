// Calculator Tools > General system design > Resistor Calculator (`PANEL_R_CALCULATOR`, pcb_calculator/calculator_panels/panel_r_calculator{,_base}.cpp
// at KiCad 25ec2335): "Inputs" (Required resistance, Exclude value 1 and 2, all in kΩ; the series E1 / E3 / E6 / E12 / E24, E6 at first), "Solutions"
// (Simple solution, 3R solution, 4R solution, each with its Approximation in %), Calculate, and the help text (r_calculator_help.md). The search is
// kicad-port/calc/resistorSubstitution.ts. The panel saves no settings (its LoadSettings / SaveSettings are empty), so what was typed lasts while the
// window is open.
import { useMemo, useState } from "react";
import { R_CALC_MAX_TARGET, R_CALC_MIN_TARGET, ResEquivCalc, S2R, S3R, S4R, solutionTexts } from "../../kicad-port/calc/resistorSubstitution";
import { E1, E12, E24, E3, E6 } from "../../kicad-port/calc/eseries";
import { R_CALCULATOR_HELP } from "../../kicad-port/calc/eseriesHelp";
import { doubleFromString } from "../../kicad-port/calc/unitSelectors";
import { formatFixed } from "../../kicad-port/sim/spiceValue";
import { CalcBox, CalcHelp, CalcInput, CalcPanel, CalcResult } from "./calcKit";

const SERIES: { label: string; value: number }[] = [
  { label: "E1", value: E1 },
  { label: "E3", value: E3 },
  { label: "E6", value: E6 },
  { label: "E12", value: E12 },
  { label: "E24", value: E24 },
];

/** `es.Printf( "%+.2f", error )`. */
const signed2 = (x: number) => {
  const t = formatFixed(x, 2);
  return t.startsWith("-") ? t : `+${t}`;
};

type Row = { formula: string; error: string };

export function PanelRCalculator() {
  const calc = useMemo(() => new ResEquivCalc(), []);
  const [required, setRequired] = useState("");
  const [ex1, setEx1] = useState("");
  const [ex2, setEx2] = useState("");
  const [series, setSeries] = useState(E6);
  const [rows, setRows] = useState<Row[]>([
    { formula: "", error: "" },
    { formula: "", error: "" },
    { formula: "", error: "" },
  ]);

  // `OnCalculateESeries`.
  const calculate = () => {
    const reqr = 1000 * doubleFromString(required);
    if (Number.isNaN(reqr) || reqr < R_CALC_MIN_TARGET || reqr > R_CALC_MAX_TARGET) {
      window.alert(`Incorrect required resistance value: ${required}`);
      return;
    }
    calc.setSeries(series);
    calc.newCalc(reqr);
    // The required value itself is excluded (a value found in the series is assumed to need a replacement), and the two values entered.
    calc.exclude(reqr);
    calc.exclude(1000 * doubleFromString(ex1));
    calc.exclude(1000 * doubleFromString(ex2));
    try {
      calc.calculate();
    } catch (e) {
      window.alert(`Internal error: ${e instanceof Error ? e.message : String(e)}`);
      return;
    }
    const r = calc.getResults();
    setRows([S2R, S3R, S4R].map((k) => solutionTexts(r[k] ?? null, reqr, signed2)));
  };

  const inputRow = (label: string, value: string, set: (v: string) => void, testid: string) => (
    <>
      <span>{label}</span>
      <CalcInput value={value} onChange={set} testid={testid} label={label} width={90} onEnter={calculate} />
      <span>kΩ</span>
    </>
  );
  const solRow = (label: string, row: Row, id: string) => (
    <>
      <span>{label}</span>
      <CalcResult value={row.formula} testid={`r-calc-sol-${id}`} label={label} width={230} />
      <span style={{ paddingLeft: 10 }}>Approximation:</span>
      <CalcResult value={row.error} testid={`r-calc-err-${id}`} label={`${label} approximation`} width={70} />
      <span>%</span>
    </>
  );

  return (
    <CalcPanel testid="calc-panel-r_calculator">
      <div style={{ display: "flex", gap: 10, alignItems: "stretch", flexWrap: "wrap" }}>
        <CalcBox title="Inputs">
          <div className="calc-grid" style={{ gridTemplateColumns: "auto auto auto" }}>
            {inputRow("Required resistance:", required, setRequired, "r-calc-required")}
            {inputRow("Exclude value 1:", ex1, setEx1, "r-calc-exclude1")}
            {inputRow("Exclude value 2:", ex2, setEx2, "r-calc-exclude2")}
          </div>
          <hr style={{ border: 0, borderTop: "1px solid var(--chrome-border)" }} />
          <div style={{ display: "flex", gap: 12 }}>
            {SERIES.map((s) => (
              <label key={s.label}>
                <input type="radio" name="calc-r-series" data-testid={`r-calc-series-${s.label}`} checked={series === s.value} onChange={() => setSeries(s.value)} /> {s.label}
              </label>
            ))}
          </div>
        </CalcBox>
        <CalcBox title="Solutions" style={{ flex: 1 }}>
          <div className="calc-grid" style={{ gridTemplateColumns: "auto auto auto auto auto" }}>
            {solRow("Simple solution:", rows[0]!, "2r")}
            {solRow("3R solution:", rows[1]!, "3r")}
            {solRow("4R solution:", rows[2]!, "4r")}
          </div>
          <hr style={{ border: 0, borderTop: "1px solid var(--chrome-border)" }} />
          <button type="button" data-testid="r-calc-calculate" onClick={calculate}>
            Calculate
          </button>
        </CalcBox>
      </div>
      <CalcBox title="Help" style={{ flex: 1 }}>
        <CalcHelp markdown={R_CALCULATOR_HELP} testid="r-calc-help" />
      </CalcBox>
    </CalcPanel>
  );
}
