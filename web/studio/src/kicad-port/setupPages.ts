// The logic behind four Board Setup pages that hold a handful of settings each, as pure functions: Text & Graphics > Formatting (panel_setup_formatting.cpp), Board Stackup > Zone
// Hatch Offsets (panel_setup_zone_hatch_offsets.cpp), Design Rules > Length-tuning Patterns (panel_setup_tuning_patterns.cpp) and Design Rules > Tuning Profiles
// (panel_setup_tuning_profiles.cpp, panel_setup_tuning_profile_info.cpp). The rows of each page, the checks the panel makes before OK and the page a draft sends
// (`set_formatting`, `set_zone_hatch_offsets`, `set_tuning_patterns`, `set_tuning_profiles`, crates/ops/src/board_setup.rs); the backend checks it again
// (crates/model/src/setup_pages.rs) and the two are held to the same cases in their tests.
//
// Ported from:
//   panel_setup_formatting.cpp            the dash and gap lengths (`PCB_PLOT_PARAMS`, 12 and 3: ISO 128-2) and the five "apply board defaults to footprint .." boxes
//   panel_setup_zone_hatch_offsets.cpp    one offset per copper layer in `LSET::AllCuMask().UIOrder()`
//   panel_setup_tuning_patterns.cpp       three groups of meander defaults (`PNS::MEANDER_SETTINGS`)
//   panel_setup_tuning_profile_info.cpp   OnAddTrackRow (which layers a new row starts with), OnAddViaOverride, ValidateProfile
//   pns_meander_placer_base.cpp           AmplitudeStep: the amplitude hot keys step the maximum amplitude and never below the minimum
//
// Beyond KiCad's panels: a dash or gap ratio must be above 0 and at most 1000, a hatch offset at most a metre, an amplitude or a spacing above 0 and at most 100 mm, a corner
// radius from 0 to 100 per cent and the minimum amplitude not above the maximum; a profile's rows may only name the board's copper layers and a delay is at most what KiCad's `int` holds.

import type { FormattingSettings, HatchOffsets, MeanderSettings, TuningPatterns, TuningProfile, TuningProfiles, TuningProfileType } from "../api/types";

// ------------------------------------------------------------------------------------------------------------------------------------ Formatting

export const DEFAULT_DASH_RATIO = 12;
export const DEFAULT_GAP_RATIO = 3;
export const MAX_DASH_RATIO = 1_000;

export const DEFAULT_FORMATTING: FormattingSettings = {
  dash_length_ratio: DEFAULT_DASH_RATIO,
  gap_length_ratio: DEFAULT_GAP_RATIO,
  apply_to_fp_fields: false,
  apply_to_fp_text: false,
  apply_to_fp_shapes: false,
  apply_to_fp_dimensions: false,
  apply_to_fp_barcodes: false,
};

/** The page a board has: what its backend says, else KiCad's start. */
export function formattingOf(f: FormattingSettings | undefined | null): FormattingSettings {
  return f ? { ...DEFAULT_FORMATTING, ...f } : { ...DEFAULT_FORMATTING };
}

/** What is wrong with each ratio (`NaN` stands for text that is no number), in the page's words. */
export function formattingProblems(f: FormattingSettings): Partial<Record<"dash_length_ratio" | "gap_length_ratio", string>> {
  const out: Partial<Record<"dash_length_ratio" | "gap_length_ratio", string>> = {};
  for (const [key, label] of [["dash_length_ratio", "Dash length"], ["gap_length_ratio", "Gap length"]] as const) {
    const v = f[key];
    if (!Number.isFinite(v) || v <= 0 || v > MAX_DASH_RATIO) out[key] = `${label} must be a number above 0 and at most ${MAX_DASH_RATIO} (a ratio of the line width)`;
  }
  return out;
}

export function sameFormatting(a: FormattingSettings | undefined | null, b: FormattingSettings | undefined | null): boolean {
  return JSON.stringify(formattingOf(a)) === JSON.stringify(formattingOf(b));
}

// ------------------------------------------------------------------------------------------------------------------------------------ Zone Hatch Offsets

