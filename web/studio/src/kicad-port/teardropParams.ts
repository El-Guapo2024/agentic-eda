// A via's and a pad's own teardrop parameters (`TEARDROP_PARAMETERS`, pcbnew/teardrop/teardrop_parameters.h at KiCad 8303b2ad; `eda_model::teardrop_params`): what
// Track & Via Properties' and Pad Properties' Teardrops parts and the Properties panel's Teardrops rows show and set. An item that has none of its own is served
// by Board Setup's block (the studio's stand-in for the Edit Teardrops dialog), so that is what the dialogs show for it; an edit gives the item its own
// (`set_teardrop_params`), which then govern it whatever Board Setup says.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { BoardState, Nm, TeardropParams, TeardropSettings } from "../api/types";

/** `TEARDROP_PARAMETERS()`: disabled, 1 mm and 2 mm at most, half the anchor long and as wide as it, 90% filter, straight edges, two segments, zone connections preferred. */
export const DEFAULT_TEARDROP_PARAMS: TeardropParams = {
  enabled: false,
  allow_two_segments: true,
  prefer_zone_connections: true,
  best_length_ratio: 0.5,
  max_length: 1_000_000,
  best_width_ratio: 1.0,
  max_width: 2_000_000,
  curved_edges: false,
  filter_ratio: 0.9,
};

/** `TeardropParams::of_settings`: what Board Setup's block gives an item without parameters of its own (`DIALOG_GLOBAL_EDIT_TEARDROPS::processItem`), for its kind. */
export function paramsOfSettings(s: Partial<TeardropSettings> | undefined, targeted: boolean): TeardropParams {
  return {
    ...DEFAULT_TEARDROP_PARAMS,
    enabled: !!s?.enabled && targeted,
    best_length_ratio: s?.best_length_ratio ?? DEFAULT_TEARDROP_PARAMS.best_length_ratio,
    best_width_ratio: s?.best_width_ratio ?? DEFAULT_TEARDROP_PARAMS.best_width_ratio,
    max_length: s?.max_len_nm ?? DEFAULT_TEARDROP_PARAMS.max_length,
    max_width: s?.max_width_nm ?? DEFAULT_TEARDROP_PARAMS.max_width,
    filter_ratio: s?.width_to_size_filter_ratio ?? DEFAULT_TEARDROP_PARAMS.filter_ratio,
  };
}

/** The parameters in effect on a via: its own, else Board Setup's (`target_vias`). */
export function viaTeardrops(board: BoardState, via: { teardrops?: TeardropParams | null }): TeardropParams {
  const s = board.routing?.teardrop_settings;
  return via.teardrops ?? paramsOfSettings(s, s?.target_vias ?? true);
}

/** The parameters in effect on a pad: its own, else Board Setup's (`target_pth_pads` for a plated hole, `target_smd_pads` for the rest). */
export function padTeardrops(board: BoardState, pad: { teardrops?: TeardropParams | null; kind?: string }): TeardropParams {
  const s = board.routing?.teardrop_settings;
  const targeted = pad.kind === "through_hole" ? (s?.target_pth_pads ?? true) : (s?.target_smd_pads ?? true);
  return pad.teardrops ?? paramsOfSettings(s, targeted);
}

/** The Teardrops part of a dialog: each field the items share, or null where they differ (`INDETERMINATE_STATE`); the ratios as percentages, as the dialogs show them. */
export interface TeardropForm {
  enabled: boolean | null;
  allowTwo: boolean | null;
  preferZone: boolean | null;
  lenPercent: number | null;
  maxLen: Nm | null;
  widthPercent: number | null;
  maxWidth: Nm | null;
  hdPercent: number | null;
  curved: boolean | null;
}

const same = <T>(vs: readonly T[]): T | null => (vs.length > 0 && vs.every((v) => v === vs[0]) ? vs[0]! : null);

/** `TransferDataToWindow`'s Teardrops part over `params` (one per item): `m_BestLengthRatio * 100` and kin, indeterminate where they differ. */
export function teardropForm(params: readonly TeardropParams[]): TeardropForm {
  const pct = (v: number) => Math.round(v * 100 * 1e6) / 1e6;
  return {
    enabled: same(params.map((p) => p.enabled)),
    allowTwo: same(params.map((p) => p.allow_two_segments)),
    preferZone: same(params.map((p) => p.prefer_zone_connections)),
    lenPercent: same(params.map((p) => pct(p.best_length_ratio))),
    maxLen: same(params.map((p) => p.max_length)),
    widthPercent: same(params.map((p) => pct(p.best_width_ratio))),
    maxWidth: same(params.map((p) => p.max_width)),
    hdPercent: same(params.map((p) => pct(p.filter_ratio))),
    curved: same(params.map((p) => p.curved_edges)),
  };
}

/** Whether `form` changed any field against `before` (only a changed field is applied, as only a determinate one is in KiCad's dialogs). */
export function teardropFormChanged(before: TeardropForm, form: TeardropForm): boolean {
  return (Object.keys(form) as (keyof TeardropForm)[]).some((k) => form[k] != null && form[k] !== before[k]);
}

/**
 * `TransferDataFromWindow`'s Teardrops part on one item: each field that is determinate and changed replaces the item's own value (`targetParams->m_Enabled =
 * m_cbTeardrops->GetValue()`, `m_BestLengthRatio = m_teardropLenPercent.GetDoubleValue() / 100.0`, ...).
 */
export function applyTeardropForm(item: TeardropParams, before: TeardropForm, form: TeardropForm): TeardropParams {
  const changed = <K extends keyof TeardropForm>(k: K) => form[k] != null && form[k] !== before[k];
  const out = { ...item };
  if (changed("enabled")) out.enabled = form.enabled!;
  if (changed("allowTwo")) out.allow_two_segments = form.allowTwo!;
  if (changed("preferZone")) out.prefer_zone_connections = form.preferZone!;
  if (changed("lenPercent")) out.best_length_ratio = form.lenPercent! / 100;
  if (changed("maxLen")) out.max_length = form.maxLen!;
  if (changed("widthPercent")) out.best_width_ratio = form.widthPercent! / 100;
  if (changed("maxWidth")) out.max_width = form.maxWidth!;
  if (changed("hdPercent")) out.filter_ratio = form.hdPercent! / 100;
  if (changed("curved")) out.curved_edges = form.curved!;
  return out;
}
