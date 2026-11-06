import { test } from "node:test";
import assert from "node:assert/strict";
import {
  bodyStyleChoiceRefusal,
  bodyStyleCount,
  bodyStyleDescription,
  bodyStyleModeOf,
  bodyStyleSelectorEntries,
  isMultiBodyStyle,
  jumperRowsOf,
  resizeUnitNameRows,
  unitDisplayName,
  unitNameRowsOf,
  unitsAndBodyStylesProblem,
} from "./symBodyStyles";
import { symbolPropertiesQuestions } from "./symUnits";

const custom = { has_alternate_body_style: false, body_style_names: ["Front", "Side", "Top"] };
const demorgan = { has_alternate_body_style: true };
const single = { has_alternate_body_style: false };

test("GetBodyStyleCount, IsMultiBodyStyle and GetBodyStyleDescription of De Morgan, custom and single symbols", () => {
  assert.deepEqual([bodyStyleCount(demorgan), bodyStyleCount(custom), bodyStyleCount(single), bodyStyleCount(null)], [2, 3, 1, 1]);
  assert.deepEqual([isMultiBodyStyle(demorgan), isMultiBodyStyle(custom), isMultiBodyStyle(single)], [true, true, false]);
  assert.deepEqual([bodyStyleDescription(demorgan, 1), bodyStyleDescription(demorgan, 2), bodyStyleDescription(custom, 2), bodyStyleDescription(custom, 4), bodyStyleDescription(single, 1)], ["Standard", "Alternate", "Side", "?", "?"]);
  // a single name is no custom style (`GetBodyStyleCount` is 1)
  assert.equal(isMultiBodyStyle({ body_style_names: ["Only"] }), false);
});

test("the body style box of RebuildSymbolUnitAndBodyStyleLists", () => {
  assert.deepEqual(bodyStyleSelectorEntries(single), [""]);
  assert.deepEqual(bodyStyleSelectorEntries(demorgan), ["Standard", "Alternate"]);
  assert.deepEqual(bodyStyleSelectorEntries(custom), ["Front", "Side", "Top"]);
});

test("GetUnitDisplayName with and without its label", () => {
  const sym = { kicad: { unit_names: { "2": "Power" } } };
  assert.deepEqual([unitDisplayName(sym, 1), unitDisplayName(sym, 2), unitDisplayName(sym, 27, false), unitDisplayName(null, 3)], ["Unit A", "Power", "AA", "Unit C"]);
});

test("TransferDataToWindow's radio buttons, grids and jumper rows", () => {
  assert.deepEqual(bodyStyleModeOf(custom), { mode: "custom", names: ["Front", "Side", "Top"] });
  assert.deepEqual(bodyStyleModeOf(demorgan), { mode: "demorgan", names: [] });
  assert.deepEqual(bodyStyleModeOf(single), { mode: "single", names: [] });
  assert.deepEqual(unitNameRowsOf({ unit_count: 3, kicad: { unit_names: { "1": "Gate", "3": "Pwr", "4": "gone" } } }), ["Gate", "", "Pwr"]);
  assert.deepEqual(jumperRowsOf({ kicad: { jumper_pin_groups: [["1", "3"], ["2"]] } }), ["1, 3", "2"]);
  assert.deepEqual(jumperRowsOf({}), []);
  assert.deepEqual(resizeUnitNameRows(["A1", "B1", "C1"], 2), ["A1", "B1"], "updateUnitCount deletes the extra rows");
  assert.deepEqual(resizeUnitNameRows(["A1"], 3), ["A1", "", ""]);
});

test("Validate (KiCad 25ec2335): unit names in use, case aside, an empty row being its letters, and custom styles of fewer than two names", () => {
  assert.equal(unitsAndBodyStylesProblem(["Gate", "gate"], "single", []), "The unit name 'gate' is already in use.");
  assert.equal(unitsAndBodyStylesProblem(["B", ""], "single", []), "The unit name 'B' is already in use.", "row 2 with no name is 'B'");
  assert.equal(unitsAndBodyStylesProblem(["", "", "Power"], "single", []), null);
  assert.equal(unitsAndBodyStylesProblem([""], "custom", ["Front", ""]), "Custom body styles must have at least 2 entries");
  assert.equal(unitsAndBodyStylesProblem([""], "custom", ["Front", "Side"]), null);
});

test("fewer body styles asks Validate's question, the custom ones counted", () => {
  const two = { unit_count: 1, has_alternate_body_style: false, body_style_names: ["A", "B"] };
  const three = { ...two, body_style_names: ["A", "B", "C"] };
  assert.deepEqual(symbolPropertiesQuestions({ ...three }, { ...two }, false), ["Delete extra body styles from symbol?"]);
  assert.deepEqual(symbolPropertiesQuestions({ ...three }, { unit_count: 1, has_alternate_body_style: true }, false), ["Delete extra body styles from symbol?"], "three to De Morgan");
  assert.deepEqual(symbolPropertiesQuestions({ ...two }, { unit_count: 1, has_alternate_body_style: true }, false), [], "two custom to De Morgan keeps two");
});

test("OnBodyStyle (KiCad 25ec2335): several body styles are refused while a pin has alternate functions", () => {
  const pins = [{ number: "1", alternates: [{}] }, { number: "2" }, { number: "3", alternates: [{}] }];
  const summary = (n: string[]) => n.join(",");
  assert.equal(bodyStyleChoiceRefusal("demorgan", pins, summary), "Multiple body styles are not supported on symbols with alternate pin functions.\n(Pins 1,3.)");
  assert.equal(bodyStyleChoiceRefusal("custom", pins, summary), "Multiple body styles are not supported on symbols with alternate pin functions.\n(Pins 1,3.)");
  assert.equal(bodyStyleChoiceRefusal("single", pins, summary), null);
  assert.equal(bodyStyleChoiceRefusal("custom", [{ number: "1" }], summary), null);
});