/** The most a hatch offset may be: a metre. */
export const MAX_HATCH_OFFSET_UM = 1_000_000;

/** One row of the page: a copper layer and its offset. `NaN` is text that is no length (the page's checks name it). */
export interface HatchRow {
  layer: string;
  x_um: number;
  y_um: number;
}

/** The rows in `LSET::AllCuMask().UIOrder()` (front to back, as the board lists its copper layers), each with the offset the board has for it (0, 0 when none). */
export function hatchRows(copper: readonly string[], offsets: HatchOffsets | undefined | null): HatchRow[] {
  return copper.map((layer) => ({ layer, x_um: offsets?.[layer]?.x_um ?? 0, y_um: offsets?.[layer]?.y_um ?? 0 }));
}

/** The problem of each row, by layer. */
export function hatchProblems(rows: readonly HatchRow[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const r of rows) {
    if (!Number.isFinite(r.x_um) || !Number.isFinite(r.y_um)) out[r.layer] = "The offset must be a length.";
    else if (Math.abs(r.x_um) > MAX_HATCH_OFFSET_UM || Math.abs(r.y_um) > MAX_HATCH_OFFSET_UM) out[r.layer] = `The hatch offset of ${r.layer} is at most ${MAX_HATCH_OFFSET_UM / 1000} mm either way`;
  }
  return out;
}

/** The page a draft sends: the layers that have an offset (one at 0, 0 is the same as none). */
export function hatchToSend(rows: readonly HatchRow[]): HatchOffsets {
  const out: HatchOffsets = {};
  for (const r of rows) if (r.x_um !== 0 || r.y_um !== 0) out[r.layer] = { x_um: Math.round(r.x_um), y_um: Math.round(r.y_um) };
  return out;
}

export function sameHatchOffsets(a: HatchOffsets | undefined | null, b: HatchOffsets | undefined | null): boolean {
  const norm = (o: HatchOffsets | undefined | null) => JSON.stringify(Object.entries(o ?? {}).filter(([, v]) => v.x_um !== 0 || v.y_um !== 0).sort(([x], [y]) => (x < y ? -1 : 1)));
  return norm(a) === norm(b);
}

// ------------------------------------------------------------------------------------------------------------------------------------ Length-tuning Patterns

export type TuningGroup = "single_track" | "diff_pair" | "diff_pair_skew";
export const TUNING_GROUPS: readonly TuningGroup[] = ["single_track", "diff_pair", "diff_pair_skew"];

/** `MEANDER_SETTINGS()` with the spacing `BOARD_DESIGN_SETTINGS` gives a single track and a skew pattern (0.6 mm). */
export const DEFAULT_SINGLE_TRACK_MEANDER: MeanderSettings = { min_amplitude_um: 200, max_amplitude_um: 1_000, spacing_um: 600, corner_style: "round", corner_radius_percentage: 80, single_sided: false };
/** The same with a differential pair's spacing (1 mm). */
export const DEFAULT_DIFF_PAIR_MEANDER: MeanderSettings = { ...DEFAULT_SINGLE_TRACK_MEANDER, spacing_um: 1_000 };

export const DEFAULT_TUNING_PATTERNS: TuningPatterns = { single_track: DEFAULT_SINGLE_TRACK_MEANDER, diff_pair: DEFAULT_DIFF_PAIR_MEANDER, diff_pair_skew: DEFAULT_SINGLE_TRACK_MEANDER };

/** The most an amplitude or a spacing may be, µm. */
export const MAX_MEANDER_UM = 100_000;

/** The page a board has: what its backend says, else KiCad's start. */
export function tuningPatternsOf(p: TuningPatterns | undefined | null): TuningPatterns {
  return {
    single_track: { ...DEFAULT_SINGLE_TRACK_MEANDER, ...(p?.single_track ?? {}) },
    diff_pair: { ...DEFAULT_DIFF_PAIR_MEANDER, ...(p?.diff_pair ?? {}) },
    diff_pair_skew: { ...DEFAULT_SINGLE_TRACK_MEANDER, ...(p?.diff_pair_skew ?? {}) },
  };
}

