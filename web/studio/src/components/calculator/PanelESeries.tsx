// Calculator Tools > Memo > E-Series (`PANEL_ESERIES_DISPLAY`, pcb_calculator/calculator_panels/panel_eseries_display{,_base}.cpp at KiCad 25ec2335):
// two tables of the preferred values, "E24,E48,E96" (four stripes of 24 rows, each E24 / E48 / E96 with a "-" gap between stripes) and
// "E1,E3,E6,E12", every cell spanning the rows of the finer series it stands for, the columns in KiCad's alternating colours (darker in a dark
// theme, text always black), and the help text below them (eseries_display_help.md, no box round it). No settings.
import { useMemo } from "react";
import { E12, E24, E96, eseriesValues } from "../../kicad-port/calc/eseries";
import { eseriesColumnColours, eseriesGrid112, eseriesGrid2496, type ESeriesCell } from "../../kicad-port/calc/colorCode";
import { ESERIES_DISPLAY_HELP } from "../../kicad-port/calc/eseriesHelp";
import { useColorScheme } from "../../hooks/useColorScheme";
import { CalcBox, CalcHelp, CalcPanel } from "./calcKit";

type SeriesName = "E1" | "E3" | "E6" | "E12" | "E24" | "E48" | "E96";

const cellStyle = (bg: string, span: number): React.CSSProperties => ({
  background: bg,
  color: "#000",
  height: span * 20 + (span - 1) * 2,
  lineHeight: `${span * 20 + (span - 1) * 2}px`,
  textAlign: "center",
  padding: "0 6px",
  boxSizing: "border-box",
});

/** One column of a grid: its title and its cells stacked, each as tall as the rows it spans (2 px grid lines between rows). */
function GridColumn(props: { title: string; cells: ESeriesCell[] | "gap"; series: SeriesName | null; dark: boolean; testid: string }) {
  const colours = props.series ? eseriesColumnColours(props.series, props.dark) : null;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 2, minWidth: props.cells === "gap" ? 14 : 40 }} data-testid={props.testid}>
      <div style={{ textAlign: "center", fontWeight: 600, background: "var(--chrome-bg)", padding: "1px 4px" }}>{props.title}</div>
      {props.cells === "gap" ? (
        <div style={{ flex: 1, background: "var(--chrome-bg)" }} />
      ) : (
        props.cells.map((c, i) => (
          <div key={i} data-eseries-value={c.text} style={cellStyle(colours![c.alternate ? 1 : 0], c.rowSpan)}>
            {c.text}
          </div>
        ))
      )}
    </div>
  );
}

export function PanelESeries() {
  // `wxSystemSettings::GetAppearance().IsDark()`: the system's light / dark mode, followed live (KiCad recolours on `ThemeChanged`).
  const dark = useColorScheme() === "dark";
  const g112 = useMemo(() => eseriesGrid112(eseriesValues(E12)), []);
  const g2496 = useMemo(() => eseriesGrid2496(eseriesValues(E24), eseriesValues(E96)), []);
  const names112: SeriesName[] = ["E1", "E3", "E6", "E12"];
  const names2496: (SeriesName | null)[] = ["E24", "E48", "E96", null];
  return (
    <CalcPanel testid="calc-panel-eseries">
      <div style={{ display: "flex", gap: 10, alignItems: "flex-start", flexWrap: "wrap" }}>
        <CalcBox title="E24,E48,E96" testid="eseries-2496">
          <div style={{ display: "flex", gap: 2 }}>
            {g2496.map((cells, i) => {
              const s = names2496[i % 4]!;
              return (
                <GridColumn key={i} title={s ?? "-"} cells={cells} series={s} dark={dark} testid={`eseries-2496-col-${i}`} />
              );
            })}
          </div>
        </CalcBox>
        <CalcBox title="E1,E3,E6,E12" testid="eseries-112">
          <div style={{ display: "flex", gap: 2 }}>
            {g112.map((cells, i) => (
              <GridColumn key={i} title={names112[i]!} cells={cells} series={names112[i]!} dark={dark} testid={`eseries-112-col-${i}`} />
            ))}
          </div>
        </CalcBox>
      </div>
      <CalcHelp markdown={ESERIES_DISPLAY_HELP} testid="eseries-help" style={{ minHeight: 100 }} />
    </CalcPanel>
  );
}
