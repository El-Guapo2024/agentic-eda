import { test } from "node:test";
import assert from "node:assert/strict";
import { applyPatch, heldCmd, turnCmd, wirePick } from "./schMove";
import type { Schematic } from "../api/types";
import type { SchMovePatch } from "../api/schEditTypes";

const sheet = (over: Partial<Schematic> = {}): Schematic =>
  ({
    lib_symbols: {},
    symbols: [{ id: "R1", lib_id: null, at: [100, 200], rot: 0, mirror: null, unit: 1, body_style: 1, value: null, mpn: null, package: null, footprint: null, datasheet: null, pins: [] }],
    power_symbols: [],
    wires: [{ id: "wire_a", net: "N", pins: ["R1.1"], pts: [[0, 0], [10, 0]], bus: false }],
    no_connects: [],
    labels: [{ id: "lbl_a", net: "A", at: [5, 5], scope: "local", shape: null }],
    texts: [],
    title_block: null,
    bus_entries: [],
    junctions: [],
    sheets: [],
    sheet_path: [],
    ...over,
  }) as Schematic;

const emptyPatch = (): SchMovePatch => ({ symbols: [], power_symbols: [], wires: [], labels: [], texts: [], no_connects: [], bus_entries: [], junctions: [], lines: [], graphics: [], sheets: [] });

test("a held selection commits as one move or drag, with the turns made on the way", () => {
  assert.equal(heldCmd({ mode: "move", ids: ["R1"], dxUm: 0, dyUm: 0 }), null, "nothing moved, nothing turned: nothing to send");
  assert.deepEqual(heldCmd({ mode: "move", ids: ["R1", "wire_a"], dxUm: 2540, dyUm: -1270 }), { op: "sch_move", verb: "move", ids: ["R1", "wire_a"], dx: 2540, dy: -1270 });
  assert.deepEqual(heldCmd({ mode: "drag", ids: ["R1"], dxUm: 1270, dyUm: 0, vertices: { wire_a: [1] } }), { op: "sch_move", verb: "drag", ids: ["R1"], vertices: { wire_a: [1] }, dx: 1270, dy: 0, ortho: true });
  assert.deepEqual(heldCmd({ mode: "drag", ids: ["R1"], dxUm: 0, dyUm: 0, turns: ["rot_ccw", "mirror_h"], holdUm: [3810, 1270] }, { ortho: false }), {
    op: "sch_move",
    verb: "drag",
    ids: ["R1"],
    vertices: undefined,
    dx: 0,
    dy: 0,
    ortho: false,
    turns: ["rot_ccw", "mirror_h"],
    about: { x: 3810, y: 1270 },
  });
});

test("R, Shift+R, X and Y send one turn command each", () => {
  assert.deepEqual(turnCmd(["R1"], "rot_ccw"), { op: "sch_move", ids: ["R1"], vertices: undefined, verb: "rotate", ccw: true });
  assert.deepEqual(turnCmd(["R1"], "rot_cw"), { op: "sch_move", ids: ["R1"], vertices: undefined, verb: "rotate", ccw: false });
  assert.deepEqual(turnCmd(["R1"], "mirror_h"), { op: "sch_move", ids: ["R1"], vertices: undefined, verb: "mirror", vertical: false });
  assert.deepEqual(turnCmd(["wire_a"], "mirror_v", { wire_a: [0] }), { op: "sch_move", ids: ["wire_a"], vertices: { wire_a: [0] }, verb: "mirror", vertical: true });
});

test("a click picks the end of a wire it is near, else the segment it is on", () => {
  const wire = { pts: [[0, 0], [10000, 0], [10000, 10000]] as Array<[number, number]> };
  assert.deepEqual(wirePick(wire, [100, 50], 400), [0], "near the first end: only that end");
  assert.deepEqual(wirePick(wire, [10000, 9900], 400), [2], "near the last end");
  assert.deepEqual(wirePick(wire, [5000, 100], 400), [0, 1], "mid-segment: both ends of that segment");
  assert.deepEqual(wirePick(wire, [10100, 5000], 400), [1, 2], "the second segment");
  assert.equal(wirePick(wire, [5000, 5000], 400), null, "off the wire");
});

test("a preview patch moves what it names and leaves the rest of the sheet", () => {
  const sch = sheet();
  const patch: SchMovePatch = {
    ...emptyPatch(),
    symbols: [{ id: "R1", unit: 1, at: [300, 200], rot: 90, mirror: "y" }],
    wires: [
      { id: "wire_a", net: "N", pts: [[0, 0], [10, 0], [10, 5]], bus: false },
      { id: "wire_new", net: "N", pts: [[10, 5], [30, 5]], bus: false },
    ],
    labels: [{ id: "lbl_a", at: [5, 6], spin: "up" }],
    junctions: [{ id: "jct_a", at: [10, 0] }],
  };
  const out = applyPatch(sch, patch);
  assert.deepEqual(out.symbols[0]!.at, [300, 200]);
  assert.equal(out.symbols[0]!.rot, 90);
  assert.equal(out.symbols[0]!.mirror, "y");
  assert.deepEqual(out.wires.map((w) => [w.id, w.pts.length]), [["wire_a", 3], ["wire_new", 2]]);
  assert.deepEqual(out.wires[0]!.pins, ["R1.1"], "a wire that was there keeps what it knew");
  assert.deepEqual(out.labels[0]!.at, [5, 6]);
  assert.equal(out.labels[0]!.spin, "up");
  assert.deepEqual(out.junctions, [{ id: "jct_a", at: [10, 0] }]);
  assert.equal(sch.symbols[0]!.at[0], 100, "the sheet itself is not changed");
});

test("a sheet's pins move with it", () => {
  const sch = sheet({ sheets: [{ id: "sheet_a", name: "S", file: "s.kicad_sch", at: [0, 0], size: [100, 50], pins: [{ id: "shpin_a", name: "IN", shape: "input", at: [0, 10] }] }] });
  const out = applyPatch(sch, { ...emptyPatch(), sheets: [{ id: "sheet_a", at: [20, 30], size: [100, 50], pins: [{ id: "shpin_a", at: [20, 40] }] }] });
  assert.deepEqual(out.sheets[0]!.at, [20, 30]);
  assert.deepEqual(out.sheets[0]!.pins[0]!.at, [20, 40]);
  assert.equal(out.sheets[0]!.pins[0]!.name, "IN");
});
