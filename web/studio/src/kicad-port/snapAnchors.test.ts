import { test } from "node:test";
import assert from "node:assert/strict";
import { ANCHOR, AnchorList, DEFAULT_PCB_MAGNETIC, computeItemAnchors, nearestFlagged, type Anchor, type AnchorContext, type DragFilter, type PcbMagnetic } from "./snapAnchors";
import { buildScene, queryScene, type SceneBoard, type SnapItem } from "./snapScene";
import { PT, box, type Pt } from "./snapGeom";

const always: PcbMagnetic = { pads: "always", tracks: "always", graphics: true, allLayers: false };
const ctx = (magnetic: PcbMagnetic = always, filter?: DragFilter): AnchorContext => ({ magnetic, visible: () => true, footprintLayerVisible: () => true, grid: [1_000_000, 1_000_000], filter });

const pad = (num: string, x: number, y: number, w = 600_000, h = 600_000, round = false, th = false) => ({ num, net: null, x, y, w, h, round, th });
const part = (ref: string, x: number, y: number, pads: ReturnType<typeof pad>[] = [], courtyard?: [number, number, number, number]) => ({ ref, placed: true, at: [x, y] as [number, number], side: "top" as const, pads, courtyard });
const common = (id: string, layer = "F.SilkS") => ({ id, layer, stroke_width: 100_000, filled: false });

function anchorsOf(board: Partial<SceneBoard>, type: SnapItem["type"], ref: Pt = [0, 0], from = false, c: AnchorContext = ctx()): Anchor[] {
  const scene = buildScene({ layers: ["F.Cu", "B.Cu"], parts: [], ...board });
  const out = new AnchorList();
  for (const item of scene.items.filter((i) => i.type === type)) computeItemAnchors(out, item, ref, from, c);
  return out.list;
}
const at = (list: Anchor[], x: number, y: number, tol = 1e-6) => list.filter((a) => Math.abs(a.pos[0] - x) <= tol && Math.abs(a.pos[1] - y) <= tol);
const withType = (list: Anchor[], t: number) => list.filter((a) => (a.pointTypes & t) === t);
const snappable = (list: Anchor[]) => list.filter((a) => a.flags & ANCHOR.SNAPPABLE);

test("flags are KiCad's values", () => {
  assert.deepEqual(ANCHOR, { CORNER: 1, OUTLINE: 2, SNAPPABLE: 4, ORIGIN: 8, VERTICAL: 16, HORIZONTAL: 32, CONSTRUCTED: 64, ALL: 127 });
});

test("a rectangular pad: its centre, then each corner and side middle", () => {
  const list = anchorsOf({ parts: [part("U1", 0, 0, [pad("1", 1_000_000, 2_000_000, 600_000, 400_000)])] }, "pad");
  const centre = at(list, 1_000_000, 2_000_000);
  assert.equal(centre.length, 1);
  assert.equal(centre[0]!.flags, ANCHOR.ORIGIN | ANCHOR.SNAPPABLE);
  assert.equal(centre[0]!.pointTypes, PT.CENTER);
  for (const [x, y] of [
    [700_000, 2_200_000],
    [1_300_000, 2_200_000],
    [1_300_000, 1_800_000],
    [700_000, 1_800_000],
  ] as [number, number][])
    assert.equal(withType(at(list, x, y), PT.CORNER).length >= 1, true, `corner ${x},${y}`);
  for (const [x, y] of [
    [1_000_000, 2_200_000],
    [1_300_000, 2_000_000],
    [1_000_000, 1_800_000],
    [700_000, 2_000_000],
  ] as [number, number][])
    assert.equal(withType(at(list, x, y), PT.MID).length >= 1, true, `middle ${x},${y}`);
  assert.ok(list.filter((a) => a.pointTypes === PT.CORNER).every((a) => a.flags === (ANCHOR.OUTLINE | ANCHOR.SNAPPABLE)));
  assert.equal(list.length, 1 + 4 + 4 + 1, "centre, four corners, four middles, and the last side repeats the first corner as KiCad does");
});

test("a round pad gives four quadrants; an oval its middles and tips", () => {
  const round = anchorsOf({ parts: [part("U1", 0, 0, [pad("1", 0, 0, 800_000, 800_000, true)])] }, "pad");
  assert.equal(round.length, 1 + 4);
  assert.equal(withType(round, PT.QUADRANT).length, 4);
  const oval = anchorsOf({ parts: [part("U1", 0, 0, [pad("1", 0, 0, 1_200_000, 600_000, true)])] }, "pad");
  assert.equal(withType(oval, PT.QUADRANT).length, 2, "the two tips, on the long axis");
  assert.equal(withType(oval, PT.MID).length, 2);
  assert.equal(at(oval, 600_000, 0).length, 1);
});

