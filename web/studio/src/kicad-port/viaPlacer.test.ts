import { test } from "node:test";
import assert from "node:assert/strict";
import { selectNetLabels, snapViaPosition, viaNet, type ViaFill, type ViaPlacerInput } from "./viaPlacer";
import type { SceneBoard } from "./snapScene";
import type { Pt } from "./snapGeom";

const MM = 1_000_000;
const COPPER = ["F.Cu", "B.Cu"];
const square = (x0: number, y0: number, x1: number, y1: number): [number, number][] => [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];

function input(board: Partial<SceneBoard>, fills: ViaFill[] = [], over: Partial<ViaPlacerInput> = {}): ViaPlacerInput {
  return {
    board: { layers: COPPER, parts: [], ...board },
    diameter: 0.6 * MM,
    layers: COPPER,
    magnetic: { tracks: "track-tool", pads: "track-tool", graphics: false },
    snap: true,
    // The grid helper's AlignToSegment on a 1 mm grid, for a horizontal segment: the cursor's x on the grid, the segment's y.
    alignToSegment: (p: Pt, a: Pt) => [Math.round(p[0] / MM) * MM, a[1]],
    fills,
    activeLayer: "F.Cu",
    highContrast: false,
    isLayerVisible: () => true,
    ...over,
  };
}

const track = (id: string, net: string, a: Pt, b: Pt) => ({ id, net, layer: "F.Cu", width: 0.2 * MM, pts: [[a[0], a[1]], [b[0], b[1]]] as [number, number][] });

test("a click in the middle of a GND pour is a GND stitching via (findStitchedZoneNet, round 5 B5-1)", () => {
  const fills: ViaFill[] = [{ net: "GND", layer: "F.Cu", fragments: [square(0, 0, 20 * MM, 20 * MM)] }];
  const i = input({}, fills);
  assert.deepEqual(snapViaPosition(i, [10.3 * MM, 10.2 * MM], [10 * MM, 10 * MM]), [10 * MM, 10 * MM], "nothing to snap to: where the grid put it");
  assert.deepEqual(viaNet(i, [10 * MM, 10 * MM]), { kind: "net", net: "GND", track: null });
  assert.deepEqual(viaNet(i, [30 * MM, 10 * MM]), { kind: "net", net: "", track: null }, "outside every fill: a via of no net");
  // A fill on a hidden layer does not count.
  assert.deepEqual(viaNet(input({}, fills, { isLayerVisible: (l) => l !== "F.Cu" }), [10 * MM, 10 * MM]), { kind: "net", net: "", track: null });
});

test("two nets' fills under the via: the user chooses ('Select Net:')", () => {
  const fills: ViaFill[] = [
    { net: "GND", layer: "F.Cu", fragments: [square(0, 0, 20 * MM, 20 * MM)] },
    { net: "VCC", layer: "B.Cu", fragments: [square(0, 0, 20 * MM, 20 * MM)] },
  ];
  assert.deepEqual(viaNet(input({}, fills), [5 * MM, 5 * MM]), { kind: "choose", nets: ["GND", "VCC"] });
  // High contrast: the active layer's one net decides.
  assert.deepEqual(viaNet(input({}, fills, { highContrast: true, activeLayer: "B.Cu" }), [5 * MM, 5 * MM]), { kind: "net", net: "VCC", track: null });
  assert.deepEqual(selectNetLabels(["GND", "VCC"]), ["1  GND", "2  VCC"]);
  assert.deepEqual(selectNetLabels(["", "GND"]), ["1  <no net>", "2  GND"], "a pour of no net is NO_NET (round 6 C6-8)");
});

test("a click beside a track snaps onto its line and takes its net; the track is split there (SnapItem, PlaceItem)", () => {
  const board = { routing: { tracks: [track("t1", "N1", [0, 5 * MM], [10 * MM, 5 * MM])], vias: [], zones: [] } };
  const i = input(board);
  // 0.3 mm beside the track (within half the track and half the via): onto the track's line, x on the grid.
  const pos = snapViaPosition(i, [4.2 * MM, 5.3 * MM], [4 * MM, 5 * MM]);
  assert.deepEqual(pos, [4 * MM, 5 * MM]);
  assert.deepEqual(viaNet(i, pos), { kind: "net", net: "N1", track: "t1" }, "the middle of a track: its net, and it is cut there");
  // Shift: no snap, and no split.
  const shift = input(board, [], { snap: false });
  assert.deepEqual(snapViaPosition(shift, [4.2 * MM, 5.3 * MM], [4 * MM, 5 * MM]), [4 * MM, 5 * MM]);
  assert.deepEqual(viaNet(shift, [4.2 * MM, 5.3 * MM]), { kind: "net", net: "N1", track: null });
  // The magnetic tracks setting "never": no snap onto it.
  assert.deepEqual(snapViaPosition(input(board, [], { magnetic: { tracks: "never", pads: "never", graphics: false } }), [4.2 * MM, 5.3 * MM], [4 * MM, 5 * MM]), [4 * MM, 5 * MM]);
});

test("a click on a pad goes to its centre with its net", () => {
  const pad = { num: "1", net: "N2", x: 20 * MM, y: 20 * MM, w: 1.2 * MM, h: 1.2 * MM, round: false, th: false };
  const board = { parts: [{ ref: "U1", placed: true, at: [20 * MM, 20 * MM] as [number, number], side: "top" as const, pads: [pad] }] };
  const i = input(board);
  const pos = snapViaPosition(i, [20.4 * MM, 19.8 * MM], [20 * MM, 20 * MM]);
  assert.deepEqual(pos, [20 * MM, 20 * MM]);
  assert.deepEqual(viaNet(i, [20.4 * MM, 19.8 * MM]), { kind: "net", net: "N2", track: null });
});

test("a track on a hidden layer is not snapped to, gives no net and is not split (VIEW::Query skips hidden layers, round 6 C6-1)", () => {
  const b = { routing: { tracks: [{ ...track("t1", "GND", [0, 5 * MM], [10 * MM, 5 * MM]), layer: "B.Cu" }], vias: [], zones: [] } };
  const hidden = input(b, [], { isLayerVisible: (l) => l !== "B.Cu" });
  assert.deepEqual(snapViaPosition(hidden, [4.2 * MM, 5.3 * MM], [4 * MM, 5 * MM]), [4 * MM, 5 * MM], "where the grid put it");
  assert.deepEqual(snapViaPosition(hidden, [4.2 * MM, 5.1 * MM], [4.2 * MM, 5.1 * MM]), [4.2 * MM, 5.1 * MM]);
  assert.deepEqual(viaNet(hidden, [4 * MM, 5 * MM]), { kind: "net", net: "", track: null });
  // Shown again: snapped, its net, split.
  assert.deepEqual(viaNet(input(b), [4 * MM, 5 * MM]), { kind: "net", net: "GND", track: "t1" });
});
