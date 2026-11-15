// Calculator Tools > Power, current and isolation > Fusing Current (`PANEL_FUSING_CURRENT`, pcb_calculator/calculator_panels/panel_fusing_current.cpp
// and its _base at KiCad 25ec2335): ambient temperature, melting point (copper's 1084 °C, not editable), and the four values of which the radio
// button picks the one Calculate computes (kicad-port/calc/fusingCurrent.ts); the comment beside the button, and the help below. KiCad saves no
// settings for this panel.
import { useState, type ReactNode } from "react";
import { UNIT_SELECTOR_LEN, UNIT_SELECTOR_THICKNESS, unitScale } from "../../kicad-port/calc/unitSelectors";
import { FUSING_CURRENT_HELP, FUSING_DEFAULTS, calculateFusing, type FusingTarget } from "../../kicad-port/calc/fusingCurrent";
import { CalcBox, CalcHelp, CalcInput, CalcPanel, UnitChoice } from "./calcKit";

type Texts = Record<"ambient" | "melting" | "width" | "thickness" | "current" | "time", string>;

export function PanelFusingCurrent() {
  const [t, setT] = useState<Texts>({ ...FUSING_DEFAULTS });
  const [widthUnit, setWidthUnit] = useState(0);
  const [thicknessUnit, setThicknessUnit] = useState(0);
  // The first radio button of the group starts checked.
  const [target, setTarget] = useState<FusingTarget>("width");
  const [comment, setComment] = useState("");
  const edit = (k: keyof Texts) => (v: string) => setT((o) => ({ ...o, [k]: v }));
  const calculate = () => {
    const r = calculateFusing({ ...t, widthScale: unitScale(UNIT_SELECTOR_LEN, widthUnit), thicknessScale: unitScale(UNIT_SELECTOR_THICKNESS, thicknessUnit), target });
    setT((o) => ({ ...o, [target]: r.value }));
    setComment(r.comment);
  };
  const radio = (k: FusingTarget) => <input type="radio" name="calc-fusing-target" data-testid={`fusing-radio-${k}`} checked={target === k} onChange={() => setTarget(k)} />;
  const row = (r: ReactNode, label: string, input: ReactNode, unit: ReactNode) => [r, label, input, unit].map((c, i) => <div key={`${label}${i}`} className="calc-cell">{c}</div>);
  return (
    <CalcPanel testid="calc-panel-fusing_current">
      <div className="calc-grid" style={{ gridTemplateColumns: "auto auto auto auto", justifyContent: "start" }}>
        {row(null, "Ambient temperature:", <CalcInput testid="fusing-ambient" label="Ambient temperature" value={t.ambient} onChange={edit("ambient")} />, "°C")}
        {row(null, "Melting point:", <CalcInput testid="fusing-melting" label="Melting point" value={t.melting} onChange={() => {}} disabled />, "°C")}
        {row(radio("width"), "Track width:", <CalcInput testid="fusing-width" label="Track width" value={t.width} onChange={edit("width")} />, <UnitChoice testid="fusing-width-unit" label="Track width unit" def={UNIT_SELECTOR_LEN} value={widthUnit} onChange={setWidthUnit} />)}
        {row(
          radio("thickness"),
          "Track thickness:",
          <CalcInput testid="fusing-thickness" label="Track thickness" value={t.thickness} onChange={edit("thickness")} />,
          <UnitChoice testid="fusing-thickness-unit" label="Track thickness unit" def={UNIT_SELECTOR_THICKNESS} value={thicknessUnit} onChange={setThicknessUnit} />
        )}
        {row(radio("current"), "Current:", <CalcInput testid="fusing-current" label="Current" value={t.current} onChange={edit("current")} />, "A")}
        {row(radio("time"), "Time to fuse:", <CalcInput testid="fusing-time" label="Time to fuse" value={t.time} onChange={edit("time")} />, "s")}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <button type="button" data-testid="fusing-calculate" onClick={calculate}>
          Calculate
        </button>
        <span data-testid="fusing-comment">{comment}</span>
      </div>
      <CalcBox title="Help" style={{ flex: 1 }}>
        <CalcHelp markdown={FUSING_CURRENT_HELP} testid="fusing-help" />
      </CalcBox>
    </CalcPanel>
  );
}
