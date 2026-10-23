import { test } from "node:test";
import assert from "node:assert/strict";
import { applyPatch } from "./schMove";
import { fixtureTable } from "./tableFixture";
import type { Schematic, SchField } from "../api/types";
import type { SchMovePatch } from "../api/schEditTypes";
import { addUserFieldCmd, autoplaceCmd, boxFields, fieldBox, fieldEditCmd, fieldId, fieldItems, fieldSizeNm, fieldVisibilityCmds, findField, hitField, isFieldId, ownerKey, parseFieldId, selectedFields, shownText, fieldTextCmd, isStandardSymbolField, newUserFieldName, fieldDialogCaption, removeUserFieldCmd, userFieldNameError, userFieldsOf } from "./schFieldEdit";

// the width of a text in these tests: 600 um a character at 1270 um, in proportion to the size
const measure = (text: string, size: number) => text.length * 0.5 * size;

const field = (name: string, text: string, at: [number, number], over: Partial<SchField> = {}): SchField => ({ name, text, at, vertical: false, h: "left", v: "center", visible: true, ...over });

const sheet = (over: Partial<Schematic> = {}): Schematic =>
  ({
    lib_symbols: {},
    symbols: [
      { id: "R1", lib_id: null, at: [0, 0], rot: 0, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [], fields: [field("Reference", "R1", [10_000_000, 10_000_000]), field("Value", "330", [10_000_000, 12_540_000]), field("Footprint", "", [10_000_000, 10_000_000], { visible: false })] },
      { id: "U1", lib_id: null, at: [0, 0], rot: 0, mirror: null, unit: 2, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [], fields: [field("Reference", "U1", [40_000_000, 10_000_000])] },
    ],
    power_symbols: [{ id: "#PWR01", lib_id: "power:GND", at: [0, 0], rot: 0, net: "GND", pin: {} as never, fields: [field("Value", "GND", [20_000_000, 30_000_000], { h: "center" })] }],
    wires: [],
    no_connects: [],
    labels: [],
    texts: [],
    title_block: null,
    bus_entries: [],
    junctions: [],
    sheets: [{ id: "sheet_a", name: "Sub", file: "sub.kicad_sch", at: [60_000_000, 10_000_000], size: [20_000_000, 10_000_000], pins: [], fields: [field("Sheetname", "Sub", [60_000_000, 8_000_000], { v: "bottom" })] }],
    sheet_path: [],
    ...over,
  }) as unknown as Schematic;

test("a field id names its item and its field, and no other id is one", () => {
  assert.equal(fieldId("R1", "Value"), "fld:R1:Value");
  assert.deepEqual(parseFieldId("fld:U1#2:Reference"), { owner: "U1#2", name: "Reference" });
  assert.deepEqual(parseFieldId("fld:#PWR01:Value"), { owner: "#PWR01", name: "Value" });
  assert.equal(parseFieldId("R1"), null);
  assert.equal(parseFieldId("fld:R1"), null);
  assert.ok(isFieldId("fld:R1:Value") && !isFieldId("wire_a"));
  assert.equal(ownerKey({ id: "U1", unit: 1 }), "U1");
  assert.equal(ownerKey({ id: "U1", unit: 2 }), "U1#2");
  assert.deepEqual(selectedFields(["R1", "fld:R1:Value", "wire_a"]), ["fld:R1:Value"]);
});

test("every field of the sheet is listed with the id the backend selects it by", () => {
  const items = fieldItems(sheet());
  assert.deepEqual(items.map((i) => i.id), ["fld:sheet_a:Sheetname", "fld:#PWR01:Value", "fld:R1:Reference", "fld:R1:Value", "fld:R1:Footprint", "fld:U1#2:Reference"]);
  assert.deepEqual(items.map((i) => i.ownerKind), ["sheet", "power", "symbol", "symbol", "symbol", "symbol"]);
  assert.equal(findField(sheet(), "fld:R1:Value")?.field.text, "330");
  assert.equal(findField(sheet(), "fld:R1:Nothing"), null);
  // an id the server sent is kept
  const s = sheet();
  s.symbols[0]!.fields![0]!.id = "fld:R1:Reference";
  assert.equal(fieldItems(s).find((i) => i.field.name === "Reference" && i.owner === "R1")?.id, "fld:R1:Reference");
});

