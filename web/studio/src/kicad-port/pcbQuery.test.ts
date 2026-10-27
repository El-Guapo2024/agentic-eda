import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { collectAllBoardItems } from "./findByProperties";
import { compilePcbExpr, itemsMatching, pcbExprEnv } from "./pcbExpr";
import { pcbExprBoard } from "./pcbExprFixture";
import { pcbItemsOf } from "./pcbProperties";
import { pcbQueryRequest, type PcbQueryRequest, type PcbQueryResponse } from "./pcbQuery";

// The equivalence of the backend's expression language (`crates/drc/src/query.rs`, the active one) and the INACTIVE TypeScript copy (`pcbExpr.ts` over
// `libeval.ts`): the request this file builds for the fixture board, and what the TypeScript copy answers for each expression, are the golden file
// `crates/drc/tests/fixtures/pcb_query_golden.json`; `crates/drc/tests/pcb_query_golden.rs` runs the same request through the Rust side and must give the
// same answers. `UPDATE_PCB_QUERY_GOLDEN=1 npm run test:unit` writes the file again.

const GOLDEN = "../../crates/drc/tests/fixtures/pcb_query_golden.json";
const board = pcbExprBoard();

/** The TypeScript copy's answer to a request. */
function tsAnswer(req: PcbQueryRequest): PcbQueryResponse {
  const env = pcbExprEnv(board);
  const errors: string[] = [];
  const c = compilePcbExpr(req.expression, env, (m) => errors.push(m));
  return { ok: c.ok, errors, ids: c.ok ? itemsMatching(c, pcbItemsOf(board, req.items.map((i) => i.id)), env) : null };
}

