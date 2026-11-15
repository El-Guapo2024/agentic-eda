// Calculator Tools > Memo > Color Code (`PANEL_COLOR_CODE`, pcb_calculator/calculator_panels/panel_color_code{,_base}.cpp at KiCad 25ec2335): the
// "Tolerance" radio box (10% / 5% or <= 2%) and a chart of the resistor colour bands: 1st Band (colour names and digits), 2nd and 3rd Band (digits),
// 4th Band (only for <= 2 %), Multiplier and Tolerance. KiCad shows the chart as bitmaps; here the same colours and texts are drawn as boxes
// (kicad-port/calc/colorCode.ts). Settings: `color_code_tolerance`.
import { COLOR_BANDS, COLOR_CODE_TOLERANCES, showFourthBand, type ColorBand } from "../../kicad-port/calc/colorCode";
import { updatePcbCalculatorSettings, usePcbCalculatorSettings } from "../../state/pcbCalculator";
import { CalcPanel } from "./calcKit";

type Column = { title: string; id: string; text: (b: ColorBand) => string };

const COLUMNS: Column[] = [
  { title: "1st Band", id: "band1", text: (b) => (b.digit === null ? b.name : `${b.name} ${b.digit}`) },
  { title: "2nd Band", id: "band2", text: (b) => (b.digit === null ? "" : String(b.digit)) },
  { title: "3rd Band", id: "band3", text: (b) => (b.digit === null ? "" : String(b.digit)) },
  { title: "4th Band", id: "band4", text: (b) => (b.digit === null ? "" : String(b.digit)) },
  { title: "Multiplier", id: "multiplier", text: (b) => b.multiplier },
  { title: "Tolerance", id: "tolerance", text: (b) => b.tolerance },
];

function BandColumn({ col }: { col: Column }) {
  return (
    <div data-testid={`color-code-${col.id}`} style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 4 }}>
      <div>{col.title}</div>
      <div style={{ display: "flex", flexDirection: "column", border: "1px solid #000", width: col.id === "band1" ? 104 : 80 }}>
        {COLOR_BANDS.map((b) => {
          // color_code_value.svg leaves Gold and Silver out of the 2nd .. 4th band columns (no colour, no digit).
          const unused = col.id !== "band1" && col.id.startsWith("band") && b.digit === null;
          return (
            <div
              key={b.name}
              data-band={b.name}
              title={b.name}
              style={{
                height: 22,
                lineHeight: "22px",
                padding: "0 6px",
                fontSize: 13,
                background: unused ? "transparent" : b.fill,
                color: b.whiteText ? "#fff" : "#000",
                borderBottom: "1px solid #000",
                whiteSpace: "pre",
                textAlign: col.id === "band1" ? "left" : "center",
              }}
            >
              {col.text(b)}
            </div>
          );
        })}
      </div>
    </div>
  );
}

export function PanelColorCode() {
  const sel = usePcbCalculatorSettings().color_code_tolerance;
  const four = showFourthBand(sel);
  return (
    <CalcPanel testid="calc-panel-color_code">
      <div style={{ display: "flex", gap: 16, alignItems: "flex-start" }}>
        <fieldset className="calc-box" data-testid="color-code-tolerance">
          <legend>Tolerance</legend>
          {COLOR_CODE_TOLERANCES.map((t, i) => (
            <label key={t} style={{ display: "block" }}>
              <input
                type="radio"
                name="calc-color-code-tolerance"
                data-testid={`color-code-tolerance-${i}`}
                checked={sel === i}
                onChange={() => updatePcbCalculatorSettings((s) => void (s.color_code_tolerance = i))}
              />{" "}
              {t}
            </label>
          ))}
        </fieldset>
        <div style={{ display: "flex", gap: 8 }}>
          {COLUMNS.filter((c) => c.id !== "band4" || four).map((c) => (
            <BandColumn key={c.id} col={c} />
          ))}
        </div>
      </div>
    </CalcPanel>
  );
}
