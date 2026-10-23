import { test } from "node:test";
import assert from "node:assert/strict";
import {
  arcStubAngleMillideg,
  centrelineRect,
  inductorEnd,
  inductorMinLengthNm,
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
  assert.equal(parseDouble("1e2"), 100_000);
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
  assert.equal(arcStubAngleMillideg(37.5), 37_500_000);
  assert.equal(arcStubAngleMillideg(0), 0);
});

test("a component value cannot have the characters FOOTPRINT_NAME_VALIDATOR forbids", () => {
  assert.ok(isValidValueName("L"));
  assert.ok(isValidValueName("L2 22nH"));
  assert.ok(!isValidValueName(""));
  for (const bad of ["a/b", "a\\b", "a:b", "a%b", "a$b", "a<b", "a>b", 'a"b', "a\tb", "a\nb"]) assert.ok(!isValidValueName(bad), JSON.stringify(bad));
});

test("the inductor's end is held by the angle snap the way TWO_POINT_GEOMETRY_MANAGER::SetEnd holds it", () => {
  const start = { x: 1_000_000, y: 1_000_000 };
  // free: where the cursor is
  assert.deepEqual(inductorEnd(start, { x: 5_000_000, y: 2_000_000 }, "direct"), { x: 5_000_000, y: 2_000_000 });
  // 45: more than twice as wide as high is on the x axis; less than that is on the diagonal
  assert.deepEqual(inductorEnd(start, { x: 5_000_000, y: 1_500_000 }, "45"), { x: 5_000_000, y: 1_000_000 });
  assert.deepEqual(inductorEnd(start, { x: 5_000_000, y: 4_000_000 }, "45"), { x: 5_000_000, y: 5_000_000 }, "the coordinates are set equal, the magnitude is not kept");
  assert.deepEqual(inductorEnd(start, { x: 1_500_000, y: -4_000_000 }, "45"), { x: 1_000_000, y: -4_000_000 });
  assert.deepEqual(inductorEnd(start, { x: -2_000_000, y: -3_500_000 }, "45"), { x: -3_500_000, y: -3_500_000 }, "between the axis and the diagonal: the nearer coordinate is brought to the farther one");
  // 90: the nearer axis
  assert.deepEqual(inductorEnd(start, { x: 5_000_000, y: 4_000_000 }, "90"), { x: 5_000_000, y: 1_000_000 });
  assert.deepEqual(inductorEnd(start, { x: 2_000_000, y: 4_000_000 }, "90"), { x: 1_000_000, y: 4_000_000 });
  assert.deepEqual(inductorEnd(start, { x: 3_000_000, y: 3_000_000 }, "90"), { x: 3_000_000, y: 1_000_000 }, "a tie goes to x (`absVec.x >= absVec.y`)");
});

test("the length box starts at the distance between the points and never below it", () => {
  assert.equal(inductorMinLengthNm({ x: 0, y: 0 }, { x: 3_000_000, y: 4_000_000 }), 5_000_000);
  assert.equal(lengthTextAtLeast(5_000_000, "mm"), "5");
  // 1.41421356 mm: KiCad's box shows 1.4142 (less than the distance, so its own default is refused); this one shows 1.4143
  assert.equal(lengthTextAtLeast(inductorMinLengthNm({ x: 0, y: 0 }, { x: 1_000_000, y: 1_000_000 }), "mm"), "1.4143");
  assert.equal(lengthTextAtLeast(10_000_000, "mil"), "393.71");
  assert.equal(lengthTextAtLeast(25_400_000, "in"), "1");
});

test("lengths are shown to the digits of the units and read with or without a unit name", () => {
  assert.equal(lengthText(250_000, "mm"), "0.25");
  assert.equal(lengthText(250_000, "mil"), "9.84");
  assert.equal(lengthText(254_000, "in"), "0.01");
  assert.equal(lengthFromText("0.25", "mm"), 250_000);
  assert.equal(lengthFromText("10", "mil"), 254_000);
  assert.equal(lengthFromText("0.5 mm", "mil"), 500_000);
  assert.equal(lengthFromText('0.01"', "mm"), 254_000);
  assert.equal(lengthFromText("nonsense", "mm"), 0, "`ValueFromString` reads what is not a length as nothing");
});

test("the size boxes of the shape dialog: the file's scale shown, sent only once it was edited", () => {
  assert.equal(shapeSizeText(271_501_000_000, "mm"), "271.501");
  assert.equal(shapeSizeText(1_001_330_000, "mm"), "1.0013");
  assert.equal(shapeSizeOverride("271.501", "271.501", "mm"), null);
  assert.equal(shapeSizeOverride(" 271.501 ", "271.501", "mm"), null);
  assert.equal(shapeSizeOverride("300", "271.501", "mm"), 300_000_000);
});

test("the preview between the inductor's clicks is a rectangle along the centre line, half as wide as the line is long", () => {
  // along +x: 2000 long, so 1000 wide, centred on the line
  assert.deepEqual(centrelineRect({ x: 0, y: 0 }, { x: 2_000_000, y: 0 }, 0.5), [[0, 500_000], [2_000_000, 500_000], [2_000_000, -500_000], [0, -500_000]]);
  // along +y
  assert.deepEqual(centrelineRect({ x: 0, y: 0 }, { x: 0, y: 1_000_000 }, 0.5), [[-250_000, 0], [-250_000, 1_000_000], [250_000, 1_000_000], [250_000, 0]]);
  // no length: one unit along x, not a degenerate polygon
  assert.deepEqual(centrelineRect({ x: 5000, y: 5000 }, { x: 5000, y: 5000 }, 0.5), [[5000, 5.25], [6000, 5.25], [6000, 4.75], [5000, 4.75]]);
});
