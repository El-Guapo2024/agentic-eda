// The Galvanic Corrosion memo (`PANEL_GALVANIC_CORROSION`, pcb_calculator/calculator_panels/panel_galvanic_corrosion.cpp and its _base at KiCad 25ec2335):
// the square table of potential differences in mV (kicad-port/calc/galvanicCorrosion.ts), each cell coloured as `fillTable` colours it, its column
// labels written upwards (`SetColLabelTextOrientation( wxVERTICAL )`); the help text under it; "Threshold voltage:" (mV) and "Material names:"
// (Chemical symbols / Names) at the bottom. Settings: `corrosion_table.threshold_voltage` (`wxString << m_corFilterValue`, the number read from the box
// printed with %g) and `corrosion_table.show_symbols`.
import { useState } from "react";
import { getPcbCalculator, updatePcbCalculatorSettings } from "../../state/pcbCalculator";
import { doubleFromString } from "../../kicad-port/calc/unitSelectors";
import { printfG as formatG } from "../../kicad-port/calc/cformat";
import { CORROSION_ENTRIES, GALVANIC_CORROSION_HELP, corrosionLabel, corrosionTable, type Rgb } from "../../kicad-port/calc/galvanicCorrosion";
import { CalcHelp, CalcInput, CalcPanel } from "./calcKit";

const rgb = (c: Rgb) => `rgb(${c[0]}, ${c[1]}, ${c[2]})`;

export function PanelGalvanicCorrosion() {
  // LoadSettings: the threshold box gets the stored text; the radio buttons the stored nomenclature.
  const [init] = useState(() => getPcbCalculator().settings.corrosion_table);
  const [threshold, setThreshold] = useState(init.threshold_voltage);
  const [symbols, setSymbols] = useState(init.show_symbols);
  const value = doubleFromString(threshold);
  const table = corrosionTable(value);
  const save = (text: string, sym: boolean) =>
    updatePcbCalculatorSettings((s) => {
      s.corrosion_table.threshold_voltage = formatG(doubleFromString(text));
      s.corrosion_table.show_symbols = sym;
    });
  const labels = CORROSION_ENTRIES.map((e) => corrosionLabel(e, symbols));
  return (
    <CalcPanel testid="calc-panel-galvanic_corrosion" style={{ height: "100%" }}>
      <div style={{ flex: 1, minHeight: 200, overflow: "auto" }}>
        <table className="calc-table" data-testid="calc-gc-table" style={{ fontSize: 11 }}>
          <thead>
            <tr>
              <th />
              {labels.map((l, j) => (
                <th key={j} data-testid={`calc-gc-col-${j}`} style={{ writingMode: "vertical-rl", transform: "rotate(180deg)", textAlign: "left", padding: "4px 2px", whiteSpace: "nowrap" }}>
                  {l}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {table.map((row, i) => (
              <tr key={i}>
                <th data-testid={`calc-gc-row-${i}`} style={{ textAlign: "right", whiteSpace: "nowrap" }}>
                  {labels[i]}
                </th>
                {row.map((c, j) => (
                  <td key={j} data-testid={`calc-gc-cell-${i}-${j}`} style={{ background: rgb(c.bg), color: rgb(c.fg), padding: "1px 3px" }}>
                    {c.text}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <CalcHelp markdown={GALVANIC_CORROSION_HELP} testid="calc-gc-help" style={{ minHeight: 110, minWidth: 400 }} />
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <span>Threshold voltage:</span>
        <CalcInput
          value={threshold}
          testid="calc-gc-threshold"
          label="Threshold voltage"
          width={100}
          onChange={(t) => {
            setThreshold(t);
            save(t, symbols);
          }}
        />
        <span>mV</span>
        <span style={{ alignSelf: "stretch", borderLeft: "1px solid var(--chrome-border)", margin: "0 10px" }} />
        <span>Material names:</span>
        <label>
          <input
            type="radio"
            name="calc-gc-names"
            data-testid="calc-gc-symbols"
            checked={symbols}
            onChange={() => {
              setSymbols(true);
              save(threshold, true);
            }}
          />
          Chemical symbols
        </label>
        <label>
          <input
            type="radio"
            name="calc-gc-names"
            data-testid="calc-gc-names"
            checked={!symbols}
            onChange={() => {
              setSymbols(false);
              save(threshold, false);
            }}
          />
          Names
        </label>
      </div>
    </CalcPanel>
  );
}
