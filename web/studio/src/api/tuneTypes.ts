// The length tuner's data (crates/cli/src/tune_api.rs, crates/model/src/tuning.rs): the pattern a tuning leaves on the board, what the status box beside the
// cursor reads, the preview of a meander. Micrometres, except `Readout`, which is millimetres (what the box prints).

import type { Um } from "./types";

/** `7` single track, `8` differential pair, `9` skew: which `pcbnew.LengthTuner.*` a pattern or a session is. */
export type TuneKind = "single" | "diffpair" | "skew";

export type PatternSide = "left" | "default" | "right";

/** `MINOPTMAX`: the length (or skew) a pattern is tuned to and the range it counts as tuned within. A pattern with no target holds `1e9` um. */
export interface TargetRange {
  min: Um;
  opt: Um;
  max: Um;
}

/** `LENGTH_UNCONSTRAINED`, micrometres: a million millimetres. */
export const LENGTH_UNCONSTRAINED_UM = 1_000_000_000;

export interface Pt {
  x: Um;
  y: Um;
}

/** `crates/model/src/tuning.rs` `TuningPattern` (+ `handles` and `info`, which the board state adds). `mode` is the file's word: `single`, `diff_pair`, `diff_pair_skew`. */
export interface TuningPattern {
  id: string;
  mode: "single" | "diff_pair" | "diff_pair_skew";
  layer: string;
  origin: Pt;
  end: Pt;
  initial_side: PatternSide;
  min_amplitude: Um;
  max_amplitude: Um;
  spacing: Um;
  corner_radius_percent: number;
  rounded: boolean;
  single_sided: boolean;
  override_custom_rules: boolean;
  target_length: TargetRange;
  target_skew: TargetRange;
  base_line: Pt[];
  base_line_coupled?: Pt[];
  last_track_width: Um;
  last_diff_pair_gap: Um;
  /** Nanometres. */
  last_tuning_length: number;
  last_netname: string;
  last_status: "tuned" | "too_short" | "too_long" | string;
  member_ids: string[];
  /** `MakeEditPoints`: where the pattern starts and ends, the width of its meanders and their spacing. */
  handles?: [Um, Um][];
  /** `PCB_TUNING_PATTERN::GetRowData`'s "Tuning": `12.3 mm (tuned)`. */
  info?: string;
}

/** The settings the tuner and its dialog carry (the keys `apply_settings` reads), over the board's Length-tuning Patterns page. A target is a number (with the default tolerance round it), a range, or null (none). */
export interface TuneSettings {
  min_amplitude?: Um;
  max_amplitude?: Um;
  spacing?: Um;
  corner_radius_percent?: number;
  rounded?: boolean;
  single_sided?: boolean;
  initial_side?: PatternSide;
  override_custom_rules?: boolean;
  target_length?: number | Partial<TargetRange> | null;
  target_skew?: number | Partial<TargetRange> | null;
}

export type TuneStatus = "tuned" | "too_short" | "too_long";

/** `TUNING_STATUS_VIEW_ITEM`: the box beside the cursor. Millimetres. `min`/`max` are null for a pattern with no target (the box shows `---`). */
export interface TuneReadout {
  label: "current length" | "current skew";
  current: number;
  min: number | null;
  opt: number | null;
  max: number | null;
  delta: number | null;
  status: TuneStatus;
  /** The custom rule the target comes from. */
  constraint: string | null;
  partner: string | null;
  /** The length of each net of a pair (P, N). */
  nets: [number, number] | null;
}

/** One track of the new line: the line before the pattern, the meander, the line after it. */
export interface TunePiece {
  pts: [Um, Um][];
  member: boolean;
  net: string;
  layer: string;
  width: Um;
}

export interface TunePreview {
  pieces: TunePiece[];
  /** The tracks the preview replaces: not drawn while it is shown. */
  hide: string[];
}

export interface TuneHoverReply {
  ok: boolean;
  hit?: boolean;
  message?: string;
  pick?: { track_id: string; net: string; layer: string; width: Um; at: Pt };
  mode?: TuneKind;
  readout?: TuneReadout;
  /** The line the track is part of. */
  path?: [Um, Um][];
  pattern?: TuningPattern;
}

/** The reply to start / move / step / settings / edit_start / edit_move. */
export interface TuneSessionReply {
  ok: boolean;
  message?: string;
  mode?: TuneKind;
  pattern?: TuningPattern;
  net?: string;
  editing?: boolean;
  moved?: boolean;
  readout?: TuneReadout;
  preview?: TunePreview;
  origin?: Pt;
  handles?: [Um, Um][];
}

export interface TuneFixReply {
  ok: boolean;
  message?: string;
  pattern_id?: string | null;
  readout?: TuneReadout;
}

export interface TunePatternRow {
  id: string;
  name: string;
  mode: TuneKind;
  layer: string;
  nets: string[];
  members: string[];
  info: string;
  status: string;
  /** Millimetres. */
  length: number;
  origin: Pt;
  end: Pt;
  locked: boolean;
}

export interface TunePatternReply {
  ok: boolean;
  message?: string;
  pattern?: TuningPattern;
  mode?: TuneKind;
  nets?: string[];
  info?: string;
  constraint?: { name: string; min: Um; opt: Um; max: Um } | null;
}
