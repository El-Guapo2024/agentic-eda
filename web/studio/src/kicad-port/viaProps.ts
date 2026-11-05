// A via's padstack settings past its size and drill, as the Properties panel reads and sets them (`TRACK_VIA_DESC`, pcbnew/pcb_track.cpp at KiCad 8303b2ad):
// the tenting, covering and plugging of each side, the capping and filling of the hole (`PADSTACK::OUTER_LAYER_PROPS`, `DRILL_PROPS`), the backdrill
// (`PADSTACK::GetBackdrillMode`, `SetBackdrillMode`, `GetBackdrillSize`, `SetBackdrillSize`, `GetBackdrillEndLayer`, `SetBackdrillEndLayer`, pcbnew/padstack.cpp)
// and the post-machining of each side (`PCB_VIA::SetFrontPostMachining`, `PADSTACK::POST_MACHINING_PROPS`). Each setter answers the via's whole `ViaProps` with
// the one change, which `set_via_props` writes to the via's tokens (crates/model/src/via_extra.rs).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Nm, PostMachiningProps, ViaDrillProps, ViaProps } from "../api/types";
import type { Choice } from "./propertyManager";

/** `ENUM_MAP<TENTING_MODE>`, `<COVERING_MODE>`, `<PLUGGING_MODE>`, `<CAPPING_MODE>`, `<FILLING_MODE>`: "From board stackup" is no value of the via's own. */
export type SideMode = "from_board" | "yes" | "no";

export const TENTING_CHOICES: Choice[] = [
  { label: "From board stackup", value: "from_board" },
  { label: "Tented", value: "yes" },
  { label: "Not tented", value: "no" },
];
export const COVERING_CHOICES: Choice[] = [
  { label: "From board stackup", value: "from_board" },
  { label: "Covered", value: "yes" },
  { label: "Not covered", value: "no" },
];
export const PLUGGING_CHOICES: Choice[] = [
  { label: "From board stackup", value: "from_board" },
  { label: "Plugged", value: "yes" },
  { label: "Not plugged", value: "no" },
];
export const CAPPING_CHOICES: Choice[] = [
  { label: "From board stackup", value: "from_board" },
  { label: "Capped", value: "yes" },
  { label: "Not capped", value: "no" },
];
export const FILLING_CHOICES: Choice[] = [
  { label: "From board stackup", value: "from_board" },
  { label: "Filled", value: "yes" },
  { label: "Not filled", value: "no" },
];

export type FlagKey = "front_tenting" | "back_tenting" | "front_covering" | "back_covering" | "front_plugging" | "back_plugging" | "capping" | "filling";

export const modeOf = (v: boolean | undefined): SideMode => (v === undefined ? "from_board" : v ? "yes" : "no");

/** The props with one of the side settings set (`SetFrontTentingMode` and kin: `FROM_BOARD` is no value of the via's own). */
export function withFlag(p: ViaProps, key: FlagKey, mode: SideMode): ViaProps {
  const next: ViaProps = { ...p };
  if (mode === "from_board") delete next[key];
  else next[key] = mode === "yes";
  return next;
}

// ------------------------------------------------------------------------------------------------------------------------- backdrill

/** `ENUM_MAP<BACKDRILL_MODE>`. */
export type BackdrillMode = "no_backdrill" | "bottom" | "top" | "both";

export const BACKDRILL_MODE_CHOICES: Choice[] = [
  { label: "No backdrill", value: "no_backdrill" },
  { label: "Backdrill bottom", value: "bottom" },
  { label: "Backdrill top", value: "top" },
  { label: "Backdrill both", value: "both" },
];

const has = (d: ViaDrillProps | undefined): d is ViaDrillProps => !!d && d.size > 0;

/** `PADSTACK::GetBackdrillMode`: the secondary drill is the bottom one unless it starts on `F.Cu`, the tertiary the top one unless it starts on `B.Cu`. */
export function backdrillMode(p: ViaProps): BackdrillMode {
  const sec = has(p.backdrill);
  const ter = has(p.tertiary_drill);
  if (!sec && !ter) return "no_backdrill";
  if (sec && ter) return "both";
  if (sec) return p.backdrill!.start === "F.Cu" ? "top" : "bottom";
  return p.tertiary_drill!.start === "B.Cu" ? "bottom" : "top";
}

/**
 * `PADSTACK::SetBackdrillMode`. "Backdrill slightly larger than main drill": a drill that is not there yet starts at 1.1 times the via's drill, on the side it
 * drills from to that same layer (`initDrill`); bottom is the secondary drill from `B.Cu`, top the tertiary from `F.Cu`, and the mode's other drill goes.
 */
