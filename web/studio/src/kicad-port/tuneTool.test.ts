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
    origin: { x: 10_000_000, y: 5_000_000 },
    end: { x: 30_000_000, y: 5_000_000 },
    initial_side: "left",
    min_amplitude: 200_000,
    max_amplitude: 1_000_000,
    spacing: 600_000,
    corner_radius_percent: 80_000,
    rounded: true,
    single_sided: false,
    override_custom_rules: false,
    target_length: { min: 21_900_000, opt: 22_000_000, max: 22_100_000 },
    target_skew: { min: -100_000, opt: 0, max: 100_000 },
    base_line: [{ x: 10_000_000, y: 5_000_000 }, { x: 30_000_000, y: 5_000_000 }],
    last_track_width: 250_000,
    last_diff_pair_gap: 0,
    last_tuning_length: 22_000_000_000,
    last_netname: "SIG",
    last_status: "tuned",
    member_ids: ["a", "b"],
    ...over,
  };
}

const readout = (over: Partial<TuneReadout> = {}): TuneReadout => ({ label: "current length", current: 21.5, min: 21.9, opt: 22_000, max: 22.1, delta: -0.5, status: "too_short", constraint: null, partner: null, nets: null, ...over });

test("the status box prints the length red outside the range, the minimum red below it and the maximum red above it", () => {
  const fmt = (um: number) => `${(um / 1000).toFixed(2)} mm`;
  assert.deepEqual(readoutBox(readout(), fmt), { header: ["current length", "min", "max"], values: ["21.50 mm", "21.90 mm", "22.10 mm"], bad: [true, true, false] });
  assert.deepEqual(readoutBox(readout({ current: 22_000 }), fmt).bad, [false, false, false]);
  assert.deepEqual(readoutBox(readout({ current: 23_000 }), fmt).bad, [true, false, true]);
  // a pattern with no target: `---` and nothing is red
  assert.deepEqual(readoutBox(readout({ min: null, max: null, opt: null, delta: null, status: "tuned" }), fmt), { header: ["current length", "min", "max"], values: ["21.50 mm", "---", "---"], bad: [false, false, false] });
  assert.equal(readoutSummary(readout({ constraint: "length_rule" }), fmt), "current length 21.50 mm, target 22.00 mm (-0.50 mm), too short, from length_rule");
  assert.equal(readoutSummary(readout({ label: "current skew", current: 0.02, min: -0.1, max: 0.1, opt: 0, delta: 0.02, status: "tuned" }), fmt), "current skew 0.02 mm, target skew 0.00 mm (+0.02 mm), tuned");
});

test("a time domain pattern's box reads the delay in picoseconds, red outside its range, and its pattern settings carry the target delay in picoseconds", () => {
  const fmt = (um: number) => `${(um / 1000).toFixed(2)} mm`;
  const timeDomain = (over: Partial<TuneReadout> = {}) => readout({ time_domain: true, delay: { label: "current delay", current: 122_000, min: 149.9, opt: 150_000, max: 150.1, delta: -28_000 }, ...over });
  assert.deepEqual(readoutBox(timeDomain(), fmt), { header: ["current delay", "min", "max"], values: ["122 ps", "149.9 ps", "150.1 ps"], bad: [true, true, false] });
  assert.deepEqual(readoutBox(timeDomain({ delay: { label: "current delay", current: 150.05, min: 149.9, opt: 150_000, max: 150.1, delta: 0.05 } }), fmt).bad, [false, false, false]);
  assert.deepEqual(readoutBox(timeDomain({ delay: { label: "current delay", current: 151_000, min: null, opt: null, max: null, delta: null } }), fmt), { header: ["current delay", "min", "max"], values: ["151 ps", "---", "---"], bad: [false, false, false] });
  assert.equal(readoutSummary(timeDomain({ status: "too_short" }), fmt), "current delay 122 ps, target 150 ps (-28 ps), too short");
  // a pattern in the time domain sends its delay targets in picoseconds (the model keeps attoseconds), and none for an unconstrained one
  const td = pattern({ is_time_domain: true, target_delay: { min: 149_900_000_000, opt: 150_000_000_000, max: 150_100_000_000 }, target_skew_delay: { min: -100_000_000, opt: 0, max: 100_000_000 } });
  const sent = settingsOfPattern(td);
  assert.equal(sent.time_domain, true);
  assert.deepEqual(sent.target_delay, { min: 149.9, opt: 150_000, max: 150.1 });
  assert.deepEqual(sent.target_skew_delay, { min: -0.1, opt: 0, max: 0.1 });
  const length = settingsOfPattern(pattern());
  assert.equal(length.time_domain, false);
  assert.equal(length.target_delay, null);
});

