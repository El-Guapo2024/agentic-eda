import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  buildWireWithHopShape,
  getHopOverScale,
  hopArcDraw,
  hopOverArcRadius,
  segIntersect,
  shouldHopOver,
  strokeWireShape,
  wireHopShapes,
  type HopWire,
  type Pt,
  type SchSeg,
  type WireShapeCanvas,
} from "./schHopOver";

const seg = (x0: number, y0: number, x1: number, y1: number): SchSeg => ({ start: [x0, y0], end: [x1, y1] });
/** Millimetres to the schematic's internal units (100 nm). */
const iu = (v: number): number => Math.round(v * 10_000);

test("ShouldHopOver: the horizontal wire hops the vertical one, else the shallower slope, of 45 degree wires the smaller slope", () => {
  const h = seg(0, 0, 100, 0);
  const v = seg(50, -50, 50, 50);
  assert.equal(shouldHopOver(h, v), true);
  assert.equal(shouldHopOver(v, h), false);
  const shallow = seg(0, 0, 100, 20);
  const steep = seg(0, 100, 20, 0);
  assert.equal(shouldHopOver(shallow, steep), true);
  assert.equal(shouldHopOver(steep, shallow), false);
  assert.equal(shouldHopOver(seg(0, 0, 100, 0), seg(0, 0, 100, 100)), true, "a horizontal wire hops a slanted one");
  // y down: (0,0)-(10,10) falls at slope 1, (0,10)-(10,0) rises at slope -1; the smaller slope hops
  const falls = seg(0, 0, 10, 10);
  const rises = seg(0, 10, 10, 0);
  assert.equal(shouldHopOver(falls, rises), false);
  assert.equal(shouldHopOver(rises, falls), true);
  assert.equal(shouldHopOver(seg(10, 10, 0, 0), rises), false, "the slope is the same whichever end starts the wire");
});

test("SEG::Intersect( aSeg, true, false ): the crossing, no crossing at a shared end, a T is a crossing the hop then skips", () => {
  assert.deepEqual(segIntersect(seg(0, 0, 100, 0), seg(50, -50, 50, 50), true), [50, 0]);
  assert.equal(segIntersect(seg(0, 0, 100, 0), seg(100, 0, 100, 50), true), null, "an L: the two ends meet");
  assert.deepEqual(segIntersect(seg(0, 0, 100, 0), seg(100, 0, 100, 50), false), [100, 0]);
  assert.deepEqual(segIntersect(seg(0, 0, 100, 0), seg(50, 0, 50, 50), true), [50, 0], "a T: one line's end on the other's middle");
  assert.equal(segIntersect(seg(0, 0, 100, 0), seg(50, 10, 50, 50), true), null, "short of the line");
  assert.equal(segIntersect(seg(0, 0, 100, 0), seg(0, 10, 100, 10), true), null, "parallel");
  assert.deepEqual(segIntersect(seg(0, 0, 100, 0), seg(40, 0, 160, 0), true), [70, 0], "collinear: the middle of the overlap");
  assert.equal(segIntersect(seg(0, 0, 100, 0), seg(100, 0, 160, 0), true), null, "collinear, end to end");
  // the point is on the second segment, rounded to the unit (`rescale`)
  assert.deepEqual(segIntersect(seg(0, 0, 3, 1), seg(0, 1, 3, 0), true), [2, 0], "(1.5, 0.5): 1.5 and -0.5 from (0, 1) round away from zero");
});

test("GetHopOverScale and the arc's radius: the default line width times the choice's multiplier, not the wire's width", () => {
  assert.deepEqual([0, 1, 2, 3, 4, 5].map(getHopOverScale), [0, 1.7, 4, 6, 9, 12]);
  assert.equal(getHopOverScale(6), 0, "past the six: None");
  // 6 mils is 1524 units
  assert.deepEqual([0, 1, 2, 3, 4, 5].map((c) => hopOverArcRadius(152_400, c)), [0, 1524 * 1.7, 6096, 9144, 13716, 18288]);
  assert.equal(hopOverArcRadius(203_200, 5), 24384, "an 8 mil default line width");
  assert.equal(hopOverArcRadius(undefined, 5), 18288, "an older backend: KiCad's 6 mils");
});

test("BuildWireWithHopShape: the hop's pieces -- R along the line each side of the crossing, its top R above", () => {
  const h = seg(0, 0, 100_000, 0);
  const v = seg(50_000, -40_000, 50_000, 40_000);
  const screen = [h, v];
  assert.deepEqual(buildWireWithHopShape(h, screen, 18288), [
    { arc: false, start: [0, 0], end: [31712, 0] },
    { arc: true, start: [31712, 0], mid: [50000, -18288], end: [68288, 0] },
    { arc: false, start: [68288, 0], end: [100_000, 0] },
  ]);
  assert.deepEqual(buildWireWithHopShape(v, screen, 18288), [{ arc: false, start: v.start, end: v.end }], "the vertical wire is hopped, not hopping");
  // Smallest: the ends KiROUND 2590.8 to 2591, the top truncates it to 2590
  assert.deepEqual(buildWireWithHopShape(h, screen, 1524 * 1.7)[1], { arc: true, start: [47409, 0], mid: [50000, -2590], end: [52591, 0] });
  // drawn the other way, the hop is still above and still runs from the wire's start
  const back = seg(100_000, 0, 0, 0);
  assert.deepEqual(buildWireWithHopShape(back, [back, v], 18288)[1], { arc: true, start: [68288, 0], mid: [50000, -18288], end: [31712, 0] });
  // a T (a wire ending on the other's middle) and an L are no crossings
  assert.equal(buildWireWithHopShape(h, [h, seg(50_000, 0, 50_000, 40_000)], 18288).length, 1);
  assert.equal(buildWireWithHopShape(h, [h, seg(100_000, 0, 100_000, 40_000)], 18288).length, 1);
});