export function withBackdrillMode(p: ViaProps, mode: BackdrillMode, viaDrill: Nm): ViaProps {
  const next: ViaProps = { ...p };
  const init = (d: ViaDrillProps | undefined, start: string): ViaDrillProps => (has(d) ? { ...d, start } : { size: Math.round(viaDrill * 1.1), start, end: start });
  if (mode === "no_backdrill" || mode === "bottom") delete next.tertiary_drill;
  if (mode === "no_backdrill" || mode === "top") delete next.backdrill;
  if (mode === "bottom" || mode === "both") next.backdrill = init(p.backdrill, "B.Cu");
  if (mode === "top" || mode === "both") next.tertiary_drill = init(p.tertiary_drill, "F.Cu");
  return next;
}

/** Which of the two drills is the top (`F.Cu`) or the bottom (`B.Cu`) one, as `GetBackdrillSize` / `GetBackdrillEndLayer` look them up. */
function drillOn(p: ViaProps, top: boolean): "backdrill" | "tertiary_drill" | null {
  const start = top ? "F.Cu" : "B.Cu";
  if (has(p.backdrill) && p.backdrill.start === start) return "backdrill";
  if (has(p.tertiary_drill) && p.tertiary_drill.start === start) return "tertiary_drill";
  return null;
}

/** `PADSTACK::GetBackdrillSize`. */
export function backdrillSize(p: ViaProps, top: boolean): Nm | null {
  const k = drillOn(p, top);
  return k ? p[k]!.size : null;
}

/** `PADSTACK::GetBackdrillEndLayer` (`UNDEFINED_LAYER`: ""). */
export function backdrillEnd(p: ViaProps, top: boolean): string {
  const k = drillOn(p, top);
  return k ? p[k]!.end : "";
}

/**
 * `PADSTACK::SetBackdrillSize`: the drill that starts on that side, else the first of the two that is not drilled, else the side's own (top the tertiary, bottom
 * the secondary); a size makes it a round drill from that side (its end stays), none takes it away.
 */
export function withBackdrillSize(p: ViaProps, top: boolean, size: Nm | null): ViaProps {
  const start = top ? "F.Cu" : "B.Cu";
  let target: "backdrill" | "tertiary_drill" | null = null;
  if (p.backdrill?.start === start) target = "backdrill";
  else if (p.tertiary_drill?.start === start) target = "tertiary_drill";
  if (!target) {
    if (!has(p.backdrill)) target = "backdrill";
    else if (!has(p.tertiary_drill)) target = "tertiary_drill";
  }
  target ??= top ? "tertiary_drill" : "backdrill";
  const next: ViaProps = { ...p };
  if (size === null || size <= 0) delete next[target];
  else next[target] = { size, start, end: p[target]?.end || start };
  return next;
}

/** `PADSTACK::SetBackdrillEndLayer`: the end of the drill that starts on that side, if there is one. */
export function withBackdrillEnd(p: ViaProps, top: boolean, layer: string): ViaProps {
  const start = top ? "F.Cu" : "B.Cu";
  const target = p.backdrill?.start === start ? "backdrill" : p.tertiary_drill?.start === start ? "tertiary_drill" : null;
  if (!target || !p[target]) return p;
  return { ...p, [target]: { ...p[target]!, end: layer } };
}

// ---------------------------------------------------------------------------------------------------------------------- post-machining

/** `ENUM_MAP<PAD_DRILL_POST_MACHINING_MODE>`: a via without one is "Not post-machined" (the file writes nothing for it). */
export type PostMachiningMode = "not_post_machined" | "counterbore" | "countersink";

export const POST_MACHINING_MODE_CHOICES: Choice[] = [
  { label: "Not post-machined", value: "not_post_machined" },
  { label: "Counterbore", value: "counterbore" },
  { label: "Countersink", value: "countersink" },
];

export const postMachiningKey = (front: boolean): "front_post_machining" | "back_post_machining" => (front ? "front_post_machining" : "back_post_machining");

export function postMachiningMode(p: ViaProps, front: boolean): PostMachiningMode {
  return p[postMachiningKey(front)]?.mode ?? "not_post_machined";
}

/** `SetFrontPostMachiningMode` / `SetBackPostMachiningMode`: the mode, keeping the size, depth and angle the side had. */
export function withPostMachiningMode(p: ViaProps, front: boolean, mode: PostMachiningMode): ViaProps {
  const key = postMachiningKey(front);
  const next: ViaProps = { ...p };
  if (mode === "not_post_machined") delete next[key];
  else next[key] = { size: 0, depth: 0, angle_ddeg: 0, ...p[key], mode };
  return next;
}

/** `SetFrontPostMachiningSize`, `..Depth`, `..Angle` (and the back's): one of the three numbers of a side that is post-machined. */
export function withPostMachining(p: ViaProps, front: boolean, field: "size" | "depth" | "angle_ddeg", value: number): ViaProps {
  const key = postMachiningKey(front);
  const now: PostMachiningProps | undefined = p[key];
  if (!now) return p;
  return { ...p, [key]: { ...now, [field]: value } };
}
