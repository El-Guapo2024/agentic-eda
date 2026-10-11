import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Pad, Part, Track, Via } from "../api/types";
import { initialTracksViasFilter, matchingTracksAndVias, NOTHING_IN_SCOPE, viaKind, viasBoxState, viaTopLayer, withAllVias, type TracksViasFilter, type TracksViasScope } from "./globalEditTracksVias";

const STACK = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];
const pad = (num: string, net: string): Pad => ({ num, net, x: 0, y: 0, w: 600, h: 600, round: false, th: false });
const part = (ref: string, pads: Pad[]): Part =>
  ({ ref, value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [2000, 1000], at: [0, 0], rot: 0, side: "top", label: "above", courtyard: [-1000, -500, 1000, 500], pads }) as Part;
const track = (id: string, net: string, layer: string, width: number): Track => ({ id, net, layer, width, pts: [[0, 0], [1000, 0]] });
const via = (id: string, net: string, from: string, to: string, d = 600): Via => ({ id, net, x: 0, y: 0, d, drill: 300, from, to });

function board(groups: { id: string; member_ids: string[] }[] = []): BoardState {
  return {
    name: "t",
    dir: "",
    outline: null,
    layers: STACK,
    snap: 100,
    parts: [part("R1", [pad("1", "VCC"), pad("2", "GND")]), part("R2", [pad("1", "SIG"), pad("2", "GND")])],
    rules: [],
    routing: {
      tracks: [track("t1", "VCC", "F.Cu", 200), track("t2", "VCC", "B.Cu", 400), track("t3", "SIG", "F.Cu", 200), track("t4", "GND", "In1.Cu", 300)],
      vias: [via("v1", "VCC", "F.Cu", "B.Cu"), via("v2", "SIG", "F.Cu", "In1.Cu", 500), via("v3", "GND", "In1.Cu", "In2.Cu", 500), via("v4", "VCC", "B.Cu", "F.Cu", 800)],
      zones: [],
    },
    drawings: { shapes: [], texts: [], groups, dimensions: [], dimension_settings: {} },
    checks: [],
    activity: [],
    job: "idle",
    board_rules: { track_width: 250, via_diameter: 600, via_drill: 300, net_classes: [{ name: "Power", nets: ["VCC", "GND"] }] },
  } as unknown as BoardState;
}

const ALL: TracksViasScope = { tracks: true, through: true, blind: true, buried: true };
const FILTER: TracksViasFilter = { selectedOnly: false, netOn: false, net: "GND", netclassOn: false, netclass: "Default", layerOn: false, layer: "F.Cu", trackWidthOn: false, trackWidth: 200, viaSizeOn: false, viaSize: 500 };
const ids = (r: { trackIds: string[]; viaIds: string[] }) => [...r.trackIds, ...r.viaIds];

test("a via's type is what its layers say: through, blind or buried", () => {
  assert.equal(viaKind({ from: "F.Cu", to: "B.Cu" }, STACK), "through");
  assert.equal(viaKind({ from: "B.Cu", to: "F.Cu" }, STACK), "through", "written the other way round");
  assert.equal(viaKind({ from: "F.Cu", to: "In1.Cu" }, STACK), "blind");
  assert.equal(viaKind({ from: "In2.Cu", to: "B.Cu" }, STACK), "blind");
  assert.equal(viaKind({ from: "In1.Cu", to: "In2.Cu" }, STACK), "buried");
  assert.equal(viaKind({ from: "F.Cu", to: "B.Cu" }, ["F.Cu", "B.Cu"]), "through");
});

test("GetLayer() of a via is the top layer of its span", () => {
  assert.equal(viaTopLayer({ from: "F.Cu", to: "B.Cu" }, STACK), "F.Cu");
  assert.equal(viaTopLayer({ from: "B.Cu", to: "F.Cu" }, STACK), "F.Cu");
  assert.equal(viaTopLayer({ from: "In2.Cu", to: "In1.Cu" }, STACK), "In1.Cu");
});

test("the scope picks tracks and the via types; with nothing in scope nothing matches", () => {
  const b = board();
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, FILTER, new Set())), ["t1", "t2", "t3", "t4", "v1", "v2", "v3", "v4"]);
  assert.deepEqual(ids(matchingTracksAndVias(b, NOTHING_IN_SCOPE, FILTER, new Set())), []);
  assert.deepEqual(ids(matchingTracksAndVias(b, { ...NOTHING_IN_SCOPE, tracks: true }, FILTER, new Set())), ["t1", "t2", "t3", "t4"]);
  assert.deepEqual(ids(matchingTracksAndVias(b, { ...NOTHING_IN_SCOPE, through: true }, FILTER, new Set())), ["v1", "v4"]);
  assert.deepEqual(ids(matchingTracksAndVias(b, { ...NOTHING_IN_SCOPE, blind: true }, FILTER, new Set())), ["v2"]);
  assert.deepEqual(ids(matchingTracksAndVias(b, { ...NOTHING_IN_SCOPE, buried: true, blind: true }, FILTER, new Set())), ["v2", "v3"]);
});

