import { test } from "node:test";
import assert from "node:assert/strict";
import type { ErcViolation, Schematic } from "../api/types";
import { DEFAULT_SEARCH_PANE_SETTINGS } from "./searchHandlers";
import { SCH_SEARCH_TABS, schSearchColumnWidths, schSearchTab, schSearchTarget, sortSchSearchRows, type SchSearchContext, type SchSearchSheet } from "./schSearchHandlers";
import { ercFindMarkers, ercFindMarkersOfSheets, ercMarkerIndex } from "./schFindMarkers";

const field = (name: string, text: string, visible: boolean) => ({ name, text, visible, at: [0, 0], vertical: false, h: "left", v: "center" });

function sheet(path: string[], page: string, over: Partial<Schematic> = {}): SchSearchSheet {
  const sch = {
    lib_symbols: { "Device:R": { description: "Resistor", keywords: "R res", unit_count: 1, pins: [], graphics: [] }, "Amp:Quad": { description: "Quad op amp", unit_count: 4, pins: [], graphics: [] } },
    symbols: [],
    power_symbols: [],
    wires: [],
    no_connects: [],
    labels: [],
    texts: [],
    title_block: null,
    bus_entries: [],
    sheets: [],
    sheet_path: [],
    ...over,
  } as unknown as Schematic;
  return { path, page, sch };
}

const sym = (id: string, x: number, over: Record<string, unknown> = {}) => ({
  id,
  lib_id: "Device:R",
  at: [x, 2_540_000],
  rot: 0,
  mirror: null,
  unit: 1,
  body_style: 1,
  value: "10k",
  mpn: null,
  package: null,
  footprint: "R_0603",
  datasheet: null,
  pins: [],
  fields: [field("Reference", id, true), field("Value", "10k", true), field("Footprint", "R_0603", false), field("MPN", "RC0603FR", false)],
  ...over,
});

const ctx = (over: Partial<SchSearchContext["settings"]> = {}): SchSearchContext => ({ units: "mm", settings: { ...DEFAULT_SEARCH_PANE_SETTINGS, ...over } });

test("the five tabs and their columns are SCH_SEARCH_PANE's", () => {
  assert.deepEqual(
    SCH_SEARCH_TABS.map((t) => t.name),
    ["Symbols", "Power", "Text", "Labels", "Groups"]
  );
  assert.deepEqual(
    SCH_SEARCH_TABS[0]!.columns.map((c) => c.name),
    ["Reference", "Value", "Footprint", "Page", "X", "Y", "Excl. Sim", "Excl. BOM", "Excl. Board", "DNP", "Library Link", "Library Description"]
  );
  assert.deepEqual(schSearchColumnWidths("groups", 500), [300, 100, 150, 150]);
});

test("Symbols: every sheet path, the fields matched as SCH_FIELD::Matches has it", () => {
  const sheets = [
    sheet([], "1", { symbols: [sym("R1", 1_000_000, { exclude_from_bom: true }), sym("U1", 3_000_000, { lib_id: "Amp:Quad", unit: 2, reference: "U1" })] as never }),
    sheet(["s1"], "2", { symbols: [sym("R7", 2_000_000, { dnp: true })] as never }),
  ];
  const all = schSearchTab("symbols", sheets, "", ctx());
  assert.deepEqual(
    all.map((r) => r.cells[0]),
    ["R1", "U1B", "R7"],
    "an empty query lists every symbol; the unit after a reference of several units"
  );
  assert.deepEqual(all[0]!.cells, ["R1", "10k", "R_0603", "1", "1.0000 mm", "2.5400 mm", " ", "X", " ", " ", "Device:R", "Resistor"]);
  assert.equal(all[2]!.cells[3], "2");
  assert.equal(all[2]!.cells[9], "X");
  // a hidden field only with Search Hidden Fields (on by default)
  assert.equal(schSearchTab("symbols", sheets, "rc0603", ctx()).length, 3);
  assert.equal(schSearchTab("symbols", sheets, "rc0603", ctx({ searchHiddenFields: false })).length, 0);
  // the reference with its unit, and a wildcard / regex (PERMISSIVE)
  assert.deepEqual(schSearchTab("symbols", sheets, "u1b", ctx()).map((r) => r.item), ["U1"]);
  assert.deepEqual(schSearchTab("symbols", sheets, "R?", ctx({ searchHiddenFields: false })).map((r) => r.item), ["R1", "R7"]);
  assert.deepEqual(schSearchTab("symbols", sheets, "R?", ctx()).map((r) => r.item), ["R1", "U1", "R7"], "the hidden Footprint R_0603 has an R and a character");
  assert.deepEqual(schSearchTab("symbols", sheets, "/^R7$", ctx()).map((r) => r.item), ["R7"]);
  // the library's description and keywords only with Search Metadata (the Description field is hidden and matches with hidden fields)
  assert.equal(schSearchTab("symbols", sheets, "res", ctx({ searchHiddenFields: false })).length, 0);
  assert.equal(schSearchTab("symbols", sheets, "res", ctx({ searchHiddenFields: false, searchMetadata: true })).length, 2);
});