/** The expressions: every one-item case of `pcbExpr.test.ts` and `findByProperties.test.ts` as a query over the whole board, and more. */
const EXPRESSIONS = [
  "",
  "A.Type == 'Via'",
  "A.Type == 'via'",
  "A.Type == 'Track'",
  "A.Type == 'Pad'",
  "A.Type == 'Footprint'",
  "A.Type == 'Graphic'",
  "A.Type == 'Text'",
  "A.Type == 'Zone'",
  "A.Type != 'Pad'",
  "A.Type == '*a*'",
  "A.Width == 0.2mm",
  "A.Width > 0.25mm",
  "A.Width < 10mil",
  "A.Width >= 200000",
  "A.Diameter == 0.6mm && A.Hole == 0.3mm",
  "A.Position_X < 20mm",
  "A.Position_X == 20mm && A.Position_Y == 50mm",
  "A.Orientation == -90deg",
  "A.Width + 0.1mm == 0.3mm",
  "A.width == 0.2mm",
  "A.Pad_Type == 'SMD'",
  "A.Pad_Type == 'Through-hole'",
  "A.Pad_Shape == 'Oval'",
  "A.Pad_Number == '3'",
  "A.Via_Type == 'Through'",
  "A.Via_Type == 'Blind'",
  "A.Via_Type == 'Buried'",
  "A.Via_Type != 'Through'",
  "A.Reference == 'U1'",
  "A.Reference == 'U*'",
  "A.Reference == 'R*'",
  "A.Value == 'MCU'",
  "A.Library_Link == 'Package_QFP:LQFP-48'",
  "A.Locked",
  "!A.Locked",
  "A.Locked == 1",
  "A.Locked == false",
  "A.Locked == true",
  "A.Layer == 'B.Cu'",
  "A.Layer == 'F.Cu'",
  "A.NetName == 'GND'",
  "A.NetName == 'G*'",
  "A.Net == 'GND'",
  "A.NetClass == 'HV'",
  "A.NetClass == 'Default'",
  "A.NetClass == 'HV,Default'",
  "A.NetClass != 'HV'",
  "A.hasNetclass('HV')",
  "A.hasNetclass('Default')",
  "A.hasExactNetclass('Default')",
  "A.hasExactNetclass('HV')",
  "A.hasNetclass('Fast')",
  "A.Net_Class == 'Default'",
  "A.inDiffPair('USB*')",
  "A.inDiffPair('USB')",
  "A.inDiffPair('SPI*')",
  "A.inDiffPair('*')",
  "A.hasNetclass('Fast') && A.inDiffPair('*')",
  "A.Layer == '*.Cu'",
  "A.Layer == 'B.*'",
  "A.Layer == 'F.Silkscreen'",
  "A.Layer == 'F.SilkS'",
  "A.Layer != 'F.SilkS'",
  "L == 'B.Cu'",
  "L == 'F.Cu'",
  "A.existsOnLayer('F.Cu')",
  "A.existsOnLayer('B.Cu')",
  "A.existsOnLayer('*.Cu')",
  "A.existsOnLayer('In1.Cu')",
  "A.existsOnLayer('In2.Cu')",
  "A.existsOnLayer('*.Mask')",
  "A.isPlated()",
  "A.isBlindVia()",
  "A.isBuriedVia()",
  "A.isBlindBuriedVia()",
  "A.isMicroVia()",
  "A.memberOfGroup('block')",
  "A.memberOf('bl*')",
  "A.memberOfFootprint('U1')",
  "A.memberOfFootprint('R*')",
  "A.memberOfFootprint('Package_QFP:*')",
  "A.getField('Supplier') == 'ACME'",
  "A.getField('Nope') == ''",
  "A.getField('Reference') == 'R*'",
  "A.Nope == 'x'",
  "A.Nope == 1",
  "A.Nope != 1",
  "C.Width > 1mm",
  "A.frobnicate('x')",
  "A.Width > 5",
  "A.memberOfGroup('')",
  "A.Width >",
  "A.Width ?? 1",
  "A.Width == ?",
  "A.Type ==",
  "A.Type == 'Via' && A.Diameter < 0.7mm",
  "A.Type == 'Via' && A.Diameter >= 0.8mm",
  "A.Type == 'Track' && A.NetName == 'GND'",
  "A.Type == 'Track' && A.Layer == 'B.Cu'",
  "A.Type == 'Pad' && A.Pad_Type == 'Through-hole'",
  "A.Type == 'Footprint' && A.Layer == 'B.Cu'",
  "A.Type == 'Track' && A.hasNetclass('Fast')",
  "A.Type == 'Track' && A.Type == 'Via' || A.Type == 'Pad'",
  "A.existsOnLayer('B.Cu') && A.Type == 'Track'",
  "A.Hole == 0.3mm && A.Via_Type == 'Through'",
  "A.Layer != 'F.Cu' && A.Locked == false && A.Net == 'GND' && A.Type == 'Track' && A.Width == 0.2mm",
  "A.Width == 7.87mil",
  "A.Type == 'Pad' && A.Library_Link == 'R_0603'",
  "A.intersectsCourtyard('U1')",
  "A.insideArea('*')",
  "A.enclosedByArea('*')",
  "A.fromTo('U1-1', 'R1-1')",
  "AB.isCoupledDiffPair()",
  "A.memberOfSheet('/')",
  "A.hasComponentClass('x')",
  "A.ComponentClass == ''",
  "A.Width > 0.1mm || A.Diameter > 0.5mm",
  "(A.Width > 0.1mm) && !(A.NetName == 'GND')",
  "1 + 2 == 3",
  "1mm == 1000000",
  "0.07mm == 70000",
  "'abc' == 'ABC'",
  "'abc' == 'a*'",
  "2 * 3 > 5 && 10 / 4 == 2.5",
  "A.Width == null",
  "A.Start_X < A.End_X",
];

test("the request for the fixture board: every item that is on a layer, with its facts", () => {
  const req = pcbQueryRequest("", board, collectAllBoardItems(board));
  assert.ok(req.items.length > 20, `${req.items.length} items`);
  const pad = req.items.find((i) => i.id === "U1.1");
  assert.ok(pad, "a pad");
  assert.equal(pad.footprint?.reference, "U1");
  assert.ok(pad.props.some((p) => p.name === "Library Link"), "a pad carries its footprint's properties");
  assert.deepEqual(req.env.classes["VCC"], ["HV,Default", ["HV", "Default"]]);
});

test("the backend's language and the TypeScript copy give the same answers (the golden file both are held to)", () => {
  const request = pcbQueryRequest("", board, collectAllBoardItems(board));
  const cases = EXPRESSIONS.map((expression) => ({ expression, ...tsAnswer({ ...request, expression }) }));
  const golden = { note: "Written by web/studio/src/kicad-port/pcbQuery.test.ts (UPDATE_PCB_QUERY_GOLDEN=1); read by crates/drc/tests/pcb_query_golden.rs.", request, cases };
  const text = JSON.stringify(golden, null, 1) + "\n";
  if (process.env.UPDATE_PCB_QUERY_GOLDEN) writeFileSync(GOLDEN, text);
  assert.equal(readFileSync(GOLDEN, "utf8"), text, "the golden file is the request and the TypeScript copy's answers of today");
});
