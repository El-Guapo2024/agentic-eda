// `DIALOG_TRACK_VIA_PROPERTIES` (pcbnew/dialogs/dialog_track_via_properties.cpp): the Track & Via Properties dialog, opened by `EDIT_TOOL::Properties` on the whole
// selection when it holds only tracks, arcs and vias (`SELECTION_CONDITIONS::OnlyTypes( { PCB_TRACE_T, PCB_ARC_T, PCB_VIA_T } )`). Every field shows the value the
// items share, or is indeterminate (null here) when they differ; an indeterminate field is left alone on OK (`if( !m_trackWidth.IsIndeterminate() ) ..`).
//
//   Common:  Net (`m_netSelector`), Locked (3-state `m_lockedCbox`)
//   Tracks:  Start X/Y, End X/Y, Width (`m_trackWidth.Validate( GEOMETRY_MIN_SIZE, INT_MAX )`), Layer
//   Vias:    X/Y, Diameter, Hole (`PCB_VIA::ValidateViaParameters`), Via type, Start layer, End layer (`m_ViaTypeChoice`, `m_ViaStartLayer`, `m_ViaEndLayer`:
//            a through via's pair is F.Cu..B.Cu and the two are disabled, `onViaEdit`), Annular rings (`m_annularRingsCtrl`, shown when `getLayerDepth() > 1`)
//
//            Padstack mode and Edit layer (`m_cbPadstackMode`, `m_cbEditLayer`: the diameter shown is the edited layer's, `onEditLayerChanged`), Protection features
//            (`m_protectionFeatures`, the IPC-4761 presets, kicad-port/viaProtection.ts), Backdrill (`m_backdrillChoice`: None, Bottom, Top, Both; the front
//            backdrill is the secondary drill from F.Cu, the back one the tertiary from B.Cu, each with its size and its must-cut layer), Hole Post-machining
//            (`m_topPostMachine`, `m_bottomPostMachine`: None, Countersink with its size and angle, Counterbore with its size and depth), Teardrops (`m_cbTeardrops`
//            and the sizes, kicad-port/teardropParams.ts)
//
// UI-LEFT P3 (task-u5-pcbfp): the via's padstack, protection, backdrill and post-machining go out as one `set_via_props` per via with its whole `ViaProps` (each
// via's own values kept where a field is indeterminate, as `TransferDataFromWindow` keeps them), the teardrops as `set_teardrop_params`. Not here: the connected
// tracks KiCad offers to resize with a via, the "Automatically update via nets" (free via) box.
import type { BoardState, Cmd, Ipc4761Preset, Nm, PostMachiningProps, TeardropParams, UnconnectedLayers, Via, ViaPadstack, ViaProps } from "../api/types";
import { validateViaParameters } from "./routerVia";
import { viaConfiguration, withPreset } from "./viaProtection";
import { applyTeardropForm, teardropForm, teardropFormChanged, viaTeardrops, type TeardropForm } from "./teardropParams";

export interface TrackViaForm {
  tracks: string[];
  vias: string[];
  net: string | null;
  locked: boolean | null;
  startX: Nm | null;
  startY: Nm | null;
  endX: Nm | null;
  endY: Nm | null;
  width: Nm | null;
  layer: string | null;
  viaX: Nm | null;
  viaY: Nm | null;
  diameter: Nm | null;
  drill: Nm | null;
  viaType: ViaType | null;
  viaFrom: string | null;
  viaTo: string | null;
  rings: UnconnectedLayers | null;
  /** "Padstack mode:" of the first via (`m_viaStack`, a copy of its padstack), and the diameter of each layer it names (`Inner` for front/inner/back). */
  padstackMode: PadstackMode;
  layerSizes: Record<string, Nm>;
  /** "Edit layer:": "F.Cu" for a normal stack (`ALL_LAYERS`), else `F.Cu`, `Inner`, `B.Cu` or a copper layer. */
  editLayer: string;
  /** "Protection features:": a preset, "custom" for none of them, null where the vias differ. */
  protection: Ipc4761Preset | "custom" | null;
  /** "Backdrill:" and its four rows (null where the vias differ; "" is the must-cut layer "None"). */
  backdrill: BackdrillChoice | null;
  backdrillFrontSize: Nm | null;
  backdrillFrontLayer: string | null;
  backdrillBackSize: Nm | null;
  backdrillBackLayer: string | null;
  /** "Hole Post-machining", Front: and Back:; `size2` is the angle in degrees of a countersink, the depth of a counterbore. */
  topPost: PostChoice | null;
  topPostSize: Nm | null;
  topPostSize2: number | null;
  bottomPost: PostChoice | null;
  bottomPostSize: Nm | null;
  bottomPostSize2: number | null;
  /** The Teardrops part, over the vias' parameters in effect (their own, else Board Setup's). */
  teardrops: TeardropForm;
}

