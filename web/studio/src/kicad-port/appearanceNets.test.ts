import { test } from "node:test";
import assert from "node:assert/strict";
import {
  classRows,
  hiddenNetsOnLoad,
  hideOtherNetclasses,
  hideOtherNets,
  listedNets,
  netEyeTip,
  netMenu,
  netRows,
  netclassMenu,
  netsContext,
  netsOfClass,
  setNetVisible,
  showAllNetclasses,
  showAllNets,
  showNetclass,
  withNetColor,
} from "./appearanceNets";
import { makeCtx } from "./appearanceFixture";
import type { BoardState } from "../api/types";

test("the net list leaves out the unconnected-(...) nets and sorts by name as wxString does (NET_GRID_TABLE::Rebuild)", () => {
  assert.deepEqual(listedNets(["b", "unconnected-(U1-Pad1)", "A", "", "a", "10"]), ["10", "A", "a", "b"]);
});

test("netsContext reads the nets from the pads, tracks, vias and zones and the classes from Board Setup", () => {
  const board = {
    layers: ["F.Cu", "B.Cu"],
    parts: [{ pads: [{ net: "GND" }, { net: "unconnected-(R1-Pad2)" }, { net: null }] }],
    routing: { tracks: [{ net: "VCC" }], vias: [], zones: [{ net: "GND" }] },
    board_rules: { net_classes: [{ name: "power", nets: ["V*"], priority: 0 }], default_class: { name: "Default", nets: [], priority: 0 } },
  } as unknown as BoardState;
  const ctx = netsContext(board);
  assert.deepEqual(ctx.nets, ["GND", "VCC"]);
  assert.equal(ctx.classOf("VCC"), "power");
  assert.equal(ctx.classOf("GND"), "Default");
  assert.deepEqual(netsContext(null).nets, []);
});

test("net rows: visible unless hidden, the net's own colour, a filter on the name", () => {
  const ctx = makeCtx();
  const rows = netRows(ctx, ["VCC"], { GND: "rgb(1, 2, 3)" });
  assert.deepEqual(rows.map((r) => [r.name, r.visible, r.color]), [
    ["GND", true, "rgb(1, 2, 3)"],
    ["SCL", true, null],
    ["USB_D+", true, null],
    ["USB_D-", true, null],
    ["VCC", false, null],
  ]);
  assert.deepEqual(netRows(ctx, [], {}, "usb").map((r) => r.name), ["USB_D+", "USB_D-"]);
});

test("the eye of a net: show, hide, show all, hide all the others (NET_GRID_TABLE)", () => {
  const ctx = makeCtx();
  assert.deepEqual(setNetVisible([], "GND", false), ["GND"]);
  assert.deepEqual(setNetVisible(["GND"], "GND", false), ["GND"], "hiding twice is hiding once");
  assert.deepEqual(setNetVisible(["GND", "VCC"], "GND", true), ["VCC"]);
  assert.deepEqual(showAllNets(["GND", "VCC", "Gone"], ctx.nets), ["Gone"], "a name the list does not hold is not touched");
  assert.deepEqual(hideOtherNets([], ctx.nets, "GND").sort(), ["SCL", "USB_D+", "USB_D-", "VCC"]);
  assert.deepEqual(hideOtherNets(["GND", "VCC"], ctx.nets, "VCC").sort(), ["GND", "SCL", "USB_D+", "USB_D-"]);
});

test("a net colour is set, replaced or cleared", () => {
  assert.deepEqual(withNetColor({}, "GND", "rgb(1, 2, 3)"), { GND: "rgb(1, 2, 3)" });
  assert.deepEqual(withNetColor({ GND: "rgb(1, 2, 3)" }, "GND", "rgb(4, 5, 6)"), { GND: "rgb(4, 5, 6)" });
  assert.deepEqual(withNetColor({ GND: "rgb(1, 2, 3)", VCC: "rgb(9, 9, 9)" }, "GND", null), { VCC: "rgb(9, 9, 9)" });
});

test("net class rows: Default first and without a colour, the others by name; counts of the nets each owns", () => {
  const ctx = makeCtx();
  const rows = classRows({ ...ctx, classes: [{ name: "zeta", nets: [], priority: 0 }, ...ctx.classes] }, { usb: "rgb(9, 9, 9)", Default: "rgb(1, 1, 1)" }, ["usb"]);
  assert.deepEqual(rows.map((r) => r.name), ["Default", "usb", "zeta"]);
  assert.deepEqual(rows.map((r) => r.isDefault), [true, false, false]);
  assert.equal(rows[0]!.color, null, "the Default class cannot have an override colour");
  assert.equal(rows[1]!.color, "rgb(9, 9, 9)");
  assert.equal(rows[1]!.visible, false);
  assert.deepEqual(rows.map((r) => r.nets), [3, 2, 0]);
  assert.deepEqual(netsOfClass(ctx, "usb"), ["USB_D+", "USB_D-"]);
});

