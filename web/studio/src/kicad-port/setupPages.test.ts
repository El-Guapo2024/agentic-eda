import { test } from "node:test";
import assert from "node:assert/strict";
import type { TuningProfile } from "../api/types";
import {
  DEFAULT_FORMATTING,
  DEFAULT_TUNING_PATTERNS,
  addTrackRow,
  addViaOverride,
  asToPs,
  checkProfiles,
  emptyProfile,
  formattingOf,
  formattingProblems,
  hatchProblems,
  hatchRows,
  hatchToSend,
  missingProfiles,
  profilesDraftOf,
  profilesToSend,
  psToAs,
  sameFormatting,
  sameHatchOffsets,
  sameProfiles,
  sameTuningPatterns,
  tuningPatternsOf,
  tuningPatternsProblems,
} from "./setupPages";

const COPPER4 = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];
const COPPER2 = ["F.Cu", "B.Cu"];

// The same cases as crates/model/src/setup_pages.rs: the page checks before Apply what the backend refuses after.
test("formatting starts at ISO 128-2 and refuses a ratio a dashed line cannot have", () => {
  assert.deepEqual([DEFAULT_FORMATTING.dash_length_ratio, DEFAULT_FORMATTING.gap_length_ratio], [12, 3]);
  assert.deepEqual(formattingProblems(DEFAULT_FORMATTING), {});
  assert.ok(sameFormatting(undefined, DEFAULT_FORMATTING) && sameFormatting({ ...DEFAULT_FORMATTING }, undefined));
  assert.deepEqual(Object.keys(formattingProblems({ ...DEFAULT_FORMATTING, dash_length_ratio: 0 })), ["dash_length_ratio"]);
  assert.deepEqual(Object.keys(formattingProblems({ ...DEFAULT_FORMATTING, gap_length_ratio: -1 })), ["gap_length_ratio"]);
  assert.deepEqual(Object.keys(formattingProblems({ ...DEFAULT_FORMATTING, dash_length_ratio: NaN })), ["dash_length_ratio"]);
  assert.deepEqual(Object.keys(formattingProblems({ ...DEFAULT_FORMATTING, gap_length_ratio: 1000.5 })), ["gap_length_ratio"]);
  assert.deepEqual(formattingProblems({ ...DEFAULT_FORMATTING, dash_length_ratio: 1000 }), {});
  assert.deepEqual(formattingOf({ dash_length_ratio: 8 } as never), { ...DEFAULT_FORMATTING, dash_length_ratio: 8 }, "what a page leaves out is the default");
  assert.ok(!sameFormatting({ ...DEFAULT_FORMATTING, apply_to_fp_text: true }, undefined));
});

test("the hatch offsets are a row for each copper layer and a layer at 0, 0 is none", () => {
  const rows = hatchRows(COPPER4, { "F.Cu": { x_um: 1500, y_um: -2000 } });
  assert.deepEqual(rows.map((r) => [r.layer, r.x_um, r.y_um]), [["F.Cu", 1500, -2000], ["In1.Cu", 0, 0], ["In2.Cu", 0, 0], ["B.Cu", 0, 0]]);
  assert.deepEqual(hatchToSend(rows), { "F.Cu": { x_um: 1500, y_um: -2000 } });
  assert.deepEqual(hatchProblems(rows), {});
  rows[1]!.x_um = 2_000_000;
  rows[2]!.y_um = NaN;
  assert.deepEqual(Object.keys(hatchProblems(rows)), ["In1.Cu", "In2.Cu"]);
  assert.ok(sameHatchOffsets({ "F.Cu": { x_um: 0, y_um: 0 } }, undefined) && sameHatchOffsets({ "B.Cu": { x_um: 1, y_um: 0 }, "F.Cu": { x_um: 2, y_um: 0 } }, { "F.Cu": { x_um: 2, y_um: 0 }, "B.Cu": { x_um: 1, y_um: 0 } }));
  assert.ok(!sameHatchOffsets({ "F.Cu": { x_um: 1, y_um: 0 } }, undefined));
});

test("the tuning patterns start as MEANDER_SETTINGS and are checked like a meander", () => {
  const p = DEFAULT_TUNING_PATTERNS;
  assert.deepEqual(tuningPatternsProblems(p), {});
  assert.deepEqual([p.single_track.spacing_um, p.diff_pair.spacing_um, p.diff_pair_skew.spacing_um], [600, 1000, 600]);
  assert.deepEqual([p.single_track.min_amplitude_um, p.single_track.max_amplitude_um, p.single_track.corner_style, p.single_track.corner_radius_percentage, p.single_track.single_sided], [200, 1000, "round", 80, false]);
  assert.ok(sameTuningPatterns(undefined, p));
  const with_ = (f: (q: ReturnType<typeof tuningPatternsOf>) => void) => {
    const q = tuningPatternsOf(undefined);
    f(q);
    return tuningPatternsProblems(q);
  };
  assert.deepEqual(Object.keys(with_((q) => (q.diff_pair.max_amplitude_um = 0))), ["diff_pair.max_amplitude"]);
  assert.deepEqual(Object.keys(with_((q) => (q.single_track.min_amplitude_um = 2000))), ["single_track.min_amplitude"], "the minimum is above the maximum");
  assert.deepEqual(Object.keys(with_((q) => (q.diff_pair_skew.corner_radius_percentage = 101))), ["diff_pair_skew.corner_radius_percentage"]);
  assert.deepEqual(Object.keys(with_((q) => (q.single_track.spacing_um = 200_000))), ["single_track.spacing"]);
  assert.deepEqual(Object.keys(with_((q) => (q.single_track.spacing_um = NaN))), ["single_track.spacing"]);
});

