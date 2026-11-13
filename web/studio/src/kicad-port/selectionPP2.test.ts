// UI-LEFT-2 PP2 and PP19: the candidates the edit tools ask about, texts weighed by their strokes, Drag's client filter, the filter's rejected rows.
// Cases worked from pcbnew/tools/pcb_selection_tool.cpp (`selectPoint`, `GuessSelectionCandidates`, `FilterCollectedItems`, `itemPassesFilter`),
// pcbnew/tools/edit_tool.cpp (`EDIT_TOOL::Drag`'s filter) and pcbnew/footprint.cpp (`GetCoverageArea`, `CoverageRatio`) at 8303b2ad.
import { test } from "node:test";
import assert from "node:assert/strict";
import type { BoardState, BoardText, FpTextInfo, Part } from "../api/types";
import {
  applyDragFilter,
  collectSelectionCandidates,
  DEFAULT_SELECTION_FILTER,
  footprintCoverageRatio,
  newFilterRejections,
  pickSelectionCandidates,
  rowsToFlash,
  toolCandidateSets,
  type SelectionCandidate,
} from "../components/canvas/selectionCandidates";
import { fieldStrokes, strokeDistance } from "./textHit";
import { boardTextAsField, strokeRings, strokesBoxRing, textCoverageArea } from "./textCoverage";
import { intersectionArea, ringArea } from "./coverage";
import { artTextAsField } from "./fpArt";

const near = (a: number, b: number, tol: number, msg?: string) => assert.ok(Math.abs(a - b) <= tol, `${msg ?? ""} ${a} vs ${b}`);

const routing = (tracks: unknown[], vias: unknown[] = []) => ({ tracks, vias, zones: [], track_width_presets: [], via_presets: [], teardrop_settings: {} }) as never;
const drawings = (o: Record<string, unknown>) => ({ shapes: [], texts: [], dimensions: [], dimension_settings: {}, groups: [], ...o }) as never;
const board = (o: Partial<BoardState>): BoardState => ({ name: "t", dir: "", outline: null, layers: ["F.Cu", "B.Cu"], snap: 100, parts: [], rules: [], routing: null, drawings: null, checks: [], activity: [], job: "idle", ...o }) as BoardState;
const track = (id: string, pts: [number, number][], layer = "F.Cu", width = 250_000) => ({ id, net: "N", layer, width, pts });
const text = (o: Partial<BoardText>): BoardText => ({ id: "T1", content: "O", x: 0, y: 0, angle: 0, layer: "F.SilkS", size: 2_000_000, stroke_width: 300_000, justify: "center", mirror: false, ...o }) as BoardText;
const ids = (c: readonly SelectionCandidate[]) => c.map((x) => x.id);

test("GetCoverageArea of a text is the area of its strokes grown by the accuracy, not its box", () => {
  // "I": one vertical stroke of the size's height (`I` in the stroke font), a 0.3 mm pen
  const s = fieldStrokes(boardTextAsField(text({ content: "I" })));
  assert.equal(s.segs.length, 1);
  const [x0, y0, x1, y1] = s.segs[0]!;
  const len = Math.hypot(x1 - x0, y1 - y0);
  // a stadium of width pen + 2 * margin: its rectangle and its two half circles
  const m = 50_000;
  const r = 150_000 + m;
  const exact = len * 2 * r + Math.PI * r * r;
  const a = textCoverageArea(s, m);
  // `ERROR_INSIDE` (UI-LEFT-3 P3-24): the round ends' chords inside the arcs, so a little under the exact area
  assert.ok(a <= exact && a > exact * 0.98, `${a} vs ${exact}`);
  // much less than its box grown by the margin once the text has holes: "O"
  const o = fieldStrokes(boardTextAsField(text({ content: "O" })));
  const [u0, v0, u1, v1] = o.box!;
  assert.ok(textCoverageArea(o, m) < 0.8 * (u1 - u0 + 2 * m) * (v1 - v0 + 2 * m));
});