test("a straight baseline has a capsule for an outline: the track's length and its width round, with round ends", () => {
  const poly = offsetOutline([[0, 0], [10_000_000, 0]], 1_000_000);
  const xs = poly.map((p) => p[0]);
  const ys = poly.map((p) => p[1]);
  assert.equal(Math.min(...xs), -1_000_000);
  assert.equal(Math.max(...xs), 11_000_000);
  assert.ok(Math.abs(Math.min(...ys) + 1_000_000) < 1e-6 && Math.abs(Math.max(...ys) - 1_000_000) < 1e-6);
  const exact = 2 * 1_000 * 10_000 + Math.PI * 1_000 * 1_000;
  assert.ok(Math.abs(area(poly) - exact) / exact < 0.01, `${area(poly)} vs ${exact}`);
});

test("an offset follows a corner with a round join outside and the crossing inside", () => {
  // an L: along +x then down +y (the screen's y is down). The right side (+1) of the first leg is +y, the inside of the turn
  const right = offsetSide([[0, 0], [10_000_000, 0], [10_000_000, 10_000_000]], 500_000, 1);
  assert.deepEqual(right[0], [0, 500_000]);
  assert.ok(right.some((p) => Math.abs(p[0] - 9_500_000) < 1e-6 && Math.abs(p[1] - 500_000) < 1e-6), "the inside of the turn is where the two offsets cross");
  assert.deepEqual(right[right.length - 1], [9_500, 10_000]);
  const left = offsetSide([[0, 0], [10_000_000, 0], [10_000_000, 10_000_000]], 500_000, -1);
  assert.deepEqual(left[0], [0, -500_000]);
  assert.deepEqual(left[left.length - 1], [10_500, 10_000]);
  assert.ok(left.length > 4, "a round join: points on the arc round the outside of the turn");
  assert.ok(left.every((p) => Math.hypot(p[0] - Math.min(Math.max(p[0], 0), 10_000_000), p[1] - Math.min(Math.max(p[1], 0), 10_000_000)) > 0), "none of them on the line");
});

test("a pattern's outline is the baseline offset by the amplitude and half the track, and one side only when it is single-sided", () => {
  const both = patternOutline(pattern())[0]!;
  const ys = both.map((p) => p[1]);
  assert.ok(Math.abs(Math.max(...ys) - (5_000_000 + 1_000_000 + 125_000)) < 1e-6 && Math.abs(Math.min(...ys) - (5_000_000 - 1_125_000)) < 1e-6, `${Math.min(...ys)}..${Math.max(...ys)}`);
  const one = patternOutline(pattern({ single_sided: true, initial_side: "left" }))[0]!;
  const oy = one.map((p) => p[1]);
  assert.ok(Math.abs(Math.min(...oy) - (5_000_000 - 1_125_000)) < 1e-6 && Math.max(...oy) <= 5_000_000 + 1e-6, "only the left side of the baseline");
  const right = patternOutline(pattern({ single_sided: true, initial_side: "right" }))[0]!;
  assert.ok(Math.min(...right.map((p) => p[1])) >= 5_000_000 - 1e-6);
  // a pair: both baselines, and the amplitude counts from the lines of the pair
  const pair = patternOutline(pattern({ mode: "diff_pair", last_diff_pair_gap: 450, base_line_coupled: [{ x: 10_000_000, y: 5_700_000 }, { x: 30_000_000, y: 5_700_000 }] }));
  assert.equal(pair.length, 2);
  assert.ok(Math.abs(Math.max(...pair[0]!.map((p) => p[1])) - (5_000_000 + 1_000_000 + 225 + 250_000)) < 1e-6);
  assert.equal(patternBaselines(pattern({ mode: "diff_pair", base_line_coupled: [{ x: 0, y: 0 }, { x: 1, y: 0 }] })).length, 2);
  assert.equal(patternBaselines(pattern({ base_line: [] }))[0]!.length, 2, "a pattern with no baseline is drawn from its origin to its end");
});

