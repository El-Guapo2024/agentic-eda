import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_ARRAY_OPTIONS,
  DEFAULT_PAD_NUMBERING,
  arrayCmd,
  arrayGeometry,
  arrayOrigin,
  checkArrayOptions,
  effectiveAngleDeg,
  entriesOf,
  fpArrayCmd,
  gridPoints,
  optionsFromEntries,
  padNumberingOf,
  previewPadNumbers,
  previewPoints,
  rememberedOptions,
  type ArrayEntries,
  type ArrayOptions,
} from "./arrayOptions";
import type { LibraryPad } from "../api/types";

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
    nx: 3000,
    ny: 2000,
    dx: 5_000_000,
    dy: 4_000_000,
    offset_x: 100_000,
    offset_y: -50_000,
    centred: true,
    stagger: -2000,
    stagger_rows: false,
    horizontal_then_vertical: false,
    reverse_alternate: true,
  });
  assert.deepEqual(arrayGeometry(circular({ centerX: 1000, centerY: 2000, count: 6, angleDeg: 60, offsetAngleDeg: 15, clockwise: false, rotateItems: true })), {
    kind: "circular",
    center: { x: 1_000_000, y: 2_000_000 },
    count: 6,
    angle_millideg: 60_000_000,
    angle_offset_millideg: 15_000_000,
    clockwise: false,
    rotate_items: true,
  });
});

test("the command is create_array over the selection, with arrange and the reference choice", () => {
  assert.deepEqual(arrayCmd(grid({ arrange: true, reannotate: false, nx: 2000, ny: 1 }), ["U1", "U2"]), {
    op: "create_array",
    ids: ["U1", "U2"],
    geometry: arrayGeometry(grid({ nx: 2000, ny: 1 })),
    arrange: true,
    reannotate: false,
  });
});

test("the circle's centre starts at a lone item's position, or the middle of the box around several", () => {
  const at: Record<string, [number, number]> = { A: [1000, 2000], B: [0, 0], C: [4_000_000, 2_000_000], D: [1_000_000, 500_000] };
  const box: Record<string, [number, number, number, number]> = { A: [900, 1900, 1100, 2100], B: [-100, -100, 100, 100], C: [3_900_000, 1_900_000, 4_100_000, 2_100_000], D: [900, 400_000, 1_100_000, 600_000] };
  const position = (id: string) => at[id] ?? null;
  const bounds = (id: string) => box[id] ?? null;
  assert.deepEqual(arrayOrigin(["A"], position, bounds), [1_000_000, 2_000_000]);
  assert.deepEqual(arrayOrigin(["B", "C", "D"], position, bounds), [2_000_000, 1_000_000], "the box is -100..4100 by -100..2100");
  assert.equal(arrayOrigin([], position, bounds), null);
  assert.equal(arrayOrigin(["nothing"], position, bounds), null);
  assert.deepEqual(arrayOrigin(["B", "nothing", "D"], position, bounds), [500_000, 250_000], "an id with no box is left out of it");
  assert.equal(arrayOrigin(["nothing", "nowhere"], position, bounds), null);
});