/** What is wrong with each setting, keyed `group.field` (`single_track.min_amplitude`), in the page's words. `NaN` stands for text that is no number. */
export function tuningPatternsProblems(p: TuningPatterns): Record<string, string> {
  const out: Record<string, string> = {};
  for (const group of TUNING_GROUPS) {
    const s = p[group];
    for (const [field, v] of [["min_amplitude", s.min_amplitude_um], ["max_amplitude", s.max_amplitude_um], ["spacing", s.spacing_um]] as const) {
      const name = field.replace("_", " ");
      if (!Number.isFinite(v)) out[`${group}.${field}`] = `The ${name} must be a length.`;
      else if (v <= 0) out[`${group}.${field}`] = `The ${name} must be above 0.`;
      else if (v > MAX_MEANDER_UM) out[`${group}.${field}`] = `The ${name} is at most ${MAX_MEANDER_UM / 1000} mm.`;
    }
    if (!out[`${group}.min_amplitude`] && !out[`${group}.max_amplitude`] && s.min_amplitude_um > s.max_amplitude_um) out[`${group}.min_amplitude`] = "The minimum amplitude cannot be above the maximum amplitude.";
    if (!Number.isInteger(s.corner_radius_percentage) || s.corner_radius_percentage < 0 || s.corner_radius_percentage > 100) out[`${group}.corner_radius_percentage`] = "The corner radius is a percentage from 0 to 100.";
  }
  return out;
}

export function sameTuningPatterns(a: TuningPatterns | undefined | null, b: TuningPatterns | undefined | null): boolean {
  return JSON.stringify(tuningPatternsOf(a)) === JSON.stringify(tuningPatternsOf(b));
}

export type TuneMode = "single" | "diffpair" | "skew";

/**
 * What the length tuner starts from for a mode (`PCB_TUNING_PATTERN` takes the board's `m_SingleTrackMeanderSettings`, `m_DiffPairMeanderSettings` or `m_SkewMeanderSettings` for the
 * pattern it makes): the amplitude is the maximum amplitude (what `AmplitudeStep` steps), the spacing the page's, and the amplitude never steps below the minimum.
 */
export function tunerStart(patterns: TuningPatterns | undefined | null, mode: TuneMode): { amplitudeUm: number; spacingUm: number; minAmplitudeUm: number } {
  const p = tuningPatternsOf(patterns);
  const s = mode === "single" ? p.single_track : mode === "diffpair" ? p.diff_pair : p.diff_pair_skew;
  return { amplitudeUm: s.max_amplitude_um, spacingUm: s.spacing_um, minAmplitudeUm: s.min_amplitude_um };
}

// ------------------------------------------------------------------------------------------------------------------------------------ Tuning Profiles

/** KiCad keeps a delay in attoseconds per millimetre and a via delay in attoseconds: a picosecond is this many. */
export const AS_PER_PS = 1_000_000;
/** What KiCad's `int` holds. */
export const MAX_DELAY_AS = 2_147_483_647;
export const MAX_PROFILE_NAME = 256;
export const MAX_PROFILES = 200;
export const MAX_PROFILE_ROWS = 200;

/** The page as the dialog edits it: every profile, and the profile each net class names (a class naming nothing is not in the map). */
export interface ProfilesDraft {
  profiles: TuningProfile[];
  classProfiles: Record<string, string>;
}

export function profilesDraftOf(t: TuningProfiles | undefined | null): ProfilesDraft {
  return { profiles: (t?.profiles ?? []).map((p) => ({ ...p, layer_entries: p.layer_entries.map((e) => ({ ...e })), via_overrides: p.via_overrides.map((v) => ({ ...v })) })), classProfiles: { ...(t?.class_profiles ?? {}) } };
}

/** A new profile (`OnAddTuningProfileClick`): named "Profile", single-ended, nothing in its grids. */
export function emptyProfile(): TuningProfile {
  return { name: "", profile_type: "single", target_impedance: 0, enable_time_domain_tuning: false, layer_entries: [], via_propagation_delay_as: 0, via_overrides: [] };
}

