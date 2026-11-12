import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_SCH_FORMATTING,
  EMPTY_MEMBER_ERROR,
  NEW_TEMPLATE_FIELD,
  UNIT_NOTATIONS,
  WHITESPACE_EXTENDED,
  aliasNameError,
  busAliasIssue,
  busAliasesToSend,
  connectionGridError,
  cssFromHex,
  defaultClassSchematic,
  doubleValueFromString,
  firstWhitespaceRow,
  hexFromCss,
  junctionDiameterNm,
  junctionDrawn,
  junctionRadiusNm,
  keptTemplateFields,
  lineStyleOfRow,
  lineStyleRow,
  membersLabel,
  milsText,
  milsToNm,
  moveRow,
  rangeLabel,
  rangeStored,
  ratioText,
  samePageSettings,
  schematicColumnsIssue,
  schematicToSend,
  sortAliases,
  splitMembers,
  unitNotationChoice,
  whitespaceWarning,
} from "./schSetup";

test("Formatting: the connection grid is refused in UNIT_BINDER::Validate's words, the label without its colon and the limit in mils", () => {
  assert.equal(connectionGridError(24), "Connection grid must be at least 25 mils.");
  assert.equal(connectionGridError(10_001), "Connection grid must be less than 10000 mils.");
  assert.equal(connectionGridError(25), null);
  assert.equal(connectionGridError(10_000), null);
});

test("Formatting: the fields read their text as DoubleValueFromString does, and show it as StringFromValue does", () => {
  assert.equal(doubleValueFromString(" 12,5 "), 12.5, "a comma is the decimal point");
  assert.equal(doubleValueFromString("37.5%"), 37.5, "the numeric part only");
  assert.equal(doubleValueFromString("abc"), 0, "no number reads as 0, with no message");
  assert.equal(ratioText(0.15 * 100), "15");
  assert.equal(ratioText(1.23 * 100), "123");
  assert.equal(milsText(DEFAULT_SCH_FORMATTING.default_text_size_nm), "50");
  assert.equal(milsText(254), "0.01");
  assert.equal(milsToNm("6"), 152_400);
  assert.equal(milsToNm("6.0001"), 152_400, "on the schematic's 100 nm unit");
  assert.equal(rangeLabel("~V"), "Auto");
  assert.equal(rangeStored("Auto", "A"), "~A");
  assert.equal(rangeStored("mV", "V"), "mV");
  assert.equal(samePageSettings({ a: 1, b: "x" }, { b: "x", a: 1 }), true, "the keys in another order (the backend sorts them)");
  assert.equal(samePageSettings({ ...DEFAULT_SCH_FORMATTING }, { ...DEFAULT_SCH_FORMATTING, gap_ratio: 4 }), false);
});

test("Formatting: the junction dot is the Default class's wire width times the choice, and at least 1.7 times the wire it joins", () => {
  assert.equal(junctionDiameterNm(3, 152_400), 914_400, "36 mils at Default on a 6 mil wire (DEFAULT_JUNCTION_DIAM)");
  assert.equal(junctionDiameterNm(0, 152_400), 100, "None: one unit");
  assert.equal(junctionDiameterNm(5, 254_000), 3_048_000, "Largest on a 10 mil wire");
  assert.equal(junctionRadiusNm(914_400, 152_400), 457_200);
  assert.equal(junctionRadiusNm(259_100, 508_000), 431_800, "a 20 mil wire keeps the dot at least 1.7 times its width");
  assert.equal(junctionDrawn(junctionRadiusNm(100, 152_400)), false, "None draws no dot");
  assert.equal(junctionDrawn(junctionRadiusNm(914_400, 152_400)), true);
});

test("Annotation: the seven unit notations as loadEEschemaSettings shows a project's", () => {
  UNIT_NOTATIONS.forEach((n, i) => assert.equal(unitNotationChoice(n.firstId, n.separator), i, n.label));
  assert.equal(unitNotationChoice(65, 47), 0, "an odd separator with letters is A");
  assert.equal(unitNotationChoice(97, 0), 4, "any first id but A is numbered, '.' by default");
});