test("a bold text with its name shown reads as Name: value", () => {
  assert.equal(shownText(field("Value", "330", [0, 0])), "330");
  assert.equal(shownText(field("Value", "330", [0, 0], { name_shown: true })), "Value: 330");
});

test("the box of a field is justified against its anchor and turned with a vertical text", () => {
  const left = fieldBox(field("Value", "ABCDE", [1_000_000, 5_000_000]), measure);
  assert.equal(left.minX, 1_000_000);
  assert.ok(left.maxX > 1_000_000 + 5 * 0.5 * 1270 && left.maxX < 1_000_000 + 5 * 0.5 * 1270 + 700_000, `${left.maxX}`);
  assert.ok(left.minY < 5_000_000 && left.maxY > 5_000_000, "centred on the anchor, a little taller than the text");
  const right = fieldBox(field("Value", "ABCDE", [1_000_000, 5_000_000], { h: "right" }), measure);
  assert.equal(right.maxX, 1_000_000);
  assert.equal(right.minX, 1_000_000 - (left.maxX - left.minX));
  const centre = fieldBox(field("Value", "ABCDE", [1_000_000, 5_000_000], { h: "center" }), measure);
  assert.ok(Math.abs(centre.minX + centre.maxX - 2_000_000) <= 1);
  // a vertical text runs up the sheet from its anchor
  const up = fieldBox(field("Value", "ABCDE", [1_000_000, 5_000_000], { vertical: true }), measure);
  assert.equal(up.maxY, 5_000_000);
  assert.ok(up.minY < 5_000_000 - 5 * 0.5 * 1270);
  assert.ok(up.maxX - up.minX < 2_200_000, "and is as wide as a line is tall");
  // the box follows the size and a bold pen
  const big = fieldBox(field("Value", "ABCDE", [1_000_000, 5_000_000], { size_nm: 2_540_000 }), measure);
  assert.ok(big.maxX - big.minX > 2 * (left.maxX - left.minX) - 600_000 && big.maxY - big.minY > 1.9 * (left.maxY - left.minY) - 200_000);
  const bold = fieldBox(field("Value", "ABCDE", [1_000_000, 5_000_000], { bold: true }), measure);
  assert.ok(bold.maxX - bold.minX > left.maxX - left.minX, "a bold pen reaches further");
});

test("a click hits the shown field under it and the smallest of several; a hidden or empty field is never hit", () => {
  const s = sheet();
  assert.equal(hitField(s, 10_300_000, 10_000_000, 100_000, measure), "fld:R1:Reference");
  assert.equal(hitField(s, 10_300_000, 12_540_000, 100_000, measure), "fld:R1:Value");
  assert.equal(hitField(s, 20_000_000, 30_000_000, 100_000, measure), "fld:#PWR01:Value");
  assert.equal(hitField(s, 60_300_000, 8_000_000, 100_000, measure), "fld:sheet_a:Sheetname");
  assert.equal(hitField(s, 90_000, 90_000, 100_000, measure), null, "nothing there");
  // the hidden Footprint sits exactly on the Reference and is not what a click there hits
  assert.equal(hitField(s, 10_100_000, 10_000_000, 100_000, measure), "fld:R1:Reference");
  // overlapping boxes: the smaller wins
  const crowded = sheet();
  crowded.symbols[0]!.fields![1] = field("Value", "A very long value text", [10_000_000, 10_100_000]);
  assert.equal(hitField(crowded, 10_300_000, 10_050_000, 100_000, measure), "fld:R1:Reference");
});

test("a marquee takes the fields it crosses, or only those it encloses", () => {
  const s = sheet();
  assert.deepEqual(boxFields(s, [9_000_000, 9_000_000, 10_500_000, 11_000_000], true, measure), ["fld:R1:Reference"]);
  assert.deepEqual(boxFields(s, [9_000_000, 9_000_000, 10_500_000, 11_000_000], false, measure), [], "crossing only: the text runs on past the box");
  assert.deepEqual(boxFields(s, [9_000_000, 8_500_000, 20_000_000, 14_000_000], false, measure).sort(), ["fld:R1:Reference", "fld:R1:Value"]);
});