test("picking a pad up (from) gives only its centre; the filter can exclude pads", () => {
  const board = { parts: [part("U1", 0, 0, [pad("1", 1_000_000, 2_000_000)])] };
  const from = anchorsOf(board, "pad", [1_000_000, 2_000_000], true);
  assert.equal(from.length, 1);
  assert.equal(from[0]!.pointTypes, PT.CENTER);
  assert.equal(anchorsOf(board, "pad", [1_000_000, 2_000_000], true, ctx(always, { pads: false })).length, 0);
});

test("pads are anchors for snapping only when the magnetic setting is 'always'", () => {
  const board = { parts: [part("U1", 0, 0, [pad("1", 1_000_000, 2_000_000)])] };
  assert.equal(anchorsOf(board, "pad", [1_000_000, 2_000_000], false, ctx(DEFAULT_PCB_MAGNETIC)).length, 0);
  assert.equal(anchorsOf(board, "pad", [1_000_000, 2_000_000], false, ctx({ ...DEFAULT_PCB_MAGNETIC, pads: "never" })).length, 0);
  assert.ok(anchorsOf(board, "pad", [1_000_000, 2_000_000], false, ctx({ ...DEFAULT_PCB_MAGNETIC, pads: "always" })).length > 0);
});

test("a footprint: its position, and the centre of its box when that is farther than a grid", () => {
  const near = anchorsOf({ parts: [part("U1", 5_000_000, 5_000_000, [], [4_000_000, 4_000_000, 6_500_000, 6_000_000])] }, "footprint");
  assert.equal(near.length, 1);
  assert.equal(near[0]!.flags, ANCHOR.ORIGIN | ANCHOR.SNAPPABLE);
  const far = anchorsOf({ parts: [part("U1", 5_000_000, 5_000_000, [], [4_000_000, 4_000_000, 9_000_000, 6_000_000])] }, "footprint");
  assert.equal(far.length, 2);
  assert.deepEqual(far[1]!.pos, [6_500_000, 5_000_000]);
});

test("a footprint contributes the pads under the cursor only", () => {
  const board = { parts: [part("U1", 5_000_000, 5_000_000, [pad("1", 5_000_000, 5_000_000), pad("2", 8_000_000, 5_000_000)], [4_000_000, 4_000_000, 9_000_000, 6_000_000])] };
  const list = anchorsOf(board, "footprint", [5_050_000, 5_020_000]);
  assert.equal(at(list, 5_000_000, 5_000_000).length >= 2, true, "pad 1 under the cursor, and the position");
  assert.equal(at(list, 8_000_000, 5_000_000).length, 0, "pad 2 is not under the cursor");
});

test("an unplaced or hidden footprint gives nothing", () => {
  const board = { parts: [part("U1", 5_000_000, 5_000_000)] };
  const hidden = ctx(always);
  hidden.visible = () => false;
  assert.equal(anchorsOf(board, "footprint", [5_000_000, 5_000_000], false, hidden).length, 0);
  assert.equal(anchorsOf(board, "footprint", [5_000_000, 5_000_000], true, hidden).length, 1, "picking up: the position regardless of visibility");
  const layerOff = ctx(always);
  layerOff.footprintLayerVisible = () => false;
  assert.equal(anchorsOf(board, "footprint", [5_000_000, 5_000_000], true, layerOff).length, 0);
  const scene = buildScene({ parts: [{ ref: "U2", placed: false }] });
  assert.equal(scene.items.length, 0);
});

