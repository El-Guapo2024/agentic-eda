// The message panel of a selected tuning pattern: `PCB_TUNING_PATTERN::GetMsgPanelInfo` (pcbnew/generators/pcb_tuning_pattern.cpp at 8303b2ad),
// UI-LEFT-2 RR5. The server reads the board (`/api/tune/pattern`'s `panel`: the member tracks' nets, class, width, `BOARD::GetTrackLength` of each
// net and the rule `EvalRules` gives the two tracks); the rows and their words are built here, as KiCad builds them.
//
// Dependency-free (no React), so it runs under `npm run test:unit`.

import type { TargetRange, TuningPattern } from "../api/tuneTypes";

/** One field of the message panel (`MSG_PANEL_ITEM( aUpperText, aLowerText )`). */
export interface PanelItem {
  upper: string;
  lower: string;
}

/** `BOARD::GetTrackLength` of a track of a net: the track and via length together, the pad-to-die length (nanometres), and the same as delays
 *  (attoseconds). */
export interface NetLengths {
  length: number;
  die_length: number;
  delay: number;
  die_delay: number;
}

/** What `/api/tune/pattern` answers as `panel`. */
export interface TuningPanelData {
  primary_net?: string | null;
  coupled_net?: string | null;
  netclass?: string | null;
  /** The members' width; none when they differ (`mixedWidth`) or there are none. */
  width?: number | null;
  /** None for a net of no net code (`primaryItem->GetNetCode() > 0`). */
  primary?: NetLengths | null;
  coupled?: NetLengths | null;
  /** `EvalRules( SKEW_CONSTRAINT / LENGTH_CONSTRAINT, primaryItem, coupledItem, m_layer )` as the rule has it: nanometres, or attoseconds with `time_domain`. */
  constraint?: { name: string; min?: number | null; opt?: number | null; max?: number | null; time_domain?: boolean } | null;
}

/** `EDA_UNIT_UTILS::UI::MessageTextFromMinOptMax`: "min %s" (a minimum above 0), "opt %s", "max %s", joined by "; ". */
export function messageTextFromMinOptMax(v: { min?: number | null; opt?: number | null; max?: number | null }, fmt: (n: number) => string): string {
  const parts: string[] = [];
  if (v.min != null && v.min > 0) parts.push(`min ${fmt(v.min)}`);
  if (v.opt != null) parts.push(`opt ${fmt(v.opt)}`);
  if (v.max != null) parts.push(`max ${fmt(v.max)}`);
  return parts.join("; ");
}

/**
 * `PCB_TUNING_PATTERN::GetMsgPanelInfo`: "Type"; "Nets" (two) or "Net"; "Resolved Netclass"; "Layer" (`LayerMaskDescribe`, the layer's name); "Width" when the
 * members share one; for a net of its own the routed length or delay -- "Routed Lengths" / "Routed Delays" of a pair (the lengths unless both nets have a
 * delay), "Routed Length" / "Routed Delay" -- then "Pad To Die Length" and "Full Length" (a pad-to-die length and no pad-to-die delay) or "Pad To Die Delay"
 * and "Full Delay"; and the target: for a skew pattern "Target Skew: %s" "(from tuning pattern properties)" (no rule, or "Override custom rules") or "Skew
 * Constraints: %s" "(from %s)" (KiCad formats the rule's range as a length whatever its kind), else "Target Length: %s" / "Target Delay: %s" (the time
 * domain) or "Length Constraints: %s" / "Delay Constraints: %s" with the rule's name, a constraint row only when its text is not empty.
 * `fmt` is `MessageTextFromValue` of a length, `fmtDelay` of a time (attoseconds); `layerName` the board's name of the layer.
 */
export function tuningPatternMsgPanel(pattern: TuningPattern, panel: TuningPanelData, fmt: (nm: number) => string, fmtDelay: (as: number) => string, layerName: (layer: string) => string): PanelItem[] {
  const items: PanelItem[] = [{ upper: "Type", lower: "Tuning Pattern" }];
  const timeDomain = pattern.is_time_domain === true;
  if (panel.coupled_net != null) items.push({ upper: "Nets", lower: `${panel.primary_net ?? ""}, ${panel.coupled_net}` });
  else if (panel.primary_net != null) items.push({ upper: "Net", lower: panel.primary_net });
  if (panel.netclass != null) items.push({ upper: "Resolved Netclass", lower: panel.netclass });
  items.push({ upper: "Layer", lower: layerName(pattern.layer) });
  if (panel.width != null && panel.width !== 0) items.push({ upper: "Width", lower: fmt(panel.width) });
  const p = panel.primary;
  if (p) {
    const c = panel.coupled;
    if (c) {
      if (p.delay === 0 || c.delay === 0) items.push({ upper: "Routed Lengths", lower: `${fmt(p.length)}, ${fmt(c.length)}` });
      else items.push({ upper: "Routed Delays", lower: `${fmtDelay(p.delay)}, ${fmtDelay(c.delay)}` });
    } else if (p.delay === 0) items.push({ upper: "Routed Length", lower: fmt(p.length) });
    else items.push({ upper: "Routed Delay", lower: fmtDelay(p.delay) });
    // "lenPadToDie" and "delayPadToDie" are the primary's (the coupled call's are read into other names)
    if (p.die_length !== 0 && p.die_delay === 0) {
      items.push({ upper: "Pad To Die Length", lower: fmt(p.die_length) });
      items.push({ upper: "Full Length", lower: fmt(p.length + p.die_length) });
    } else if (p.die_delay > 0) {
      items.push({ upper: "Pad To Die Delay", lower: fmtDelay(p.die_delay) });
      items.push({ upper: "Full Delay", lower: fmtDelay(p.delay + p.die_delay) });
    }
  }
  const rule = panel.constraint ?? null;
  const fromProperties = "(from tuning pattern properties)";
  const opt = (r: TargetRange | undefined) => r?.opt ?? 0;
  if (pattern.mode === "diff_pair_skew") {
    if (rule == null || pattern.override_custom_rules) items.push({ upper: `Target Skew: ${fmt(opt(pattern.target_skew))}`, lower: fromProperties });
    else {
      const msg = messageTextFromMinOptMax(rule, fmt);
      if (msg !== "") items.push({ upper: `Skew Constraints: ${msg}`, lower: `(from ${rule.name})` });
    }
  } else if (rule == null || pattern.override_custom_rules) {
    if (timeDomain) items.push({ upper: `Target Delay: ${fmtDelay(opt(pattern.target_delay))}`, lower: fromProperties });
    else items.push({ upper: `Target Length: ${fmt(opt(pattern.target_length))}`, lower: fromProperties });
  } else {
    // `MessageTextFromMinOptMax( constraint.GetValue(), unitType )`: the pattern's own domain decides the unit
    const msg = messageTextFromMinOptMax(rule, timeDomain ? fmtDelay : fmt);
    if (msg !== "") items.push({ upper: `${timeDomain ? "Delay Constraints" : "Length Constraints"}: ${msg}`, lower: `(from ${rule.name})` });
  }
  return items;
}