test("Field Name Templates: a new row, the white-space question word for word, the rows kept as AddTemplateFieldName keeps them", () => {
  assert.deepEqual(NEW_TEMPLATE_FIELD, { name: "Untitled Field", visible: false, url: false });
  assert.equal(whitespaceWarning(" MPN"), "The field name ' MPN' contains trailing and/or leading white space.");
  assert.equal(whitespaceWarning("MPN"), null);
  assert.match(WHITESPACE_EXTENDED, /^This may result in what appears to be duplicate field names but are actually unique names differing only by white space characters\.  Removing/);
  assert.deepEqual(firstWhitespaceRow([{ name: "A", visible: false, url: false }, { name: "B ", visible: false, url: false }]), { row: 1, message: "The field name 'B ' contains trailing and/or leading white space." });
  const kept = keptTemplateFields([
    { name: "MPN", visible: false, url: false },
    { name: "", visible: true, url: false },
    { name: "Value", visible: true, url: false },
    { name: "MPN", visible: true, url: true },
  ]);
  assert.deepEqual(kept, [{ name: "MPN", visible: true, url: true }], "no name, a mandatory name, a name twice (the second replaces the first)");
  assert.deepEqual(moveRow(["a", "b", "c"], 2, -1), ["a", "c", "b"]);
  assert.deepEqual(moveRow(["a", "b"], 0, -1), ["a", "b"], "the first row cannot move up");
});

test("Bus Alias Definitions: the grid's vetoes in its words, members split at spaces, sorted without regard to case", () => {
  const list = [
    { name: "data", members: ["D0"] },
    { name: "DATA", members: ["D1"] },
  ];
  assert.equal(aliasNameError(list, 0, "DATA"), "Alias name 'DATA' already in use.", "names are compared as typed");
  assert.equal(aliasNameError(list, 1, "DATA"), null);
  assert.equal(busAliasIssue([...list, { name: "DATA", members: [] }])?.message, "Alias name 'DATA' already in use.");
  assert.deepEqual(busAliasIssue([{ name: "X", members: ["A", ""] }]), { row: 0, member: 1, message: EMPTY_MEMBER_ERROR });
  assert.equal(EMPTY_MEMBER_ERROR, "Member net/alias name cannot be empty.");
  assert.equal(membersLabel("USB"), "Members of 'USB'");
  assert.deepEqual(splitMembers("D0 D1  D2"), ["D0", "D1", "D2"]);
  assert.deepEqual(busAliasesToSend([{ name: "B", members: ["D0 D1", "D2"] }]), [{ name: "B", members: ["D0", "D1", "D2"] }]);
  assert.deepEqual(sortAliases([{ name: "b", members: [] }, { name: "A", members: [] }]).map((a) => a.name), ["A", "b"]);
});

test("Net Classes: the Eeschema columns' line styles, colours and checks", () => {
  assert.equal(lineStyleRow(undefined), 0, "<Not defined>");
  assert.equal(lineStyleRow(0), 1, "Solid");
  assert.equal(lineStyleRow(4), 5, "Dash-Dot-Dot");
  assert.equal(lineStyleRow(9), 0, "out of the list is <Not defined>");
  assert.equal(lineStyleOfRow(0), null);
  assert.equal(lineStyleOfRow(2), 1);
  assert.equal(cssFromHex("#ff0000"), "rgb(255, 0, 0)");
  assert.equal(hexFromCss("rgba(0, 128, 255, 0.500)"), "#0080ff");
  assert.equal(hexFromCss(null), null);
  assert.deepEqual(defaultClassSchematic(undefined), { wire_width: 152_400, bus_width: 304_800, color: null, line_style: 0 });
  assert.equal(schematicColumnsIssue({ wire_width: 0 }, false), null, "KiCad takes a 0 as typed");
  assert.equal(schematicColumnsIssue({ wire_width: -1 }, false), "Wire thickness cannot be negative.");
  assert.equal(schematicColumnsIssue({}, false), null, "a class other than Default may leave them unset");
  assert.deepEqual(schematicToSend({ Wide: { wire_width: 508_000, color: null }, Gone: { wire_width: 1 }, Empty: {} }, ["Wide", "Empty"]), { Wide: { wire_width: 508_000 } });
});