test("graphic shapes: segment, rectangle, circle, arc, polygon, Bezier", () => {
  const shapes = [
    { ...common("seg"), kind: "segment" as const, start: [0, 0] as [number, number], end: [1_000_000, 0] as [number, number] },
    { ...common("rect"), kind: "rect" as const, start: [2_000_000, 0] as [number, number], end: [3_000_000, 1_000_000] as [number, number] },
    { ...common("circ"), kind: "circle" as const, center: [5_000_000, 0] as [number, number], end: [5_500_000, 0] as [number, number] },
    { ...common("arc"), kind: "arc" as const, start: [7_000_000, 0] as [number, number], mid: [7_500_000, 500_000] as [number, number], end: [8_000_000, 0] as [number, number] },
    { ...common("poly"), kind: "polygon" as const, pts: [[0, 3_000_000], [1_000_000, 3_000_000], [1_000_000, 4_000_000]] as [number, number][] },
    { ...common("bez"), kind: "bezier" as const, start: [0, 6_000_000] as [number, number], c1: [100_000, 7_000_000] as [number, number], c2: [900_000, 7_000_000] as [number, number], end: [1_000_000, 6_000_000] as [number, number] },
  ];
  const scene = buildScene({ parts: [], drawings: { shapes, texts: [], dimensions: [] } });
  const by = (id: string) => {
    const out = new AnchorList();
    computeItemAnchors(out, scene.items.find((i) => i.id === id)!, [500_000, 3_500_000], false, ctx());
    return out.list;
  };
  const seg = by("seg");
  assert.equal(seg.length, 3);
  assert.equal(withType(seg, PT.END).length, 2);
  assert.equal(withType(seg, PT.MID).length, 1);
  assert.deepEqual(withType(seg, PT.MID)[0]!.pos, [500_000, 0]);
  const rect = by("rect");
  assert.equal(rect.length, 9, "centre, four corners, four middles");
  assert.equal(withType(rect, PT.CORNER).length, 4);
  assert.equal(withType(rect, PT.MID).length, 4);
  assert.equal(withType(rect, PT.CENTER).length, 1);
  assert.deepEqual(withType(rect, PT.CENTER)[0]!.pos, [2_500_000, 500_000]);
  const circ = by("circ");
  assert.equal(circ.length, 5);
  assert.equal(circ.find((a) => a.pointTypes === PT.CENTER)!.flags, ANCHOR.ORIGIN | ANCHOR.SNAPPABLE);
  assert.equal(withType(circ, PT.QUADRANT).length, 4);
  assert.ok(withType(circ, PT.QUADRANT).every((a) => a.flags === (ANCHOR.OUTLINE | ANCHOR.SNAPPABLE)));
  const arc = by("arc");
  assert.equal(withType(arc, PT.END).length, 2);
  assert.equal(withType(arc, PT.MID).length, 1);
  assert.equal(withType(arc, PT.CENTER).length, 1);
  const poly = by("poly");
  assert.equal(withType(poly, PT.CORNER).length, 3);
  const outline = poly.filter((a) => a.flags === ANCHOR.OUTLINE);
  assert.equal(outline.length, 1, "the point of the outline nearest the cursor is an outline anchor, not a snap target");
  assert.ok(snappable(poly).length === 3);
  const bez = by("bez");
  assert.equal(withType(bez, PT.END).length, 2);
  assert.ok(bez.some((a) => a.flags === (ANCHOR.ORIGIN | ANCHOR.SNAPPABLE)), "the default case adds the position");
});

test("graphics are anchors only when 'snap to graphics' is on", () => {
  const shapes = [{ ...common("seg"), kind: "segment" as const, start: [0, 0] as [number, number], end: [1_000_000, 0] as [number, number] }];
  const board = { drawings: { shapes, texts: [], dimensions: [] } };
  assert.equal(anchorsOf(board, "shape", [0, 0], false, ctx(DEFAULT_PCB_MAGNETIC)).length, 0);
  assert.equal(anchorsOf(board, "shape", [0, 0], false, ctx({ ...DEFAULT_PCB_MAGNETIC, graphics: true })).length, 3);
  assert.equal(anchorsOf(board, "shape", [0, 0], true, ctx(DEFAULT_PCB_MAGNETIC)).length, 3, "picking up ignores the magnetic setting");
  assert.equal(anchorsOf(board, "shape", [0, 0], true, ctx(DEFAULT_PCB_MAGNETIC, { graphics: false })).length, 0, "and obeys the filter");
});

test("tracks: ends are snap targets, the middle only a place to pick the track up by; vias have one centre", () => {
  const track = { id: "t1", net: "N", layer: "F.Cu", width: 250_000, pts: [[0, 0], [3_000_000, 0], [3_000_000, 2_000_000]] as [number, number][] };
  const list = anchorsOf({ routing: { tracks: [track], vias: [], zones: [] } }, "track");
  assert.equal(list.length, 6, "two segments, three anchors each");
  const mids = withType(list, PT.MID);
  assert.equal(mids.length, 2);
  assert.ok(mids.every((a) => a.flags === ANCHOR.ORIGIN));
  assert.equal(snappable(list).length, 4);
  assert.equal(anchorsOf({ routing: { tracks: [track], vias: [], zones: [] } }, "track", [0, 0], false, ctx(DEFAULT_PCB_MAGNETIC)).length, 0, "magnetic tracks");
  const v = anchorsOf({ routing: { tracks: [], vias: [{ id: "v", net: "N", x: 100_000, y: 200_000, d: 600_000, drill: 300_000, from: "F.Cu", to: "B.Cu" }], zones: [] } }, "via");
  assert.equal(v.length, 1);
  assert.equal(v[0]!.flags, ANCHOR.ORIGIN | ANCHOR.CORNER | ANCHOR.SNAPPABLE);
  assert.equal(anchorsOf({ routing: { tracks: [], vias: [{ id: "v", net: "N", x: 100_000, y: 200_000, d: 600_000, drill: 300_000, from: "F.Cu", to: "B.Cu" }], zones: [] } }, "via", [0, 0], false, ctx(DEFAULT_PCB_MAGNETIC)).length, 0);
});