export type PadstackMode = "normal" | "front_inner_back" | "custom";
/** `m_cbPadstackModeChoices`. */
export const PADSTACK_MODE_CHOICES: readonly [PadstackMode, string][] = [
  ["normal", "Normal"],
  ["front_inner_back", "Front/Inner/Back"],
  ["custom", "Custom"],
];
/** `m_backdrillChoiceChoices` (`BACKDRILL_MODE`'s order). */
export type BackdrillChoice = "none" | "bottom" | "top" | "both";
export const BACKDRILL_CHOICES: readonly [BackdrillChoice, string][] = [
  ["none", "None"],
  ["bottom", "Bottom"],
  ["top", "Top"],
  ["both", "Both"],
];
/** `m_topPostMachineChoices`: None, Countersink, Counterbore. */
export type PostChoice = "none" | "countersink" | "counterbore";
export const POST_MACHINE_CHOICES: readonly [PostChoice, string][] = [
  ["none", "None"],
  ["countersink", "Countersink"],
  ["counterbore", "Counterbore"],
];

/** `TransferDataToWindow`'s backdrill type of one via: "top = secondary, bottom = tertiary" by which of the two has an end layer. */
export function backdrillChoiceOf(p: ViaProps | null | undefined): BackdrillChoice {
  const sec = !!p?.backdrill && p.backdrill.size > 0;
  const ter = !!p?.tertiary_drill && p.tertiary_drill.size > 0;
  return sec && ter ? "both" : ter ? "bottom" : sec ? "top" : "none";
}

/** The edit layers of a padstack mode (`afterPadstackModeChanged`): All layers for a normal stack, the front, Inner Layers and the back, or each copper layer. */
export function editLayerChoices(mode: PadstackMode, copper: readonly string[]): string[] {
  if (mode === "normal") return ["F.Cu"];
  if (mode === "front_inner_back") return ["F.Cu", "Inner", "B.Cu"];
  return [...copper];
}

/** `PADSTACK::Size( aLayer ).x` of the dialog's stack: the layer's own, else the front's (`CopperLayer`'s fallback to `ALL_LAYERS`). */
export function sizeOn(form: Pick<TrackViaForm, "padstackMode" | "layerSizes" | "diameter">, layer: string): Nm | null {
  if (layer === "F.Cu" || form.padstackMode === "normal") return form.diameter;
  const key = form.padstackMode === "front_inner_back" && layer !== "B.Cu" ? "Inner" : layer;
  return form.layerSizes[key] ?? form.diameter;
}

export type ViaType = "through" | "micro" | "blind" | "buried";
/** `m_ViaTypeChoiceChoices`, in the dialog's order. */
export const VIA_TYPE_CHOICES: readonly [ViaType, string][] = [
  ["through", "Through"],
  ["micro", "Micro"],
  ["blind", "Blind"],
  ["buried", "Buried"],
];
/** `m_annularRingsCtrlChoices` (`getAnnularRingSelection`'s order). */
export const ANNULAR_RING_CHOICES: readonly [UnconnectedLayers, string][] = [
  ["keep_all", "All copper layers"],
  ["remove_except_start_and_end", "Start, end, and connected layers"],
  ["remove_all", "Connected layers only"],
  ["start_end_only", "Start and end layers only"],
];

/** `BOARD::LayerDepth` as `getLayerDepth` asks it: a through via (or a pair not chosen) spans the whole stack, `copperCount - 1`. */
export function viaLayerDepth(type: ViaType | null, from: string | null, to: string | null, copper: readonly string[]): number {
  const whole = copper.length - 1;
  if (type == null || type === "through" || from == null || to == null) return whole;
  const a = copper.indexOf(from);
  const b = copper.indexOf(to);
  return a < 0 || b < 0 ? whole : Math.abs(b - a);
}

/** The one value every element of `values` has, else null (an indeterminate field). */
function common<T>(values: readonly T[]): T | null {
  return values.length > 0 && values.every((v) => v === values[0]) ? values[0]! : null;
}