function profile(name: string): TuningProfile {
  return { ...emptyProfile(), name, profile_type: "differential", target_impedance: 90, enable_time_domain_tuning: true, layer_entries: [{ signal_layer: "F.Cu", top_reference_layer: null, bottom_reference_layer: "In1.Cu", width_um: 130, diff_pair_gap_um: 150, delay_as_per_mm: 6_100_000 }], via_propagation_delay_as: 25_000_000 };
}

test("a new track row starts as KiCad's does: front layer first, then the next layer down with its neighbours as references, and round again after the back layer", () => {
  let p = emptyProfile();
  const layers = () => p.layer_entries.map((e) => [e.signal_layer, e.top_reference_layer, e.bottom_reference_layer]);
  p = addTrackRow(p, COPPER4);
  assert.deepEqual(layers(), [["F.Cu", null, "In1.Cu"]]);
  p = addTrackRow(p, COPPER4);
  p = addTrackRow(p, COPPER4);
  p = addTrackRow(p, COPPER4);
  assert.deepEqual(layers(), [["F.Cu", null, "In1.Cu"], ["In1.Cu", "F.Cu", "In2.Cu"], ["In2.Cu", "In1.Cu", "B.Cu"], ["B.Cu", "In2.Cu", null]]);
  p = addTrackRow(p, COPPER4);
  assert.deepEqual(layers()[4], ["F.Cu", null, "In1.Cu"], "after the back layer it starts again at the front");
  assert.deepEqual(addTrackRow(emptyProfile(), ["F.Cu"]).layer_entries.map((e) => [e.signal_layer, e.bottom_reference_layer]), [["F.Cu", null]]);
  assert.equal(addTrackRow({ ...emptyProfile(), layer_entries: [{ signal_layer: "Gone", width_um: 0, diff_pair_gap_um: 0, delay_as_per_mm: 0 }] }, COPPER2).layer_entries[1]!.signal_layer, "", "a last row on a layer the board lacks leaves the new row without one");
  assert.deepEqual(addViaOverride(emptyProfile(), COPPER4).via_overrides, [{ signal_layer_from: "F.Cu", signal_layer_to: "B.Cu", via_layer_from: "F.Cu", via_layer_to: "B.Cu", delay_as: 0 }]);
});

test("a profile must be named and name each layer once, and its rows only copper the board has", () => {
  const ok = profilesDraftOf({ profiles: [profile("50 ohm")], class_profiles: { Default: "50 ohm" } });
  const clean = checkProfiles(ok, COPPER4);
  assert.deepEqual([clean.errors, clean.rowErrors, clean.page], [{}, {}, null]);
  const bad = (f: (p: TuningProfile) => void, copper = COPPER4) => {
    const p = profile("50 ohm");
    f(p);
    return checkProfiles({ profiles: [p], classProfiles: {} }, copper);
  };
  assert.equal(bad((p) => (p.name = "")).errors[0], "Tuning profile must have a name");
  assert.equal(bad((p) => p.layer_entries.push({ ...p.layer_entries[0]! })).errors[0], "Duplicated signal layer configuration in tuning profile");
  assert.equal(bad((p) => p.layer_entries.push({ ...p.layer_entries[0]! })).rowErrors["0:1"], "Duplicated signal layer configuration in tuning profile");
  assert.match(bad((p) => (p.layer_entries[0]!.signal_layer = "B.SilkS")).errors[0]!, /not a copper layer/);
  assert.match(bad((p) => (p.layer_entries[0]!.bottom_reference_layer = "In9.Cu")).errors[0]!, /reference layer/);
  assert.match(bad((p) => (p.target_impedance = -1)).errors[0]!, /ohms/);
  assert.match(bad((p) => (p.layer_entries[0]!.delay_as_per_mm = 3e9)).errors[0]!, /delay/);
  assert.match(bad((p) => p.via_overrides.push({ signal_layer_from: "F.Cu", signal_layer_to: "In7.Cu", via_layer_from: "F.Cu", via_layer_to: "B.Cu", delay_as: 0 })).errors[0]!, /Via override 1/);
  assert.match(bad(() => {}, COPPER2).errors[0]!, /In1\.Cu|reference layer/, "a board with fewer layers than the profile names");
  assert.equal(checkProfiles({ profiles: [], classProfiles: { "": "x" } }, COPPER4).page, "A net class that names a profile has a name");
});

test("a class that names a profile the page does not have is what kicad-cli reports as missing", () => {
  const d = profilesDraftOf({ profiles: [profile("50 ohm"), profile("")], class_profiles: { Default: "50 ohm", USB: "90 ohm diff", Power: "", Blank: "" } });
  assert.deepEqual(missingProfiles(d), [["USB", "90 ohm diff"]], "a class that names nothing names no missing profile, and an unnamed profile is no profile");
  assert.deepEqual(profilesToSend(d).class_profiles, { Default: "50 ohm", USB: "90 ohm diff" }, "a class that names nothing is not sent");
  assert.equal(profilesToSend({ profiles: [], classProfiles: { A: "" } }).class_profiles, undefined);
  assert.ok(sameProfiles({ profiles: [profile("a")], class_profiles: { X: "" } }, { profiles: [profile("a")] }));
  assert.ok(!sameProfiles({ profiles: [profile("a")] }, { profiles: [profile("b")] }));
  assert.deepEqual([psToAs(5.6), asToPs(6_100_000), psToAs(0.0000004)], [5_600_000, 6.1, 0]);
});
