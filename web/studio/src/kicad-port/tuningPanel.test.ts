import { test } from "node:test";
import assert from "node:assert/strict";
import type { TuningPattern } from "../api/tuneTypes";
import { messageTextFromMinOptMax, tuningPatternMsgPanel } from "./tuningPanel";

const mm = (nm: number) => `${(nm / 1e6).toFixed(4).replace(/\.?0+$/, "")} mm`;
const ps = (as: number) => `${(as / 1e6).toFixed(2)} ps`;
const range = (opt: number) => ({ min: opt - 100_000, opt, max: opt + 100_000 });
const pattern = (over: Partial<TuningPattern>): TuningPattern =>
  ({ id: "tun_1", mode: "single", layer: "F.Cu", origin: { x: 0, y: 0 }, end: { x: 0, y: 0 }, initial_side: "left", min_amplitude: 0, max_amplitude: 0, spacing: 0, corner_radius_percent: 100, rounded: true, single_sided: false, override_custom_rules: false, target_length: range(30_000_000), target_skew: range(0), base_line: [], last_track_width: 0, last_diff_pair_gap: 0, last_tuning_length: 0, last_netname: "", last_status: "tuned", member_ids: [], ...over }) as TuningPattern;

// UI-LEFT-2 RR5: `PCB_TUNING_PATTERN::GetMsgPanelInfo`.
test("a single-track pattern's panel: its net, class, layer, width, routed length and target", () => {
  const items = tuningPatternMsgPanel(pattern({}), { primary_net: "CLK", netclass: "Default", width: 200_000, primary: { length: 25_000_000, die_length: 0, delay: 0, die_delay: 0 } }, mm, ps, (l) => l);
  assert.deepEqual(items, [
    { upper: "Type", lower: "Tuning Pattern" },
    { upper: "Net", lower: "CLK" },
    { upper: "Resolved Netclass", lower: "Default" },
    { upper: "Layer", lower: "F.Cu" },
    { upper: "Width", lower: "0.2 mm" },
    { upper: "Routed Length", lower: "25 mm" },
    { upper: "Target Length: 30 mm", lower: "(from tuning pattern properties)" },
  ]);
});

test("a pair's lengths, the pad-to-die rows and a rule's constraints", () => {
  const items = tuningPatternMsgPanel(
    pattern({ mode: "diff_pair" }),
    { primary_net: "D+", coupled_net: "D-", netclass: "USB", width: null, primary: { length: 20_000_000, die_length: 1_000_000, delay: 0, die_delay: 0 }, coupled: { length: 21_000_000, die_length: 0, delay: 0, die_delay: 0 }, constraint: { name: "usb_len", min: 29_000_000, opt: null, max: 31_000_000 } },
    mm,
    ps,
    (l) => l,
  );
  assert.deepEqual(items.slice(1), [
    { upper: "Nets", lower: "D+, D-" },
    { upper: "Resolved Netclass", lower: "USB" },
    { upper: "Layer", lower: "F.Cu" },
    { upper: "Routed Lengths", lower: "20 mm, 21 mm" },
    { upper: "Pad To Die Length", lower: "1 mm" },
    { upper: "Full Length", lower: "21 mm" },
    { upper: "Length Constraints: min 29 mm; max 31 mm", lower: "(from usb_len)" },
  ]);
  // "Override custom rules": the pattern's own target
  const own = tuningPatternMsgPanel(pattern({ mode: "diff_pair", override_custom_rules: true }), { primary_net: "D+", constraint: { name: "r", opt: 1 } }, mm, ps, (l) => l);
  assert.equal(own[own.length - 1]!.upper, "Target Length: 30 mm");
});

test("time domain rows and a skew pattern's target", () => {
  const delayed = tuningPatternMsgPanel(pattern({ is_time_domain: true, target_delay: { min: 0, opt: 150_000_000, max: 0 } }), { primary_net: "CLK", primary: { length: 25_000_000, die_length: 0, delay: 160_000_000, die_delay: 2_000_000 } }, mm, ps, (l) => l);
  assert.deepEqual(delayed.slice(-4), [
    { upper: "Routed Delay", lower: "160.00 ps" },
    { upper: "Pad To Die Delay", lower: "2.00 ps" },
    { upper: "Full Delay", lower: "162.00 ps" },
    { upper: "Target Delay: 150.00 ps", lower: "(from tuning pattern properties)" },
  ]);
  const skew = tuningPatternMsgPanel(pattern({ mode: "diff_pair_skew", target_skew: range(500_000) }), { primary_net: "D+", coupled_net: "D-" }, mm, ps, (l) => l);
  assert.deepEqual(skew[skew.length - 1], { upper: "Target Skew: 0.5 mm", lower: "(from tuning pattern properties)" });
  // `MessageTextFromMinOptMax`: a minimum of 0 is left out; an empty text gives no row
  assert.equal(messageTextFromMinOptMax({ min: 0, max: 5 }, String), "max 5");
  const none = tuningPatternMsgPanel(pattern({ mode: "diff_pair_skew" }), { primary_net: "D+", constraint: { name: "x", min: 0 } }, mm, ps, (l) => l);
  assert.equal(none[none.length - 1]!.upper, "Layer");
});