test("the grid's points are the backend's: across then down, a serpentine, skewed, staggered and centred", () => {
  const p = (o: Partial<ArrayOptions>) => gridPoints(grid({ stagger: 0, centred: false, offsetX: 0, offsetY: 0, ...o }));
  assert.deepEqual(p({ nx: 3000, ny: 2000, dx: 1_000_000, dy: 2_000_000 }), [[0, 0], [1_000_000, 0], [2_000_000, 0], [0, 2_000_000], [1_000_000, 2_000_000], [2_000_000, 2_000_000]]);
  assert.deepEqual(p({ nx: 3000, ny: 2000, dx: 1_000_000, dy: 2_000_000, verticalFirst: true }), [[0, 0], [0, 2_000_000], [1_000_000, 0], [1_000_000, 2_000_000], [2_000_000, 0], [2_000_000, 2_000_000]]);
  assert.deepEqual(p({ nx: 3000, ny: 2000, dx: 1_000_000, dy: 2_000_000, reverseAlternate: true }), [[0, 0], [1_000_000, 0], [2_000_000, 0], [2_000_000, 2_000_000], [1_000_000, 2_000_000], [0, 2_000_000]]);
  assert.deepEqual(p({ nx: 2, ny: 2, dx: 1000, dy: 2000, offsetX: 300, offsetY: 50 })[3], [1300, 2050]);
  assert.deepEqual(p({ nx: 2, ny: 2, dx: 1000, dy: 2000, offsetX: 300, offsetY: 50, centred: true })[0], [-650, -1025]);
  assert.deepEqual(p({ nx: 2000, ny: 3000, dx: 1_000_000, dy: 2_000_000, stagger: 2000 }).filter((_, i) => i % 2 === 0), [[0, 0], [500_000, 2_000_000], [0, 4_000_000]]);
  assert.deepEqual(p({ nx: 1, ny: 3000, dx: 1_000_000, dy: 2_000_000, stagger: 3000 }).slice(1), [[333_000, 2_000_000], [666_000, 4_000_000]], "1000 / 3 truncates toward zero");
  assert.deepEqual(p({ nx: 2000, ny: 2000, dx: 1_000_000, dy: 2_000_000, stagger: 2000, staggerRows: false })[1], [1_000_000, 1_000_000]);
  assert.deepEqual(p({ nx: 0, ny: 2000 }), [], "no column, no point");
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
  if (r.ok) assert.equal(effectiveAngleDeg(r.options), 36_000);
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
  near(previewPoints(grid({ nx: 2000, ny: 1, dx: 1_000_000, stagger: 0, centred: false }), [500_000, 700_000]), [[500_000, 700_000], [1_500_000, 700_000]]);
  near(previewPoints(circular({ centerX: 0, centerY: 0, count: 4, angleDeg: 90, offsetAngleDeg: 0 }), [1000, 0]), [[1000, 0], [0, 1000], [-1000, 0], [0, -1000]]);
  near(previewPoints(circular({ centerX: 0, centerY: 0, count: 2, angleDeg: 90, clockwise: false }), [1000, 0]), [[1000, 0], [0, -1000]]);
  near(previewPoints(circular({ centerX: 0, centerY: 0, count: 4, fullCircle: true, offsetAngleDeg: 90 }), [1000, 0]), [[0, 1000], [-1000, 0], [0, -1000], [1000, 0]]);
  assert.deepEqual(previewPoints(grid({ dx: 0 }), [0, 0]), [], "options that make nothing show nothing");
});

// ------------------------------------------------------------------------------------------------------------------- the footprint editor's pad numbering

test("the footprint editor's dialog starts renumbering from the start value 1, in one continuous run of numerals on both tabs", () => {
  const p = DEFAULT_PAD_NUMBERING;
  assert.deepEqual([p.renumber, p.gridFromStart, p.gridCoordinates, p.circFromStart], [true, true, false, true]);
  assert.deepEqual(p.primary, { kind: "numeric", start: "1", step: "1" });
  assert.deepEqual(padNumberingOf(grid()), { ok: true, numbering: { start_specified: true, two_d: false, primary: { kind: "numeric", start: "1", step: 1 }, secondary: { kind: "numeric", start: "1", step: 1 } } });
});

test("\"Renumber pads\" off leaves a grid's copies the number of their source; a circle always renumbers", () => {
  assert.deepEqual(padNumberingOf(grid({ pads: { ...DEFAULT_PAD_NUMBERING, renumber: false } })), { ok: true, numbering: null });
  const r = padNumberingOf(circular({ pads: { ...DEFAULT_PAD_NUMBERING, renumber: false } }));
  assert.ok(r.ok && r.numbering !== null);
});

test("a coordinate scheme sends both axes; \"Use first free number\" sends none of the entries' worth", () => {
  const pads = { ...DEFAULT_PAD_NUMBERING, gridCoordinates: true, primary: { kind: "alpha_full" as const, start: "b", step: "2" }, secondary: { kind: "hex" as const, start: "A", step: "1" } };
  assert.deepEqual(padNumberingOf(grid({ pads })), {
    ok: true,
    numbering: { start_specified: true, two_d: true, primary: { kind: "alpha_full", start: "b", step: 2000 }, secondary: { kind: "hex", start: "A", step: 1 } },
  });
  const free = padNumberingOf(grid({ pads: { ...pads, gridFromStart: false } }));
  assert.ok(free.ok && free.numbering?.start_specified === false && free.numbering.two_d === false);
  // a circle's one axis is the circular one
  const c = padNumberingOf(circular({ pads: { ...pads, circular: { kind: "hex", start: "F", step: "-1" } } }));
  assert.ok(c.ok && c.numbering?.primary.start === "F" && c.numbering.primary.step === -1 && c.numbering.two_d === false);
});

