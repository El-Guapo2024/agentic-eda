import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_NET_INSPECTOR_OPTIONS,
  DEFAULT_NET_INSPECTOR_SETTINGS,
  addGroupRule,
  autofitNetInspectorColumns,
  autosizeNetInspectorColumn,
  moveNetInspectorColumn,
  netInspectorColumnMovable,
  netInspectorColumnWidths,
  netInspectorDefaultColumnWidth,
  netInspectorHeaderMenu,
  netInspectorMinColumnWidth,
  orderedNetInspectorColumns,
  resizeNetInspectorColumn,
  addNetStep,
  deleteGroupQuestion,
  deleteNetQuestion,
  formatDelay,
  lineKey,
  netInspectorCell,
  netInspectorColumns,
  netInspectorConfigMenu,
  netInspectorContextMenu,
  netInspectorGroupCell,
  netInspectorReport,
  netInspectorRows,
  netInspectorTree,
  netShown,
  parseNetInspectorSettings,
  removeGroupRule,
  renameParts,
  renameStep,
  sortBy,
  toggleExpanded,
  toggleSetting,
  type NetInspectorMenuEntry,
  type NetInspectorRow,
  type NetInspectorSettings,
} from "./netInspector";

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

const S = (over: Partial<NetInspectorSettings> = {}): NetInspectorSettings => ({ ...DEFAULT_NET_INSPECTOR_SETTINGS, ...over });
const labels = (m: readonly NetInspectorMenuEntry[]) => m.map((e) => (e.kind === "separator" ? "-" : e.kind === "submenu" ? `>${e.label}` : `${e.label}${e.enabled ? "" : " (off)"}${e.checked === undefined ? "" : e.checked ? " [x]" : " [ ]"}`));

test("Group by Netclass: a group per class that has a listed net, before the nets, its numbers the sums; open only when expanded", () => {
  const rows = [row("A", { netclass: "Power", track_length: 2, pad_count: 1 }), row("B", { netclass: "Power", track_length: 3, pad_count: 2 }), row("C", { netclass: "Default", track_length: 1 })];
  const closed = netInspectorTree(rows, S({ groupByNetclass: true }));
  assert.deepEqual(closed.map(lineKey), ["group:netclass:Default", "group:netclass:Power"], "the groups by name, nothing open");
  const power = closed[1]!;
  assert.equal(power.kind, "group");
  if (power.kind !== "group") return;
  assert.equal(netInspectorGroupCell(power.group, "name", "mm"), "Netclass: Power");
  assert.equal(netInspectorGroupCell(power.group, "netclass", "mm"), "");
  assert.equal(netInspectorGroupCell(power.group, "pad_count", "mm"), "3");
  assert.equal(power.group.totals.track_length, 5);
  const open = netInspectorTree(rows, S({ groupByNetclass: true, expandedRows: ["Power"] }));
  assert.deepEqual(open.map(lineKey), ["group:netclass:Default", "group:netclass:Power", "net:A", "net:B"]);
  // without grouping: the nets alone
  assert.deepEqual(netInspectorTree(rows, S()).map(lineKey), ["net:A", "net:B", "net:C"]);
});

test("custom groups: every rule a group, shown when empty; a net in the first rule that finds it, before Group by Netclass", () => {
  const rows = [row("/usb/D+", { netclass: "USB" }), row("/usb/D-", { netclass: "USB" }), row("GND", { netclass: "Power" }), row("VBUS", { netclass: "Power" })];
  const s = S({ customGroupRules: ["/usb/", "NOPE", "/^V/"], groupByNetclass: true, expandedRows: ["/usb/", "/^V/", "Power"] });
  const lines = netInspectorTree(rows, s);
  assert.deepEqual(lines.map(lineKey), ["group:custom:/^V/", "net:VBUS", "group:custom:/usb/", "net:/usb/D+", "net:/usb/D-", "group:custom:NOPE", "group:netclass:Power", "net:GND"]);
  const usb = lines.find((l) => l.kind === "group" && l.group.name === "/usb/");
  assert.ok(usb && usb.kind === "group" && netInspectorGroupCell(usb.group, "name", "mm") === "Custom: /usb/");
  assert.deepEqual(addGroupRule(s, "/usb/").customGroupRules, s.customGroupRules, "a rule already there is not added twice");
  assert.deepEqual(addGroupRule(S(), "A{slash}B").customGroupRules, ["A/B"], "the pattern typed is unescaped");
  assert.deepEqual(removeGroupRule(s, "NOPE").customGroupRules, ["/usb/", "/^V/"]);
  assert.deepEqual(removeGroupRule(s, "Power").customGroupRules, s.customGroupRules, "a netclass group has no rule");
});