test("BuildWireWithHopShape: several hops from the start, a point crossed by three wires hopped once", () => {
  const h = seg(100_000, 0, 0, 0);
  const near = seg(80_000, -40_000, 80_000, 40_000);
  const far = seg(20_000, -40_000, 20_000, 40_000);
  const pieces = buildWireWithHopShape(h, [h, far, near], 5000);
  assert.deepEqual(pieces.filter((p) => p.arc).map((p) => p.start), [[85_000, 0], [25_000, 0]], "sorted by the distance from the start");
  assert.equal(pieces.length, 5);
  const v2 = seg(80_000, -30_000, 80_000, 30_000);
  assert.equal(buildWireWithHopShape(h, [h, near, v2], 5000).filter((p) => p.arc).length, 1, "the point comes twice in a row: kept once");
});

/** The crossings `sch_setup_tests.rs`' slow test plots with kicad-cli: the wires, and for each choice the hop arcs' ends, in mm, as kicad-cli's
 *  `sch export svg --draw-hop-over` wrote them (both tests read this file). */
interface Fixture {
  wires: Array<{ pts: Array<[number, number]>; bus?: boolean }>;
  default_line_width_nm: number;
  arcs: Record<string, Array<{ wire: number; ends: [Pt, Pt]; radius_mm: number }>>;
}
const fixture = JSON.parse(readFileSync("src/kicad-port/schHopOver.kicadcli.json", "utf8")) as Fixture;

test("the hops kicad-cli plots: at each choice, the same arcs to the 100 nm unit", () => {
  const wires: HopWire[] = fixture.wires.map((w, i) => ({ id: `w${i}`, bus: w.bus ?? false, pts: w.pts.map(([x, y]) => [Math.round(x * 1e6), Math.round(y * 1e6)] as [number, number]) }));
  for (const [choice, expected] of Object.entries(fixture.arcs)) {
    const shapes = wireHopShapes(wires, Number(choice), fixture.default_line_width_nm);
    const got = [...shapes].flatMap(([id, pieces]) => pieces.flatMap((p) => (p.arc ? [{ wire: Number(id.slice(1)), ends: [p.start, p.end] as [Pt, Pt] }] : [])));
    assert.deepEqual(
      got,
      expected.map((a) => ({ wire: a.wire, ends: a.ends.map(([x, y]) => [iu(x), iu(y)]) })),
      `choice ${choice}`,
    );
    for (const a of expected) {
      const arc = shapes.get(`w${a.wire}`)!.find((p) => p.arc)!;
      assert.ok(arc.arc && Math.abs(hopArcDraw(arc).radius / 10_000 - a.radius_mm) < 0.0002, `choice ${choice}: radius ${a.radius_mm} mm`);
    }
  }
  assert.equal(wireHopShapes(wires, 0, fixture.default_line_width_nm).size, 0, "None: no hops (`hopOverScale > 0.0`)");
  assert.equal(wireHopShapes(wires, 5, fixture.default_line_width_nm), wireHopShapes(wires, 5, fixture.default_line_width_nm), "kept per wires array");
});

test("the painter's stroke: the straight pieces in the wire's dashes, each its own subpath, then the arcs solid, the hop over the top", () => {
  const calls: string[] = [];
  const n = (v: number): number => Math.round(v) || 0;
  const ctx: WireShapeCanvas = {
    setLineDash: (d) => calls.push(`dash ${d.join(",")}`),
    beginPath: () => calls.push("begin"),
    moveTo: (x, y) => calls.push(`M ${n(x)} ${n(y)}`),
    lineTo: (x, y) => calls.push(`L ${n(x)} ${n(y)}`),
    arc: (x, y, r, a0, a1, ccw) => calls.push(`A ${n(x)} ${n(y)} ${n(r)} ${a0.toFixed(4)} ${a1.toFixed(4)} ${ccw}`),
    stroke: () => calls.push("stroke"),
  };
  const h = seg(0, 0, 100_000, 0);
  strokeWireShape(ctx, buildWireWithHopShape(h, [h, seg(50_000, -40_000, 50_000, 40_000)], 18288), [5, 2]);
  assert.deepEqual(calls, [
    "dash 5,2",
    "begin",
    "M 0 0",
    "L 3171200 0",
    "M 6828800 0",
    "L 10000000 0",
    "stroke",
    "dash ",
    "begin",
    "M 3171200 0",
    // from the left (-180 degrees) through the top (y down: -90) to the right
    "A 5000000 0 1828800 -3.1416 0.0000 false",
    "stroke",
  ]);
  const d = hopArcDraw({ start: [31712, 0], mid: [50000, -18288], end: [68288, 0] });
  assert.deepEqual([d.center, d.radius, d.startAngle, d.sweep], [[50000, 0], 18288, -180, 180]);
});
