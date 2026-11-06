import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState } from "../api/types";
import { DEFAULT_SEARCH_PANE_SETTINGS, clickSearchColumn, drillEntries, effectiveNetClassName, parseSearchPaneSettings, searchRowsText, searchTab, sortSearchRows, SEARCH_TABS, toggleSearchSetting, type SearchContext } from "./searchHandlers";

const pad = (num: string, x: number, net: string | null, over: Record<string, unknown> = {}) => ({ num, net, x, y: 0, w: 1_000_000, h: 1_000_000, round: false, th: false, ...over });
const field = (name: string, text: string, visible = true) => ({ id: `x:${name}`, name, text, x: 0, y: 0, angle: 0, w: 1, h: 1, thickness: 1, layer: "F.SilkS", visible, halign: 0, valign: 0, mirror: false, bold: false, italic: false, upright: false, knockout: false, lx: 0, ly: 0, langle: 0, custom: false });

function board(): BoardState {
  return {
    name: "b",
    dir: "/b",
    outline: null,
    layers: ["F.Cu", "B.Cu"],
    snap: 1,
    parts: [
      { ref: "R10", value: "10k", package: null, mpn: null, footprint: "Resistor_SMD:R_0603_1608Metric", block: null, placed: true, size: null, at: [10_000_000, 5_000_000], side: "top", lib_description: "Resistor, small", keywords: "resistor", pads: [pad("1", 9_000_000, "GND"), pad("2", 11_000_000, "/sig/A")], fields: [field("Reference", "R10"), field("Value", "10k"), field("MPN", "RC0603", false)] },
      { ref: "R2", value: "4k7", package: null, mpn: null, footprint: "Resistor_SMD:R_0603_1608Metric", block: null, placed: true, size: null, at: [20_000_000, 5_000_000], side: "bottom", pads: [pad("1", 19_000_000, "GND"), pad("2", 21_000_000, "N{slash}C")] },
      { ref: "J1", value: "Conn", package: null, mpn: null, footprint: "Conn:Pin", block: null, placed: true, size: null, at: [0, 0], side: "top", pads: [pad("1", 0, "GND", { th: true, kind: "through_hole", drill: 1_000_000 }), pad("2", 2_540_000, "/sig/A", { th: true, kind: "through_hole", drill: 1_000_000 }), pad("MH", 5_000_000, null, { th: true, kind: "non_plated_hole", drill: 3_200_000 })] },
      { ref: "U9", value: "X", package: null, mpn: null, footprint: null, block: null, placed: false, size: null },
    ],
    rules: [],
    routing: {
      tracks: [],
      vias: [
        { id: "via_1", net: "GND", x: 1, y: 1, d: 600_000, drill: 300_000, from: "B.Cu", to: "F.Cu" },
        { id: "via_2", net: "GND", x: 2, y: 2, d: 600_000, drill: 300_000, from: "F.Cu", to: "B.Cu" },
      ],
      zones: [
        { id: "zone_a", net: "GND", layer: "F.Cu", layers: ["F.Cu", "B.Cu"], name: "Ground", priority: 1, outline: [[0, 0], [10_000_000, 0], [10_000_000, 10_000_000], [0, 10_000_000]], teardrop: false, is_rule_area: false },
        { id: "zone_k", net: "", layer: "F.Cu", name: "Keepout", priority: 0, outline: [[0, 0], [2_000_000, 0], [2_000_000, 2_000_000], [0, 2_000_000]], teardrop: false, is_rule_area: true },
      ],
    },
    drawings: {
      shapes: [],
      texts: [{ id: "txt_1", content: "Rev ${REVISION}", x: 3_000_000, y: 4_000_000, angle: 0, layer: "F.SilkS", size: 1, stroke_width: 1, justify: "center", mirror: false }],
      groups: [
        { id: "grp_1", name: "Power", member_ids: ["R10"] },
        { id: "grp_t", name: "Tune", member_ids: [], generator: "tuning_pattern" },
      ],
      dimensions: [],
      dimension_settings: {},
    },
    checks: [],
    activity: [],
    job: "",
  } as unknown as BoardState;
}

