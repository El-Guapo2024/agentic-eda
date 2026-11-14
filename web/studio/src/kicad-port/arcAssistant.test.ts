import { test } from "node:test";
import assert from "node:assert/strict";
import { angleIsSpecial, arcAssistant, arcAssistantTexts, specialAngleColour } from "./arcAssistant";
import { arcSessionClick } from "./arcDrawSession";
import { arcMotion, newArcGeomIn, type ArcDrawMode, type ArcGeom } from "./arcGeom";

const MM = 1_000_000;
const noSeed = () => null;
/** 1 px per 0.01 mm. */
const SCALE = 1 / 10_000;

function clicks(mode: ArcDrawMode, pts: [number, number][], cursor: [number, number]): ArcGeom {
  let g: ArcGeom | null = null;
  for (const p of pts) g = arcSessionClick(g, p, mode, noSeed).geom;
  return arcMotion(g!, cursor, false);
}

test("ARC_ASSISTANT drawCenterGuides: r and θ while the start is placed, Δθ and θ while the end is (UI-LEFT-3 S3-28)", () => {
  // centre at the origin, the start 10 mm up (on screen: -y), counterclockwise 90 degrees from +x
  const placingStart = clicks("center_start_end", [[0, 0]], [0, -10 * MM]);
  const a = arcAssistant(placingStart, SCALE, "mm")!;
  assert.deepEqual(a.strings, ["r: 10.000 mm", "θ: 90.0°"]);
  assert.equal(a.lines.length, 1);
  assert.ok(a.lines[0]!.special && !a.lines[0]!.deemph, "the radius at 90 degrees is a special angle, not dimmed while the start is placed");
  // the text goes on the side away from the centre
  assert.deepEqual(a.quadrant, [0, 10 * MM]);

  const placingEnd = clicks("center_start_end", [[0, 0], [10 * MM, 0]], [0, -10 * MM]);
  const b = arcAssistant(placingEnd, SCALE, "mm")!;
  assert.equal(b.lines.length, 3, "the start radius, the end radius and the dimmed line to the cursor");
  assert.ok(b.lines[0]!.deemph && !b.lines[1]!.deemph && b.lines[2]!.deemph);
  assert.equal(b.strings[0], "Δθ: 90.0°");
  assert.equal(b.strings[1], "θ: 90.0°");
  assert.equal(arcAssistant(clicks("center_start_end", [[0, 0]], [3 * MM, 4 * MM]), SCALE, "mil")!.strings[0], "r: 196.9 mils");
});

test("ARC_ASSISTANT drawEndpointGuides: the chord modes give r, s and Δθ; the direction modes θ, then r and Δθ", () => {
  // start-end-mid: a half circle of radius 5 mm over the chord (0,0)-(10,0)
  const sem = clicks("start_end_mid", [[0, 0], [10 * MM, 0]], [5 * MM, -5 * MM]);
  const a = arcAssistant(sem, SCALE, "mm")!;
  assert.deepEqual(a.strings, ["r: 5.000 mm", "s: 5.000 mm", "Δθ: 180.0°"].map((s, i) => (i === 2 ? a.strings[2]! : s)));
  assert.match(a.strings[2]!, /^Δθ: -?180\.0°$/);
  assert.ok(a.lines[0]!.deemph && a.lines[0]!.special, "the fixed chord, dimmed, horizontal");
  assert.ok(a.lines.some((l) => l.dash), "the bisector guide is dashed");
  assert.equal(a.circles.length, 1, "the marker on the derived mid point");
  // the chord being placed: the line to the cursor and no strings
  const chord = arcAssistant(clicks("start_end_mid", [[0, 0]], [10 * MM, 0]), SCALE, "mm")!;
  assert.deepEqual(chord.strings, []);
  assert.ok(!chord.lines[0]!.deemph);

  // start-end-centre: dimmed lines from the centre to the ends
  const sec = arcAssistant(clicks("start_end_center", [[0, 0], [10 * MM, 0]], [5 * MM, 3 * MM]), SCALE, "mm")!;
  assert.equal(sec.lines.filter((l) => l.deemph).length, 3, "the fixed chord and the two radii");
  assert.equal(sec.strings[0]!.slice(0, 3), "r: ");

  // start-direction-end: the heading while the direction is placed, as `EDA_ANGLE( dir )` (+y down: 45 here)
  const dir = arcAssistant(clicks("start_dir_end", [[0, 0]], [MM, MM]), SCALE, "mm")!;
  assert.deepEqual(dir.strings, ["θ: 45.0°"]);
  const end = arcAssistant(clicks("start_dir_end", [[0, 0], [MM, 0]], [10 * MM, 10 * MM]), SCALE, "mm")!;
  assert.equal(end.strings.length, 2);
  assert.equal(end.strings[0], "r: 10.000 mm");
  assert.match(end.strings[1]!, /^Δθ: -?90\.0°$/);
});

test("DrawTextNextToCursor: the strings 15 px beside the cursor on the side the quadrant gives, one line pitch apart; the shadow pass wider", () => {
  const g = clicks("center_start_end", [[0, 0]], [0, -10 * MM]);
  const d = arcAssistant(g, SCALE, "mm")!;
  const t = arcAssistantTexts(d, SCALE);
  assert.equal(t.length, 2);
  // the quadrant's x is 0 (not negative): right-aligned on the left of the cursor; its y positive: above the cursor
  assert.equal(t[0]!.halign, "right");
  assert.ok(Math.abs(t[0]!.at[0] - -15 / SCALE) < 1);
  assert.ok(t[0]!.at[1] < -10 * MM && t[1]!.at[1] > t[0]!.at[1]);
  const s = arcAssistantTexts(d, SCALE, true);
  assert.ok(s[0]!.stroke > t[0]!.stroke);
});

test("angleIsSpecial and the special-angle colour", () => {
  assert.ok(angleIsSpecial([1, 0]) && angleIsSpecial([1, 1]) && angleIsSpecial([0, -3]) && angleIsSpecial([-2, 2]));
  assert.ok(!angleIsSpecial([2, 1]));
  assert.notEqual(specialAngleColour(true), specialAngleColour(false));
  // nothing to draw before a first click
  assert.equal(arcAssistant(newArcGeomIn("start_end_mid"), SCALE, "mm"), null);
});
