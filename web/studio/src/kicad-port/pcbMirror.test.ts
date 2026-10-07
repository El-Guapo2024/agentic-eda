import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Cmd, Shape } from "../api/types";
import { mirrorPoint, mirrorReferencePoint, mirrorShape, mirrorableIds, mirroredJustify, planMirror } from "./pcbMirror";

function board(partial: Partial<BoardState>): BoardState {
  return {
    name: "t",
    dir: "",
    outline: null,
    layers: ["F.Cu", "B.Cu"],
    snap: 100,
    parts: [],
    rules: [],
    routing: null,
    drawings: null,
    checks: [],
    activity: [],
    job: "idle",
    ...partial,
  } as BoardState;
}

const seg = (id: string, ax: number, ay: number, bx: number, by: number): Shape => ({ kind: "segment", id, layer: "F.SilkS", stroke_width: 150, filled: false, start: [ax, ay], end: [bx, by] });

test("mirrorPoint: left-right flips x about the centre, top-bottom flips y (MIRROR)", () => {
  assert.deepEqual(mirrorPoint([10, 4], [3, 7], "leftRight"), [-4, 4]);
  assert.deepEqual(mirrorPoint([10, 4], [3, 7], "topBottom"), [10, 10]);
});

test("mirrorShape: a segment's end points, a circle's centre and rim point (EDA_SHAPE::flip)", () => {
  const s = mirrorShape(seg("s", 0, 0, 10, 5), [0, 0], "leftRight");
  assert.deepEqual(s, { kind: "segment", layer: "F.SilkS", stroke_width: 150, filled: false, start: { x: 0, y: 0 }, end: { x: -10, y: 5 } });
  const c = mirrorShape({ kind: "circle", id: "c", layer: "F.Fab", stroke_width: 100, filled: true, center: [10, 10], end: [15, 10] }, [0, 0], "topBottom");
  assert.deepEqual(c, { kind: "circle", layer: "F.Fab", stroke_width: 100, filled: true, center: { x: 10, y: -10 }, end: { x: 15, y: -10 } });
});

test("mirrorShape: an arc swaps start and end so it keeps its direction", () => {
  const a = mirrorShape({ kind: "arc", id: "a", layer: "F.SilkS", stroke_width: 100, filled: false, start: [10, 0], mid: [7, 7], end: [0, 10] }, [0, 0], "leftRight");
  assert.equal(a.kind, "arc");
  if (a.kind !== "arc") return;
  assert.deepEqual(a.start, { x: 0, y: 10 });
  assert.deepEqual(a.mid, { x: -7, y: 7 });
  assert.deepEqual(a.end, { x: -10, y: 0 });
});

test("mirrorShape: polygon points keep their order", () => {
  const p = mirrorShape({ kind: "polygon", id: "p", layer: "F.Fab", stroke_width: 100, filled: false, pts: [[0, 0], [10, 0], [0, 10]] }, [5, 0], "leftRight");
  assert.equal(p.kind, "polygon");
  if (p.kind !== "polygon") return;
  assert.deepEqual(p.pts, [{ x: 10, y: 0 }, { x: 0, y: 0 }, { x: 10, y: 10 }]);
});

test("mirroredJustify: left-right flips a horizontal text's justification, top-bottom a vertical one's (PCB_TEXT::Mirror)", () => {
  assert.equal(mirroredJustify("left", 0, "leftRight"), "right");
  assert.equal(mirroredJustify("right", 0, "leftRight"), "left");
  assert.equal(mirroredJustify("center", 0, "leftRight"), "center");
  assert.equal(mirroredJustify("left", 90000, "leftRight"), "left");
  assert.equal(mirroredJustify("left", 90000, "topBottom"), "right");
  assert.equal(mirroredJustify("left", 0, "topBottom"), "left");
});

test("mirror reference point: a single item's own position, else the centre of the union box", () => {
  const b = board({
    drawings: { shapes: [seg("a", 0, 0, 10, 0), seg("b", 20, 10, 30, 10)], texts: [], groups: [], dimensions: [], dimension_settings: {} as never },
  });
  assert.deepEqual(mirrorReferencePoint(b, ["a"]), [0, 0]);
  const c = mirrorReferencePoint(b, ["a", "b"])!;
  // stroke width 150 pads each box by 75
  assert.deepEqual(c, [15, 5]);
});

test("mirrorableIds: groups expand to members and footprints are not mirrorable", () => {
  const b = board({
    parts: [{ ref: "U1", placed: true, at: [0, 0] } as never],
    drawings: { shapes: [seg("a", 0, 0, 10, 0)], texts: [], groups: [{ id: "g", name: "", member_ids: ["a", "U1"] }], dimensions: [], dimension_settings: {} as never },
  });
  assert.deepEqual(mirrorableIds(b, ["g", "U1"]), ["a"]);
});

test("planMirror: shapes are replaced, vias and text move, a zone's outline is rewritten, tracks come back through commit_route", () => {
  const b = board({
    routing: {
      tracks: [
        { id: "t1", net: "N", layer: "F.Cu", width: 200, pts: [[0, 0], [100, 0]] },
        { id: "t2", net: "N", layer: "F.Cu", width: 200, pts: [[0, 0], [10, 10]], arc_mid: [3, 7] },
      ],
      vias: [{ id: "v1", net: "N", x: 50, y: 20, d: 600, drill: 300, from: "F.Cu", to: "B.Cu" }],
      zones: [{ id: "z1", net: "N", layer: "F.Cu", outline: [[0, 0], [10, 0], [10, 10]] } as never],
      track_width_presets: [],
      via_presets: [],
      teardrop_settings: {} as never,
    },
    drawings: {
      shapes: [seg("s1", 0, 0, 10, 0)],
      texts: [{ id: "x1", content: "R1", x: 40, y: 10, angle: 0, layer: "F.SilkS", size: 1000, stroke_width: 150, justify: "left", mirror: false }],
      groups: [],
      dimensions: [],
      dimension_settings: {} as never,
    },
  });
  const plan = planMirror(b, ["t1", "t2", "v1", "z1", "s1", "x1"], [0, 0], "leftRight");
  const ops = plan.cmds.map((c: Cmd) => c.op);
  assert.deepEqual(ops, ["move_via", "set_zone_outline", "delete_shape", "add_shape", "edit_text", "move_text", "commit_route"]); // in id order, the tracks last
  assert.deepEqual(plan.removed.sort(), ["s1", "t1", "t2"]);
  const route = plan.cmds[plan.cmds.length - 1]!;
  assert.equal(route.op, "commit_route");
  if (route.op !== "commit_route") return;
  assert.deepEqual(route.remove_track_ids.sort(), ["t1", "t2"]);
  const [line, arc] = route.tracks!;
  assert.deepEqual(line!.pts, [{ x: 0, y: 0 }, { x: -100, y: 0 }]);
  assert.equal(line!.arc_mid_offset, undefined);
  // the arc keeps being an arc: its mid offset is relative to the new start
  assert.deepEqual(arc!.arc_mid_offset, { x: -3, y: 7 });
  assert.equal(arc!.pts.length, 33);
  const move = plan.cmds.find((c) => c.op === "move_via")!;
  assert.deepEqual(move, { op: "move_via", id: "v1", x: -50, y: 20 });
  const edit = plan.cmds.find((c) => c.op === "edit_text")!;
  assert.equal(edit.op === "edit_text" ? edit.justify : "", "right");
});