const ctx = (over: Partial<SearchContext> = {}): SearchContext => ({ units: "mm", settings: DEFAULT_SEARCH_PANE_SETTINGS, filledArea: new Map([["zone_a", 50_000_000_000_000]]), ratsnestNets: new Set(["/sig/A"]), groupCentre: new Map([["grp_1", [1_000_000, 2_000_000] as const]]), ...over });

test("the seven tabs, in the order the pane adds them, with KiCad's column names", () => {
  assert.deepEqual(SEARCH_TABS.map((t) => t.name), ["Footprints", "Zones", "Nets", "Ratsnest", "Text", "Groups", "Drills"]);
  assert.deepEqual(SEARCH_TABS[0]!.columns.map((c) => c.name), ["Reference", "Value", "Layer", "X", "Y", "Library Link", "Library Description"]);
  assert.deepEqual(SEARCH_TABS[6]!.columns.map((c) => c.name), ["Count", "Shape", "X Size", "Y Size", "Plated", "Via/Pad", "Start Layer", "Stop Layer"]);
});

test("Footprints: every placed footprint for an empty query; by a field (a hidden one with Search Hidden Fields), by metadata only with Search Metadata", () => {
  const b = board();
  assert.deepEqual(searchTab("footprints", b, "", ctx()).map((r) => r.id), ["R10", "R2", "J1"], "the unplaced U9 is not on the board");
  assert.deepEqual(searchTab("footprints", b, "rc06", ctx()).map((r) => r.id), ["R10"], "the hidden MPN field");
  assert.deepEqual(searchTab("footprints", b, "rc06", ctx({ settings: { ...DEFAULT_SEARCH_PANE_SETTINGS, searchHiddenFields: false } })).map((r) => r.id), []);
  assert.deepEqual(searchTab("footprints", b, "1608", ctx()).map((r) => r.id), [], "the library link is metadata");
  assert.deepEqual(searchTab("footprints", b, "1608", ctx({ settings: { ...DEFAULT_SEARCH_PANE_SETTINGS, searchMetadata: true } })).map((r) => r.id), ["R10", "R2"]);
  assert.deepEqual(searchTab("footprints", b, "R*0", ctx()).map((r) => r.id), ["R10"], "a wildcard");
  const r10 = searchTab("footprints", b, "R10", ctx())[0]!;
  assert.deepEqual(r10.cells, ["R10", "10k", "F.Cu", "10.0000 mm", "5.0000 mm", "Resistor_SMD:R_0603_1608Metric", "Resistor, small"]);
  assert.equal(searchTab("footprints", b, "R2", ctx())[0]!.cells[2], "B.Cu");
});

test("the rows sort by the first column with StrNumCmp unless a column is chosen; the same heading again turns the order round", () => {
  const rows = searchTab("footprints", board(), "", ctx());
  assert.deepEqual(sortSearchRows(rows, "footprints", null, true).map((r) => r.id), ["J1", "R2", "R10"], "R2 before R10");
  // StrNumCmp, case-blind: the letter C after the digits, 10k after 4k7
  assert.deepEqual(sortSearchRows(rows, "footprints", 1, false).map((r) => r.cells[1]), ["Conn", "10k", "4k7"]);
  assert.deepEqual(clickSearchColumn({ col: null, ascending: true }, 1), { col: 1, ascending: true });
  assert.deepEqual(clickSearchColumn({ col: 1, ascending: true }, 1), { col: 1, ascending: false });
});

