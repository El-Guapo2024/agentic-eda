import { test } from "node:test";
import assert from "node:assert/strict";
import { sheetFileBaseName, sheetFileInWork } from "./sheetFileInWork";

test("a sheet above the root's folder is written under __up__, an absolute one under __root__ (eda_kicad::sheet_file_in_work)", () => {
  assert.equal(sheetFileInWork("amp.kicad_sch"), "amp.kicad_sch");
  assert.equal(sheetFileInWork("../shared/amp.kicad_sch"), "__up__/shared/amp.kicad_sch");
  assert.equal(sheetFileInWork("./sub/./b.kicad_sch"), "sub/b.kicad_sch");
  assert.equal(sheetFileInWork("/abs/x.kicad_sch"), "__root__/abs/x.kicad_sch");
  assert.equal(sheetFileBaseName("../shared/amp.kicad_sch"), "amp.kicad_sch");
});