/** The tracks and vias of `ids` (in that order) and the form the dialog opens with. */
export function trackViaForm(board: BoardState, ids: readonly string[]): TrackViaForm {
  const tracks = (board.routing?.tracks ?? []).filter((t) => ids.includes(t.id));
  const vias = (board.routing?.vias ?? []).filter((v) => ids.includes(v.id));
  const locked = new Set(board.locked ?? []);
  const first = (t: (typeof tracks)[number]) => t.pts[0]!;
  const last = (t: (typeof tracks)[number]) => t.pts[t.pts.length - 1]!;
  const stack = vias[0]?.props?.padstack;
  const post = (v: Via, front: boolean): PostMachiningProps | undefined => v.props?.[front ? "front_post_machining" : "back_post_machining"];
  const postChoice = (p: PostMachiningProps | undefined): PostChoice => p?.mode ?? "none";
  const postSize2 = (p: PostMachiningProps | undefined): number | null => (!p ? null : p.mode === "countersink" ? p.angle_ddeg / 10 : p.depth);
  return {
    tracks: tracks.map((t) => t.id),
    vias: vias.map((v) => v.id),
    net: common([...tracks.map((t) => t.net), ...vias.map((v) => v.net)]),
    locked: common([...tracks.map((t) => locked.has(t.id)), ...vias.map((v) => locked.has(v.id))]),
    startX: common(tracks.map((t) => first(t)[0])),
    startY: common(tracks.map((t) => first(t)[1])),
    endX: common(tracks.map((t) => last(t)[0])),
    endY: common(tracks.map((t) => last(t)[1])),
    width: common(tracks.map((t) => t.width)),
    layer: common(tracks.map((t) => t.layer)),
    viaX: common(vias.map((v) => v.x)),
    viaY: common(vias.map((v) => v.y)),
    diameter: common(vias.map((v) => v.d)),
    drill: common(vias.map((v) => v.drill)),
    viaType: common(vias.map((v) => (v.type ?? "through") as ViaType)),
    viaFrom: common(vias.map((v) => v.from)),
    viaTo: common(vias.map((v) => v.to)),
    rings: common(vias.map((v) => v.unconnected ?? "keep_all")),
    padstackMode: stack?.mode ?? "normal",
    layerSizes: { ...(stack?.sizes ?? {}) },
    editLayer: "F.Cu",
    protection: common(vias.map((v) => viaConfiguration(v.props))),
    backdrill: common(vias.map((v) => backdrillChoiceOf(v.props))),
    backdrillFrontSize: common(vias.map((v) => v.props?.backdrill?.size ?? 0)) || null,
    backdrillFrontLayer: common(vias.map((v) => v.props?.backdrill?.end ?? "")),
    backdrillBackSize: common(vias.map((v) => v.props?.tertiary_drill?.size ?? 0)) || null,
    backdrillBackLayer: common(vias.map((v) => v.props?.tertiary_drill?.end ?? "")),
    topPost: common(vias.map((v) => postChoice(post(v, true)))),
    topPostSize: common(vias.map((v) => post(v, true)?.size ?? 0)) || null,
    topPostSize2: common(vias.map((v) => postSize2(post(v, true)))),
    bottomPost: common(vias.map((v) => postChoice(post(v, false)))),
    bottomPostSize: common(vias.map((v) => post(v, false)?.size ?? 0)) || null,
    bottomPostSize2: common(vias.map((v) => postSize2(post(v, false)))),
    teardrops: teardropForm(vias.map((v) => viaTeardrops(board, v))),
  };
}

/**
 * The via's `ViaProps` once the dialog's padstack, backdrill, post-machining and protection rows are applied, as `TransferDataFromWindow` applies them to each via:
 * a row that is indeterminate (null) leaves the via's own value; the padstack is the dialog's one stack (`via->SetPadstack( *m_viaStack )`) only when it was changed.
 */