test("an arc track gives its two ends and the middle of its chord, as one item", () => {
  const arc = { id: "a1", net: "N", layer: "F.Cu", width: 250_000, pts: [[0, 0], [500_000, 134_000], [1_000_000, 500_000], [1_366_000, 1_000_000]] as [number, number][], arc_mid: [500_000, 134_000] as [number, number] };
  const list = anchorsOf({ routing: { tracks: [arc], vias: [], zones: [] } }, "track");
  assert.equal(list.length, 3);
});

test("a zone: every corner is a snap target, and the nearest point of the outline an outline anchor; zones need no magnetic setting", () => {
  const zone = { id: "z1", net: "N", layer: "F.Cu", outline: [[0, 0], [4_000_000, 0], [4_000_000, 3_000_000], [0, 3_000_000]] as [number, number][] } as unknown as import("../api/types").Zone;
  const list = anchorsOf({ routing: { tracks: [], vias: [], zones: [zone] } }, "zone", [2_000_000, 100_000], false, ctx(DEFAULT_PCB_MAGNETIC));
  assert.equal(withType(list, PT.CORNER).length, 4);
  const outline = list.filter((a) => a.flags === ANCHOR.OUTLINE);
  assert.equal(outline.length, 1);
  assert.deepEqual(outline[0]!.pos, [2_000_000, 0]);
});

test("a dimension: its ends and the points of its crossbar", () => {
  const dim = (over: object) => ({ id: "d1", layer: "Dwgs.User", kind: "aligned", height: 1_000_000, horizontal: null, leader_length: null, start: [0, 0], end: [4_000_000, 0], lines: [[[0, 0], [4_000_000, 0]]], text_at: [2_000_000, 1_000_000], stroke_width: 100_000, ...over }) as unknown as import("../api/types").Dimension;
  const aligned = anchorsOf({ drawings: { shapes: [], texts: [], dimensions: [dim({})] } }, "dimension");
  assert.equal(aligned.length, 4);
  assert.equal(at(aligned, 0, 1_000_000).length, 1, "crossbar start: height 1000 along the normal (-dy, dx)");
  assert.equal(at(aligned, 4_000_000, 1_000_000).length, 1);
  const flipped = anchorsOf({ drawings: { shapes: [], texts: [], dimensions: [dim({ height: -1_000_000 })] } }, "dimension");
  assert.equal(at(flipped, 0, -1_000_000).length, 1, "negative height: the other side");
  const ortho = anchorsOf({ drawings: { shapes: [], texts: [], dimensions: [dim({ kind: "orthogonal", height: 500_000, horizontal: true, start: [0, 0], end: [4_000_000, 2_000_000] })] } }, "dimension");
  assert.equal(at(ortho, 0, 500_000).length, 1);
  assert.equal(at(ortho, 4_000_000, 500_000).length, 1, "the crossbar of a horizontal orthogonal dimension ends above the end point");
  const leader = anchorsOf({ drawings: { shapes: [], texts: [], dimensions: [dim({ kind: "leader" })] } }, "dimension");
  assert.equal(leader.length, 3);
  const center = anchorsOf({ drawings: { shapes: [], texts: [], dimensions: [dim({ kind: "center", start: [0, 0], end: [1_000_000, 0] })] } }, "dimension");
  assert.equal(center.length, 4);
  assert.equal(at(center, 0, 1_000_000).length, 1, "the radial turned by -90 degrees");
  assert.equal(at(center, -1_000_000, 0).length, 1, "and again");
});

test("text is a place to pick it up by, not a snap target", () => {
  const text = { id: "t1", content: "HI", x: 100_000, y: 200_000, angle: 0, layer: "F.SilkS", size: 1_000_000, stroke_width: 150_000, justify: "center" as const, mirror: false };
  const list = anchorsOf({ drawings: { shapes: [], texts: [text], dimensions: [] } }, "text");
  assert.equal(list.length, 1);
  assert.equal(list[0]!.flags, ANCHOR.ORIGIN);
});

