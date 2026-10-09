import { test } from "node:test";
import assert from "node:assert/strict";
import type { LibPin } from "../../api/types";
import { DEFAULT_PIN_TEXTS, pinTextPlacements, shownPinName } from "./pinText";
import { resolvePin, symbolTransformMatrix } from "./transform";

const close = (a: number, b: number) => assert.ok(Math.abs(a - b) < 0.01, `${a} is not ${b}`);
const at = (p: { at: [number, number] } | null, x: number, y: number) => {
  assert.ok(p, "the text is there");
  close(p.at[0], x);
  close(p.at[1], y);
};

// the same cases, with the same numbers, as `pin_texts_sit_where_the_studio_painter_has_them` in crates/model/src/kicad_geom/tests.rs
const OFF = 203.2 + 635 + 152.4; // clearance, half of 1.27 mm, the pen

test("a pin running right: the name inside the body past the offset, the number over the line", () => {
  const p = pinTextPlacements({ name: "VIN", number: "3" }, [0, 0], [2540, 0]);
  at(p.name, 2540 + 508, 0);
  assert.equal(p.name!.h, "left");
  assert.equal(p.name!.vertical, false);
  at(p.number, 1270, -OFF);
  assert.equal(p.number!.h, "center");
});

test("a pin running left writes its name right-justified", () => {
  const p = pinTextPlacements({ name: "OUT", number: "2" }, [0, 0], [-2540, 0]);
  at(p.name, -2540 - 508, 0);
  assert.equal(p.name!.h, "right");
  at(p.number, -1270, -OFF);
});

test("a pin running up writes its name turned a quarter, from the inner end up; the number is left of the line", () => {
  const p = pinTextPlacements({ name: "VDD", number: "1" }, [0, 0], [0, -2540]);
  at(p.name, 0, -2540 - 508);
  assert.equal(p.name!.vertical, true);
  assert.equal(p.name!.h, "left");
  at(p.number, -OFF, -1270);
  assert.equal(p.number!.vertical, true);
});

test("a pin running down writes its name right-justified", () => {
  const p = pinTextPlacements({ name: "GND", number: "2" }, [0, 0], [0, 2540]);
  at(p.name, 0, 2540 + 508);
  assert.equal(p.name!.h, "right");
  at(p.number, -OFF, 1270);
});

test("a symbol whose names are at offset zero writes them over the pin and the numbers under it", () => {
  const texts = { ...DEFAULT_PIN_TEXTS, nameOffsetUm: 0 };
  const h = pinTextPlacements({ name: "IN", number: "1" }, [0, 0], [2540, 0], texts);
  at(h.name, 1270, -OFF);
  at(h.number, 1270, OFF);
  const v = pinTextPlacements({ name: "IN", number: "1" }, [0, 0], [0, -2540], texts);
  at(v.name, -OFF, -1270);
  at(v.number, OFF, -1270);
});

test("hidden names and hidden numbers are not placed; a name of ~ is no name", () => {
  const a = pinTextPlacements({ name: "A", number: "1" }, [0, 0], [2540, 0], { ...DEFAULT_PIN_TEXTS, namesHidden: true });
  assert.equal(a.name, null);
  assert.ok(a.number);
  const b = pinTextPlacements({ name: "A", number: "1" }, [0, 0], [2540, 0], { ...DEFAULT_PIN_TEXTS, numbersHidden: true });
  assert.ok(b.name);
  assert.equal(b.number, null);
  const c = pinTextPlacements({ name: "~", number: "1" }, [0, 0], [2540, 0]);
  assert.equal(c.name, null);
  assert.equal(shownPinName("~"), "");
  // with no name shown the number stays over the line even for a symbol whose names are over the pins
  const d = pinTextPlacements({ name: "~", number: "1" }, [0, 0], [2540, 0], { ...DEFAULT_PIN_TEXTS, nameOffsetUm: 0 });
  at(d.number, 1270, -OFF);
});

test("a library pin as the painter resolves it: on the left of a body, its name is inside the body and over the pin line is its number", () => {
  // the pin's free end is at its `at`; it runs into the body at `angle_deg`; `resolvePin` gives the tip and the root
  const lib: LibPin = { number: "1", name: "A", electrical_type: "passive", shape: "line", at: [-3.81, 0], angle_deg: 0, length_mm: 2.54, unit: 1, body_style: 1, hidden: false };
  const rp = resolvePin(lib, symbolTransformMatrix(0, null), [10_000, 20_000]);
  const p = pinTextPlacements(rp.pin, rp.tip, rp.root);
  assert.ok(p.name!.at[0] > rp.root[0], "the name starts inside the body, past the pin's inner end");
  assert.equal(p.name!.h, "left");
  assert.ok(p.number!.at[0] > rp.tip[0] && p.number!.at[0] < rp.root[0], "the number is along the pin line");
  assert.ok(p.number!.at[1] < rp.tip[1], "over it");
  // mirrored, the pin is on the right of the body and everything turns about
  const mirrored = resolvePin(lib, symbolTransformMatrix(0, "y"), [10_000, 20_000]);
  const q = pinTextPlacements(mirrored.pin, mirrored.tip, mirrored.root);
  assert.ok(q.name!.at[0] < mirrored.root[0]);
  assert.equal(q.name!.h, "right");
});