export function viaPropsAfter(via: Via, before: TrackViaForm, form: TrackViaForm): ViaProps {
  const p: ViaProps = JSON.parse(JSON.stringify(via.props ?? {}));
  const changed = <K extends keyof TrackViaForm>(k: K) => form[k] != null && JSON.stringify(form[k]) !== JSON.stringify(before[k]);
  // Padstack mode and the other layers' diameters
  if (changed("padstackMode") || changed("layerSizes")) {
    if (form.padstackMode === "normal") delete p.padstack;
    else {
      const keys = form.padstackMode === "front_inner_back" ? ["Inner", "B.Cu"] : Object.keys(form.layerSizes).filter((l) => l !== "F.Cu" && l !== "Inner");
      const stack: ViaPadstack = { mode: form.padstackMode, sizes: Object.fromEntries(keys.filter((k) => form.layerSizes[k] != null).map((k) => [k, form.layerSizes[k]!])) };
      p.padstack = stack;
    }
  }
  // Backdrill: the dialog's mode makes both drills anew (a size not typed keeps the via's, else 1.1 times its hole, as `PADSTACK::SetBackdrillMode` starts one)
  const drillSize = (typed: Nm | null, now: Nm | undefined) => typed ?? now ?? Math.round(via.drill * 1.1);
  if (form.backdrill != null && (changed("backdrill") || changed("backdrillFrontSize") || changed("backdrillFrontLayer") || changed("backdrillBackSize") || changed("backdrillBackLayer"))) {
    const top = form.backdrill === "top" || form.backdrill === "both";
    const bottom = form.backdrill === "bottom" || form.backdrill === "both";
    if (top) p.backdrill = { size: drillSize(changed("backdrillFrontSize") ? form.backdrillFrontSize : null, via.props?.backdrill?.size), start: "F.Cu", end: (changed("backdrillFrontLayer") ? form.backdrillFrontLayer : via.props?.backdrill?.end) || "F.Cu" };
    else delete p.backdrill;
    if (bottom) p.tertiary_drill = { size: drillSize(changed("backdrillBackSize") ? form.backdrillBackSize : null, via.props?.tertiary_drill?.size), start: "B.Cu", end: (changed("backdrillBackLayer") ? form.backdrillBackLayer : via.props?.tertiary_drill?.end) || "B.Cu" };
    else delete p.tertiary_drill;
  } else if (form.backdrill == null) {
    // the mode left mixed: a size typed resizes the drills the via has
    if (changed("backdrillFrontSize") && p.backdrill) p.backdrill = { ...p.backdrill, size: form.backdrillFrontSize! };
    if (changed("backdrillBackSize") && p.tertiary_drill) p.tertiary_drill = { ...p.tertiary_drill, size: form.backdrillBackSize! };
  }
  // Post-machining of each side: the mode, the size, and the angle (countersink, tenths of a degree) or the depth (counterbore)
  for (const front of [true, false]) {
    const [mk, sk, s2k] = front ? (["topPost", "topPostSize", "topPostSize2"] as const) : (["bottomPost", "bottomPostSize", "bottomPostSize2"] as const);
    const key = front ? "front_post_machining" : "back_post_machining";
    const mode = form[mk];
    if (mode == null || !(changed(mk) || changed(sk) || changed(s2k))) continue;
    if (mode === "none") {
      delete p[key];
      continue;
    }
    const now = via.props?.[key];
    const size = changed(sk) && form[sk] != null ? form[sk]! : (now?.size ?? 0);
    const two = changed(s2k) ? form[s2k] : null;
    p[key] = {
      mode,
      size,
      angle_ddeg: mode === "countersink" && two != null ? Math.round(two * 10) : (now?.angle_ddeg ?? 0),
      depth: mode === "counterbore" && two != null ? Math.round(two) : (now?.depth ?? 0),
    };
  }
  // `setViaConfiguration( via, m_protectionFeatures->GetSelection() )`: a preset (not "custom", not indeterminate) is applied last
  if (form.protection != null && form.protection !== "custom") return withPreset(p, form.protection);
  return p;
}

/** `GEOMETRY_MIN_SIZE`: `pcbIUScale.mmToIU( 0.001 )`. */
const GEOMETRY_MIN_SIZE: Nm = 1_000;

/**
 * `TransferDataFromWindow`: the edits of `form` against `before` (what the dialog opened with) as one batch, or the message that keeps the dialog open. Only a field
 * that is determinate and changed is sent.
 */