test("the mask drops anchors whose flags are not all in it", () => {
  const out = new AnchorList(ANCHOR.SNAPPABLE | ANCHOR.CORNER);
  out.add([0, 0], ANCHOR.SNAPPABLE | ANCHOR.CORNER, []);
  out.add([1000, 1000], ANCHOR.SNAPPABLE | ANCHOR.ORIGIN, []);
  out.add([2000, 2000], ANCHOR.CORNER, []);
  assert.equal(out.list.length, 2);
});

test("the nearest anchor with the flags", () => {
  const out = new AnchorList();
  out.add([10_000, 0], ANCHOR.CORNER, []);
  out.add([5000, 0], ANCHOR.ORIGIN, []);
  out.add([20_000, 0], ANCHOR.ORIGIN | ANCHOR.SNAPPABLE, []);
  assert.deepEqual(nearestFlagged(out.list, [0, 0], ANCHOR.ORIGIN)!.pos, [5000, 0]);
  assert.deepEqual(nearestFlagged(out.list, [0, 0], ANCHOR.SNAPPABLE)!.pos, [20_000, 0]);
  assert.deepEqual(nearestFlagged(out.list, [0, 0], ANCHOR.CORNER | ANCHOR.ORIGIN), null);
});

test("the scene: polylines are segments, pads and vias are on the right layers, queries select by box and skip by id or owner", () => {
  const track = { id: "t1", net: "N", layer: "B.Cu", width: 200_000, pts: [[0, 0], [1_000_000, 0], [1_000_000, 1_000_000]] as [number, number][] };
  const board: SceneBoard = {
    layers: ["F.Cu", "B.Cu"],
    parts: [part("U1", 5_000_000, 5_000_000, [pad("1", 5_000_000, 5_000_000), pad("2", 5_600_000, 5_000_000, 600_000, 600_000, false, true)])],
    routing: { tracks: [track], vias: [{ id: "v", net: "N", x: 9_000_000, y: 9_000_000, d: 600_000, drill: 300_000, from: "F.Cu", to: "B.Cu" }], zones: [] },
  };
  const scene = buildScene(board);
  const t = scene.items.filter((i) => i.type === "track");
  assert.equal(t.length, 2);
  assert.deepEqual(t.map((i) => i.id), ["t1#0", "t1#1"]);
  assert.ok(t.every((i) => i.owner === "t1" && i.layers.length === 1 && i.layers[0] === "B.Cu"));
  const pads = scene.items.filter((i) => i.type === "pad");
  assert.deepEqual(pads[0]!.layers, ["F.Cu"]);
  assert.deepEqual(pads[1]!.layers, ["F.Cu", "B.Cu"], "a through-hole pad is on every copper layer");
  assert.deepEqual(scene.items.find((i) => i.type === "via")!.layers, ["F.Cu", "B.Cu"]);
  assert.equal(queryScene(scene, box(4_900_000, 4_900_000, 5_100_000, 5_100_000)).filter((i) => i.type === "pad").length, 1);
  assert.equal(queryScene(scene, box(4_900_000, 4_900_000, 5_100_000, 5_100_000), new Set(["U1"])).length, 0, "skipping the footprint skips its pads");
  assert.equal(queryScene(scene, box(0, -100_000, 100_000, 100_000), new Set(["t1"])).length, 0, "skipping a track skips its segments");
  assert.equal(queryScene(scene, box(0, -100_000, 100_000, 100_000), new Set(["t1#1"])).length, 1);
});

test("a point: its position is an origin anchor, snappable, of the centre type; the selection filter's Points switch takes it away, picking up or not", () => {
  const board = { drawings: { shapes: [], texts: [], dimensions: [], points: [{ id: "pnt_a", layer: "F.SilkS", x: 3_000_000, y: 4_000_000, size: 1_000_000 }] } };
  for (const from of [false, true]) {
    const list = anchorsOf(board, "point", [0, 0], from);
    assert.equal(list.length, 1);
    assert.deepEqual([list[0]!.pos, list[0]!.flags, list[0]!.pointTypes], [[3_000_000, 4_000_000], ANCHOR.ORIGIN | ANCHOR.SNAPPABLE, PT.CENTER]);
    assert.deepEqual(anchorsOf(board, "point", [0, 0], from, ctx(always, { points: false })), [], `filtered out (from ${from})`);
  }
  // a point that is not shown has no anchor
  const hidden: AnchorContext = { ...ctx(), visible: () => false };
  assert.deepEqual(anchorsOf(board, "point", [0, 0], false, hidden), []);
});
