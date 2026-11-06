import { test } from "node:test";
import assert from "node:assert/strict";
import type { Schematic, SchematicSymbol } from "../api/types";
import {
  ADD_VARIANT_ENTRY,
  DEFAULT_VARIANT_NAME,
  pickVariantEntry,
  resolveCurrentVariant,
  selectorValue,
  setSymbolVariantCmd,
  sortVariantNames,
  symbolInVariant,
  symbolPropertiesTitle,
  trimName,
  validateCopyNameInTable,
  validateNewVariantName,
  variantChoices,
  variantDescription,
  variantFieldText,
  variantFlags,
  variantNamesForUI,
  variantSelectorEntries,
  VARIANT_SEPARATOR,
  withVariant,
} from "./designVariants";

const sym = (over: Partial<SchematicSymbol> = {}): SchematicSymbol =>
  ({
    id: "R1",
    lib_id: "Device:R",
    at: [0, 0],
    rot: 0,
    mirror: null,
    unit: 1,
    body_style: 1,
    value: "10k",
    mpn: null,
    package: null,
    footprint: "Resistor_SMD:R_0603",
    datasheet: "~",
    pins: [],
    dnp: false,
    exclude_from_bom: false,
    fields: [
      { name: "Reference", text: "R1", at: [0, 0], vertical: false, h: "left", v: "center", visible: true },
      { name: "Value", text: "${VAL}", shown: "10k", at: [0, 0], vertical: false, h: "left", v: "center", visible: true },
      { name: "MPN", text: "RC0603", at: [0, 0], vertical: false, h: "left", v: "center", visible: false },
    ],
    ...over,
  }) as SchematicSymbol;

test("the default placeholder is KiCad's, untranslated", () => {
  assert.equal(DEFAULT_VARIANT_NAME, "< Default >");
});

test("SortVariantNames: the default first, then StrNumCmp (digit runs as numbers)", () => {
  assert.deepEqual(["V10", DEFAULT_VARIANT_NAME, "V2", "Alpha"].sort(sortVariantNames), [DEFAULT_VARIANT_NAME, "Alpha", "V2", "V10"]);
  assert.deepEqual(variantNamesForUI([{ name: "Variant2" }, { name: "Variant 1" }]), [DEFAULT_VARIANT_NAME, "Variant 1", "Variant2"]);
  assert.deepEqual(variantNamesForUI(undefined), [DEFAULT_VARIANT_NAME]);
  assert.deepEqual(variantChoices(["B", "A"]), ["A", "B"], "the choosers leave the default out");
});

test("SetCurrentVariant: a name the schematic has, else the default", () => {
  const vs = [{ name: "Lite", description: "" }];
  assert.equal(resolveCurrentVariant(vs, "Lite"), "Lite");
  assert.equal(resolveCurrentVariant(vs, "lite"), "", "the name is matched exactly");
  assert.equal(resolveCurrentVariant(vs, "Gone"), "");
  assert.equal(resolveCurrentVariant(vs, DEFAULT_VARIANT_NAME), "");
  assert.equal(selectorValue(vs, "Gone"), DEFAULT_VARIANT_NAME);
  assert.equal(selectorValue(vs, "Lite"), "Lite");
});

test("ValidateNewVariantName's three messages, without regard to case", () => {
  const vs = ["Lite", "Pro"];
  assert.equal(validateNewVariantName(vs, ""), "Variant name cannot be empty.");
  assert.equal(validateNewVariantName(vs, "< default >"), "'< Default >' is a reserved variant name.");
  assert.equal(validateNewVariantName(vs, "LITE"), "Variant 'Lite' already exists.");
  assert.equal(validateNewVariantName(vs, "Max"), null);
  // a rename may keep its own name in another case, not take another's
  assert.equal(validateNewVariantName(vs, "LITE", "Lite"), null);
  assert.equal(validateNewVariantName(vs, "pro", "Lite"), "Variant 'Pro' already exists.");
});

test("the Fields Table's own copy check: empty, or a name the list box has (FindString ignores case)", () => {
  const listed = variantNamesForUI(["Lite"]);
  assert.equal(validateCopyNameInTable(listed, ""), "Variant name cannot be empty.");
  assert.equal(validateCopyNameInTable(listed, "lite"), "Variant 'lite' already exists.");
  assert.equal(validateCopyNameInTable(listed, "Lite_copy"), null);
});

test("Trim().Trim( false ) strips white space at both ends only", () => {
  assert.equal(trimName("  My Variant \t\n"), "My Variant");
});

