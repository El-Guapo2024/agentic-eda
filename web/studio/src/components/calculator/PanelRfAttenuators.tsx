// The RF Attenuators panel (`PANEL_RF_ATTENUATORS`, pcb_calculator/calculator_panels/panel_rf_attenuators.cpp and its _base, KiCad 25ec2335): the
// "Attenuators" radio box (Pi, Tee, Bridged tee, Resistive splitter), the Parameters (Attenuation (a) in dB, Zin, Zout in Ω), Calculate, the Values
// R1..R3, the Messages pane and the Formula pane (the attenuator's *_formula.md). The calculators are kicad-port/calc/attenuators.ts.
//
// As in KiCad, the values typed reach the attenuator only on Calculate (`TransfPanelDataToAttenuator`), choosing another attenuator shows its own
// stored values and clears the results (`SetAttenuator`), and the settings keep each attenuator's attenuation, Zin and Zout (`ReadConfig` /
// `WriteConfig`) and the attenuator chosen (`attenuators.type`). KiCad's schematic bitmap of each attenuator (`m_attenuatorBitmap`) is not drawn:
// the formula pane names the resistors.
import { useRef, useState } from "react";
import { makeAttenuatorList, type Attenuator } from "../../kicad-port/calc/attenuators";
import { printfF, printfG } from "../../kicad-port/calc/cformat";
import { doubleFromString } from "../../kicad-port/calc/unitSelectors";
import type { PcbCalculatorSettings } from "../../kicad-port/calc/calcSettings";
import { getPcbCalculator, updatePcbCalculatorSettings } from "../../state/pcbCalculator";
import { CalcBox, CalcGrid, CalcHelp, CalcInput, CalcPanel, CalcResult } from "./calcKit";

const CHOICES = ["Pi", "Tee", "Bridged tee", "Resistive splitter"];

interface Fields {
  att: string;
  zin: string;
  zout: string;
  r1: string;
  r2: string;
  r3: string;
  /** `m_Attenuator_Messages`' HTML. */
  messages: string;
}

/** `TransfAttenuatorDataToPanel` (the parameter boxes) with `SetAttenuator`'s cleared results. */
function fieldsOf(a: Attenuator): Fields {
  return { att: printfG(a.attenuation), zin: a.zinEnable ? printfG(a.zin) : "", zout: printfG(a.zout), r1: "", r2: "", r3: "", messages: "" };
}

/** `ATTENUATOR::ReadConfig` for each attenuator. */
function readConfig(list: Attenuator[], s: PcbCalculatorSettings): void {
  for (const a of list) {
    const c = s.attenuators[a.name];
    a.attenuation = c.attenuation;
    a.zin = c.zin;
    a.zout = c.zout;
  }
}

export function PanelRfAttenuators() {
  const listRef = useRef<Attenuator[] | null>(null);
  if (!listRef.current) {
    listRef.current = makeAttenuatorList();
    readConfig(listRef.current, getPcbCalculator().settings);
  }
  const list = listRef.current;
  // `SetAttenuator`: an index past the list is the last attenuator.
  const clampIdx = (i: number) => Math.max(0, Math.min(list.length - 1, Number.isInteger(i) ? i : 0));
  const [idx, setIdx] = useState(() => clampIdx(getPcbCalculator().settings.attenuators.type));
  const [f, setF] = useState<Fields>(() => fieldsOf(list[idx]!));
  const cur = list[idx]!;

  const select = (i: number) => {
    const k = clampIdx(i);
    setIdx(k);
    setF(fieldsOf(list[k]!));
    updatePcbCalculatorSettings((s) => void (s.attenuators.type = k));
  };

  // `OnCalculateAttenuator`: TransfPanelDataToAttenuator, Calculate, TransfAttenuatorResultsToPanel.
  const calculate = () => {
    cur.attenuation = doubleFromString(f.att);
    cur.zin = doubleFromString(f.zin);
    cur.zout = doubleFromString(f.zout);
    cur.calculate();
    updatePcbCalculatorSettings((s) => {
      for (const a of list) s.attenuators[a.name] = { attenuation: a.attenuation, zin: a.zin, zout: a.zout };
    });
    if (cur.error) {
      const msg = `Attenuation more than ${printfF(cur.minimumAtt)} dB`;
      setF({ ...f, messages: `<br><b>Error!</b></br><br><em>${msg}</em></br>`, r1: "--", r2: "--", r3: cur.resultCount >= 3 ? "--" : f.r3 });
      return;
    }
    setF({ ...f, messages: "", r1: printfG(cur.r1), r2: printfG(cur.r2), r3: cur.resultCount < 3 ? "" : printfG(cur.r3) });
  };

  // Disable R3 for the bridged T attenuator only.
  const r3Enabled = idx !== 2;
  const ohm = "Ω";
  return (
    <CalcPanel testid="calc-panel-rf_attenuators" style={{ flexDirection: "row", alignItems: "flex-start", flexWrap: "wrap" }}>
      <CalcBox title="Attenuators" testid="att-selection">
        {CHOICES.map((c, i) => (
          <label key={c} style={{ display: "block", margin: "3px 0" }}>
            <input type="radio" name="calc-attenuator" data-testid={`att-type-${i}`} checked={i === idx} onChange={() => select(i)} /> {c}
          </label>
        ))}
      </CalcBox>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <CalcBox title="Parameters">
          <CalcGrid
            rows={[
              ["Attenuation (a):", <CalcInput testid="att-attenuation" label="Attenuation" value={f.att} disabled={!cur.attenuationEnable} onChange={(v) => setF({ ...f, att: v })} />, "dB"],
              ["Zin:", <CalcInput testid="att-zin" label="Zin" value={f.zin} disabled={!cur.zinEnable} onChange={(v) => setF({ ...f, zin: v })} />, ohm],
              ["Zout:", <CalcInput testid="att-zout" label="Zout" value={f.zout} onChange={(v) => setF({ ...f, zout: v })} />, ohm],
            ]}
          />
        </CalcBox>
        <div>
          <button type="button" data-testid="att-calculate" onClick={calculate}>
            Calculate ▾
          </button>
        </div>
        <CalcBox title="Values">
          <CalcGrid
            rows={[
              ["R1:", <CalcResult testid="att-r1" label="R1" value={f.r1} />, ohm],
              ["R2:", <CalcResult testid="att-r2" label="R2" value={f.r2} />, ohm],
              [
                <span style={{ opacity: r3Enabled ? 1 : 0.5 }}>R3:</span>,
                <CalcResult testid="att-r3" label="R3" value={r3Enabled ? f.r3 : ""} />,
                <span style={{ opacity: r3Enabled ? 1 : 0.5 }}>{ohm}</span>,
              ],
            ]}
          />
        </CalcBox>
        <div style={{ fontStyle: "italic", fontSize: 11 }}>Messages</div>
        <div className="calc-help" data-testid="att-messages" style={{ minHeight: 60, width: 260 }} dangerouslySetInnerHTML={{ __html: f.messages }} />
      </div>
      <CalcBox title="Formula" style={{ flex: 1, minWidth: 260 }}>
        <CalcHelp testid="att-formula" markdown={cur.formula ?? ""} />
      </CalcBox>
    </CalcPanel>
  );
}
