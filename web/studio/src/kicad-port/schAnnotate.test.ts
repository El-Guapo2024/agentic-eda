import { test } from "node:test";
import assert from "node:assert/strict";
import type { Schematic } from "../api/types";
import { annotateCmd, annotateRequest, annotateSelectionOf, defaultAnnotateSettings, scopeOnOpen, startNumber } from "./schAnnotate";

const none = { symbols: [], sheets: [] };

test("the dialog starts as KiCad's settings do: the whole schematic, recursive, keep, by X, first free after 0", () => {
  const s = defaultAnnotateSettings();
  assert.deepEqual(s, { scope: "all", recursive: true, reset: false, regroupUnits: false, sort: "x", algo: "first_free", start: "0" });
  const req = annotateRequest(s, none);
  assert.deepEqual(req, { reset_existing: false, order: "x_then_y", options: { scope: "all", algo: "first_free", start_number: 0, recursive: true, regroup_units: false } });
});

test("the order: X position is left to right (x first), Y position top to bottom", () => {
  assert.equal(annotateRequest({ ...defaultAnnotateSettings(), sort: "x" }, none).order, "x_then_y");
  assert.equal(annotateRequest({ ...defaultAnnotateSettings(), sort: "y" }, none).order, "y_then_x");
});

test("regroup units counts only with a reset", () => {
  const s = { ...defaultAnnotateSettings(), regroupUnits: true };
  assert.equal(annotateRequest(s, none).options.regroup_units, false, "keeping the annotations keeps the units together");
  assert.equal(annotateRequest({ ...s, reset: true }, none).options.regroup_units, true);
});

test("the selection scope sends the selected symbols and sheets, the others send neither", () => {
  const selection = { symbols: ["R1", "U2"], sheets: ["sa"] };
  const sel = annotateRequest({ ...defaultAnnotateSettings(), scope: "selection" }, selection);
  assert.deepEqual(sel.ids, ["R1", "U2"]);
  assert.deepEqual(sel.options.sheets, ["sa"]);
  const whole = annotateRequest(defaultAnnotateSettings(), selection);
  assert.equal(whole.ids, undefined);
  assert.equal(whole.options.sheets, undefined);
  assert.deepEqual(annotateCmd({ ...defaultAnnotateSettings(), scope: "current_sheet" }, selection), { op: "annotate", reset_existing: false, order: "x_then_y", options: { scope: "current_sheet", algo: "first_free", start_number: 0, recursive: true, regroup_units: false } });
});

test("the start number is read as a whole number, never below zero", () => {
  assert.equal(startNumber("100"), 100);
  assert.equal(startNumber(" 7 "), 7);
  assert.equal(startNumber("abc"), 0);
  assert.equal(startNumber("9.6"), 10, "KiROUND( DoubleValueFromString )");
  assert.equal(startNumber("-5"), 0);
  assert.equal(startNumber(""), 0);
});

test("a remembered selection scope falls back to the whole schematic when nothing is selected", () => {
  assert.equal(scopeOnOpen("selection", none), "all");
  assert.equal(scopeOnOpen("selection", { symbols: ["R1"], sheets: [] }), "selection");
  assert.equal(scopeOnOpen("current_sheet", none), "current_sheet");
});

test("r2 D-A3: a selected group stands for every symbol below it (getInferredSymbols recurses into groups)", () => {
  const sch = {
    symbols: [{ id: "R1", unit: 1 }, { id: "C1", unit: 1 }, { id: "R9", unit: 1 }],
    sheets: [{ id: "sheet_a", pins: [] }],
    groups: [
      { id: "grp_inner", name: "", member_ids: ["C1"] },
      { id: "grp", name: "pair", member_ids: ["R1", "grp_inner", "w1"] },
    ],
  } as unknown as Schematic;
  const isSymbol = (id: string) => ["R1", "C1", "R9"].includes(id);
  assert.deepEqual(annotateSelectionOf(sch, ["grp"], isSymbol), { symbols: ["R1", "C1"], sheets: [] });
  assert.deepEqual(annotateSelectionOf(sch, ["R9", "sheet_a"], isSymbol), { symbols: ["R9"], sheets: ["sheet_a"] });
});