test("buildBoundingHull: the box of a text's strokes turned with it", () => {
  const s = fieldStrokes(boardTextAsField(text({ content: "AB", angle: 90_000 })));
  const ring = strokesBoxRing(s, 0)!;
  const [u0, v0, u1, v1] = s.box!;
  near(ringArea(ring), (u1 - u0) * (v1 - v0), 1);
  // turned a quarter: the box is upright on the board, its long side along y
  const xs = ring.map((p) => p[0]);
  const ys = ring.map((p) => p[1]);
  near(Math.max(...ys) - Math.min(...ys), u1 - u0, 1);
  near(Math.max(...xs) - Math.min(...xs), v1 - v0, 1);
  // every stroke's end lies inside it
  for (const [a, b] of s.segs.map((g) => [g[0], g[1]])) assert.ok(a! >= Math.min(...xs) - 1 && a! <= Math.max(...xs) + 1 && b! >= Math.min(...ys) - 1 && b! <= Math.max(...ys) + 1);
});

test("a board text is hit by its strokes (PP2): on a stroke of the O the text, in its hole the filled shape under it", () => {
  const shape = { id: "S1", kind: "rect", start: [-5_000_000, -5_000_000], end: [5_000_000, 5_000_000], layer: "F.SilkS", stroke_width: 100_000, filled: true };
  const b = board({ drawings: drawings({ texts: [text({})], shapes: [shape] }) });
  const s = fieldStrokes(boardTextAsField(text({})));
  const [x0, y0, x1, y1] = s.segs[0]!;
  const pick = (x: number, y: number) => ids(pickSelectionCandidates(b, x, y, 150_000, 10_000, DEFAULT_SELECTION_FILTER, {}, "F.SilkS", false, new Set(), false, false));
  assert.deepEqual(pick((x0 + x1) / 2, (y0 + y1) / 2), ["T1"]);
  // the middle of the O: in its box, far from its strokes -- the filled rectangle is the exact hit (it was the text, by its box)
  const [u0, v0, u1, v1] = s.box!;
  const mid: [number, number] = [(u0 + u1) / 2, (v0 + v1) / 2];
  assert.ok(strokeDistance(s, ...mid) > 400_000);
  assert.deepEqual(pick(...mid), ["S1"]);
});

test("the edit tools ask when two items are left (selectPoint -> doSelectionMenu): two crossing tracks of one width on the active layer", () => {
  const b = board({ routing: routing([track("A", [[-2_000_000, 0], [2_000_000, 0]]), track("B", [[0, -2_000_000], [0, 2_000_000]])]) });
  const sets = toolCandidateSets(b, 0, 0, 150_000, 10_000, DEFAULT_SELECTION_FILTER, {}, "F.Cu", false, new Set(), "tool");
  assert.deepEqual(ids(sets.guessed).sort(), ["A", "B"], "same slop, same `width * width`, same layer: both stay");
  // one of them on the other layer: the active-layer pass leaves the one on it
  const c = board({ routing: routing([track("A", [[-2_000_000, 0], [2_000_000, 0]]), track("B", [[0, -2_000_000], [0, 2_000_000]], "B.Cu")]) });
  assert.deepEqual(ids(toolCandidateSets(c, 0, 0, 150_000, 10_000, DEFAULT_SELECTION_FILTER, {}, "F.Cu", false, new Set(), "tool").guessed), ["A"]);
  // Show More Choices: what the heuristics set aside comes back (`Combine()`)
  const more = toolCandidateSets(c, 0, 0, 150_000, 10_000, DEFAULT_SELECTION_FILTER, {}, "F.Cu", false, new Set(), "tool");
  assert.deepEqual(ids(more.all).sort(), ["A", "B"]);
});

test("EDIT_TOOL::Drag's filter: two tracks meeting at the point drop to one, two tracks and the via they end on to the via", () => {
  const knee = board({ routing: routing([track("A", [[-2_000_000, 0], [0, 0]]), track("B", [[0, 0], [0, 2_000_000]])]) });
  const d = toolCandidateSets(knee, 0, 0, 150_000, 10_000, DEFAULT_SELECTION_FILTER, {}, "F.Cu", false, new Set(), "drag");
  assert.equal(d.guessed.length, 1, "a knee: no menu");
  // the tool filter alone asks
  assert.equal(toolCandidateSets(knee, 0, 0, 150_000, 10_000, DEFAULT_SELECTION_FILTER, {}, "F.Cu", false, new Set(), "tool").guessed.length, 2);
  const via = { id: "V1", net: "N", x: 0, y: 0, d: 600_000, drill: 300_000, from: "F.Cu", to: "B.Cu" };
  const withVia = board({ routing: routing([track("A", [[-2_000_000, 0], [0, 0]]), track("B", [[0, 0], [0, 2_000_000]], "B.Cu")], [via]) });
  const guess = (c: SelectionCandidate[]) => c;
  const all = collectSelectionCandidates(withVia, 0, 0, 150_000, DEFAULT_SELECTION_FILTER, {}, "F.Cu", false, new Set(), false, 10_000);
  const { kept, removed } = applyDragFilter(withVia, all, guess);
  assert.deepEqual(ids(kept), ["V1"]);
  assert.deepEqual([...removed].sort(), ["A", "B"]);
  // tracks that cross without meeting at an end stay both
  const cross = board({ routing: routing([track("A", [[-2_000_000, 0], [2_000_000, 0]]), track("B", [[0, -2_000_000], [0, 2_000_000]])]) });
  assert.equal(toolCandidateSets(cross, 0, 0, 150_000, 10_000, DEFAULT_SELECTION_FILTER, {}, "F.Cu", false, new Set(), "drag").guessed.length, 2);
});

