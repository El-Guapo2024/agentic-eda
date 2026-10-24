import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_NET_INSPECTOR_OPTIONS, netInspectorCell, netInspectorReport, netInspectorRows, netShown, type NetInspectorRow } from "./netInspector";

const row = (name: string, over: Partial<NetInspectorRow> = {}): NetInspectorRow => ({ name, netclass: "Default", total_length: 0, via_count: 0, via_length: 0, track_length: 0, die_length: 0, pad_count: 2, ...over });

test("netFilterMatches: the name or the class, upper-cased, as a substring; unconnected-( and zero-pad nets only when asked", () => {
  const o = DEFAULT_NET_INSPECTOR_OPTIONS;
  assert.ok(netShown(row("GND"), o));
  assert.ok(!netShown(row(""), o), "never the unconnected net");
  assert.ok(!netShown(row("N1", { pad_count: 0 }), o));
  assert.ok(netShown(row("N1", { pad_count: 0 }), { ...o, showZeroPadNets: true }));
  assert.ok(!netShown(row("unconnected-(R1-Pad1)"), o));
  assert.ok(netShown(row("unconnected-(R1-Pad1)"), { ...o, showUnconnectedNets: true }));
  assert.ok(netShown(row("/sig/CLK", { netclass: "Fast" }), { ...o, filter: "fast" }), "by the class");
  assert.ok(!netShown(row("/sig/CLK", { netclass: "Fast" }), { ...o, filter: "fast", filterByNetclass: false }));
  assert.ok(netShown(row("/sig/CLK"), { ...o, filter: "clk" }));
  assert.ok(!netShown(row("/sig/CLK"), { ...o, filter: "clk", filterByNetName: false }));
});

test("a column sorts by its value, the name breaking a tie", () => {
  const rows = [row("B", { track_length: 5 }), row("A", { track_length: 5 }), row("C", { track_length: 1 })];
  assert.deepEqual(netInspectorRows(rows, DEFAULT_NET_INSPECTOR_OPTIONS, "track_length").map((r) => r.name), ["C", "A", "B"]);
  assert.deepEqual(netInspectorRows(rows, DEFAULT_NET_INSPECTOR_OPTIONS, "track_length", false).map((r) => r.name), ["B", "A", "C"]);
  assert.deepEqual(netInspectorRows(rows, DEFAULT_NET_INSPECTOR_OPTIONS).map((r) => r.name), ["A", "B", "C"]);
});

test("lengths read as MessageTextFromValue, and the report is KiCad's CSV", () => {
  const r = row("GND", { total_length: 12_345_600, track_length: 12_345_600, via_count: 1, pad_count: 3 });
  assert.equal(netInspectorCell(r, "track_length", "mm"), "12.3456 mm");
  assert.equal(netInspectorCell(r, "pad_count", "mm"), "3");
  assert.equal(
    netInspectorReport([r], "mm"),
    '"Net Name";"Netclass";"Net Length (mm)";"Via Count";"Via Length (mm)";"Track Length (mm)";"Die Length (mm)";"Pad Count";\n"GND";"Default";12.3456;1;0.0000;12.3456;0.0000;3;\n'
  );
});