test("a heading's click sorts by its column (again: the other way); groups sort among themselves before the nets", () => {
  const rows = [row("A", { netclass: "X", pad_count: 5 }), row("B", { netclass: "Y", pad_count: 1 }), row("C", { netclass: "Y", pad_count: 1 })];
  let s = sortBy(S({ groupByNetclass: true }), "pad_count");
  assert.deepEqual([s.sortingColumn, s.sortAscending], ["pad_count", true]);
  assert.deepEqual(netInspectorTree(rows, s).map(lineKey), ["group:netclass:Y", "group:netclass:X"]);
  s = sortBy(s, "pad_count");
  assert.equal(s.sortAscending, false);
  assert.deepEqual(netInspectorTree(rows, s).map(lineKey), ["group:netclass:X", "group:netclass:Y"]);
  assert.deepEqual(toggleExpanded(toggleExpanded(s, "Y"), "Y").expandedRows, []);
});

test("Show Time Domain Details: the length columns are KiCad's delay columns, in picoseconds", () => {
  const cols = netInspectorColumns(["F.Cu", "B.Cu"], true);
  assert.deepEqual(cols.map((c) => c.label), ["Name", "Netclass", "Total Delay", "Via Count", "Via Delay", "Track Delay", "Die Delay", "Pad Count", "F.Cu", "B.Cu"]);
  assert.deepEqual(cols.slice(2, 7).map((c) => c.csv), ["Net Delay", "Via Count", "Via Delay", "Track Delay", "Die Delay"]);
  const r = row("N", { track_length: 10_000_000, track_delay: 61_234_000, total_delay: 62_000_000, layer_delays: { "F.Cu": 61_234_000 } });
  assert.equal(netInspectorCell(r, "track_length", "mm", false, true), "61.23 ps");
  assert.equal(netInspectorCell(r, "layer:F.Cu", "mm", false, true), "61.23 ps");
  assert.equal(netInspectorCell(r, "total_length", "mm", true, true), "62.00", "the report has no unit label");
  assert.equal(formatDelay(0), "0.00 ps");
  assert.equal(formatDelay(1_000), "1.000e-03 ps", "shows only zeros: %.3e");
  // the report keeps the user's length units in the header, as KiCad writes them
  assert.ok(netInspectorReport([r], "mm", cols, true).startsWith('"Net Name";"Netclass";"Net Delay (mm)";'));
  // sorted by the delays
  const t = netInspectorTree([row("A", { track_delay: 5 }), row("B", { track_delay: 1 })], S({ showTimeDomainDetails: true, sortingColumn: "track_length" }));
  assert.deepEqual(t.map(lineKey), ["net:B", "net:A"]);
});

