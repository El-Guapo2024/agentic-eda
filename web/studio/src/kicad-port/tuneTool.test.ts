import { test } from "node:test";
import assert from "node:assert/strict";
import { carriedSettings, handleAt, kindOfMode, modeOfKind, offsetOutline, offsetSide, pageSettings, patternBaselines, patternOutline, readoutBox, readoutSummary, settingsOfPattern, TUNE_KEY_ACTIONS, tuneStateFromReply } from "./tuneTool";
import type { TuneReadout, TuningPattern } from "../api/tuneTypes";

const area = (poly: readonly (readonly [number, number])[]) => {
  let a = 0;
  for (let i = 0; i < poly.length; i++) {
    const [x0, y0] = poly[i]!;
    const [x1, y1] = poly[(i + 1) % poly.length]!;
    a += x0 * y1 - x1 * y0;
  }
  return Math.abs(a) / 2;
};

function pattern(over: Partial<TuningPattern> = {}): TuningPattern {
  return {
    id: "tun_1",
    mode: "single",
    layer: "F.Cu",
    origin: { x: 10_000, y: 5_000 },
    end: { x: 30_000, y: 5_000 },
    initial_side: "left",
    min_amplitude: 200,
    max_amplitude: 1_000,
    spacing: 600,
    corner_radius_percent: 80,
    rounded: true,
    single_sided: false,
    override_custom_rules: false,
    target_length: { min: 21_900, opt: 22_000, max: 22_100 },
    target_skew: { min: -100, opt: 0, max: 100 },
    base_line: [{ x: 10_000, y: 5_000 }, { x: 30_000, y: 5_000 }],
    last_track_width: 250,
    last_diff_pair_gap: 0,
    last_tuning_length: 22_000_000,
    last_netname: "SIG",
    last_status: "tuned",
    member_ids: ["a", "b"],
    ...over,
  };
}

const readout = (over: Partial<TuneReadout> = {}): TuneReadout => ({ label: "current length", current: 21.5, min: 21.9, opt: 22, max: 22.1, delta: -0.5, status: "too_short", constraint: null, partner: null, nets: null, ...over });

test("the status box prints the length red outside the range, the minimum red below it and the maximum red above it", () => {
  const fmt = (um: number) => `${(um / 1000).toFixed(2)} mm`;
  assert.deepEqual(readoutBox(readout(), fmt), { header: ["current length", "min", "max"], values: ["21.50 mm", "21.90 mm", "22.10 mm"], bad: [true, true, false] });
  assert.deepEqual(readoutBox(readout({ current: 22 }), fmt).bad, [false, false, false]);
  assert.deepEqual(readoutBox(readout({ current: 23 }), fmt).bad, [true, false, true]);
  // a pattern with no target: `---` and nothing is red
  assert.deepEqual(readoutBox(readout({ min: null, max: null, opt: null, delta: null, status: "tuned" }), fmt), { header: ["current length", "min", "max"], values: ["21.50 mm", "---", "---"], bad: [false, false, false] });
  assert.equal(readoutSummary(readout({ constraint: "length_rule" }), fmt), "current length 21.50 mm, target 22.00 mm (-0.50 mm), too short, from length_rule");
  assert.equal(readoutSummary(readout({ label: "current skew", current: 0.02, min: -0.1, max: 0.1, opt: 0, delta: 0.02, status: "tuned" }), fmt), "current skew 0.02 mm, target skew 0.00 mm (+0.02 mm), tuned");
});

test("a straight baseline has a capsule for an outline: the track's length and its width round, with round ends", () => {
  const poly = offsetOutline([[0, 0], [10_000, 0]], 1_000);
  const xs = poly.map((p) => p[0]);
  const ys = poly.map((p) => p[1]);
  assert.equal(Math.min(...xs), -1_000);
  assert.equal(Math.max(...xs), 11_000);
  assert.ok(Math.abs(Math.min(...ys) + 1_000) < 1e-6 && Math.abs(Math.max(...ys) - 1_000) < 1e-6);
  const exact = 2 * 1_000 * 10_000 + Math.PI * 1_000 * 1_000;
  assert.ok(Math.abs(area(poly) - exact) / exact < 0.01, `${area(poly)} vs ${exact}`);
});

test("an offset follows a corner with a round join outside and the crossing inside", () => {
  // an L: along +x then down +y (the screen's y is down). The right side (+1) of the first leg is +y, the inside of the turn
  const right = offsetSide([[0, 0], [10_000, 0], [10_000, 10_000]], 500, 1);
  assert.deepEqual(right[0], [0, 500]);
  assert.ok(right.some((p) => Math.abs(p[0] - 9_500) < 1e-6 && Math.abs(p[1] - 500) < 1e-6), "the inside of the turn is where the two offsets cross");
  assert.deepEqual(right[right.length - 1], [9_500, 10_000]);
  const left = offsetSide([[0, 0], [10_000, 0], [10_000, 10_000]], 500, -1);
  assert.deepEqual(left[0], [0, -500]);
  assert.deepEqual(left[left.length - 1], [10_500, 10_000]);
  assert.ok(left.length > 4, "a round join: points on the arc round the outside of the turn");
  assert.ok(left.every((p) => Math.hypot(p[0] - Math.min(Math.max(p[0], 0), 10_000), p[1] - Math.min(Math.max(p[1], 0), 10_000)) > 0), "none of them on the line");
});