test("Zones: by name; every layer of the zone; the filled area, or a rule area's outline area", () => {
  const rows = searchTab("zones", board(), "", ctx());
  assert.deepEqual(rows.map((r) => r.id), ["zone_a", "zone_k"]);
  assert.deepEqual(rows[0]!.cells, ["Ground", "GND", "F.Cu,B.Cu", "1", "0.0000 mm", "0.0000 mm", "50.00 mm²"], "an area in mm is trimmed to 2-1/2 digits");
  assert.equal(rows[1]!.cells[6], "4.00 mm²");
  assert.deepEqual(searchTab("zones", board(), "keep", ctx()).map((r) => r.id), ["zone_k"]);
});

test("Nets: the unconnected net as No Net, the names unescaped, the class; Ratsnest: the matching nets with an unrouted connection", () => {
  const rows = searchTab("nets", board(), "", ctx({ netClassName: (n) => (n === "GND" ? "Power" : "Default") }));
  assert.deepEqual(rows.map((r) => r.cells), [["No Net", ""], ["/sig/A", "Default"], ["GND", "Power"], ["N/C", "Default"]]);
  assert.deepEqual(searchTab("nets", board(), "gnd", ctx()).map((r) => r.id), ["GND"]);
  assert.deepEqual(searchTab("ratsnest", board(), "", ctx()).map((r) => r.id), ["/sig/A"]);
  assert.deepEqual(searchTab("ratsnest", board(), "gnd", ctx()).map((r) => r.id), []);
  assert.equal(effectiveNetClassName("X", "Power"), "Power");
  assert.equal(effectiveNetClassName("X", "Power,Default"), "Effective for net: X");
});

test("Text and Groups: the text as stored, a generator left out, a group at its box's centre", () => {
  assert.deepEqual(searchTab("text", board(), "rev", ctx())[0]!.cells, ["Text", "Rev ${REVISION}", "F.SilkS", "3.0000 mm", "4.0000 mm"]);
  assert.deepEqual(searchTab("groups", board(), "", ctx()).map((r) => r.cells), [["Group", "Power", "1.0000 mm", "2.0000 mm"]]);
});

test("Drills: one row per kind of hole with its count, the most numerous first; a row is found by any cell", () => {
  const e = drillEntries(board());
  assert.deepEqual(e.map((x) => [x.qty, x.xSize, x.isPlated, x.isPad, x.startLayer, x.stopLayer]), [
    [2, 1_000_000, true, true, "F.Cu", "B.Cu"],
    [2, 300_000, true, false, "F.Cu", "B.Cu"],
    [1, 3_200_000, false, true, null, null],
  ]);
  assert.deepEqual(e[1]!.items, ["via_1", "via_2"], "a via's layers are in stack order whichever way it was drawn");
  const rows = searchTab("drills", board(), "npth", ctx());
  assert.equal(rows.length, 1);
  assert.deepEqual(rows[0]!.cells, ["1", "Round", "3.2000 mm", "3.2000 mm", "NPTH", "Pad", "N/A", "N/A"]);
  assert.deepEqual(rows[0]!.select, ["J1.MH"]);
  assert.deepEqual(sortSearchRows(searchTab("drills", board(), "", ctx()), "drills", 2, true).map((r) => r.drill!.xSize), [300_000, 1_000_000, 3_200_000]);
});

test("the pane's menu: Zoom and Pan to Selection are one setting; the settings read back with their defaults", () => {
  const s = DEFAULT_SEARCH_PANE_SETTINGS;
  assert.equal(s.selectionZoom, "pan");
  assert.equal(toggleSearchSetting(s, "zoom").selectionZoom, "zoom");
  assert.equal(toggleSearchSetting(s, "pan").selectionZoom, "none");
  assert.equal(toggleSearchSetting(s, "hidden").searchHiddenFields, false);
  assert.deepEqual(parseSearchPaneSettings({ selectionZoom: "zoom", searchMetadata: true, searchHiddenFields: "no" }), { selectionZoom: "zoom", searchHiddenFields: true, searchMetadata: true });
  assert.equal(searchRowsText([{ id: "a", cells: ["R1", "10k"], select: [] }]), "R1\t10k\n");
});
