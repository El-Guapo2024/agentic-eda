import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_ARRAY_OPTIONS,
  arrayCmd,
  arrayGeometry,
  arrayOrigin,
  checkArrayOptions,
  effectiveAngleDeg,
  entriesOf,
  gridPoints,
  optionsFromEntries,
  previewPoints,
  rememberedOptions,
  type ArrayEntries,
  type ArrayOptions,
} from "./arrayOptions";

const grid = (over: Partial<ArrayOptions> = {}): ArrayOptions => ({ ...DEFAULT_ARRAY_OPTIONS, tab: "grid", ...over });
const circular = (over: Partial<ArrayOptions> = {}): ArrayOptions => ({ ...DEFAULT_ARRAY_OPTIONS, tab: "circular", ...over });
const typed = (base: ArrayOptions, over: Partial<ArrayEntries> = {}): ArrayEntries => ({ ...entriesOf(base, "mm"), ...over });

test("the dialog starts from KiCad's own entries: 5 x 5 at 2.54 mm, four points a quarter turn apart, unique references", () => {
  const d = DEFAULT_ARRAY_OPTIONS;
  assert.deepEqual([d.nx, d.ny, d.dx, d.dy, d.stagger, d.count, d.angleDeg, d.clockwise, d.arrange, d.reannotate], [5, 5, 2540, 2540, 1, 4, 90, true, false, true]);
  assert.equal(d.centred, true, "both position radios are restored true, the second one stays selected");
  assert.equal(checkArrayOptions(d), null);
});

test("a grid with spacing of zero between several, or no rows, is refused with the dialog's words", () => {
  assert.equal(checkArrayOptions(grid({ dx: 0 })), "horizontal delta of zero with 5 objects");
  assert.equal(checkArrayOptions(grid({ dy: 0 })), "vertical delta of zero with 5 objects");
  assert.equal(checkArrayOptions(grid({ dx: 0, dy: 0 })), "horizontal delta of zero with 5 objects\nvertical delta of zero with 5 objects", "every complaint, one to a line");
  assert.equal(checkArrayOptions(grid({ nx: 1, dx: 0 })), null, "one column needs no spacing");
  assert.equal(checkArrayOptions(grid({ ny: 0 })), "A grid array needs at least one row and one column.");
  assert.equal(checkArrayOptions(grid({ nx: 2.5 })), "Bad numeric value for horizontal count: 2.5");
  assert.equal(checkArrayOptions(grid({ stagger: 0.5 })), "Bad numeric value for stagger: 0.5");
});

test("a circle with an angle of zero between several points is refused, unless Full circle gives it one", () => {
  assert.equal(checkArrayOptions(circular({ angleDeg: 0 })), "angular delta of zero with 4 objects");
  assert.equal(checkArrayOptions(circular({ angleDeg: 0, fullCircle: true })), null);
  assert.equal(checkArrayOptions(circular({ angleDeg: 0, count: 1 })), null, "one point needs no angle");
  assert.equal(checkArrayOptions(circular({ count: 0 })), "A circular array needs at least one point.");
  assert.equal(checkArrayOptions(circular({ count: 1.5 })), "Bad numeric value for point count: 1.5");
});

test("only the tab that is shown is checked", () => {
  assert.equal(checkArrayOptions(circular({ dx: 0, nx: 0 })), null, "the grid's entries are of no concern to the circle");
  assert.equal(checkArrayOptions(grid({ count: 0, angleDeg: 0 })), null);
});

test("Full circle is 360 over the count", () => {
  assert.equal(effectiveAngleDeg({ fullCircle: true, count: 8, angleDeg: 90 }), 45);
  assert.equal(effectiveAngleDeg({ fullCircle: false, count: 8, angleDeg: 90 }), 90);
  const g = arrayGeometry(circular({ fullCircle: true, count: 7 }));
  assert.equal(g.kind === "circular" && g.angle_millideg, Math.round(360_000 / 7));
});

test("the geometry the backend takes carries every option of the current tab", () => {
  assert.deepEqual(arrayGeometry(grid({ nx: 3, ny: 2, dx: 5000, dy: 4000, offsetX: 100, offsetY: -50, stagger: -2, staggerRows: false, centred: true, verticalFirst: true, reverseAlternate: true })), {
    kind: "grid",
    nx: 3,
    ny: 2,
    dx: 5000,
    dy: 4000,
    offset_x: 100,
    offset_y: -50,
    centred: true,
    stagger: -2,
    stagger_rows: false,
    horizontal_then_vertical: false,
    reverse_alternate: true,
  });
  assert.deepEqual(arrayGeometry(circular({ centerX: 1000, centerY: 2000, count: 6, angleDeg: 60, offsetAngleDeg: 15, clockwise: false, rotateItems: true })), {
    kind: "circular",
    center: { x: 1000, y: 2000 },
    count: 6,
    angle_millideg: 60_000,
    angle_offset_millideg: 15_000,
    clockwise: false,
    rotate_items: true,
  });
});