/**
 * `OnAddTrackRow`: a new row of the track grid. The first row is the front layer, with the second layer as its bottom reference; each next one is the layer below the last row's
 * (the one above is its top reference, the one below its bottom reference), and after the back layer it starts again at the front.
 */
export function addTrackRow(p: TuningProfile, copper: readonly string[]): TuningProfile {
  const rows = p.layer_entries;
  const row = { signal_layer: "", top_reference_layer: null as string | null, bottom_reference_layer: null as string | null, width_um: 0, diff_pair_gap_um: 0, delay_as_per_mm: 0 };
  const front = () => {
    if (copper.length > 0) row.signal_layer = copper[0]!;
    if (copper.length > 1) row.bottom_reference_layer = copper[1]!;
  };
  if (rows.length === 0) front();
  else {
    const at = copper.indexOf(rows[rows.length - 1]!.signal_layer);
    if (at < 0) row.signal_layer = "";
    else if (at === copper.length - 1) front();
    else {
      row.signal_layer = copper[at + 1]!;
      row.top_reference_layer = copper[at]!;
      if (at + 2 < copper.length) row.bottom_reference_layer = copper[at + 2]!;
    }
  }
  return { ...p, layer_entries: [...rows, row] };
}

/** `OnAddViaOverride`: a via override from the front layer to the back one, with no delay. */
export function addViaOverride(p: TuningProfile, copper: readonly string[]): TuningProfile {
  const first = copper[0] ?? "";
  const last = copper[copper.length - 1] ?? "";
  return { ...p, via_overrides: [...p.via_overrides, { signal_layer_from: first, signal_layer_to: last, via_layer_from: first, via_layer_to: last, delay_as: 0 }] };
}

export interface ProfilesCheck {
  /** Profile index -> the first thing wrong with it, in the page's words. */
  errors: Record<number, string>;
  /** `profile:row` (`0:2` is the third track row of the first profile) -> what is wrong with that row of the track grid; `v0:1` for a via override. */
  rowErrors: Record<string, string>;
  /** A problem with the page as a whole. */
  page: string | null;
}

