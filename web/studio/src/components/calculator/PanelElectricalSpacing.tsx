// The Electrical Spacing calculator (`PANEL_ELECTRICAL_SPACING`, pcb_calculator/calculator_panels/panel_electrical_spacing*.cpp at KiCad 25ec2335): a
// notebook of two pages, "IPC 2221" (`PANEL_ELECTRICAL_SPACING_IPC2221`, the minimum spacing grid) and "IEC 60664" (`PANEL_ELECTRICAL_SPACING_IEC60664`,
// the impulse voltage, clearance, creepage and groove width). The arithmetic is kicad-port/calc/electricalSpacing.ts and iec60664.ts.
//
// Settings as each page's LoadSettings / SaveSettings: `electrical.spacing_units` and `electrical.spacing_voltage` (the text as typed) for IPC 2221;
// for IEC 60664 the five numbers (`DoubleFromString` of each box; LoadSettings writes them back with `wxString << double`, i.e. %g), the four choices
// and the PCB material box. KiCad stores them when the window closes; here each change is stored at once.
import { useState, type ReactNode } from "react";
import { getPcbCalculator, updatePcbCalculatorSettings } from "../../state/pcbCalculator";
import { useIconScheme } from "../../hooks/useColorScheme";
import { UNIT_SELECTOR_LEN, doubleFromString, unitScale } from "../../kicad-port/calc/unitSelectors";
import { printfG as formatG } from "../../kicad-port/calc/cformat";
import {
  IEC60664_HELP,
  IEC_INSULATION_CHOICES,
  IEC_MATERIAL_CHOICES,
  IEC_OVC_CHOICES,
  IEC_POLLUTION_CHOICES,
  IPC2221_COLUMNS,
  IPC2221_LEGEND,
  IPC2221_ROWS,
  iecImpulseVoltageText,
  iecSpacing,
  ipc2221Table,
  type IecSpacingInput,
} from "../../kicad-port/calc/electricalSpacing";
import { CalcBox, CalcHelp, CalcInput, CalcPanel, CalcResult, Choice, UnitChoice } from "./calcKit";

