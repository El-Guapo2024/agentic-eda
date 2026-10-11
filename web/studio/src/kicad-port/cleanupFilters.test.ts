import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Pad, Part } from "../api/types";
import { anyFilterOn, filterBoxDefaults, filterChoices, filterFields } from "./cleanupFilters";

const pad = (num: string, net: string | null): Pad => ({ num, net, x: 0, y: 0, w: 600, h: 600, round: false, th: false });
const part = (ref: string, pads: Pad[]): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [2000, 1000], at: [0, 0], rot: 0, side: "top", label: "above", courtyard: [-1000, -500, 1000, 500], pads }) as Part;
const board = (extra: Partial<BoardState> = {}): BoardState =>
  ({
    name: "t",
    dir: "",
    outline: null,
    layers: ["F.Cu", "In1.Cu", "B.Cu"],
    snap: 100,
    parts: [part("R1", [pad("1", "VCC"), pad("2", "GND")]), part("R2", [pad("1", "VCC"), pad("2", "SIG")])],
    rules: [],
    routing: null,
    drawings: null,
    checks: [],
    activity: [],
    job: "idle",
    board_rules: { net_classes: [{ name: "Power", nets: ["VCC", "GND"] }] },
    ...extra,
  }) as unknown as BoardState;

test("the lists offer the board's nets, the Default class first and then the others, and the copper layers", () => {
  const c = filterChoices(board());
  assert.deepEqual(c.nets, ["GND", "SIG", "VCC"]);
  assert.deepEqual(c.netclasses, ["Default", "Power"]);
  assert.deepEqual(c.layers, ["F.Cu", "In1.Cu", "B.Cu"]);
  assert.deepEqual(filterChoices(null), { nets: [], netclasses: ["Default"], layers: [] });
});

test("TransferDataToWindow: the highlighted net, the Default class, the active layer; every filter off", () => {
  const box = filterBoxDefaults(board(), "VCC", "B.Cu");
  assert.deepEqual(box, { netOn: false, net: "VCC", netclassOn: false, netclass: "Default", layerOn: false, layer: "B.Cu", selectedOnly: false });
  assert.equal(anyFilterOn(box), false);
});

test("nothing highlighted, or an active layer that is not copper, starts on the first of the list", () => {
  const box = filterBoxDefaults(board(), null, "F.SilkS");
  assert.equal(box.net, "GND");
  assert.equal(box.layer, "F.Cu");
  assert.equal(filterBoxDefaults(board(), "NOPE", null).net, "GND", "a net the board does not have is not offered");
  assert.deepEqual(filterBoxDefaults(null, null, null), { netOn: false, net: "", netclassOn: false, netclass: "Default", layerOn: false, layer: "", selectedOnly: false });
});

test("only a filter that is on is sent", () => {
  const box = filterBoxDefaults(board(), "SIG", "In1.Cu");
  assert.deepEqual(filterFields(box, ["t1"]), {});
  assert.deepEqual(filterFields({ ...box, netOn: true }, []), { filter_net: "SIG" });
  assert.deepEqual(filterFields({ ...box, netclassOn: true, netclass: "Power", layerOn: true }, []), { filter_netclass: "Power", filter_layer: "In1.Cu" });
  assert.equal(anyFilterOn({ ...box, layerOn: true }), true);
});

test("Selected items only sends the selection, even an empty one", () => {
  const box = { ...filterBoxDefaults(board(), null, null), selectedOnly: true };
  assert.deepEqual(filterFields(box, new Set(["t1", "grp_abc", "v2"])), { selected_ids: ["t1", "grp_abc", "v2"] });
  assert.deepEqual(filterFields(box, new Set()), { selected_ids: [] }, "nothing selected = nothing to clean, not 'no filter'");
});

test("a filter that is on with nothing to choose from is not sent", () => {
  const box = { ...filterBoxDefaults(board({ parts: [] }), null, null), netOn: true };
  assert.deepEqual(filterFields(box, []), {});
});