test("the amplitude is never drawn smaller than the track needs", () => {
  const thin = patternOutline(pattern({ max_amplitude: 100_000, last_track_width: 250_000 }))[0]!;
  assert.ok(Math.abs(Math.max(...thin.map((p) => p[1])) - (5_000_000 + 250_000 + 125_000)) < 1e-6, "a round corner needs the track's width");
});

test("an edit point is picked within the radius, the nearest first", () => {
  const handles: [number, number][] = [[0, 0], [100_000, 0], [0, 100_000], [60_000, 60_000]];
  assert.equal(handleAt(handles, { x: 5000, y: 5000 }, 20_000), 0);
  assert.equal(handleAt(handles, { x: 90, y: 4000 }, 20_000), 1);
  assert.equal(handleAt(handles, { x: 50_000, y: 50_000 }, 20_000), 3000);
  assert.equal(handleAt(handles, { x: 50_000, y: -50_000 }, 20_000), null);
});

test("the session replies make the draw state, keeping what a reply does not carry", () => {
  const first = tuneStateFromReply({ ok: true, pattern: pattern(), net: "SIG", origin: { x: 10_000_000, y: 5_000_000 }, handles: [[1, 2000], [3000, 4000], [5000, 6000], [7000, 8000]] }, null, { mode: "single", layer: "F.Cu" });
  assert.deepEqual([first.kind, first.net, first.origin, first.moved, first.pieces.length], ["tune", "SIG", [10_000, 5_000], false, 0]);
  const piece = { pts: [[0, 0], [1, 1]] as [number, number][], member: true, net: "SIG", layer: "F.Cu", width: 250_000 };
  const second = tuneStateFromReply({ ok: true, moved: true, readout: readout(), preview: { pieces: [piece], hide: ["trk_1"] } }, first, { mode: "single", layer: "F.Cu" });
  assert.deepEqual([second.moved, second.pieces.length, second.hide, second.readout?.current, second.handles.length, second.net], [true, 1, ["trk_1"], 21.5, 4, "SIG"]);
  const edit = tuneStateFromReply({ ok: true, editing: true, pattern: pattern() }, null, { mode: "single", layer: "F.Cu" });
  assert.equal(edit.editing, "tun_1");
});

test("the settings a tuner starts from are the board's page for its kind; what a session leaves is carried on without its target", () => {
  const s = pageSettings(undefined, "diffpair");
  assert.deepEqual(s, { min_amplitude: 200_000, max_amplitude: 1_000_000, spacing: 1_000_000, corner_radius_percent: 80_000, rounded: true, single_sided: false }, "KiCad's pair defaults: a spacing of 1 mm");
  assert.deepEqual(pageSettings({ single_track: { min_amplitude_nm: 300_000, max_amplitude_nm: 900, spacing_nm: 700_000, corner_style: "chamfer", corner_radius_percentage: 50_000, single_sided: true }, diff_pair: s2(), diff_pair_skew: s2() }, "single"), { min_amplitude: 300_000, max_amplitude: 900, spacing: 700_000, corner_radius_percent: 50_000, rounded: false, single_sided: true });
  const carried = carriedSettings(pattern({ max_amplitude: 950_000, spacing: 650_000 }), { target_length: 5000 });
  assert.equal(carried.max_amplitude, 950_000);
  assert.equal(carried.spacing, 650_000);
  assert.equal(carried.target_length, 5000, "the target the tool had stays what it was");
  assert.deepEqual(settingsOfPattern(pattern({ target_length: { min: 0, opt: 1_000_000_000_000, max: 1_000_000_000_000 } })).target_length, null, "no target");
});

function s2() {
  return { min_amplitude_nm: 200_000, max_amplitude_nm: 1_000_000, spacing_nm: 1_000_000, corner_style: "round" as const, corner_radius_percentage: 80_000, single_sided: false };
}

test("modes and keys", () => {
  assert.equal(kindOfMode("diff_pair_skew"), "skew");
  assert.equal(modeOfKind("diffpair"), "diff_pair");
  assert.equal(modeOfKind(kindOfMode("single")), "single");
  assert.deepEqual(Object.values(TUNE_KEY_ACTIONS).sort(), ["amplitude_down", "amplitude_up", "spacing_down", "spacing_up"]);
});