test("Field Properties sends only what changed, and nothing when it changed nothing", () => {
  const f = findField(sheet(), "fld:R1:Value")!.field;
  assert.equal(fieldEditCmd(f, {}), null);
  assert.equal(fieldEditCmd(f, { text: "330", sizeNm: 1_270_000, bold: false, visible: true, allowAutoplace: true, at: [10_000.4, 12_540_000] }), null);
  assert.deepEqual(fieldEditCmd(f, { text: "470", sizeNm: 2_000_000, bold: true, vertical: true, at: [11_000_000, 12_540_000] }), {
    op: "sch_edit",
    verb: "edit_field",
    id: "fld:R1:Value",
    text: "470",
    at: { x: 11_000_000, y: 12_540_000 },
    vertical: true,
    size_nm: 2_000_000,
    bold: true,
  });
  assert.deepEqual(fieldEditCmd(f, { h: "right", v: "top", italic: true, visible: false, nameShown: true, allowAutoplace: false }), {
    op: "sch_edit",
    verb: "edit_field",
    id: "fld:R1:Value",
    h: "right",
    v: "top",
    italic: true,
    visible: false,
    name_shown: true,
    allow_autoplace: false,
  });
  assert.equal(fieldSizeNm(f), 1_270_000);
  assert.equal(fieldSizeNm({ size_nm: 2_000_000 }), 2_000_000);
});

test("Autoplace Fields takes the items of the selection and the item each selected field belongs to", () => {
  const s = sheet();
  assert.equal(autoplaceCmd(s, []), null);
  assert.equal(autoplaceCmd(s, ["wire_a", "lbl_a"]), null, "nothing that has fields");
  assert.deepEqual(autoplaceCmd(s, ["R1", "fld:R1:Value", "fld:U1#2:Reference", "wire_a", "sheet_a", "#PWR01"]), { op: "sch_edit", verb: "autoplace_fields", ids: ["R1", "U1#2", "sheet_a", "#PWR01"] });
});

test("a preview patch carries the fields to where the server put them", () => {
  const s = sheet();
  const patch: SchMovePatch = {
    symbols: [{ id: "R1", unit: 1, at: [2_540_000, 0], rot: 0, mirror: null, fields: [field("Reference", "R1", [12_540_000, 10_000_000])] }],
    power_symbols: [{ id: "#PWR01", at: [0, 0], rot: 0, fields: [field("Value", "GND", [25_000_000, 30_000_000])] }],
    wires: [],
    labels: [],
    texts: [],
    no_connects: [],
    bus_entries: [],
    junctions: [],
    lines: [],
    graphics: [],
    sheets: [{ id: "sheet_a", at: [60_000_000, 10_000_000], size: [20_000_000, 10_000_000], pins: [], fields: [field("Sheetname", "Sub", [65_000_000, 8_000_000])] }],
  };
  const out = applyPatch(s, patch);
  assert.deepEqual(out.symbols[0]!.fields!.map((f) => f.at), [[12_540_000, 10_000_000]]);
  assert.deepEqual(out.symbols[1]!.fields, s.symbols[1]!.fields, "a symbol the patch does not name keeps its fields");
  assert.deepEqual(out.power_symbols[0]!.fields![0]!.at, [25_000_000, 30_000_000]);
  assert.deepEqual(out.sheets[0]!.fields![0]!.at, [65_000_000, 8_000_000]);
  // an older server sends none: the fields stay
  const old = applyPatch(s, { ...patch, symbols: [{ id: "R1", unit: 1, at: [2_540_000, 0], rot: 0, mirror: null }] });
  assert.deepEqual(old.symbols[0]!.fields, s.symbols[0]!.fields);
});

test("the Show column of Symbol Properties sends one edit per field whose visibility changed", () => {
  const fields = [field("Reference", "R1", [0, 0]), field("Value", "330", [0, 0]), field("Footprint", "", [0, 0], { visible: false })];
  const cmds = fieldVisibilityCmds("R1", fields, { Reference: true, Value: false, Footprint: true, Datasheet: true }) as unknown as Array<{ verb: string; id: string; visible: boolean }>;
  assert.deepEqual(
    cmds.map((c) => [c.verb, c.id, c.visible]),
    [
      ["edit_field", fieldId("R1", "Value"), false],
      ["edit_field", fieldId("R1", "Footprint"), true],
    ],
    "Reference is shown already, Datasheet is a field the symbol does not have"
  );
  assert.deepEqual(fieldVisibilityCmds("R1", fields, {}), []);
  assert.deepEqual(fieldVisibilityCmds("R1", undefined, { Value: false }), [], "an older server sends no fields");
});

