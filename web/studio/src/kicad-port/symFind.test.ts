import { test } from "node:test";
import assert from "node:assert/strict";
import { currentSymMatch, findStringFromSelection, matchedItems, nextSymMatch, replaceEdit, replaceTexts, symFindItems, SYMBOL_END_MESSAGE, textsOf } from "./symFind";
import { allFields } from "./symFields";
import type { LibrarySymbol } from "../api/types";

const sym = (): LibrarySymbol =>
  ({
    lib_id: "Lib:U",
    reference_prefix: "U",
    description: "an op amp",
    keywords: "",
    datasheet: "",
    power: false,
    in_bom: true,
    on_board: true,
    pin_numbers_hidden: false,
    pin_names_hidden: false,
    pin_name_offset_mm: 0.508,
    unit_count: 2,
    has_alternate_body_style: false,
    footprint_filters: [],
    published: false,
    fields: [
      { name: "Reference", text: "U", at: { x: 0, y: 5 } },
      { name: "Value", text: "VCC_REG", at: { x: 0, y: 3 } },
      { name: "MPN", text: "VCC-1", at: { x: 10, y: 0 }, hidden: true },
    ],
    pins: [
      { id: "p2", number: "2", name: "VCC", electrical_type: "power_in", shape: "line", at: { x: -5, y: 0 }, angle_deg: 0, length_mm: 2.54, unit: 2, body_style: 1, hidden: false },
      { id: "p1", number: "1", name: "GND", electrical_type: "power_in", shape: "line", at: { x: -5, y: 2 }, angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false },
    ],
    graphics: [{ kind: "text", id: "t1", unit: 1, body_style: 1, text: "VCC rail\nsecond", at: { x: 3, y: 0 }, angle_deg: 0, size_mm: 1.27 }],
  }) as unknown as LibrarySymbol;

test("the walk: every unit's pins, the fields but the Reference (a hidden one only when asked), the texts, by x then y (nextMatch in a SYMBOL_EDIT_FRAME)", () => {
  const s = sym();
  const fields = allFields(s);
  const items = symFindItems(s, fields, false, null);
  assert.ok(!items.some((i) => i.id === "fld:lib:Reference"), "the Reference of a LIB_SYMBOL never matches");
  assert.ok(!items.some((i) => i.id === "fld:lib:MPN"), "a hidden field only with Include hidden fields");
  assert.deepEqual(items.map((i) => i.id), ["p1", "p2", "fld:lib:Value", "t1"], "the hidden Footprint, Datasheet and Description are left out too");
  // x first: the pins (-5) before the fields at x 0 before the text at 3; at equal x, y down (KiCad's): the pin at file y 2 (internal -2) before the one at 0
  assert.equal(items[0]!.id, "p1");
  assert.equal(items[1]!.id, "p2");
  assert.equal(items[items.length - 1]!.id, "t1");
  assert.deepEqual(items.find((i) => i.id === "p2")!.texts, ["VCC", "2"], "a pin matches on its name or its number");
  assert.ok(symFindItems(s, fields, true, null).some((i) => i.id === "fld:lib:MPN"));
  assert.deepEqual(
    symFindItems(s, fields, false, new Set(["p2", "t1"])).map((i) => i.id),
    ["p2", "t1"],
    "with Search the current selection only, the selection"
  );
});

test("Find Next and Previous from the last match, nothing after the end (FindNext, nextMatch's past_item)", () => {
  const s = sym();
  const items = symFindItems(s, allFields(s), false, null);
  const { texts } = textsOf(items);
  const matches = texts.map((t) => t.toUpperCase().includes("VCC"));
  const matched = matchedItems(items, matches);
  const first = nextSymMatch(items, matched, null, false);
  assert.equal(first?.id, "p2");
  const second = nextSymMatch(items, matched, first!.id, false);
  assert.equal(second?.id, "fld:lib:Value");
  const third = nextSymMatch(items, matched, second!.id, false);
  assert.equal(third?.id, "t1");
  assert.equal(nextSymMatch(items, matched, third!.id, false), null, "the end of the symbol");
  assert.equal(nextSymMatch(items, matched, "t1", true)?.id, "fld:lib:Value", "Find Previous walks back");
  assert.equal(nextSymMatch(items, matched, "gone", false), null, "a cursor the walk no longer has finds nothing");
  assert.equal(SYMBOL_END_MESSAGE, "Reached end of symbol. Find again to wrap around to the start.");
});

test("Replace acts on the selection's first item, or with selected only on the last match (getCurrentMatch)", () => {
  assert.equal(currentSymMatch(["p2", "t1"], "t1", false), "p2");
  assert.equal(currentSymMatch(["p2"], "t1", true), "t1");
  assert.equal(currentSymMatch([], null, false), null);
});

test("a pin has its name and number replaced, a field and a text their text (SCH_PIN::Replace, EDA_TEXT::Replace)", () => {
  const s = sym();
  const fields = allFields(s);
  const items = symFindItems(s, fields, true, null);
  const pin = items.find((i) => i.id === "p2")!;
  assert.deepEqual(replaceTexts(pin, s, fields), ["VCC", "2"]);
  const e = replaceEdit(pin, s, fields, ["VDD", null]);
  assert.ok(e && e.kind === "pin" && e.pin.name === "VDD" && e.pin.number === "2");
  assert.equal(replaceEdit(pin, s, fields, [null, null]), null, "nothing replaced, no edit");
  const value = items.find((i) => i.id === "fld:lib:Value")!;
  const fe = replaceEdit(value, s, fields, ["VDD_REG"]);
  assert.ok(fe && fe.kind === "field" && fe.field.name === "Value" && fe.field.text === "VDD_REG");
  const text = items.find((i) => i.id === "t1")!;
  const te = replaceEdit(text, s, fields, ["VDD rail\nsecond"]);
  assert.ok(te && te.kind === "graphic" && (te.graphic as { text: string }).text === "VDD rail\nsecond");
});

test("the dialog starts with the selected field's text, or a selected text's first line (ShowFindReplaceDialog)", () => {
  const s = sym();
  const fields = allFields(s);
  assert.equal(findStringFromSelection(["fld:lib:Value"], s, fields), "VCC_REG");
  assert.equal(findStringFromSelection(["t1"], s, fields), "VCC rail");
  assert.equal(findStringFromSelection(["p1"], s, fields), "", "a pin gives nothing");
  assert.equal(findStringFromSelection(["t1", "p1"], s, fields), "", "nor two items");
});

test("UI-LEFT-2 YY6: the find history -- the text moved to the top or put there, case counting, ten kept when the dialog opens", async () => {
  const { pushHistory, capHistory } = await import("./symFind");
  assert.deepEqual(pushHistory([], "R1"), ["R1"]);
  assert.deepEqual(pushHistory(["a", "b", "c"], "c"), ["c", "a", "b"]);
  assert.deepEqual(pushHistory(["a", "b"], "a"), ["a", "b"]);
  assert.deepEqual(pushHistory(["a"], "A"), ["A", "a"], "FindString( .., true ): case counts");
  assert.equal(capHistory(Array.from({ length: 14 }, (_, i) => String(i))).length, 10);
});
