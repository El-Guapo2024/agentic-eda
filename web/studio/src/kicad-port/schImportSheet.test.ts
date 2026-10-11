import { test } from "node:test";
import assert from "node:assert/strict";
import type { SheetFileCheck } from "../api/hierarchyTypes";
import { defaultAnnotateSettings } from "./schAnnotate";
import { defaultImportOptions, describeCheck, fileOfPath, importContentsCmd, importNumbering, importSheetCmd, importToolHint, looksLikeSchematic, sheetFileOfFile, sheetNameOfFile, stemOfFile } from "./schImportSheet";

const check = (patch: Partial<SheetFileCheck> = {}): SheetFileCheck => ({ ok: true, path: "ldo.kicad_sch", stem: "ldo", sheets: [], symbols: 3, references: ["U1", "C1", "C2"], libraries: ["Device"], findings: [], blocked: false, dest_file: "ldo.kicad_sch", ...patch });

test("a sheet drawn from a file is named after the file and shows `<name>.kicad_sch` (DrawSheet: fn.GetName(), fn.GetName() + ext)", () => {
  assert.equal(sheetNameOfFile("/home/me/boards/power supply.kicad_sch"), "power supply");
  assert.equal(sheetFileOfFile("/home/me/boards/power supply.kicad_sch"), "power supply.kicad_sch");
  assert.equal(sheetNameOfFile("sub\\ldo.kicad_sch"), "ldo", "either separator");
  assert.equal(sheetFileOfFile("ldo.KICAD_SCH"), "ldo.kicad_sch", "the file is always written with the lowercase extension");
  assert.equal(stemOfFile("a.b.kicad_sch"), "a.b", "only the last extension goes");
  assert.equal(fileOfPath("  /x/y/z.kicad_sch "), "z.kicad_sch");
});

test("a path names a schematic when it ends in .kicad_sch", () => {
  assert.equal(looksLikeSchematic("ldo.kicad_sch"), true);
  assert.equal(looksLikeSchematic("  ldo.KICAD_SCH "), true);
  assert.equal(looksLikeSchematic("ldo.sch"), false);
  assert.equal(looksLikeSchematic(""), false);
});

test("the chooser starts with the sheet, annotating, one copy", () => {
  assert.deepEqual(defaultImportOptions(), { asSheet: true, keepAnnotations: false, repeated: false });
});

test("the numbering is the one the Annotate dialog last held (m_AnnotateSortOrder / m_AnnotateMethod / m_AnnotateStartNum)", () => {
  assert.deepEqual(importNumbering(defaultAnnotateSettings()), { order: "x_then_y", options: { algo: "first_free", start_number: 0 } });
  assert.deepEqual(importNumbering({ sort: "y", algo: "sheet_x100", start: "7" }), { order: "y_then_x", options: { algo: "sheet_x100", start_number: 7 } });
  assert.equal(importNumbering({ sort: "x", algo: "first_free", start: "-3" }).options.start_number, 0, "a negative start is no start");
});

test("the sheet import is one import_sheet verb with the file, the sheet, the numbering and the annotations choice", () => {
  const cmd = importSheetCmd({ path: " ldo.kicad_sch ", keepAnnotations: false }, { name: " LDO ", file: "ldo.kicad_sch", at: [10000, 20000], size: [30000, 8000] }, importNumbering(defaultAnnotateSettings()));
  assert.deepEqual(cmd, {
    op: "import_sheet",
    source: "ldo.kicad_sch",
    as_sheet: true,
    name: "LDO",
    file: "ldo.kicad_sch",
    at: { x: 10000, y: 20000 },
    size: [30000, 8000],
    keep_annotations: false,
    order: "x_then_y",
    options: { algo: "first_free", start_number: 0 },
  });
});

test("the contents import has no sheet: the file's items come to the click", () => {
  const cmd = importContentsCmd({ path: "ldo.kicad_sch", keepAnnotations: true }, [5000, 6000], importNumbering(defaultAnnotateSettings()));
  assert.equal(cmd.op, "import_sheet");
  assert.deepEqual({ ...cmd }, { op: "import_sheet", source: "ldo.kicad_sch", as_sheet: false, at: { x: 5000, y: 6000 }, keep_annotations: true, order: "x_then_y", options: { algo: "first_free", start_number: 0 } });
});

test("the chooser says what a file brings and keeps the errors apart from the warnings", () => {
  const plain = describeCheck(check());
  assert.equal(plain.summary, "3 symbols, 1 library (Device)");
  assert.deepEqual(plain.errors, []);
  const busy = describeCheck(check({ symbols: 1, sheets: ["Power", "IO"], libraries: ["Device", "Regulator_Linear"], findings: [{ severity: "error", text: "loop" }, { severity: "warning", text: "libs" }] }));
  assert.equal(busy.summary, "1 symbol, 2 sheets (Power, IO), 2 libraries (Device, Regulator_Linear)");
  assert.deepEqual(busy.errors, ["loop"]);
  assert.deepEqual(busy.warnings, ["libs"]);
});

test("the status line names the file and what a click does", () => {
  assert.match(importToolHint({ path: "a/ldo.kicad_sch", asSheet: true }), /ldo\.kicad_sch.*corner/);
  assert.match(importToolHint({ path: "a/ldo.kicad_sch", asSheet: false }), /where the items go/);
});