export function trackViaCmds(board: BoardState, before: TrackViaForm, form: TrackViaForm, fmtMin: (nm: Nm) => string): { cmds: Cmd[] } | { error: string } {
  if (form.vias.length > 0) {
    const why = validateViaParameters(form.diameter, form.drill);
    if (why) return { error: why };
  }
  // `m_trackWidth.Validate( GEOMETRY_MIN_SIZE, INT_MAX )`: `UNIT_BINDER::Validate`'s "%s must be at least %s." with the label less its colon
  if (form.tracks.length > 0 && form.width != null && form.width < GEOMETRY_MIN_SIZE) return { error: `Track width must be at least ${fmtMin(GEOMETRY_MIN_SIZE)}.` };
  const cmds: Cmd[] = [];
  const all = [...form.tracks, ...form.vias];
  const changed = <K extends keyof TrackViaForm>(k: K) => form[k] != null && form[k] !== before[k];
  if (form.locked != null && form.locked !== before.locked) cmds.push({ op: "set_locked", ids: all, locked: form.locked });
  if (form.tracks.length > 0) {
    const width = changed("width") ? form.width : null;
    const layer = changed("layer") ? form.layer : null;
    if (width != null || layer != null) cmds.push({ op: "edit_tracks_and_vias", ids: form.tracks, track_width: width != null ? { kind: "value", nm: width } : null, via_size: null, layer });
    if (changed("startX") || changed("startY") || changed("endX") || changed("endY")) {
      for (const id of form.tracks) {
        const t = board.routing!.tracks.find((q) => q.id === id)!;
        const [sx, sy] = t.pts[0]!;
        const [ex, ey] = t.pts[t.pts.length - 1]!;
        const start = changed("startX") || changed("startY") ? { x: form.startX ?? sx, y: form.startY ?? sy } : undefined;
        const end = changed("endX") || changed("endY") ? { x: form.endX ?? ex, y: form.endY ?? ey } : undefined;
        cmds.push({ op: "edit_track", id, ...(start ? { start } : {}), ...(end ? { end } : {}) });
      }
    }
  }
  if (form.vias.length > 0) {
    // `ValidateViaParameters`' layer check, before anything is sent
    if (form.viaType !== "through" && form.viaFrom != null && form.viaFrom === form.viaTo) return { error: "Via start layer and end layer cannot be the same" };
    const viaRow: Extract<Cmd, { op: "set_via_layers" }> = { op: "set_via_layers", ids: form.vias };
    if (changed("viaType")) viaRow.kind = form.viaType!;
    if (form.viaType !== "through") {
      if (changed("viaFrom")) viaRow.from_layer = form.viaFrom!;
      if (changed("viaTo")) viaRow.to_layer = form.viaTo!;
    }
    if (changed("rings")) viaRow.unconnected = form.rings!;
    if (Object.keys(viaRow).length > 2) cmds.push(viaRow);
  }
  for (const id of form.vias) {
    const v = board.routing!.vias.find((q) => q.id === id)!;
    if (changed("viaX") || changed("viaY")) cmds.push({ op: "move_via", id, x: form.viaX ?? v.x, y: form.viaY ?? v.y });
    if (changed("diameter") || changed("drill")) cmds.push({ op: "edit_via", id, diameter: form.diameter ?? v.d, drill: form.drill ?? v.drill });
    // the padstack, protection, backdrill and post-machining: the via's whole props, when they change
    const props = viaPropsAfter(v, before, form);
    if (JSON.stringify(sorted(props)) !== JSON.stringify(sorted(v.props ?? {}))) cmds.push({ op: "set_via_props", id, props });
  }
  // the teardrops: each via's parameters in effect with the changed fields, the vias that come to the same parameters in one command
  if (form.vias.length > 0 && teardropFormChanged(before.teardrops, form.teardrops)) {
    const groups = new Map<string, { params: TeardropParams; ids: string[] }>();
    for (const id of form.vias) {
      const v = board.routing!.vias.find((q) => q.id === id)!;
      const params = applyTeardropForm(viaTeardrops(board, v), before.teardrops, form.teardrops);
      const key = JSON.stringify(params);
      const g = groups.get(key) ?? { params, ids: [] };
      g.ids.push(id);
      groups.set(key, g);
    }
    for (const g of groups.values()) cmds.push({ op: "set_teardrop_params", ids: g.ids, params: g.params });
  }
  // The net is set last, by `track_via_nets` (crates/ops/src/pad_nets.rs), which first asks about the shorts the edited copper makes and the pads a net change
  // takes along (`confirmShortingNets`, `confirmPadChange`): KiCad's TransferDataFromWindow does both after every other edit, whatever changed, and sets the net
  // only when the box is not indeterminate.
  if (cmds.length > 0 || changed("net")) cmds.push({ op: "track_via_nets", ids: all, net: form.net ?? null });
  return { cmds };
}

/** An object with its keys in order, to compare two `ViaProps` whatever order their keys were set in. */
function sorted(v: unknown): unknown {
  if (Array.isArray(v)) return v.map(sorted);
  if (v && typeof v === "object") return Object.fromEntries(Object.keys(v as object).sort().map((k) => [k, sorted((v as Record<string, unknown>)[k])]));
  return v;
}

/** The answers to the questions `track_via_nets` asks, put on the batch's last command: "Apply Anyway" and "Change Nets" / "Leave Nets Unchanged". */
export function withNetAnswers(cmds: readonly Cmd[], answers: { apply_anyway?: boolean; change_pads?: boolean }): Cmd[] {
  return cmds.map((c) => (c.op === "track_via_nets" ? { ...c, ...answers } : c));
}