test("a start the alphabet lacks, or a step that is no whole number, is named the way the dialog names it, every one on a line", () => {
  const pads = { ...DEFAULT_PAD_NUMBERING, gridCoordinates: true, primary: { kind: "alpha_no_iosqxz" as const, start: "I", step: "1" }, secondary: { kind: "numeric" as const, start: "1", step: "1.5" } };
  assert.deepEqual(padNumberingOf(grid({ pads })), {
    ok: false,
    error: "Could not determine numbering start from 'I': expected value consistent with alphabet 'ABCDEFGHJKLMNPRTUVWY'.\nBad numeric value for step value: 1.5",
  });
  // the entries of an axis the scheme does not use are not looked at
  assert.equal(padNumberingOf(grid({ pads: { ...pads, gridCoordinates: false, primary: DEFAULT_PAD_NUMBERING.primary } })).ok, true);
});

test("the command sends the geometry and the numbering, and \"Arrange selection\" numbers nothing", () => {
  const r = fpArrayCmd(grid({ nx: 4000, ny: 1, dx: 1_270_000, dy: 0 }), "Lib:Row", ["pad_1", "pad_2"]);
  assert.ok(r.ok);
  if (!r.ok || r.cmd.op !== "create_footprint_array") return;
  assert.equal(r.cmd.footprint, "Lib:Row");
  assert.deepEqual(r.cmd.ids, ["pad_1", "pad_2"]);
  assert.equal(r.cmd.geometry.kind, "grid");
  assert.equal(r.cmd.arrange, false);
  assert.equal(r.cmd.numbering?.start_specified, true);
  const arranged = fpArrayCmd(grid({ arrange: true }), "Lib:Row", ["pad_1"]);
  assert.ok(arranged.ok && arranged.cmd.op === "create_footprint_array" && arranged.cmd.numbering === null);
  assert.deepEqual(fpArrayCmd(grid({ pads: { ...DEFAULT_PAD_NUMBERING, primary: { kind: "hex", start: "Z", step: "1" } } }), "F", ["a"]).ok, false);
});

const padAt = (id: string, number: string, kind: LibraryPad["kind"] = "smd"): LibraryPad =>
  ({ id, number, at: { x: 0, y: 0 }, size: [1_000_000, 600_000], offset: { x: 0, y: 0 }, shape: "rect", kind, drill: null, rot: 0, layers: [] }) as unknown as LibraryPad;

test("the preview tells the numbers the pads will take: the first free ones skip the pads that stay, a start value does not", () => {
  const pads = [padAt("a", "1"), padAt("b", "3"), padAt("c", "9")];
  const free = grid({ nx: 4000, ny: 1, pads: { ...DEFAULT_PAD_NUMBERING, gridFromStart: false } });
  assert.deepEqual(previewPadNumbers(free, pads, new Set(["c"]), 10_000), ["2", "4", "5", "6"]);
  assert.deepEqual(previewPadNumbers(grid({ nx: 4000, ny: 1 }), pads, new Set(["c"]), 10_000), ["1", "2", "3", "4"]);
  assert.deepEqual(previewPadNumbers(grid({ nx: 4000, ny: 1 }), pads, new Set(["c"]), 2000), ["1", "2"], "a limit");
  // two pads a point, a hole that takes none
  const withHole = [padAt("a", "1"), padAt("h", "", "non_plated_hole"), padAt("c", "2")];
  assert.deepEqual(previewPadNumbers(grid({ nx: 2000, ny: 1 }), withHole, new Set(["a", "h", "c"]), 10_000), ["1", "2", "3", "4"]);
  assert.deepEqual(previewPadNumbers(grid({ nx: 2000, ny: 1, arrange: true }), pads, new Set(["a"]), 10_000), [], "arranging numbers nothing");
  assert.deepEqual(previewPadNumbers(grid({ nx: 2000, ny: 1, pads: { ...DEFAULT_PAD_NUMBERING, renumber: false } }), pads, new Set(["a"]), 10_000), []);
});