/** `PANEL_SETUP_TUNING_PROFILES::Validate` / `ValidateProfile`, and the limits a stored page needs. */
export function checkProfiles(d: ProfilesDraft, copper: readonly string[]): ProfilesCheck {
  const errors: Record<number, string> = {};
  const rowErrors: Record<string, string> = {};
  let page: string | null = null;
  if (d.profiles.length > MAX_PROFILES) page = `A board has at most ${MAX_PROFILES} tuning profiles`;
  const have = (l: string) => copper.includes(l);
  d.profiles.forEach((p, i) => {
    const fail = (msg: string) => {
      if (errors[i] === undefined) errors[i] = msg;
    };
    if (p.name === "") fail("Tuning profile must have a name");
    else if (p.name.length > MAX_PROFILE_NAME || /[\u0000-\u001f\u007f-\u009f]/.test(p.name)) fail(`A profile's name is at most ${MAX_PROFILE_NAME} characters, with no control characters`);
    if (!Number.isFinite(p.target_impedance) || p.target_impedance < 0 || p.target_impedance > 1e6) fail("The target impedance is a number of ohms from 0 up");
    if (p.layer_entries.length > MAX_PROFILE_ROWS || p.via_overrides.length > MAX_PROFILE_ROWS) fail(`A profile has at most ${MAX_PROFILE_ROWS} rows in each grid`);
    if (!Number.isFinite(p.via_propagation_delay_as) || p.via_propagation_delay_as < 0 || p.via_propagation_delay_as > MAX_DELAY_AS) fail("The via propagation delay is from 0 to 2147 ps");
    const seen = new Set<string>();
    p.layer_entries.forEach((e, row) => {
      const key = `${i}:${row}`;
      const rowFail = (msg: string) => {
        if (rowErrors[key] === undefined) rowErrors[key] = msg;
        fail(`Row ${row + 1}: ${msg}`);
      };
      if (!have(e.signal_layer)) rowFail(`${e.signal_layer || "(no layer)"} is not a copper layer of this board`);
      for (const r of [e.top_reference_layer, e.bottom_reference_layer]) if (r && !have(r)) rowFail(`the reference layer ${r} is not a copper layer of this board`);
      if (seen.has(e.signal_layer)) {
        rowErrors[key] = "Duplicated signal layer configuration in tuning profile";
        errors[i] = errors[i] ?? "Duplicated signal layer configuration in tuning profile";
      }
      seen.add(e.signal_layer);
      if (![e.width_um, e.diff_pair_gap_um].every((v) => Number.isFinite(v) && v >= 0 && v <= 100_000)) rowFail("the width and the gap are from 0 to 100 mm");
      if (!Number.isFinite(e.delay_as_per_mm) || e.delay_as_per_mm < 0 || e.delay_as_per_mm > MAX_DELAY_AS) rowFail("the delay is from 0 to 2147 ps/mm");
    });
    p.via_overrides.forEach((v, row) => {
      const key = `v${i}:${row}`;
      const rowFail = (msg: string) => {
        rowErrors[key] = rowErrors[key] ?? msg;
        fail(`Via override ${row + 1}: ${msg}`);
      };
      for (const l of [v.signal_layer_from, v.signal_layer_to, v.via_layer_from, v.via_layer_to]) if (!have(l)) rowFail(`${l || "(no layer)"} is not a copper layer of this board`);
      if (!Number.isFinite(v.delay_as) || v.delay_as < 0 || v.delay_as > MAX_DELAY_AS) rowFail("the delay is from 0 to 2147 ps");
    });
  });
  for (const [cls, profile] of Object.entries(d.classProfiles)) {
    if (cls === "") page = page ?? "A net class that names a profile has a name";
    else if (profile.length > MAX_PROFILE_NAME || /[\u0000-\u001f\u007f-\u009f]/.test(profile)) page = page ?? `The profile ${cls} names is at most ${MAX_PROFILE_NAME} characters, with no control characters`;
  }
  return { errors, rowErrors, page };
}

/** The page a draft sends: the profiles as they are, the classes that name a profile. */
export function profilesToSend(d: ProfilesDraft): TuningProfiles {
  const class_profiles = Object.fromEntries(Object.entries(d.classProfiles).filter(([, p]) => p !== ""));
  return { profiles: d.profiles, ...(Object.keys(class_profiles).length > 0 ? { class_profiles } : {}) };
}

/** The classes that name a profile the page does not define -- what kicad-cli's DRC reports as `missing_tuning_profile` (an empty name is no profile). */
export function missingProfiles(d: ProfilesDraft): Array<[string, string]> {
  const have = new Set(d.profiles.map((p) => p.name).filter((n) => n !== ""));
  return Object.entries(d.classProfiles).filter(([, p]) => p !== "" && !have.has(p)).sort(([a], [b]) => (a < b ? -1 : 1));
}

export function sameProfiles(a: TuningProfiles | undefined | null, b: TuningProfiles | undefined | null): boolean {
  const norm = (t: TuningProfiles | undefined | null) => JSON.stringify([(t?.profiles ?? []).map((p) => ({ ...p, layer_entries: p.layer_entries.map((e) => ({ ...e, top_reference_layer: e.top_reference_layer ?? null, bottom_reference_layer: e.bottom_reference_layer ?? null })) })), Object.entries(t?.class_profiles ?? {}).filter(([, p]) => p !== "").sort(([x], [y]) => (x < y ? -1 : 1))]);
  return norm(a) === norm(b);
}

export const PROFILE_TYPE_LABEL: Readonly<Record<TuningProfileType, string>> = { single: "Single", differential: "Differential pair" };

/** A delay in the unit KiCad keeps (as per mm, as) as the picoseconds the page shows. */
export const asToPs = (as: number): number => as / AS_PER_PS;
export const psToAs = (ps: number): number => Math.round(ps * AS_PER_PS);