test("the configure menu and the rows' menu, as KiCad builds them", () => {
  const cols = netInspectorColumns(["F.Cu", "B.Cu"]);
  const config = netInspectorConfigMenu(S({ customGroupRules: ["x"] }), { count: 1, oneIsGroup: true }, cols);
  assert.deepEqual(labels(config), [
    "Filter by Net Name [x]",
    "Filter by Netclass [x]",
    "-",
    "Group by Netclass [ ]",
    "-",
    "Add Custom Group...",
    "Remove Selected Custom Group",
    "Remove All Custom Groups",
    "-",
    "Show Zero Pad Nets [ ]",
    "Show Unconnected Nets [ ]",
    "-",
    "Show Time Domain Details [ ]",
    "-",
    "Save Net Inspector Report...",
    "-",
    ">Show / Hide Columns",
  ]);
  const sub = config[config.length - 1]!;
  assert.ok(sub.kind === "submenu" && labels(sub.items).join("|") === "Netclass [x]|Total Length [x]|Via Count [x]|Via Length [x]|Track Length [x]|Die Length [x]|Pad Count [x]|-|F.Cu [x]|B.Cu [x]");
  assert.deepEqual(labels(netInspectorConfigMenu(S(), { count: 0, oneIsGroup: false }, cols)).slice(6, 8), ["Remove Selected Custom Group (off)", "Remove All Custom Groups (off)"]);
  assert.deepEqual(labels(netInspectorContextMenu({ count: 1, oneIsGroup: false }, false)), [
    "Highlight Selected Net",
    "Clear Net Highlighting (off)",
    "-",
    "Rename Selected Net...",
    "Delete Selected Net",
    "-",
    "Add Net...",
    "-",
    "Remove Selected Custom Group (off)",
  ]);
  assert.deepEqual(labels(netInspectorContextMenu({ count: 2, oneIsGroup: false }, true)).slice(0, 5), ["Highlight Selected Nets", "Clear Net Highlighting", "-", "Rename Selected Net... (off)", "Delete Selected Nets"]);
  assert.deepEqual(labels(netInspectorContextMenu({ count: 0, oneIsGroup: false }, false)).slice(0, 5), ["Highlight Selected Net (off)", "Clear Net Highlighting (off)", "-", "Rename Selected Net... (off)", "Delete Selected Net (off)"]);
  assert.equal(toggleSetting(S(), "group_by_netclass")!.groupByNetclass, true);
  assert.deepEqual(toggleSetting(S(), "hide_column:via_count")!.hiddenColumns, ["via_count"]);
  assert.deepEqual(toggleSetting(S({ customGroupRules: ["a"] }), "remove_groups")!.customGroupRules, []);
  assert.equal(toggleSetting(S(), "add_net"), null);
});

test("Rename Net: the short name in the dialog, the path kept, KiCad's messages and its loop", () => {
  assert.deepEqual(renameParts("/power/V{slash}IN"), { path: "/power/", short: "V/IN" });
  assert.deepEqual(renameParts("GND"), { path: "", short: "GND" });
  const inUse = (n: string) => ["/power/VCC", "GND", "VDD"].includes(n);
  assert.deepEqual(renameStep("/power/VIN", "VIN", "VIN", inUse), { kind: "done" });
  assert.deepEqual(renameStep("/power/VIN", "", "VIN", inUse), { kind: "error", message: "Net name cannot be empty.", retry: "" });
  assert.deepEqual(renameStep("/power/VIN", "VCC", "VIN", inUse), { kind: "error", message: "Net name 'VCC' is already in use.", retry: "" });
  assert.deepEqual(renameStep("/power/VIN", "VDD", "VIN", inUse), { kind: "error", message: "Net name 'VDD' is already in use.", retry: "" }, "the short name is looked up too");
  assert.deepEqual(renameStep("/power/VIN", "V/OUT", "VIN", inUse), { kind: "rename", to: "/power/V{slash}OUT" });
  assert.deepEqual(renameStep("/power/VIN", "", "", inUse), { kind: "done" }, "after an error an empty name closes the dialog");
  assert.deepEqual(addNetStep("", inUse), { kind: "done" });
  assert.deepEqual(addNetStep("GND", inUse), { kind: "error", message: "Net name 'GND' is already in use.", retry: "" });
  assert.deepEqual(addNetStep("NEW/1", inUse), { kind: "rename", to: "NEW/1" }, "Add Net keeps the name as typed");
});

test("Delete: a net with pads is asked about, a group asks for all of its nets", () => {
  assert.equal(deleteNetQuestion(row("A{slash}B", { pad_count: 2 })), "Net 'A/B' is in use.  Delete anyway?");
  assert.equal(deleteNetQuestion(row("X", { pad_count: 0 })), null);
  const [g] = netInspectorTree([row("A", { netclass: "P" })], S({ groupByNetclass: true }));
  assert.ok(g && g.kind === "group" && deleteGroupQuestion(g.group) === "Delete all nets in group 'P'?");
  const [empty] = netInspectorTree([], S({ customGroupRules: ["Z"] }));
  assert.ok(empty && empty.kind === "group" && deleteGroupQuestion(empty.group) === null);
});