/** `PANEL_ELECTRICAL_SPACING_IPC2221`. */
function Ipc2221Page() {
  // LoadSettings: the unit and the voltage, then `ElectricalSpacingUpdateData`.
  const [init] = useState(() => {
    const e = getPcbCalculator().settings.electrical;
    return { units: e.spacing_units, table: ipc2221Table(e.spacing_voltage, unitScale(UNIT_SELECTOR_LEN, e.spacing_units)) };
  });
  const [units, setUnits] = useState(init.units);
  const [voltage, setVoltage] = useState(init.table.voltageText);
  const [cells, setCells] = useState(init.table.cells);
  const save = (u: number, v: string) =>
    updatePcbCalculatorSettings((s) => {
      s.electrical.spacing_units = u;
      s.electrical.spacing_voltage = v;
    });
  // `ElectricalSpacingUpdateData`: on Update Values and on a unit chosen; it rewrites the voltage box.
  const update = (u: number, v: string) => {
    const t = ipc2221Table(v, unitScale(UNIT_SELECTOR_LEN, u));
    setVoltage(t.voltageText);
    setCells(t.cells);
    save(u, t.voltageText);
  };
  return (
    <div style={{ display: "flex", gap: 20, alignItems: "flex-start" }} data-testid="calc-es-ipc2221">
      <div style={{ display: "flex", flexDirection: "column", gap: 6, width: 140, flex: "none" }}>
        <span>Unit:</span>
        <UnitChoice
          def={UNIT_SELECTOR_LEN}
          value={units}
          testid="calc-es-units"
          label="Unit"
          onChange={(u) => {
            setUnits(u);
            update(u, voltage);
          }}
        />
        <hr style={{ width: "100%", border: 0, borderTop: "1px solid var(--chrome-border)" }} />
        <span>Voltage &gt; 500 V:</span>
        <CalcInput
          value={voltage}
          testid="calc-es-voltage"
          label="Voltage > 500 V"
          width={130}
          onChange={(v) => {
            setVoltage(v);
            save(units, v);
          }}
        />
        <button type="button" data-testid="calc-es-update" onClick={() => update(units, voltage)}>
          Update Values
        </button>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
        <div style={{ textAlign: "center", fontStyle: "italic", fontWeight: 700 }}>Note: Values are minimal values (from IPC 2221)</div>
        <table className="calc-table" data-testid="calc-es-grid">
          <thead>
            <tr>
              <th />
              {IPC2221_COLUMNS.map((c) => (
                <th key={c} style={{ minWidth: 60 }}>
                  {c}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {cells.map((row, i) => (
              <tr key={i}>
                <th style={{ textAlign: "right", whiteSpace: "pre" }}>{IPC2221_ROWS[i]}</th>
                {row.map((c, j) => (
                  <td key={j} data-testid={`calc-es-cell-${i}-${j}`}>
                    {c}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
        <div style={{ whiteSpace: "pre-wrap" }}>{IPC2221_LEGEND}</div>
      </div>
    </div>
  );
}

const TIP_RATED = "Voltage of the mains supply";
const TIP_OVC =
  "OVC I: Equipment with no direct connection to mains supply\n\nOVC II: Energy-consuming equipment to be supplied from the fixed installation. (eg: appliances, portable tools, household loads). OVCIII applies if there are reliability and availability requirements\n\nOVC III :  Equipment in fixed installations with reliability and availability requirements. (eg: electrical switches, equipment for industrial use)\n\nOVC IV: Equipment at the origin of the installation (eg: electricity meters, primary overcurrent protection devices)";
const TIP_IMPULSE =
  "Given the rated voltage and the overvoltage category, a device should withstand this value without a breakdown of insulation. This impulse voltage is a standard 1.2/50µs wave";
const TIP_RMS = "Expected RMS voltage.";
const TIP_TRANSIENT =
  "Transient overvoltages due to:\n\n- Atmospheric disturbances transmitted by the mains supply (eg: a lightning strike)\n- Switching loads in the main supplys\n- External circuits\n- Internal generation\n\nEvents that last for a few milliseconds or less.";
const TIP_PEAK = "- Steady-state voltage value\n- Temporary overvoltage\n- Recurring peak voltage\n\nEvents of relatively long duration.";
const TIP_INSULATION =
  "Functional: insulation is necessary only for the functioning of the equipment\n\nBasic: Insulation of hazardous-live parts.\n\nReinforced: Single insulation that provides a degree of protection equivalent to a double insulation. ( which is two separate basic insulations, in case one of them fails  ).";
const TIP_PD =
  "PD1: No pollution or only dry, non-conductive pollution occurs\n\nPD2: Only non-conductive pollution occurs . Condensation may occur.\n\nPD3: Conductive pollution occurs, or non-conductive pollution occurs which becomes conductive due to expected condensation.\n\nPD4: Continuous conductivity occurs due to conductive dust, rain, ...";
const TIP_MG =
  "Materials with a high comparative tracking index (CTI) are better at providing isolation.\n\nMaterial group I: 600 <= CTI\nMaterial group II: 400 <= CTI < 600\nMaterial group IIIa: 175 <= CTI < 400\nMaterial group IIIb: 100 <= CTI < 175";
const TIP_PCB = "Printed wiring material can benefit of a creepage distance reduction for RMS voltages lower than 1000V";
const TIP_ALTITUDE =
  "Coating and potting allows for clearance and creepage distances reduction. Not supported by the calculator.\n\nA coating that could easily delaminate in the lifespan of the product (such as a soldermask) should not be considered for a reduction.";
const TIP_GROOVE = "A groove which width is smaller has no effect on the path considered for creepage";

interface IecState extends IecSpacingInput {
  ratedVoltage: string;
  ovcSel: number;
}

const GRID = { display: "grid", gridTemplateColumns: "auto auto auto", gap: "4px 6px", alignItems: "center", alignContent: "start" } as const;

function Row(props: { label: string; tip?: string; unit?: string; children: ReactNode }) {
  return (
    <>
      <span title={props.tip}>{props.label}</span>
      <span>{props.children}</span>
      <span>{props.unit ?? ""}</span>
    </>
  );
}

/** `PANEL_ELECTRICAL_SPACING_IEC60664`. */
function Iec60664Page() {
  const icons = useIconScheme();
  // LoadSettings, then CalculateTransientImpulse and CalculateClearanceCreepage.
  const [init] = useState(() => {
    const e = getPcbCalculator().settings.electrical;
    const st: IecState = {
      ratedVoltage: formatG(e.iec60664_ratedVoltage),
      rmsVoltage: formatG(e.iec60664_RMSvoltage),
      transientOvervoltage: formatG(e.iec60664_transientOV),
      peakVoltage: formatG(e.iec60664_peakOV),
      altitude: formatG(e.iec60664_altitude),
      ovcSel: e.iec60664_OVC,
      insulationSel: e.iec60664_insulationType,
      pollutionSel: e.iec60664_pollutionDegree,
      materialSel: e.iec60664_materialGroup,
      pcbMaterial: e.iec60664_pcbMaterial !== 0,
    };
    return { st, impulse: iecImpulseVoltageText(st.ratedVoltage, st.ovcSel) ?? "" };
  });
  const [st, setSt] = useState<IecState>(init.st);
  const [impulse, setImpulse] = useState(init.impulse);
  const result = iecSpacing(st);
  const change = (patch: Partial<IecState>) => {
    const next = { ...st, ...patch };
    setSt(next);
    // UpdateTransientImpulse on the rated voltage and the category; a rated voltage `ToDouble` cannot read leaves the box as it was.
    if ("ratedVoltage" in patch || "ovcSel" in patch) {
      const t = iecImpulseVoltageText(next.ratedVoltage, next.ovcSel);
      if (t !== null) setImpulse(t);
    }
    updatePcbCalculatorSettings((s) => {
      const e = s.electrical;
      e.iec60664_ratedVoltage = doubleFromString(next.ratedVoltage);
      e.iec60664_RMSvoltage = doubleFromString(next.rmsVoltage);
      e.iec60664_transientOV = doubleFromString(next.transientOvervoltage);
      e.iec60664_peakOV = doubleFromString(next.peakVoltage);
      e.iec60664_altitude = doubleFromString(next.altitude);
      e.iec60664_OVC = next.ovcSel;
      e.iec60664_insulationType = next.insulationSel;
      e.iec60664_pollutionDegree = next.pollutionSel;
      e.iec60664_materialGroup = next.materialSel;
      e.iec60664_pcbMaterial = next.pcbMaterial ? 1 : 0;
    });
  };
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }} data-testid="calc-es-iec60664">
      <div style={{ fontWeight: 700 }}>Insulation for equipment within low-voltage supply systems</div>
      <CalcBox title="Determine the transient impulse voltage to withstand">
        <div style={{ display: "flex", gap: 40, flexWrap: "wrap" }}>
          <div style={GRID}>
            <Row label="Rated Voltage (RMS or DC):" tip={TIP_RATED} unit="V">
              <CalcInput value={st.ratedVoltage} testid="calc-iec-rated" label="Rated Voltage (RMS or DC)" onChange={(v) => change({ ratedVoltage: v })} />
            </Row>
            <Row label="Overvoltage category:" tip={TIP_OVC}>
              <Choice options={IEC_OVC_CHOICES} value={st.ovcSel} testid="calc-iec-ovc" label="Overvoltage category" onChange={(i) => change({ ovcSel: i })} />
            </Row>
          </div>
          <div style={GRID}>
            <Row label="Impulse voltage:" tip={TIP_IMPULSE} unit="kV">
              <CalcResult value={impulse} testid="calc-iec-impulse" label="Impulse voltage" width={100} />
            </Row>
          </div>
        </div>
      </CalcBox>
      <CalcBox title="Compute the clearance and creepage distances">
        <div style={{ display: "flex", gap: 30, flexWrap: "wrap", alignItems: "flex-start" }}>
          <div style={GRID}>
            <Row label="RMS Voltage:" tip={TIP_RMS} unit="V">
              <CalcInput value={st.rmsVoltage} testid="calc-iec-rms" label="RMS Voltage" onChange={(v) => change({ rmsVoltage: v })} />
            </Row>
            <Row label="Transient overvoltage:" tip={TIP_TRANSIENT} unit="kV">
              <CalcInput value={st.transientOvervoltage} testid="calc-iec-transient" label="Transient overvoltage" onChange={(v) => change({ transientOvervoltage: v })} />
            </Row>
            <Row label="Recurring peak voltage:" tip={TIP_PEAK} unit="kV">
              <CalcInput value={st.peakVoltage} testid="calc-iec-peak" label="Recurring peak voltage" onChange={(v) => change({ peakVoltage: v })} />
            </Row>
            <Row label="Type of insulation:" tip={TIP_INSULATION}>
              <Choice options={IEC_INSULATION_CHOICES} value={st.insulationSel} testid="calc-iec-insulation" label="Type of insulation" onChange={(i) => change({ insulationSel: i })} />
            </Row>
            <Row label="Pollution Degree:" tip={TIP_PD}>
              <Choice options={IEC_POLLUTION_CHOICES} value={st.pollutionSel} testid="calc-iec-pollution" label="Pollution Degree" onChange={(i) => change({ pollutionSel: i })} />
            </Row>
            <Row label="Material group:" tip={TIP_MG}>
              <Choice options={IEC_MATERIAL_CHOICES} value={st.materialSel} testid="calc-iec-material" label="Material group" onChange={(i) => change({ materialSel: i })} />
            </Row>
            <Row label="PCB material:" tip={TIP_PCB}>
              <input type="checkbox" data-testid="calc-iec-pcb" aria-label="PCB material" checked={st.pcbMaterial} onChange={(e) => change({ pcbMaterial: e.target.checked })} />
            </Row>
            <Row label="Max altitude:" tip={TIP_ALTITUDE} unit="m">
              <CalcInput value={st.altitude} testid="calc-iec-altitude" label="Max altitude" onChange={(v) => change({ altitude: v })} />
            </Row>
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            <div style={GRID}>
              <Row label="Clearance:" unit="mm">
                <CalcResult value={result.clearance} testid="calc-iec-clearance" label="Clearance" width={100} />
              </Row>
              <Row label="Creepage:" unit="mm">
                <CalcResult value={result.creepage} testid="calc-iec-creepage" label="Creepage" width={100} />
              </Row>
              <Row label="Min groove width:" tip={TIP_GROOVE} unit="mm">
                <CalcResult value={result.minGrooveWidth} testid="calc-iec-groove" label="Min groove width" width={100} />
              </Row>
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <img src={`/icons/${icons}/creepage_clearance.png`} alt="" draggable={false} data-testid="calc-iec-bitmap" />
              <span style={{ whiteSpace: "pre" }}>{"solid: clearance\ndashed: creepage"}</span>
            </div>
          </div>
        </div>
      </CalcBox>
      <CalcBox title="Help">
        <CalcHelp markdown={IEC60664_HELP} testid="calc-iec-help" />
      </CalcBox>
    </div>
  );
}

/** The notebook (`m_notebook1`): "IPC 2221" first. Both pages stay mounted, as wxNotebook keeps its pages. */
export function PanelElectricalSpacing() {
  const [page, setPage] = useState(0);
  const tabs = ["IPC 2221", "IEC 60664"];
  return (
    <CalcPanel testid="calc-panel-electrical_spacing">
      <div role="tablist" style={{ display: "flex", gap: 2, borderBottom: "1px solid var(--chrome-border)" }}>
        {tabs.map((t, i) => (
          <button
            key={t}
            type="button"
            role="tab"
            aria-selected={page === i}
            data-testid={`calc-es-tab-${i}`}
            onClick={() => setPage(i)}
            style={{ borderBottomLeftRadius: 0, borderBottomRightRadius: 0, fontWeight: page === i ? 700 : 400, background: page === i ? "var(--chrome-active)" : undefined }}
          >
            {t}
          </button>
        ))}
      </div>
      <div hidden={page !== 0}>
        <Ipc2221Page />
      </div>
      <div hidden={page !== 1}>
        <Iec60664Page />
      </div>
    </CalcPanel>
  );
}