test("the command is create_array over the selection, with arrange and the reference choice", () => {
  assert.deepEqual(arrayCmd(grid({ arrange: true, reannotate: false, nx: 2, ny: 1 }), ["U1", "U2"]), {
    op: "create_array",
    ids: ["U1", "U2"],
    geometry: arrayGeometry(grid({ nx: 2, ny: 1 })),
    arrange: true,
    reannotate: false,
  });
});

test("the circle's centre starts at a lone item's position, or the middle of the box around several", () => {
  const at: Record<string, [number, number]> = { A: [1000, 2000], B: [0, 0], C: [4000, 2000], D: [1000, 500] };
  const box: Record<string, [number, number, number, number]> = { A: [900, 1900, 1100, 2100], B: [-100, -100, 100, 100], C: [3900, 1900, 4100, 2100], D: [900, 400, 1100, 600] };
  const position = (id: string) => at[id] ?? null;
  const bounds = (id: string) => box[id] ?? null;
  assert.deepEqual(arrayOrigin(["A"], position, bounds), [1000, 2000]);
  assert.deepEqual(arrayOrigin(["B", "C", "D"], position, bounds), [2000, 1000], "the box is -100..4100 by -100..2100");
  assert.equal(arrayOrigin([], position, bounds), null);
  assert.equal(arrayOrigin(["nothing"], position, bounds), null);
  assert.deepEqual(arrayOrigin(["B", "nothing", "D"], position, bounds), [500, 250], "an id with no box is left out of it");
  assert.equal(arrayOrigin(["nothing", "nowhere"], position, bounds), null);
});

test("the grid's points are the backend's: across then down, a serpentine, skewed, staggered and centred", () => {
  const p = (o: Partial<ArrayOptions>) => gridPoints(grid({ stagger: 0, centred: false, offsetX: 0, offsetY: 0, ...o }));
  assert.deepEqual(p({ nx: 3, ny: 2, dx: 1000, dy: 2000 }), [[0, 0], [1000, 0], [2000, 0], [0, 2000], [1000, 2000], [2000, 2000]]);
  assert.deepEqual(p({ nx: 3, ny: 2, dx: 1000, dy: 2000, verticalFirst: true }), [[0, 0], [0, 2000], [1000, 0], [1000, 2000], [2000, 0], [2000, 2000]]);
  assert.deepEqual(p({ nx: 3, ny: 2, dx: 1000, dy: 2000, reverseAlternate: true }), [[0, 0], [1000, 0], [2000, 0], [2000, 2000], [1000, 2000], [0, 2000]]);
  assert.deepEqual(p({ nx: 2, ny: 2, dx: 1000, dy: 2000, offsetX: 300, offsetY: 50 })[3], [1300, 2050]);
  assert.deepEqual(p({ nx: 2, ny: 2, dx: 1000, dy: 2000, offsetX: 300, offsetY: 50, centred: true })[0], [-650, -1025]);
  assert.deepEqual(p({ nx: 2, ny: 3, dx: 1000, dy: 2000, stagger: 2 }).filter((_, i) => i % 2 === 0), [[0, 0], [500, 2000], [0, 4000]]);
  assert.deepEqual(p({ nx: 1, ny: 3, dx: 1000, dy: 2000, stagger: 3 }).slice(1), [[333, 2000], [666, 4000]], "1000 / 3 truncates toward zero");
  assert.deepEqual(p({ nx: 2, ny: 2, dx: 1000, dy: 2000, stagger: 2, staggerRows: false })[1], [1000, 1000]);
  assert.deepEqual(p({ nx: 0, ny: 2 }), [], "no column, no point");
});

test("the entries show the options in the display units, and the angle as the division when Full circle is on", () => {
  const e = entriesOf(grid({ dx: 2540, dy: 1270, offsetX: 5, centerX: -12345, count: 8, fullCircle: true }), "mm");
  assert.deepEqual([e.nx, e.ny, e.dx, e.dy, e.offsetX, e.offsetY, e.stagger], ["5", "5", "2.54", "1.27", "0.005", "0", "1"]);
  assert.deepEqual([e.centerX, e.count, e.angle, e.offsetAngle], ["-12.345", "8", "45", "0"]);
  assert.equal(entriesOf(grid(), "mil").dx, "100");
  assert.equal(entriesOf(grid(), "in").dx, "0.1");
});

