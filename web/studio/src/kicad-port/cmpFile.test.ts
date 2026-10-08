import { test } from "node:test";
import assert from "node:assert/strict";
import { parseCmpFile } from "./cmpFile";

const SAMPLE = `Cmp-Mod V01 Created by CvPcb (7.0.0) date = Sat 01 Oct 2026

BeginCmp
TimeStamp = /5f1e2a10;
Reference = R1;
ValeurCmp = 10k;
IdModule  = Resistor_SMD:R_0603_1608Metric;
EndCmp

BeginCmp
TimeStamp = /5f1e2a11;
Reference = C1;
ValeurCmp = 100n;
IdModule  = Capacitor_SMD:C_0402_1005Metric;
EndCmp

EndListe
`;

test("each block gives its reference and footprint", () => {
  assert.deepEqual(parseCmpFile(SAMPLE), [
    { reference: "R1", footprint: "Resistor_SMD:R_0603_1608Metric" },
    { reference: "C1", footprint: "Capacitor_SMD:C_0402_1005Metric" },
  ]);
});

test("a value is between the first '=' and the last ';' and trimmed", () => {
  const text = "BeginCmp\nReference =   U2  ;\nIdModule  = Lib:A=B;C ;\nEndCmp\n";
  assert.deepEqual(parseCmpFile(text), [{ reference: "U2", footprint: "Lib:A=B;C" }]);
});

test("a block without a reference is skipped and one without a footprint links an empty one; lines without ';' have no value", () => {
  const text = "BeginCmp\nIdModule = Lib:A;\nEndCmp\nBeginCmp\nReference = R9;\nEndCmp\nBeginCmp\nReference = R8\nEndCmp\n";
  assert.deepEqual(parseCmpFile(text), [{ reference: "R9", footprint: "" }]);
});

test("Windows line ends and an unterminated last block are read", () => {
  assert.deepEqual(parseCmpFile("BeginCmp\r\nReference = R1;\r\nIdModule = F:A;\r\nEndCmp\r\nBeginCmp\r\nReference = R2;\r\nIdModule = F:B;"), [
    { reference: "R1", footprint: "F:A" },
    { reference: "R2", footprint: "F:B" },
  ]);
});
