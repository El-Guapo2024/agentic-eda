import { test } from "node:test";
import assert from "node:assert/strict";
import { DEFAULT_NET_INSPECTOR_OPTIONS, netInspectorCell, netInspectorColumns, netInspectorReport, netInspectorRows, netShown, type NetInspectorRow } from "./netInspector";

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

test("one length column per copper layer, in layer-id order (F.Cu, B.Cu, then the inner ones), shown and in the report", () => {
  const cols = netInspectorColumns(["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]);
  assert.deepEqual(cols.slice(8).map((c) => c.label), ["F.Cu", "B.Cu", "In1.Cu", "In2.Cu"]);
  const r = row("GND", { track_length: 3_000_000, layer_lengths: { "F.Cu": 1_000_000, "In1.Cu": 2_000_000 } });
  assert.equal(netInspectorCell(r, "layer:In1.Cu", "mm"), "2.0000 mm");
  assert.equal(netInspectorCell(r, "layer:B.Cu", "mm"), "0.0000 mm");
  const csv = netInspectorReport([r], "mm", netInspectorColumns(["F.Cu", "B.Cu"]));
  assert.ok(csv.startsWith('"Net Name";"Netclass";"Net Length (mm)";"Via Count";"Via Length (mm)";"Track Length (mm)";"Die Length (mm)";"Pad Count";"F.Cu (mm)";"B.Cu (mm)";\n'), csv);
  assert.ok(csv.endsWith(";1.0000;0.0000;\n"), csv);
  assert.deepEqual(netInspectorRows([row("A", { layer_lengths: { "F.Cu": 5 } }), row("B")], DEFAULT_NET_INSPECTOR_OPTIONS, "layer:F.Cu").map((x) => x.name), ["B", "A"]);
});

test("names sort as ValueStringCompare sorts them; the netclass filter reads the effective class's name (r2-pcb-edit B3, B4)", () => {
  assert.deepEqual(netInspectorRows([row("D10"), row("D2")], DEFAULT_NET_INSPECTOR_OPTIONS).map((r) => r.name), ["D2", "D10"]);
  const multi = row("X", { netclass: "A and B", netclass_name: "Effective for net: X" });
  const opts = { ...DEFAULT_NET_INSPECTOR_OPTIONS, filter: "and", filterByNetName: false };
  assert.equal(netShown(multi, opts), false, "the column's 'A and B' is not what the filter reads");
  assert.equal(netShown(multi, { ...opts, filter: "effective" }), true);
});
