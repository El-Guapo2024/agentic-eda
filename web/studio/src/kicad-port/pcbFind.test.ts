import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, Part } from "../api/types";
import { boardNetNames, defaultFindOptions, findHits, findStatus, focusView, preloadText, stepFind, textMatches, type PcbFindOptions, type PcbHit } from "./pcbFind";

const opts = (over: Partial<PcbFindOptions> = {}): PcbFindOptions => ({ ...defaultFindOptions(), ...over });

function board(partial: Partial<BoardState>): BoardState {
  return { name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle", ...partial } as BoardState;
}
const part = (ref: string, value: string | null, nets: string[] = [], placed = true): Part =>
  ({ ref, value, package: null, mpn: null, footprint: null, block: null, placed, size: [2000, 1000], at: [0, 0], rot: 0, side: "top", label: "above", pads: nets.map((net, i) => ({ num: String(i + 1), net, x: 0, y: 0, w: 600, h: 600, round: false, th: false })) }) as unknown as Part;
const text = (id: string, content: string) => ({ id, content, x: 0, y: 0, angle: 0, layer: "F.SilkS", size: 1000, stroke_width: 150, justify: "center", mirror: false });
const drawings = (texts: ReturnType<typeof text>[]) => ({ shapes: [], texts, groups: [], dimensions: [], dimension_settings: {} }) as unknown as NonNullable<BoardState["drawings"]>;

// ---------------------------------------------------------------------------------------------------------------------------------------- matching

test("plain: the search text occurs anywhere in the text; case does not matter unless Match case is on", () => {
  assert.ok(textMatches("Resistor R12", { text: "r1", matchCase: false, wholeWord: false, wildcards: false }));
  assert.ok(!textMatches("Resistor R12", { text: "r1", matchCase: true, wholeWord: false, wildcards: false }));
  assert.ok(textMatches("Resistor R12", { text: "R1", matchCase: true, wholeWord: false, wildcards: false }));
  assert.ok(!textMatches("C12", { text: "R1", matchCase: false, wholeWord: false, wildcards: false }));
});

test("whole words: no letter, digit or underscore just before or after the occurrence", () => {
  const w = (t: string, s: string) => textMatches(t, { text: s, matchCase: false, wholeWord: true, wildcards: false });
  assert.ok(w("R1", "R1") && w("the R1 resistor", "R1") && w("NET-R1", "R1") && w("R1_", "R1") === false);
  assert.ok(!w("R12", "R1") && !w("XR1", "R1") && !w("R1A", "R1"));
  assert.ok(w("R12 and R1", "R1"), "a later occurrence counts when the first is part of a longer word");
  assert.ok(!w("R12 R120", "R1"));
});

test("wildcards: * is any run, ? any one character, and the mask has to match the whole text", () => {
  const m = (t: string, s: string) => textMatches(t, { text: s, matchCase: false, wholeWord: false, wildcards: true });
  assert.ok(m("GND", "G*") && m("GND", "*D") && m("GND", "G?D") && m("GND", "*") && m("GND", "gnd"));
  assert.ok(!m("GND", "G") && !m("GND", "G?") && !m("GND", "ND") && !m("GND", "*X*"));
  assert.ok(m("NET-(R1-Pad1)", "NET-(R?-Pad*)"), "the characters that mean something to a regular expression are plain here");
  assert.ok(m("a.b", "a.b") && !m("axb", "a.b"));
});

test("whole words win over wildcards when both are on, as in the dialog", () => {
  assert.ok(!textMatches("GND", { text: "G*", matchCase: false, wholeWord: true, wildcards: true }));
});

// ----------------------------------------------------------------------------------------------------------------------------------------- the hits

const sample = (): BoardState =>
  board({
    parts: [part("R1", "10k", ["VCC", "SIG"]), part("R2", "10k", ["SIG", "GND"]), part("C1", "100n", ["VCC", "GND"]), part("U9", "10kfake", [], false)],
    routing: { tracks: [{ id: "t1", net: "SIG", layer: "F.Cu", width: 200, pts: [[0, 0], [1, 1]] }], vias: [{ id: "v1", net: "VCC", x: 0, y: 0, d: 600, drill: 300, from_layer: "F.Cu", to_layer: "B.Cu" }], zones: [], track_width_presets: [], via_presets: [], teardrop_settings: {} } as unknown as BoardState["routing"],
    drawings: drawings([text("x1", "R1 stays here"), text("x2", "REV B")]),
  });

test("the list is footprints (references and values), then board texts, then DRC markers, then nets, each in its own order", () => {
  const markers = [{ description: "Clearance violation between R1 and R2" }, { description: "Courtyards overlap" }];
  const hits = findHits(sample(), markers, opts({ text: "r1" }));
  assert.deepEqual(hits, [{ kind: "footprint", id: "R1" }, { kind: "text", id: "x1" }, { kind: "marker", index: 0 }]);
  assert.deepEqual(findHits(sample(), markers, opts({ text: "10k" })), [
    { kind: "footprint", id: "R1" },
    { kind: "footprint", id: "R2" },
  ], "values; the footprint that is not placed is not on the board");
  assert.deepEqual(findHits(sample(), markers, opts({ text: "vcc" })), [{ kind: "net", id: "VCC" }]);
});

test("every checkbox narrows the search: references, values, texts, markers, nets", () => {
  const markers = [{ description: "R1 clearance" }];
  const b = sample();
  assert.deepEqual(findHits(b, markers, opts({ text: "R1", references: false })), [{ kind: "text", id: "x1" }, { kind: "marker", index: 0 }]);
  assert.deepEqual(findHits(b, markers, opts({ text: "10k", values: false })), []);
  assert.deepEqual(findHits(b, markers, opts({ text: "R1", texts: false })), [{ kind: "footprint", id: "R1" }, { kind: "marker", index: 0 }], "texts off drops the board texts, not the footprint's reference");
  assert.deepEqual(findHits(b, markers, opts({ text: "R1", markers: false })), [{ kind: "footprint", id: "R1" }, { kind: "text", id: "x1" }]);
  assert.deepEqual(findHits(b, markers, opts({ text: "SIG", nets: false })), []);
  assert.deepEqual(findHits(b, markers, opts({ text: "R1", references: false, values: false, texts: false, markers: false, nets: false })), []);
});

test("the options reach the matching: case, whole words, wildcards", () => {
  const b = sample();
  assert.deepEqual(findHits(b, [], opts({ text: "r1", matchCase: true })), []);
  assert.deepEqual(findHits(b, [], opts({ text: "R", wholeWord: true })), []);
  assert.deepEqual(findHits(b, [], opts({ text: "R?", wildcards: true })).map((h) => (h.kind === "footprint" ? h.id : "")), ["R1", "R2"]);
  assert.deepEqual(findHits(b, [], opts({ text: "*", wildcards: true, texts: false, markers: false, nets: false })).length, 3, "every placed footprint");
});

test("an empty search finds nothing", () => {
  assert.deepEqual(findHits(sample(), [], opts({ text: "" })), []);
});

test("the nets of the board are those of its pads, tracks, vias and zones, by name", () => {
  assert.deepEqual(boardNetNames(sample()), ["GND", "SIG", "VCC"]);
});

// ------------------------------------------------------------------------------------------------------------------------------------------ the walk

const hit = (n: number): PcbHit[] => Array.from({ length: n }, (_, i) => ({ kind: "footprint", id: `U${i + 1}` }));

test("Find Next from a fresh list shows the first hit, then each next, and wraps to the first with Wrap on", () => {
  const hits = hit(3);
  const a = stepFind(hits, 0, true, true, true);
  assert.deepEqual([a.cursor, a.hit?.kind === "footprint" ? a.hit.id : null, a.endReached], [0, "U1", false]);
  const b = stepFind(hits, a.cursor, true, true, false);
  assert.equal(b.cursor, 1);
  const c = stepFind(hits, b.cursor, true, true, false);
  assert.equal(c.cursor, 2);
  const d = stepFind(hits, c.cursor, true, true, false);
  assert.deepEqual([d.cursor, d.endReached, d.hit?.kind === "footprint" ? d.hit.id : null], [0, false, "U1"], "past the last: back to the first");
});

test("Find Next with Wrap off reports the end and stays on the last real hit", () => {
  const hits = hit(2);
  const last = stepFind(hits, 1, true, false, false);
  assert.deepEqual([last.cursor, last.endReached, last.hit], [1, true, null]);
  assert.equal(findStatus("x", hits, last), "No hits");
  assert.equal(stepFind(hits, last.cursor, false, false, false).cursor, 0, "and Find Previous goes back from there");
});

test("Find Previous from a fresh list shows the last hit, and walks back; before the first it wraps to the last (or ends)", () => {
  const hits = hit(3);
  const a = stepFind(hits, 0, false, true, true);
  assert.equal(a.cursor, 2);
  const b = stepFind(hits, a.cursor, false, true, false);
  assert.equal(b.cursor, 1);
  const c = stepFind(hits, 0, false, true, false);
  assert.deepEqual([c.cursor, c.endReached], [2, false], "before the first: round to the last");
  const d = stepFind(hits, 0, false, false, false);
  assert.deepEqual([d.cursor, d.endReached, d.hit], [0, true, null]);
});

test("a single hit is shown again and again with Wrap on", () => {
  const hits = hit(1);
  assert.equal(stepFind(hits, 0, true, true, true).cursor, 0);
  assert.equal(stepFind(hits, 0, true, true, false).cursor, 0);
  assert.equal(stepFind(hits, 0, false, true, false).cursor, 0);
});

test("no hits: nothing to show, and the status says the text was not found", () => {
  const step = stepFind([], 0, true, true, true);
  assert.deepEqual([step.hit, step.endReached], [null, false]);
  assert.equal(findStatus("zzz", [], step), "'zzz' not found");
});

test("the status counts from one", () => {
  const hits = hit(5);
  assert.equal(findStatus("U", hits, stepFind(hits, 0, true, true, true)), "Hit(s): 1 / 5");
  assert.equal(findStatus("U", hits, stepFind(hits, 3, true, true, false)), "Hit(s): 5 / 5");
});

// -------------------------------------------------------------------------------------------------------------------------------------- the preload

test("the dialog opens with the value of the selected footprint or the first line of the selected text", () => {
  const b = board({ parts: [part("R1", "10k")], drawings: drawings([text("x1", "first\nsecond")]) });
  assert.equal(preloadText(b, new Set(["R1"])), "10k");
  assert.equal(preloadText(b, new Set(["x1"])), "first");
  assert.equal(preloadText(b, new Set(["R1", "x1"])), "");
  assert.equal(preloadText(b, new Set()), "");
  assert.equal(preloadText(null, new Set(["R1"])), "");
});

// ----------------------------------------------------------------------------------------------------------------------------------------- the view

test("a hit near the middle of the view leaves the view alone; one outside its middle 80% is centred", () => {
  const view = { scale: 0.01, x: 0, y: 0 }; // 800 x 600 px = 80 000 x 60 000 um
  assert.deepEqual(focusView(view, 800, 600, [39_000, 29_000, 41_000, 31_000]), view);
  const moved = focusView(view, 800, 600, [78_000, 29_000, 79_000, 31_000]);
  assert.equal(moved.scale, view.scale, "centring does not zoom");
  assert.ok(Math.abs((78_500 * moved.scale + moved.x) - 400) < 1e-6 && Math.abs((30_000 * moved.scale + moved.y) - 300) < 1e-6, "the hit is at the centre of the canvas");
});

test("a hit bigger than half the view zooms out until it fits in half, centred, and never zooms in", () => {
  const view = { scale: 0.01, x: 0, y: 0 };
  const big = focusView(view, 800, 600, [0, 0, 160_000, 20_000]); // twice the view's width
  assert.ok(big.scale < view.scale);
  assert.ok(Math.abs(160_000 * big.scale - 400) < 1e-6, "its width is half the canvas");
  assert.ok(Math.abs((80_000 * big.scale + big.x) - 400) < 1e-6, "centred");
  const tiny = focusView(view, 800, 600, [39_900, 29_900, 40_100, 30_100]);
  assert.equal(tiny.scale, view.scale);
});
