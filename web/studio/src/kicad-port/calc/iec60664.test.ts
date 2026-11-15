// kicad-port/calc/iec60664.ts and electricalSpacing.ts's IEC 60664 page against KiCad's own iec60664.cpp: iec60664.golden.json is the output of
// tools/iec60664-golden/iec60664_golden.cpp, which compiles pcb_calculator/calculator_panels/iec60664.cpp of KiCad 25ec2335 unchanged.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { IEC60664, type InsulationType, type MaterialGroup, type OvCategory, type PollutionDegree } from "./iec60664";
import { iecImpulseVoltageText, iecSpacing } from "./electricalSpacing";

const golden = JSON.parse(readFileSync("src/kicad-port/calc/iec60664.golden.json", "utf8")) as { impulse: number[][]; spacing: number[][] };

test("GetRatedImpulseWithstandVoltage matches KiCad over every category and the table's edges", () => {
  assert.ok(golden.impulse.length > 50);
  for (const [ovc, v, want] of golden.impulse) {
    const cal = new IEC60664();
    cal.SetOvervoltageCategory(ovc as OvCategory);
    cal.SetRatedVoltage(v!);
    assert.equal(cal.GetRatedImpulseWithstandVoltage(), want, `OVC ${ovc} at ${v} V`);
  }
});

test("Compute (clearance, creepage, groove) matches KiCad bit for bit over a sample of the input grid", () => {
  assert.ok(golden.spacing.length > 1000);
  for (const [ins, pd, mg, pcb, t, p, r, a, groove, creepage, clearance] of golden.spacing) {
    const cal = new IEC60664();
    cal.SetPollutionDegree(pd as PollutionDegree);
    cal.SetMaterialGroup(mg as MaterialGroup);
    cal.SetInsulationType(ins as InsulationType);
    cal.SetAltitude(a!);
    cal.SetTransientVoltage(t!);
    cal.SetPeakVoltage(p!);
    cal.SetRMSVoltage(r!);
    cal.SetPCBMaterial(pcb === 1);
    cal.Compute();
    const at = `ins ${ins} pd ${pd} mg ${mg} pcb ${pcb} t ${t} p ${p} rms ${r} alt ${a}`;
    assert.equal(cal.GetMinGrooveWidth(), groove, `groove, ${at}`);
    assert.equal(cal.GetCreepageDistance(), creepage, `creepage, ${at}`);
    assert.equal(cal.GetClearanceDistance(), clearance, `clearance, ${at}`);
  }
});

test("the panel with KiCad's default settings: 1.5 kV; clearance 0.15, creepage 0.56, groove 0.05 mm", () => {
  assert.equal(iecImpulseVoltageText("230", 0), "1.5");
  assert.equal(iecImpulseVoltageText("230", 3), "6");
  assert.equal(iecImpulseVoltageText("2000", 0), "Error");
  assert.equal(iecImpulseVoltageText("23,0", 0), null, "ToDouble fails: the box is left as it was");
  const base = { rmsVoltage: "230", transientOvervoltage: "1", peakVoltage: "0.5", altitude: "2000", insulationSel: 0, pollutionSel: 0, materialSel: 0, pcbMaterial: true };
  assert.deepEqual(iecSpacing(base), { clearance: "0.15", creepage: "0.56", minGrooveWidth: "0.05" });
  // Reinforced: transient 1 * 1.6 = 1.6 kV -> 0.5 (PD1 falls through to PD2's rows... 1.6 > 1.5 -> PD3 rows: 1.0), creepage 0.56 * 2.
  assert.deepEqual(iecSpacing({ ...base, insulationSel: 2 }), { clearance: "1", creepage: "1.12", minGrooveWidth: "0.333333" });
  assert.deepEqual(iecSpacing({ ...base, pollutionSel: 3 }), { clearance: "1.6", creepage: "N/A", minGrooveWidth: "N/A" });
  assert.deepEqual(iecSpacing({ ...base, altitude: "high" }), { clearance: "Error", creepage: "Error", minGrooveWidth: "Error" });
  assert.deepEqual(iecSpacing({ ...base, transientOvervoltage: "500" }), { clearance: "Error", creepage: "Error", minGrooveWidth: "Error" });
});