test("the settings read back with KiCad's defaults; Show Unconnected Nets is not restored", () => {
  assert.deepEqual(parseNetInspectorSettings(null), DEFAULT_NET_INSPECTOR_SETTINGS);
  const back = parseNetInspectorSettings({ filter: "clk", groupByNetclass: true, showUnconnectedNets: true, sortingColumn: "layer:F.Cu", sortAscending: false, hiddenColumns: ["via_count", "bogus"], customGroupRules: ["a", "a", ""], expandedRows: ["a"] });
  assert.equal(back.filter, "clk");
  assert.equal(back.groupByNetclass, true);
  assert.equal(back.showUnconnectedNets, false);
  assert.equal(back.sortingColumn, "layer:F.Cu");
  assert.deepEqual(back.hiddenColumns, ["via_count"]);
  assert.deepEqual(back.customGroupRules, ["a"]);
});

// ------------------------------------------------------------------------------------------------------------------- the columns' order and widths (PP13)

/** A monospace stand-in for `GetTextExtent`: 7 px a character. */
const mono = (t: string) => t.length * 7;

test("the columns in the kept order (col_order): the ones it does not name after them, a stored column the board lost left out", () => {
  const cols = netInspectorColumns(["F.Cu", "B.Cu"]);
  const keys = (o: readonly { key: string }[]) => o.map((c) => c.key).join(",");
  assert.equal(keys(orderedNetInspectorColumns(cols, [])), "name,netclass,total_length,via_count,via_length,track_length,die_length,pad_count,layer:F.Cu,layer:B.Cu");
  assert.equal(keys(orderedNetInspectorColumns(cols, ["pad_count", "name", "netclass", "layer:In1.Cu"])), "pad_count,name,netclass,total_length,via_count,via_length,track_length,die_length,layer:F.Cu,layer:B.Cu");
  const four = netInspectorColumns(["F.Cu", "B.Cu", "In1.Cu"]);
  const kept = ["name", "netclass", "total_length", "via_count", "via_length", "track_length", "die_length", "pad_count", "layer:B.Cu", "layer:F.Cu"] as const;
  assert.equal(keys(orderedNetInspectorColumns(four, [...kept])), "name,netclass,total_length,via_count,via_length,track_length,die_length,pad_count,layer:B.Cu,layer:F.Cu,layer:In1.Cu", "a new layer's column at the end");
  // corrupted settings: a column stored twice is shown once
  assert.deepEqual(parseNetInspectorSettings({ colOrder: ["netclass", "netclass", "bogus", "name"], colWidths: { name: 120.4, bogus: 3, netclass: -1 } }).colOrder, ["netclass", "name"]);
  assert.deepEqual(parseNetInspectorSettings({ colWidths: { name: 120.4, bogus: 3, netclass: -1 } }).colWidths, { name: 120 });
});

test("a heading dragged to another place (wxDATAVIEW_COL_REORDERABLE): every column but the Name moves", () => {
  const cols = netInspectorColumns(["F.Cu", "B.Cu"]);
  const s = DEFAULT_NET_INSPECTOR_SETTINGS;
  assert.equal(netInspectorColumnMovable("name"), false);
  assert.equal(moveNetInspectorColumn(s, cols, "name", 5), s, "the Name stays");
  const a = moveNetInspectorColumn(s, cols, "pad_count", 1);
  assert.deepEqual(a.colOrder.slice(0, 3), ["name", "pad_count", "netclass"], "before the column at position 1");
  const b = moveNetInspectorColumn(a, cols, "netclass", 99);
  assert.equal(b.colOrder.at(-1), "netclass", "past the last: at the end");
  assert.equal(moveNetInspectorColumn(b, cols, "netclass", b.colOrder.length - 1), b, "dropped where it is: no change");
  const c = moveNetInspectorColumn(b, cols, "layer:F.Cu", 0);
  assert.equal(c.colOrder[0], "layer:F.Cu", "another column may go before the Name");
});