test("a pattern's outline is the baseline offset by the amplitude and half the track, and one side only when it is single-sided", () => {
  const both = patternOutline(pattern())[0]!;
  const ys = both.map((p) => p[1]);
  assert.ok(Math.abs(Math.max(...ys) - (5_000 + 1_000 + 125)) < 1e-6 && Math.abs(Math.min(...ys) - (5_000 - 1_125)) < 1e-6, `${Math.min(...ys)}..${Math.max(...ys)}`);
  const one = patternOutline(pattern({ single_sided: true, initial_side: "left" }))[0]!;
  const oy = one.map((p) => p[1]);
  assert.ok(Math.abs(Math.min(...oy) - (5_000 - 1_125)) < 1e-6 && Math.max(...oy) <= 5_000 + 1e-6, "only the left side of the baseline");
  const right = patternOutline(pattern({ single_sided: true, initial_side: "right" }))[0]!;
  assert.ok(Math.min(...right.map((p) => p[1])) >= 5_000 - 1e-6);
  // a pair: both baselines, and the amplitude counts from the lines of the pair
  const pair = patternOutline(pattern({ mode: "diff_pair", last_diff_pair_gap: 450, base_line_coupled: [{ x: 10_000, y: 5_700 }, { x: 30_000, y: 5_700 }] }));
  assert.equal(pair.length, 2);
  assert.ok(Math.abs(Math.max(...pair[0]!.map((p) => p[1])) - (5_000 + 1_000 + 225 + 250)) < 1e-6);
  assert.equal(patternBaselines(pattern({ mode: "diff_pair", base_line_coupled: [{ x: 0, y: 0 }, { x: 1, y: 0 }] })).length, 2);
  assert.equal(patternBaselines(pattern({ base_line: [] }))[0]!.length, 2, "a pattern with no baseline is drawn from its origin to its end");
});

test("the amplitude is never drawn smaller than the track needs", () => {
  const thin = patternOutline(pattern({ max_amplitude: 100, last_track_width: 250 }))[0]!;
  assert.ok(Math.abs(Math.max(...thin.map((p) => p[1])) - (5_000 + 250 + 125)) < 1e-6, "a round corner needs the track's width");
});

test("an edit point is picked within the radius, the nearest first", () => {
  const handles: [number, number][] = [[0, 0], [100, 0], [0, 100], [60, 60]];
  assert.equal(handleAt(handles, { x: 5, y: 5 }, 20), 0);
  assert.equal(handleAt(handles, { x: 90, y: 4 }, 20), 1);
  assert.equal(handleAt(handles, { x: 50, y: 50 }, 20), 3);
  assert.equal(handleAt(handles, { x: 50, y: -50 }, 20), null);
});

test("the session replies make the draw state, keeping what a reply does not carry", () => {
  const first = tuneStateFromReply({ ok: true, pattern: pattern(), net: "SIG", origin: { x: 10_000, y: 5_000 }, handles: [[1, 2], [3, 4], [5, 6], [7, 8]] }, null, { mode: "single", layer: "F.Cu" });
  assert.deepEqual([first.kind, first.net, first.origin, first.moved, first.pieces.length], ["tune", "SIG", [10_000, 5_000], false, 0]);
  const piece = { pts: [[0, 0], [1, 1]] as [number, number][], member: true, net: "SIG", layer: "F.Cu", width: 250 };
  const second = tuneStateFromReply({ ok: true, moved: true, readout: readout(), preview: { pieces: [piece], hide: ["trk_1"] } }, first, { mode: "single", layer: "F.Cu" });
  assert.deepEqual([second.moved, second.pieces.length, second.hide, second.readout?.current, second.handles.length, second.net], [true, 1, ["trk_1"], 21.5, 4, "SIG"]);
  const edit = tuneStateFromReply({ ok: true, editing: true, pattern: pattern() }, null, { mode: "single", layer: "F.Cu" });
  assert.equal(edit.editing, "tun_1");
});

test("the settings a tuner starts from are the board's page for its kind; what a session leaves is carried on without its target", () => {
  const s = pageSettings(undefined, "diffpair");
  assert.deepEqual(s, { min_amplitude: 200, max_amplitude: 1_000, spacing: 1_000, corner_radius_percent: 80, rounded: true, single_sided: false }, "KiCad's pair defaults: a spacing of 1 mm");
  assert.deepEqual(pageSettings({ single_track: { min_amplitude_um: 300, max_amplitude_um: 900, spacing_um: 700, corner_style: "chamfer", corner_radius_percentage: 50, single_sided: true }, diff_pair: s2(), diff_pair_skew: s2() }, "single"), { min_amplitude: 300, max_amplitude: 900, spacing: 700, corner_radius_percent: 50, rounded: false, single_sided: true });
  const carried = carriedSettings(pattern({ max_amplitude: 950, spacing: 650 }), { target_length: 5 });
  assert.equal(carried.max_amplitude, 950);
  assert.equal(carried.spacing, 650);
  assert.equal(carried.target_length, 5, "the target the tool had stays what it was");
  assert.deepEqual(settingsOfPattern(pattern({ target_length: { min: 0, opt: 1_000_000_000, max: 1_000_000_000 } })).target_length, null, "no target");
});

function s2() {
  return { min_amplitude_um: 200, max_amplitude_um: 1_000, spacing_um: 1_000, corner_style: "round" as const, corner_radius_percentage: 80, single_sided: false };
}

test("modes and keys", () => {
  assert.equal(kindOfMode("diff_pair_skew"), "skew");
  assert.equal(modeOfKind("diffpair"), "diff_pair");
  assert.equal(modeOfKind(kindOfMode("single")), "single");
  assert.deepEqual(Object.values(TUNE_KEY_ACTIONS).sort(), ["amplitude_down", "amplitude_up", "spacing_down", "spacing_up"]);
});
