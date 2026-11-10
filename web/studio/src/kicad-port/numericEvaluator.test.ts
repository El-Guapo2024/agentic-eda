import { test } from "node:test";
import assert from "node:assert/strict";
import { evaluatedBoxText, lengthBoxValue, lengthFromString, NumericEvaluator, uiDouble2Str } from "./numericEvaluator";

const run = (text: string, units: ConstructorParameters<typeof NumericEvaluator>[0] = "mm"): string | null => {
  const ev = new NumericEvaluator(units);
  return ev.process(text) ? ev.result() : null;
};

test("arithmetic and brackets, with the grammar's precedences", () => {
  assert.equal(run("1+2"), "3");
  assert.equal(run("2*(0.2+0.05)"), "0.5");
  assert.equal(run("10/4"), "2.5");
  assert.equal(run("-1+2"), "1");
  assert.equal(run("-2*3"), "-6");
  assert.equal(run("+4"), "4");
  assert.equal(run("1-2-3"), "-4", "%left");
  assert.equal(run("8/2/2"), "2");
  assert.equal(run("  3 "), "3");
});

test("a unit binds tighter than + and -, looser than * and /, and is turned into the default unit", () => {
  assert.equal(run("1mm+10mil"), "1.254");
  assert.equal(run("1+2mm"), "3");
  assert.equal(run("2*3mm"), "6", "(2*3) mm");
  assert.equal(run("1in"), "25.4");
  assert.equal(run('1"'), "25.4");
  assert.equal(run("100um"), "0.1");
  assert.equal(run("1cm"), "10");
  assert.equal(run("5thou", "mil"), "5");
  assert.equal(run("1mm", "mil"), "39.37007874");
  assert.equal(run("1mm", "in"), "0.03937007874");
  assert.equal(run("-2mm"), "-2");
  // a unit of another kind scales by 0 (the switch's `default: break`)
  assert.equal(run("5ps"), "0");
  assert.equal(run("1,5"), "1.5", "a comma is a decimal separator");
});

test("variables stay with the evaluator, and only the last statement is the result", () => {
  const ev = new NumericEvaluator("mm");
  assert.ok(ev.process("w=0.3; w*2"));
  assert.equal(ev.result(), "0.6");
  assert.ok(ev.process("w+1"));
  assert.equal(ev.result(), "1.3");
  assert.equal(run("x=2"), "NaN", "an assignment has no value");
  assert.equal(run("1;"), "NaN");
});

test("errors: syntax, characters, unknown variables, a division by zero", () => {
  for (const bad of ["1 2", "(1+2", "1+", "2 & 3", "y*2", "1/0", "mm", "-- leave unchanged --"]) assert.equal(run(bad), null, bad);
  assert.equal(run(""), "0", "an empty text is fine and says nothing");
});

test("UIDouble2Str: %.10g, and %.16f for the small values", () => {
  assert.equal(uiDouble2Str(0.00005), "0.00005");
  assert.equal(uiDouble2Str(1 / 3), "0.3333333333");
  assert.equal(uiDouble2Str(12345678901), "1.23456789e+10");
  assert.equal(uiDouble2Str(-2.5), "-2.5");
  assert.equal(uiDouble2Str(NaN), "NaN");
});

test("a length box: the expression in the editor's units, else the text read by ValueFromString; empty leaves the value unchanged", () => {
  assert.equal(lengthBoxValue("0.1+0.05", "mm"), 150_000);
  assert.equal(lengthBoxValue("1mm", "mil"), 1_000_000);
  assert.equal(lengthBoxValue("6", "mil"), 152_400);
  assert.equal(lengthBoxValue("", "mm"), null);
  assert.equal(lengthBoxValue("abc", "mm"), 0, "no number: 0, as DoubleValueFromString gives");
  assert.equal(lengthFromString("10 mils", "mm"), 254_000);
  assert.equal(lengthFromString("0,2", "mm"), 200_000);
  assert.equal(evaluatedBoxText("2*0.1", "mm"), "0.2");
  assert.equal(evaluatedBoxText("2*", "mm"), "2*", "a text that does not work out is left as typed");
});