test("net, net class and layer filters; a via is on its top layer", () => {
  const b = board();
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, { ...FILTER, netOn: true, net: "VCC" }, new Set())), ["t1", "t2", "v1", "v4"]);
  // Power owns VCC and GND; SIG is in Default
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, { ...FILTER, netclassOn: true, netclass: "Power" }, new Set())), ["t1", "t2", "t4", "v1", "v3", "v4"]);
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, { ...FILTER, netclassOn: true, netclass: "Default" }, new Set())), ["t3", "v2"]);
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, { ...FILTER, layerOn: true, layer: "F.Cu" }, new Set())), ["t1", "t3", "v1", "v2", "v4"], "v4 is written B.Cu to F.Cu: its top layer is F.Cu");
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, { ...FILTER, layerOn: true, layer: "In1.Cu" }, new Set())), ["t4", "v3"]);
});

test("exact width and diameter filters, one for tracks and one for vias", () => {
  const b = board();
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, { ...FILTER, trackWidthOn: true, trackWidth: 200 }, new Set())), ["t1", "t3", "v1", "v2", "v3", "v4"], "the width filter leaves the vias alone");
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, { ...FILTER, viaSizeOn: true, viaSize: 500 }, new Set())), ["t1", "t2", "t3", "t4", "v2", "v3"], "and the via filter the tracks");
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, { ...FILTER, trackWidthOn: true, trackWidth: 400, viaSizeOn: true, viaSize: 800 }, new Set())), ["t2", "v4"]);
});

test("the filters combine, and the scope narrows them", () => {
  const b = board();
  const f = { ...FILTER, netOn: true, net: "VCC", layerOn: true, layer: "F.Cu", trackWidthOn: true, trackWidth: 200 };
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, f, new Set())), ["t1", "v1", "v4"]);
  assert.deepEqual(ids(matchingTracksAndVias(b, { ...NOTHING_IN_SCOPE, tracks: true }, f, new Set())), ["t1"]);
});

test("selected items only takes the selection and the members of a selected group, nested ones too", () => {
  const b = board([
    { id: "inner", member_ids: ["t2", "v3"] },
    { id: "outer", member_ids: ["inner", "t4"] },
  ]);
  const f = { ...FILTER, selectedOnly: true };
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, f, new Set(["t1", "v2"]))), ["t1", "v2"]);
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, f, new Set())), [], "nothing selected, nothing matches");
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, f, new Set(["inner"]))), ["t2", "v3"]);
  assert.deepEqual(ids(matchingTracksAndVias(b, ALL, f, new Set(["outer"]))), ["t2", "t4", "v3"]);
});

test("a board with no routing matches nothing", () => {
  const b = { ...board(), routing: null } as unknown as BoardState;
  assert.deepEqual(matchingTracksAndVias(b, ALL, FILTER, new Set()), { trackIds: [], viaIds: [] });
  assert.deepEqual(matchingTracksAndVias(null, ALL, FILTER, new Set()), { trackIds: [], viaIds: [] });
});

test("the tri-state Vias box follows the three types, and clicking it sets them all", () => {
  assert.equal(viasBoxState({ through: false, blind: false, buried: false }), "off");
  assert.equal(viasBoxState({ through: true, blind: true, buried: true }), "on");
  assert.equal(viasBoxState({ through: true, blind: false, buried: true }), "mixed");
  assert.deepEqual(withAllVias(NOTHING_IN_SCOPE, true), { tracks: false, through: true, blind: true, buried: true });
  assert.deepEqual(withAllVias(ALL, false), { tracks: true, through: false, blind: false, buried: false });
});

test("the dialog opens on the remembered net and class, else the highlighted net and the Default class; every filter off", () => {
  const b = board();
  const f = initialTracksViasFilter(b, "SIG", "In2.Cu", { net: "", netclass: "" });
  assert.deepEqual(f, { selectedOnly: false, netOn: false, net: "SIG", netclassOn: false, netclass: "Default", layerOn: false, layer: "In2.Cu", trackWidthOn: false, trackWidth: 250, viaSizeOn: false, viaSize: 600 });
  const again = initialTracksViasFilter(b, "SIG", null, { net: "GND", netclass: "Power" });
  assert.equal(again.net, "GND", "the net of the last session wins while the board still has it");
  assert.equal(again.netclass, "Power");
  assert.equal(again.layer, "F.Cu", "no active layer: the first copper layer");
  assert.equal(initialTracksViasFilter(b, null, null, { net: "GONE", netclass: "Gone" }).net, "GND", "a remembered net the board lost is dropped");
  assert.equal(initialTracksViasFilter(b, null, null, { net: "GONE", netclass: "Gone" }).netclass, "Default");
});