test("a field of a symbol that is not one of the four standard ones is user-defined, whatever the case of the standard names", () => {
  assert.ok(isStandardSymbolField("Value") && isStandardSymbolField("datasheet") && isStandardSymbolField("REFERENCE"));
  assert.ok(!isStandardSymbolField("MPN") && !isStandardSymbolField("Description"));
  const fields = [field("Reference", "R1", [0, 0]), field("Value", "330", [0, 0]), field("MPN", "ERJ", [0, 0], { visible: false }), field("Maker", "Panasonic", [0, 0])];
  assert.deepEqual(userFieldsOf(fields).map((f) => f.name), ["MPN", "Maker"]);
  assert.deepEqual(userFieldsOf(undefined), []);
});

test("a new field is called Field<N> and the name is checked against the fields the symbol has", () => {
  // `GetUserFieldName( m_fields->size() )` over the five mandatory fields: a plain resistor's first new field is Field5
  assert.equal(newUserFieldName(["Reference", "Value", "Footprint", "Datasheet"]), "Field5");
  assert.equal(newUserFieldName(["Reference", "Value", "Footprint", "Datasheet", "MPN"]), "Field6");
  assert.equal(newUserFieldName(["Reference", "Value", "Footprint", "Datasheet", "Field6"]), "Field7", "the first that no field has");
  assert.equal(newUserFieldName(["Reference", "Value", "Footprint", "Datasheet", "field6"]), "Field7", "whatever the case");
  assert.equal(userFieldNameError("MPN", ["MPN"]), "Field name 'MPN' already in use.");
  assert.equal(userFieldNameError("mpn", ["MPN"]), "Field name 'mpn' already in use.");
  assert.equal(userFieldNameError("value", []), "Field name 'value' already in use.");
  assert.equal(userFieldNameError("Description", []), "Field name 'Description' already in use.");
  assert.equal(userFieldNameError("  ", []), "Fields must have a name.");
  assert.equal(userFieldNameError("${X}", []), '"${X}" is not a valid field name.');
  assert.equal(userFieldNameError(" Maker ", ["MPN"]), null);
});

test("the commands of a user-defined field are the backend's verbs", () => {
  assert.deepEqual(addUserFieldCmd("R1", " Maker ", "Panasonic"), { op: "sch_edit", verb: "add_user_field", id: "R1", name: "Maker", text: "Panasonic" });
  assert.deepEqual(removeUserFieldCmd("R1", "Maker"), { op: "sch_edit", verb: "remove_user_field", id: "R1", name: "Maker" });
  assert.deepEqual(fieldTextCmd("U1#2", "MPN", "x"), { op: "sch_edit", verb: "edit_field", id: "fld:U1#2:MPN", text: "x" });
});

test("a held table is shown as the server laid it out for where it would be; a table the patch does not name stays", () => {
  const moved = { ...fixtureTable(1, 2000), id: "tbl_a" };
  const other = { ...fixtureTable(1, 1), id: "tbl_b" };
  const shifted = { ...moved, x: 5_000_000, cells: moved.cells.map((c) => ({ ...c, corners: c.corners.map(([x, y]) => [x + 5_000_000, y] as [number, number]) as typeof c.corners })) };
  const sch = { symbols: [], power_symbols: [], wires: [], labels: [], texts: [], no_connects: [], bus_entries: [], junctions: [], lines: [], graphics: [], sheets: [], tables: [moved, other] } as unknown as Parameters<typeof applyPatch>[0];
  const patch = { symbols: [], power_symbols: [], wires: [], labels: [], texts: [], no_connects: [], bus_entries: [], junctions: [], lines: [], graphics: [], sheets: [], tables: [shifted] };
  const out = applyPatch(sch, patch);
  assert.equal(out.tables![0], shifted);
  assert.equal(out.tables![1], other);
  // a patch of an older server, or of a sheet with no table, leaves the tables as they are
  assert.equal(applyPatch(sch, { ...patch, tables: undefined }).tables![0], moved);
});

test("Field Properties is captioned as editFieldText does: a mandatory field by its title-capped name, any other quoted", () => {
  assert.equal(fieldDialogCaption("Value", false), "Edit Value Field");
  assert.equal(fieldDialogCaption("Sheetname", true), "Edit Sheet Name Field");
  assert.equal(fieldDialogCaption("Sheetfile", true), "Edit Sheet File Field");
  assert.equal(fieldDialogCaption("MPN", false), "Edit 'MPN' Field");
});
