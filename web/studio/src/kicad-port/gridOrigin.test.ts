import { test } from "node:test";
import assert from "node:assert/strict";
import { NO_ORIGIN, brightness, originFromEntries, originIsSet, originMarkerColor, originOf, parseCssColor, snapAxis } from "./gridOrigin";

test("a point snaps to the grid anchored at the origin, not at zero", () => {
  assert.equal(snapAxis(10_234, 100, 0), 10_200);
  assert.equal(snapAxis(10_234, 100, 50), 10_250);
  assert.equal(snapAxis(10_234, 100, 30), 10_230);
  assert.equal(snapAxis(-120, 100, 50), -150);
  assert.equal(snapAxis(10_234, 0, 50), 10_234, "no grid: the value, rounded");
  assert.equal(snapAxis(10.4, 0, 50), 10);
});

test("snapping to an origin is a shift of the ordinary grid", () => {
  for (const v of [0, 17, 99, 101, 12_345, -777]) {
    for (const o of [0, 25, -40, 1_250]) assert.equal(snapAxis(v, 100, o), snapAxis(v - o, 100, 0) + o);
  }
});

test("the marker is drawn only for an origin that was set", () => {
  assert.equal(originIsSet(null), false);
  assert.equal(originIsSet(NO_ORIGIN), false);
  assert.equal(originIsSet({ x: 0, y: 1 }), true);
  assert.deepEqual(originOf(null), NO_ORIGIN);
  assert.deepEqual(originOf([5, -6]), { x: 5, y: -6 });
});

test("CSS colours are read in the forms the studio writes", () => {
  assert.deepEqual(parseCssColor("#ff0000"), { r: 1, g: 0, b: 0, a: 1 });
  assert.equal(parseCssColor("#00ff0080")!.a.toFixed(2), "0.50");
  assert.deepEqual(parseCssColor("#fff"), { r: 1, g: 1, b: 1, a: 1 });
  assert.deepEqual(parseCssColor("rgb(255, 0, 255)"), { r: 1, g: 0, b: 1, a: 1 });
  assert.equal(parseCssColor("rgba(0, 0, 0, 0.4)")!.a, 0.4);
  assert.equal(parseCssColor("hotpink"), null);
});

test("brightness is the weighted W3C formula", () => {
  assert.equal(brightness({ r: 1, g: 1, b: 1, a: 1 }), 0.299 + 0.587 + 0.117);
  assert.equal(brightness({ r: 0, g: 0, b: 0, a: 1 }), 0);
});

test("the marker is the grid colour darkened on a bright background and brightened on a dark one", () => {
  // grid #808080 (0.502 per channel): darkened x0.75 = 0.3765 -> 96; brightened x0.75 + 0.25 = 0.6265 -> 160
  assert.equal(originMarkerColor("#808080", "#f5f4ef"), "rgba(96, 96, 96, 1)");
  assert.equal(originMarkerColor("#808080", "#001023"), "rgba(160, 160, 160, 1)");
  assert.equal(originMarkerColor("not a colour", "#001023"), "not a colour");
});

test("the dialog's X and Y entries are numbers in the display unit, converted to um", () => {
  const mm = (v: number) => v * 1000;
  assert.deepEqual(originFromEntries("13.37", "-8,1", mm), { x: 13_370, y: -8_100 });
  assert.equal(originFromEntries("", "1", mm), null);
  assert.equal(originFromEntries("abc", "1", mm), null);
  assert.deepEqual(originFromEntries(" 2 ", " 3 ", mm), { x: 2_000, y: 3_000 });
});