test("selectPoint's HighlightSelectionFilter: the rows that refused everything under the click, none once an item got through; Text never flashes", () => {
  const b = board({ routing: routing([track("A", [[-2_000_000, 0], [2_000_000, 0]])]), drawings: drawings({ texts: [text({ id: "T9", x: 10_000_000, y: 0, content: "I" })] }) });
  const run = (f: typeof DEFAULT_SELECTION_FILTER, x: number, y = 0) => {
    const r = newFilterRejections();
    const c = pickSelectionCandidates(b, x, y, 150_000, 10_000, f, {}, "F.Cu", false, new Set(), false, false, r);
    return { c: ids(c), rows: rowsToFlash(r) };
  };
  assert.deepEqual(run({ ...DEFAULT_SELECTION_FILTER, tracks: false }, 0), { c: [], rows: ["tracks"] });
  assert.deepEqual(run(DEFAULT_SELECTION_FILTER, 0), { c: ["A"], rows: [] });
  // a locked track: "Locked items" flashes (the lock is checked first: `aRejected->lockedItems`), whatever the Tracks row says
  const locked = { ...b, locked: ["A"] } as BoardState;
  const r = newFilterRejections();
  pickSelectionCandidates(locked, 0, 0, 150_000, 10_000, { ...DEFAULT_SELECTION_FILTER, tracks: false }, {}, "F.Cu", false, new Set(), false, false, r);
  assert.deepEqual(rowsToFlash(r), ["lockedItems"]);
  // a text refused: nothing flashes (`case PCB_TEXT_T: if( !m_filter.text ) return false;`, no `aRejected->text`)
  const s = fieldStrokes(boardTextAsField(text({ id: "T9", x: 10_000_000, y: 0, content: "I" })));
  const [x0, y0, x1, y1] = s.segs[0]!;
  assert.deepEqual(run({ ...DEFAULT_SELECTION_FILTER, text: false }, (x0 + x1) / 2, (y0 + y1) / 2), { c: [], rows: [] });
});

test("CoverageRatio counts a footprint's texts by their strokes grown by the accuracy and the error, other texts by their strokes' box", () => {
  // a footprint whose hull is a 4 mm square, its fab text "W" (2 mm) at the centre
  const w: FpTextInfo = { id: "U1~t0", text: "W", x: 0, y: 0, angle: 0, w: 2_000_000, h: 2_000_000, thickness: 200_000, layer: "F.Fab", halign: 0, valign: 0, mirror: false, bold: false, italic: false, upright: true, knockout: false };
  const part = { ref: "U1", value: null, package: null, mpn: null, footprint: null, block: null, placed: true, size: [4_000_000, 4_000_000], at: [0, 0], rot: 0, side: "top", label: "above", courtyard: [-2_000_000, -2_000_000, 2_000_000, 2_000_000], pads: [], texts: [w] } as unknown as Part;
  const b = board({ parts: [part] });
  const acc = 50_000;
  const ratio = footprintCoverageRatio(b, "U1", [], acc);
  const strokes = intersectionArea(strokeRings(fieldStrokes(artTextAsField(w)), acc + 20_000), [[[-2_000_000, -2_000_000], [2_000_000, -2_000_000], [2_000_000, 2_000_000], [-2_000_000, 2_000_000]]]);
  near(ratio, strokes / 16e12, 1e-9);
  // the box of the strokes would have covered much more
  const [u0, v0, u1, v1] = fieldStrokes(artTextAsField(w)).box!;
  assert.ok(ratio < (0.8 * ((u1 - u0) * (v1 - v0))) / 16e12, String(ratio));
});
