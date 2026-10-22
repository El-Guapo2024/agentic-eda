import { test } from "node:test";
import assert from "node:assert/strict";
import {
  arcStubAngleMillideg,
  centrelineRect,
  inductorEnd,
  inductorMinLengthUm,
  isValidValueName,
  lengthFromText,
  lengthText,
  lengthTextAtLeast,
  parseDouble,
  shapeSizeOverride,
  shapeSizeText,
} from "./microwave";

test("the angle is read as wxString::ToDouble reads it: all of the text, one number", () => {
  assert.equal(parseDouble("0.0"), 0);
  assert.equal(parseDouble("  90"), 90);
  assert.equal(parseDouble("-45.5"), -45.5);
  assert.equal(parseDouble("+.5"), 0.5);
  assert.equal(parseDouble("1e2"), 100);
  assert.equal(parseDouble(""), null);
  assert.equal(parseDouble("abc"), null);
  assert.equal(parseDouble("12 "), null, "a trailing blank is not part of a number");
  assert.equal(parseDouble("12deg"), null);
  assert.equal(parseDouble("1,5"), null, "the C locale has no decimal comma");
});

test("an arc stub's angle is its absolute value, at most a half turn", () => {
  assert.equal(arcStubAngleMillideg(90), 90_000);
  assert.equal(arcStubAngleMillideg(-90), 90_000);
  assert.equal(arcStubAngleMillideg(270), 180_000);
  assert.equal(arcStubAngleMillideg(37.5), 37_500);
  assert.equal(arcStubAngleMillideg(0), 0);
});

test("a component value cannot have the characters FOOTPRINT_NAME_VALIDATOR forbids", () => {
  assert.ok(isValidValueName("L"));
  assert.ok(isValidValueName("L2 22nH"));
  assert.ok(!isValidValueName(""));
  for (const bad of ["a/b", "a\\b", "a:b", "a%b", "a$b", "a<b", "a>b", 'a"b', "a\tb", "a\nb"]) assert.ok(!isValidValueName(bad), JSON.stringify(bad));
});

test("the inductor's end is held by the angle snap the way TWO_POINT_GEOMETRY_MANAGER::SetEnd holds it", () => {
  const start = { x: 1000, y: 1000 };
  // free: where the cursor is
  assert.deepEqual(inductorEnd(start, { x: 5000, y: 2000 }, "direct"), { x: 5000, y: 2000 });
  // 45: more than twice as wide as high is on the x axis; less than that is on the diagonal
  assert.deepEqual(inductorEnd(start, { x: 5000, y: 1500 }, "45"), { x: 5000, y: 1000 });
  assert.deepEqual(inductorEnd(start, { x: 5000, y: 4000 }, "45"), { x: 5000, y: 5000 }, "the coordinates are set equal, the magnitude is not kept");
  assert.deepEqual(inductorEnd(start, { x: 1500, y: -4000 }, "45"), { x: 1000, y: -4000 });
  assert.deepEqual(inductorEnd(start, { x: -2000, y: -3500 }, "45"), { x: -3500, y: -3500 }, "between the axis and the diagonal: the nearer coordinate is brought to the farther one");
  // 90: the nearer axis
  assert.deepEqual(inductorEnd(start, { x: 5000, y: 4000 }, "90"), { x: 5000, y: 1000 });
  assert.deepEqual(inductorEnd(start, { x: 2000, y: 4000 }, "90"), { x: 1000, y: 4000 });
  assert.deepEqual(inductorEnd(start, { x: 3000, y: 3000 }, "90"), { x: 3000, y: 1000 }, "a tie goes to x (`absVec.x >= absVec.y`)");
});

test("the length box starts at the distance between the points and never below it", () => {
  assert.equal(inductorMinLengthUm({ x: 0, y: 0 }, { x: 3000, y: 4000 }), 5000);
  assert.equal(lengthTextAtLeast(5000, "mm"), "5");
  // 1.41421356 mm: KiCad's box shows 1.4142 (less than the distance, so its own default is refused); this one shows 1.4143
  assert.equal(lengthTextAtLeast(inductorMinLengthUm({ x: 0, y: 0 }, { x: 1000, y: 1000 }), "mm"), "1.4143");
  assert.equal(lengthTextAtLeast(10_000, "mil"), "393.71");
  assert.equal(lengthTextAtLeast(25_400, "in"), "1");
});

test("lengths are shown to the digits of the units and read with or without a unit name", () => {
  assert.equal(lengthText(250, "mm"), "0.25");
  assert.equal(lengthText(250, "mil"), "9.84");
  assert.equal(lengthText(254, "in"), "0.01");
  assert.equal(lengthFromText("0.25", "mm"), 250);
  assert.equal(lengthFromText("10", "mil"), 254);
  assert.equal(lengthFromText("0.5 mm", "mil"), 500);
  assert.equal(lengthFromText('0.01"', "mm"), 254);
  assert.equal(lengthFromText("nonsense", "mm"), 0, "`ValueFromString` reads what is not a length as nothing");
});

test("the size boxes of the shape dialog: the file's scale shown, sent only once it was edited", () => {
  assert.equal(shapeSizeText(271_501_000, "mm"), "271.501");
  assert.equal(shapeSizeText(1_001_330, "mm"), "1.0013");
  assert.equal(shapeSizeOverride("271.501", "271.501", "mm"), null);
  assert.equal(shapeSizeOverride(" 271.501 ", "271.501", "mm"), null);
  assert.equal(shapeSizeOverride("300", "271.501", "mm"), 300_000);
});

test("the preview between the inductor's clicks is a rectangle along the centre line, half as wide as the line is long", () => {
  // along +x: 2000 long, so 1000 wide, centred on the line
  assert.deepEqual(centrelineRect({ x: 0, y: 0 }, { x: 2000, y: 0 }, 0.5), [[0, 500], [2000, 500], [2000, -500], [0, -500]]);
  // along +y
  assert.deepEqual(centrelineRect({ x: 0, y: 0 }, { x: 0, y: 1000 }, 0.5), [[-250, 0], [-250, 1000], [250, 1000], [250, 0]]);
  // no length: one unit along x, not a degenerate polygon
  assert.deepEqual(centrelineRect({ x: 5, y: 5 }, { x: 5, y: 5 }, 0.5), [[5, 5.25], [6, 5.25], [6, 4.75], [5, 4.75]]);
});