test("hiding a net class hides the ratsnest of every net it owns and remembers the class (showNetclass)", () => {
  const ctx = makeCtx();
  const none = { hiddenNets: [] as string[], hiddenNetclasses: [] as string[] };
  const hidden = showNetclass(none, ctx, "usb", false);
  assert.deepEqual(hidden.hiddenNets.sort(), ["USB_D+", "USB_D-"]);
  assert.deepEqual(hidden.hiddenNetclasses, ["usb"]);
  assert.deepEqual(showNetclass(hidden, ctx, "usb", false).hiddenNetclasses, ["usb"], "twice is once");
  const shown = showNetclass({ hiddenNets: [...hidden.hiddenNets, "GND"], hiddenNetclasses: ["usb"] }, ctx, "usb", true);
  assert.deepEqual(shown.hiddenNets, ["GND"], "a net of another class stays as it was");
  assert.deepEqual(shown.hiddenNetclasses, []);
});

test("Show All Netclasses, and Hide All Other Netclasses with the Default class hidden unless it is the one clicked", () => {
  const ctx = makeCtx();
  const all = showAllNetclasses({ hiddenNets: ["USB_D+", "GND"], hiddenNetclasses: ["usb", "Default"] }, ctx);
  assert.deepEqual(all, { hiddenNets: [], hiddenNetclasses: [] }, "every class is shown, and so is each net they own");
  const onlyUsb = hideOtherNetclasses({ hiddenNets: [], hiddenNetclasses: [] }, ctx, "usb");
  assert.deepEqual(onlyUsb.hiddenNetclasses, ["Default"]);
  assert.deepEqual(onlyUsb.hiddenNets.sort(), ["GND", "SCL", "VCC"]);
  const onlyDefault = hideOtherNetclasses({ hiddenNets: [], hiddenNetclasses: [] }, ctx, "Default");
  assert.deepEqual(onlyDefault.hiddenNetclasses, ["usb"]);
  assert.deepEqual(onlyDefault.hiddenNets.sort(), ["USB_D+", "USB_D-"]);
});

test("opening a project hides the nets it names and every net of a hidden class (LoadProjectSettings)", () => {
  const ctx = makeCtx();
  assert.deepEqual(hiddenNetsOnLoad(["GND"], ["usb"], ctx).sort(), ["GND", "USB_D+", "USB_D-"]);
  assert.deepEqual(hiddenNetsOnLoad([], [], ctx), []);
});

test("the right-click menu of a net (OnNetGridRightClick), with the net's name in three entries", () => {
  const m = netMenu("GND");
  assert.deepEqual(m.map((e) => e.label), [
    "Set Net Color", "Clear Net Color", "", "Highlight GND", "Select Tracks and Vias in GND", "Unselect Tracks and Vias in GND", "", "Show All Nets", "Hide All Other Nets",
  ]);
  assert.deepEqual(m.filter((e) => e.action === "separator").length, 2);
});

test("the menu of a net class: no colour entries for Default; Use Color from Schematic is disabled while the class has none (buildNetClassMenu)", () => {
  assert.deepEqual(netclassMenu("Default", true).map((e) => e.label), [
    "Highlight Nets in Default", "Select Tracks and Vias in Default", "Unselect Tracks and Vias in Default", "", "Show All Netclasses", "Hide All Other Netclasses",
  ]);
  const usb = netclassMenu("usb", false);
  assert.deepEqual(usb.slice(0, 4).map((e) => e.label), ["Set Netclass Color", "Use Color from Schematic", "Clear Netclass Color", ""]);
  assert.equal(usb[1]!.disabled, true);
  assert.equal(netclassMenu("usb", false, "rgb(1, 2, 3)")[1]!.disabled, false);
});

test("the tip of a net's eye says what the click will do (OnNetGridMouseEvent)", () => {
  assert.equal(netEyeTip("GND", true), "Click to hide ratsnest for GND");
  assert.equal(netEyeTip("GND", false), "Click to show ratsnest for GND");
});