test("the widths: adjustListColumnSizes' with none kept, the kept one never under getMinColumnWidth, a dragged edge kept", () => {
  const cols = netInspectorColumns(["F.Cu"]);
  const by = (k: string) => cols.find((c) => c.key === k)!;
  // the heading and its margins (the Name's expander room: 30, else 15)
  assert.equal(netInspectorMinColumnWidth(by("name"), mono), 4 * 7 + 30);
  assert.equal(netInspectorMinColumnWidth(by("via_count"), mono), 9 * 7 + 15);
  // "MMMMMMMMMMMM" for the Name and the Netclass, "000" for the counts, "00000,000 mm" for the lengths, or the heading when wider
  assert.equal(netInspectorDefaultColumnWidth(by("name"), mono), 12 * 7 + 30);
  assert.equal(netInspectorDefaultColumnWidth(by("netclass"), mono), 12 * 7 + 15);
  assert.equal(netInspectorDefaultColumnWidth(by("via_count"), mono), 9 * 7 + 15, "Via Count's heading is wider than 000");
  assert.equal(netInspectorDefaultColumnWidth(by("total_length"), mono), 12 * 7 + 15);
  assert.equal(netInspectorDefaultColumnWidth(by("layer:F.Cu"), mono), 12 * 7 + 15);
  const s = resizeNetInspectorColumn(resizeNetInspectorColumn(DEFAULT_NET_INSPECTOR_SETTINGS, "netclass", 200.6), "pad_count", 2);
  assert.deepEqual(s.colWidths, { netclass: 201, pad_count: 16 }, "never under the list's own limit while dragged");
  const w = netInspectorColumnWidths(cols, s, mono);
  assert.equal(w.netclass, 201);
  assert.equal(w.pad_count, 9 * 7 + 15, "restored no narrower than getMinColumnWidth");
  assert.equal(w.name, 12 * 7 + 30, "a column with no width kept: adjustListColumnSizes'");
});

test("Auto-fit Column and Auto-fit All Columns (autosizeColumn): the widest value of every item, a closed group's nets too, the indent on the Name", () => {
  const rows = [row("/power/VERY_LONG_NET_NAME", { netclass: "Power", pad_count: 12345 }), row("A", { netclass: "Default" })];
  const s: NetInspectorSettings = { ...DEFAULT_NET_INSPECTOR_SETTINGS, groupByNetclass: true };
  const cols = netInspectorColumns([]);
  const by = (k: string) => cols.find((c) => c.key === k)!;
  // the groups are closed; their nets still count, with the indent: "/power/VERY_LONG_NET_NAME" (25 characters) + 30 + 18
  assert.equal(autosizeNetInspectorColumn(by("name"), rows, s, "mm", mono), 25 * 7 + 30 + 18);
  assert.equal(autosizeNetInspectorColumn(by("pad_count"), rows, s, "mm", mono), 9 * 7 + 15, "no value wider than the heading: the minimum");
  assert.equal(autosizeNetInspectorColumn(by("pad_count"), rows, { ...s, hiddenColumns: ["pad_count"] }, "mm", mono), null, "a hidden column is left as it is");
  const one = autofitNetInspectorColumns(s, cols, rows, "mm", mono, "netclass");
  assert.deepEqual(Object.keys(one.colWidths), ["netclass"], "Auto-fit Column: that column only");
  const every = autofitNetInspectorColumns(s, cols, rows, "mm", mono);
  assert.equal(Object.keys(every.colWidths).length, cols.length);
  assert.equal(every.colWidths.name, 25 * 7 + 30 + 18);
});

test("OnHeaderContextMenu: Auto-fit Column, Auto-fit All Columns, a separator, then Show / Hide Columns' entries in the menu itself", () => {
  const cols = netInspectorColumns(["F.Cu"]);
  const menu = netInspectorHeaderMenu(cols, ["via_count"]);
  const labels = menu.map((e) => (e.kind === "item" ? e.label : e.kind));
  assert.deepEqual(labels.slice(0, 4), ["Auto-fit Column", "Auto-fit All Columns", "separator", "Netclass"]);
  assert.ok(!labels.includes("Name"), "the Name is never hidden");
  const via = menu.find((e) => e.kind === "item" && e.label === "Via Count");
  assert.ok(via && via.kind === "item" && via.checked === false);
});