test("typing in the entries is read the way TransferDataFromWindow reads them", () => {
  const ok = optionsFromEntries(grid(), typed(grid(), { nx: " 3 ", ny: "2", dx: "5", dy: "1.5mm", offsetX: "10mil", stagger: "-2" }), "mm");
  assert.equal(ok.ok, true);
  if (ok.ok) {
    assert.deepEqual([ok.options.nx, ok.options.ny, ok.options.dx, ok.options.dy, ok.options.offsetX, ok.options.stagger], [3, 2, 5000, 1500, 254, -2]);
    assert.equal(ok.options.tab, "grid");
    assert.equal(ok.options.reannotate, true, "the flags are the base's");
  }
  const circle = optionsFromEntries(circular(), typed(circular(), { centerX: "10", centerY: "-2.5", count: "6", angle: "60°", offsetAngle: "15" }), "mm");
  assert.equal(circle.ok, true);
  if (circle.ok) assert.deepEqual([circle.options.centerX, circle.options.centerY, circle.options.count, circle.options.angleDeg, circle.options.offsetAngleDeg], [10000, -2500, 6, 60, 15]);
});

test("a count that is not whole, or a length that is not a length, is reported with the entry's name and what was typed", () => {
  const r = optionsFromEntries(grid(), typed(grid(), { nx: "3.5", ny: "abc", dx: "wide", stagger: "" }), "mm");
  assert.deepEqual(r, {
    ok: false,
    error: ["Bad numeric value for horizontal count: 3.5", "Bad numeric value for vertical count: abc", "Bad numeric value for horizontal spacing: wide", "Bad numeric value for stagger: "].join("\n"),
  });
  const c = optionsFromEntries(circular(), typed(circular(), { count: "x", angle: "round" }), "mm");
  assert.deepEqual(c, { ok: false, error: "Bad numeric value for point count: x\nBad numeric value for angle between items: round" });
});

test("what is wrong on the tab that is not shown is not held against the options", () => {
  const r = optionsFromEntries(grid(), typed(grid(), { count: "x", angle: "round", centerX: "?" }), "mm");
  assert.equal(r.ok, true);
  if (r.ok) assert.deepEqual([r.options.count, r.options.angleDeg, r.options.centerX], [4, 90, 0], "the previous numbers stay");
});

test("a zero spacing is found after the entries are read, and is the dialog's message", () => {
  assert.deepEqual(optionsFromEntries(grid(), typed(grid(), { dx: "0" }), "mm"), { ok: false, error: "horizontal delta of zero with 5 objects" });
  assert.deepEqual(optionsFromEntries(circular(), typed(circular(), { angle: "0" }), "mm"), { ok: false, error: "angular delta of zero with 4 objects" });
});

test("with Full circle on the typed angle is not read: the division is the angle", () => {
  const r = optionsFromEntries(circular({ fullCircle: true }), typed(circular({ fullCircle: true }), { angle: "nonsense", count: "10" }), "mm");
  assert.equal(r.ok, true);
  if (r.ok) assert.equal(effectiveAngleDeg(r.options), 36);
});

test("what is remembered for the next time keeps the angle the entry held", () => {
  assert.equal(rememberedOptions(circular({ fullCircle: true, count: 5, angleDeg: 90 })).angleDeg, 72);
  assert.equal(rememberedOptions(circular({ count: 5, angleDeg: 33 })).angleDeg, 33);
});

test("the preview puts a grid at the origin and a circle about its centre, clockwise on the screen", () => {
  const near = (got: Array<[number, number]>, want: Array<[number, number]>) => {
    assert.equal(got.length, want.length);
    got.forEach((g, i) => {
      assert.ok(Math.abs(g[0] - want[i]![0]) < 1e-6 && Math.abs(g[1] - want[i]![1]) < 1e-6, `${JSON.stringify(g)} is not ${JSON.stringify(want[i])}`);
    });
  };
  near(previewPoints(grid({ nx: 2, ny: 1, dx: 1000, stagger: 0, centred: false }), [500, 700]), [[500, 700], [1500, 700]]);
  near(previewPoints(circular({ centerX: 0, centerY: 0, count: 4, angleDeg: 90, offsetAngleDeg: 0 }), [1000, 0]), [[1000, 0], [0, 1000], [-1000, 0], [0, -1000]]);
  near(previewPoints(circular({ centerX: 0, centerY: 0, count: 2, angleDeg: 90, clockwise: false }), [1000, 0]), [[1000, 0], [0, -1000]]);
  near(previewPoints(circular({ centerX: 0, centerY: 0, count: 4, fullCircle: true, offsetAngleDeg: 90 }), [1000, 0]), [[0, 1000], [-1000, 0], [0, -1000], [1000, 0]]);
  assert.deepEqual(previewPoints(grid({ dx: 0 }), [0, 0]), [], "options that make nothing show nothing");
});