test("Power, Text, Labels and Groups", () => {
  const s = sheet([], "1", {
    power_symbols: [{ id: "#PWR01", lib_id: "power:GND", at: [5_080_000, 0], rot: 0, net: "GND", pin: {} }] as never,
    texts: [{ id: "txt_1", content: "Note ${X}", shown: "Note 5", at: [0, 0], angle: 0, size_nm: 1_270_000 }] as never,
    graphics: [{ id: "tb_1", shape: { type: "text_box", start: { x: 1_000_000, y: 0 }, end: { x: 2_000_000, y: 0 }, text: "Box note", size_nm: 1_270_000 } }, { id: "dl_1", shape: { type: "directive", at: { x: 0, y: 0 }, pin_length_nm: 0 } }] as never,
    labels: [{ id: "lbl_1", net: "SDA{slash}SCL", at: [0, 0], scope: "global", shape: null }] as never,
    groups: [{ id: "grp_1", name: "Input", member_ids: ["lbl_1"] }] as never,
  });
  assert.deepEqual(schSearchTab("power", [s], "gnd", ctx()).map((r) => r.cells), [["#PWR01", "GND", "1", "5.0800 mm", "0.0000 mm"]]);
  assert.equal(schSearchTab("power", [s], "pwr", ctx({ searchHiddenFields: false })).length, 0, "its Reference is hidden");
  assert.deepEqual(schSearchTab("text", [s], "note", ctx()).map((r) => [r.cells[0], r.cells[1]]), [["Text", "Note 5"], ["Text Box", "Box note"]]);
  assert.equal(schSearchTab("text", [s], "note 5", ctx()).length, 0, "a text is matched by its text as stored");
  assert.deepEqual(schSearchTab("labels", [s], "sda/", ctx()).map((r) => r.cells.slice(0, 2)), [["Global", "SDA/SCL"]]);
  assert.deepEqual(schSearchTab("labels", [s], "", ctx()).map((r) => r.cells[0]), ["Global", "Directive"]);
  const groups = schSearchTab("groups", [s], "inp", { ...ctx(), groupCentre: () => [2_540_000, 0] });
  assert.deepEqual(groups.map((r) => r.cells), [["Input", "1", "2.5400 mm", "0.0000 mm"]]);
});

test("the sort, and the rows a selection goes to", () => {
  const sheets = [sheet([], "1", { symbols: [sym("R10", 0), sym("R9", 1), sym("R1", 2)] as never }), sheet(["s"], "2", { symbols: [sym("C1", 0)] as never })];
  const rows = schSearchTab("symbols", sheets, "", ctx());
  assert.deepEqual(sortSchSearchRows(rows, null, true).map((r) => r.cells[0]), ["C1", "R1", "R9", "R10"]);
  assert.deepEqual(sortSchSearchRows(rows, 0, false).map((r) => r.cells[0]), ["R10", "R9", "R1", "C1"]);
  assert.deepEqual(schSearchTarget(rows.slice(0, 2)), { sheet: [], items: ["R10", "R9"] });
  assert.equal(schSearchTarget(rows), null, "rows of two sheets select nothing (`allHitsOnSamePage`)");
});

test("Find's ERC markers: the ones the canvas draws on the sheet, by their message", () => {
  const s = sheet([], "1", { symbols: [sym("R1", 1_000_000)] as never }).sch;
  const v = (location: string | null, hint: string): ErcViolation => ({ check: "pin_not_connected", severity: "error", location, hint });
  const markers = ercFindMarkers([v("R1", "Pin not connected"), v("Q9", "elsewhere"), v("123,456", "Unconnected wire endpoint")], s);
  assert.deepEqual(
    markers.map((m) => [m.key, m.text]),
    [
      ["erc:0", "Pin not connected"],
      ["erc:2", "Unconnected wire endpoint"],
    ]
  );
  assert.deepEqual(markers[1]!.at, [123, 456]);
  assert.equal(ercMarkerIndex("erc:2"), 2);
  assert.equal(ercMarkerIndex("R1"), null);
});

test("Find over every sheet: each sheet's markers, those the canvas draws there, each with its sheet (UI-LEFT-3 S3-34)", () => {
  const root = sheet([], "1", { symbols: [sym("R1", 1_000_000)] as never }).sch;
  const sub = sheet(["s1"], "2", { symbols: [sym("Q9", 2_000_000)] as never }).sch;
  const v = (location: string | null, hint: string): ErcViolation => ({ check: "pin_not_connected", severity: "error", location, hint });
  const markers = ercFindMarkersOfSheets([v("R1", "Pin not connected"), v("Q9", "Pin not driven")], [
    { path: [], sch: root },
    { path: ["s1", "s2"], sch: sub },
  ]);
  assert.deepEqual(
    markers.map((m) => [m.key, m.sheet]),
    [
      ["erc:0", ""],
      ["erc:1", "s1/s2"],
    ]
  );
  assert.deepEqual(ercFindMarkersOfSheets([], [{ path: [], sch: root }]), [], "no report, no markers");
});
