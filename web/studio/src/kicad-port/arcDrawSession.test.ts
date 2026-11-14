import { test } from "node:test";
import assert from "node:assert/strict";
import { arcDrawModeSetting, arcSessionBack, arcSessionClick, arcSessionFinish, arcSessionSetMode, arcSessionShape } from "./arcDrawSession";
import { arcMotion, type ArcGeom } from "./arcGeom";
import { symTangentSeedAt } from "./symArcDraw";
import type { LibrarySymbol } from "../api/types";

const MM = 1_000_000;
const near = (a: readonly number[], b: readonly number[], tol = 2) => a.every((v, i) => Math.abs(v - b[i]!) <= tol);
const noSeed = () => null;

test("arcSessionClick: the centre mode's three clicks make the arc; the first click starts it, the last gives it and leaves nothing in progress (UI-LEFT-3 Y3-8)", () => {
  let s = arcSessionClick(null, [0, 0], "center_start_end", noSeed);
  assert.ok(s.started && s.geom && !s.arc);
  s = arcSessionClick(s.geom, [10 * MM, 0], "center_start_end", noSeed);
  assert.ok(!s.started && s.geom && !s.arc);
  const g = arcMotion(s.geom!, [0, 10 * MM], false);
  assert.ok(arcSessionShape(g), "a preview once the end angle follows the cursor");
  s = arcSessionClick(g, [0, 10 * MM], "center_start_end", noSeed);
  assert.ok(s.arc && s.geom === null);
  assert.ok(near(s.arc.center, [0, 0]) && Math.abs(s.arc.radius - 10 * MM) < 2);
});

test("arcSessionClick: start, end, mid keeps the clicked ends; a TANGENT arc chains its next from its end", () => {
  let s = arcSessionClick(null, [0, 0], "start_end_mid", noSeed);
  s = arcSessionClick(s.geom, [10 * MM, 0], "start_end_mid", noSeed);
  s = arcSessionClick(s.geom, [5 * MM, -5 * MM], "start_end_mid", noSeed);
  assert.ok(s.arc && s.geom === null);
  assert.ok(near(s.arc.start, [0, 0]) || near(s.arc.end, [0, 0]));
  assert.ok(near(s.arc.start, [10 * MM, 0]) || near(s.arc.end, [10 * MM, 0]));
  // tangent: start, direction, end; the committed arc seeds the next one at its end
  let t = arcSessionClick(null, [0, 0], "tangent", noSeed);
  t = arcSessionClick(t.geom, [5 * MM, 0], "tangent", noSeed);
  t = arcSessionClick(t.geom, [10 * MM, 10 * MM], "tangent", noSeed);
  assert.ok(t.arc, "the third click ends the tangent arc");
  assert.ok(t.geom?.hasSeed && t.geom.mode === "tangent", "the next arc waits, seeded at the end");
});

test("arcSessionClick: a TANGENT arc's first click on a graphic starts from it instead of placing a point (`findTangentSeed`)", () => {
  const seed = { start: [3 * MM, 0] as [number, number], direction: [1, 0] as [number, number], directionIsAxis: false };
  const s = arcSessionClick(null, [3 * MM, 100], "tangent", () => seed);
  assert.ok(s.started && s.geom?.hasSeed && !s.arc);
  // the other modes never ask
  let asked = false;
  arcSessionClick(null, [0, 0], "start_end_mid", () => ((asked = true), seed));
  assert.equal(asked, false);
});

test("arcSessionFinish / arcSessionBack / arcSessionSetMode: what finishing keeps, Backspace, the mode switched mid-draw", () => {
  let s = arcSessionClick(null, [0, 0], "center_start_end", noSeed);
  assert.equal(arcSessionFinish(s.geom), null, "one click is no arc");
  s = arcSessionClick(s.geom, [10 * MM, 0], "center_start_end", noSeed);
  const g = arcMotion(s.geom!, [0, 10 * MM], false);
  assert.ok(arcSessionFinish(g), "with its end angle it goes in as it stands");
  assert.equal(arcSessionFinish(null), null);
  const back = arcSessionBack(g)!;
  assert.ok(back && back.step < g.step, "Backspace steps back");
  assert.equal(arcSessionBack(arcSessionBack(back) ?? back), null, "back at the first point nothing is left");
  const switched = arcSessionSetMode(arcSessionClick(null, [0, 0], "start_end_mid", noSeed).geom, "start_end_center") as ArcGeom;
  assert.equal(switched.mode, "start_end_center");
  assert.equal(arcSessionSetMode(null, "tangent"), null);
});

test("arcDrawModeSetting: each editor keeps its own mode, CENTER_START_END at first", () => {
  const a = arcDrawModeSetting("test.a");
  const b = arcDrawModeSetting("test.b");
  assert.equal(a.get(), "center_start_end");
  a.set("tangent");
  assert.equal(a.get(), "tangent");
  assert.equal(b.get(), "center_start_end");
});

test("symTangentSeedAt: the symbol's graphics of the unit and body style being edited, within the pick distance (Symbol Editor's `findTangentSeed`)", () => {
  const sym = {
    graphics: [
      { kind: "polyline", unit: 2, body_style: 0, stroke_mm: 0, fill: "none", pts: [{ x: 0, y: 0 }, { x: 10, y: 0 }] },
      { kind: "polyline", unit: 0, body_style: 0, stroke_mm: 0, fill: "none", pts: [{ x: 0, y: 5 }, { x: 10, y: 5 }] },
      { kind: "rectangle", unit: 0, body_style: 0, stroke_mm: 0, fill: "none", start: { x: 20, y: 0 }, end: { x: 30, y: 10 } },
    ],
  } as unknown as Pick<LibrarySymbol, "graphics">;
  // the editor's space is +y down: the line at y = 5 mm is at -5 mm
  const at10 = symTangentSeedAt(sym, [10 * MM, -5 * MM], 1e-5, 1, 1);
  assert.ok(at10 && near(at10.start, [10 * MM, -5 * MM]), JSON.stringify(at10));
  assert.equal(symTangentSeedAt(sym, [10 * MM, 0], 1e-5, 1, 1), null, "unit 2's line is not on unit 1");
  assert.ok(symTangentSeedAt(sym, [10 * MM, 0], 1e-5, 2, 1), "but is on unit 2");
  assert.equal(symTangentSeedAt(sym, [20 * MM, 0], 1e-5, 1, 1), null, "a rectangle gives none");
});