test("the toolbar choice: names, '---', 'Add New Design Variant...'; picking each", () => {
  const entries = variantSelectorEntries(["Lite"]);
  assert.deepEqual(entries, [DEFAULT_VARIANT_NAME, "Lite", VARIANT_SEPARATOR, ADD_VARIANT_ENTRY]);
  assert.equal(ADD_VARIANT_ENTRY, "Add New Design Variant...");
  assert.deepEqual(pickVariantEntry(entries, 3), { kind: "add" });
  assert.deepEqual(pickVariantEntry(entries, 2), { kind: "none" }, "the separator restores the selection");
  assert.deepEqual(pickVariantEntry(entries, 1), { kind: "select", variant: "Lite" });
  assert.deepEqual(pickVariantEntry(entries, 0), { kind: "select", variant: "" }, "the default is the empty name");
  assert.deepEqual(pickVariantEntry(entries, -1), { kind: "none" });
});

test("GetDNP( sheet, variant ): the variant's record, else the symbol's own", () => {
  const s = sym({ dnp: false, exclude_from_bom: true, variants: { Lite: { dnp: true }, Pro: { exclude_from_bom: false } } });
  assert.equal(variantFlags(s, "").dnp, false);
  assert.equal(variantFlags(s, "Lite").dnp, true);
  assert.equal(variantFlags(s, "Lite").exclude_from_bom, true, "a flag the record leaves out is the symbol's");
  assert.equal(variantFlags(s, "Pro").exclude_from_bom, false);
  assert.equal(variantFlags(s, "Max").dnp, false, "no record: the symbol's own");
});

test("GetFieldText( field, sheet, variant ): the override, never for the Reference", () => {
  const s = sym({ variants: { Lite: { fields: { Value: "22k", Reference: "R9", MPN: "" } } } });
  assert.equal(variantFieldText(s, "Value", "Lite"), "22k");
  assert.equal(variantFieldText(s, "Value", ""), "${VAL}");
  assert.equal(variantFieldText(s, "Reference", "Lite"), "R1");
  assert.equal(variantFieldText(s, "MPN", "Lite"), "", "an empty override is an override");
  assert.equal(variantFieldText(s, "Footprint", "Lite"), "Resistor_SMD:R_0603");
});

test("symbolInVariant: flags, Value / Footprint and the fields' texts; an overridden field loses its own `shown`", () => {
  const s = sym({ variants: { Lite: { dnp: true, exclude_from_sim: true, fields: { Value: "22k", Footprint: "R_0402" } } } });
  const v = symbolInVariant(s, "Lite");
  assert.equal(v.dnp, true);
  assert.equal(v.exclude_from_sim, true);
  assert.equal(v.value, "22k");
  assert.equal(v.footprint, "R_0402");
  const value = v.fields!.find((f) => f.name === "Value")!;
  assert.equal(value.text, "22k");
  assert.equal(value.shown, undefined);
  assert.equal(v.fields!.find((f) => f.name === "MPN")!.text, "RC0603");
  assert.equal(symbolInVariant(s, ""), s, "the default is the symbol as it is");
  assert.equal(symbolInVariant(s, "Pro"), s, "no record: as it is");
});

test("withVariant lays the variant over every symbol, and is the same sheet for the default", () => {
  const sch = { symbols: [sym({ variants: { Lite: { dnp: true } } }), sym({ id: "R2" })] } as unknown as Schematic;
  assert.equal(withVariant(sch, ""), sch);
  assert.equal(withVariant(sch, "Nobody"), sch);
  const lite = withVariant(sch, "Lite");
  assert.deepEqual(lite.symbols.map((s) => s.dnp), [true, false]);
  assert.equal(sch.symbols[0]!.dnp, false, "the state itself is untouched");
});

test("set_symbol_variant: only what changes, never for the default", () => {
  assert.deepEqual(setSymbolVariantCmd("Lite", ["R1"], { flags: { dnp: true }, fields: { Value: "22k" } }), { op: "set_symbol_variant", variant: "Lite", ids: ["R1"], dnp: true, fields: { Value: "22k" } });
  assert.deepEqual(setSymbolVariantCmd("Lite", ["R1"], { flags: { exclude_from_bom: false } }), { op: "set_symbol_variant", variant: "Lite", ids: ["R1"], exclude_from_bom: false });
  assert.equal(setSymbolVariantCmd("", ["R1"], { flags: { dnp: true } }), null);
  assert.equal(setSymbolVariantCmd("Lite", [], { flags: { dnp: true } }), null);
  assert.equal(setSymbolVariantCmd("Lite", ["R1"], {}), null);
});

test("Symbol Properties names the variant being edited", () => {
  assert.equal(symbolPropertiesTitle(""), "Symbol Properties");
  assert.equal(symbolPropertiesTitle("Lite"), "Symbol Properties - Lite Design Variant");
});

test("GetVariantDescription: empty for none", () => {
  const vs = [{ name: "Lite", description: "no radio" }];
  assert.equal(variantDescription(vs, "Lite"), "no radio");
  assert.equal(variantDescription(vs, "Pro"), "");
});
