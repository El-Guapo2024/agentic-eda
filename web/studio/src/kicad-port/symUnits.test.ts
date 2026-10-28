import { test } from "node:test";
import assert from "node:assert/strict";
import { symbolPropertiesQuestions } from "./symUnits";

const sym = (unit_count: number, has_alternate_body_style: boolean) => ({ unit_count, has_alternate_body_style });

test("fewer units asks Validate's question; more or the same asks nothing (r7 E7-S5)", () => {
  assert.deepEqual(symbolPropertiesQuestions(sym(3, false), sym(2, false), false), ["Delete extra units from symbol?"]);
  assert.deepEqual(symbolPropertiesQuestions(sym(2, false), sym(3, false), false), []);
  assert.deepEqual(symbolPropertiesQuestions(sym(2, false), sym(2, false), false), []);
});

test("De Morgan off asks about the body styles, after the units (r7 E7-S7)", () => {
  assert.deepEqual(symbolPropertiesQuestions(sym(2, true), sym(1, false), false), ["Delete extra units from symbol?", "Delete extra body styles from symbol?"]);
  assert.deepEqual(symbolPropertiesQuestions(sym(1, false), sym(1, true), false), []);
});

test("a derived symbol's units are its parent's: no question", () => {
  assert.deepEqual(symbolPropertiesQuestions(sym(3, true), sym(1, false), true), []);
});
