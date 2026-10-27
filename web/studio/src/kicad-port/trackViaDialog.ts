// `DIALOG_TRACK_VIA_PROPERTIES` (pcbnew/dialogs/dialog_track_via_properties.cpp): the Track & Via Properties dialog, opened by `EDIT_TOOL::Properties` on the whole
// selection when it holds only tracks, arcs and vias (`SELECTION_CONDITIONS::OnlyTypes( { PCB_TRACE_T, PCB_ARC_T, PCB_VIA_T } )`). Every field shows the value the
// items share, or is indeterminate (null here) when they differ; an indeterminate field is left alone on OK (`if( !m_trackWidth.IsIndeterminate() ) ..`).
//
//   Common:  Net (`m_netSelector`), Locked (3-state `m_lockedCbox`)
//   Tracks:  Start X/Y, End X/Y, Width (`m_trackWidth.Validate( GEOMETRY_MIN_SIZE, INT_MAX )`), Layer
//   Vias:    X/Y, Diameter, Hole (`PCB_VIA::ValidateViaParameters`), Via type, Start layer, End layer (`m_ViaTypeChoice`, `m_ViaStartLayer`, `m_ViaEndLayer`:
//            a through via's pair is F.Cu..B.Cu and the two are disabled, `onViaEdit`), Annular rings (`m_annularRingsCtrl`, shown when `getLayerDepth() > 1`)
//
// Not here: the protection features (IPC-4761 tenting, covering, plugging, capping, filling), backdrill and post-machining rows: all but tenting exist only in
// KiCad 10's board format (covering and the rest from 20250228, backdrill and post-machining later), which the writer does not write for a board that never had
// them; and the connected tracks KiCad offers to resize with a via.
import type { BoardState, Cmd, Nm, UnconnectedLayers } from "../api/types";
import { validateViaParameters } from "./routerVia";

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
  };
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
  }
  // The net is set last, by `track_via_nets` (crates/ops/src/pad_nets.rs), which first asks about the shorts the edited copper makes and the pads a net change
  // takes along (`confirmShortingNets`, `confirmPadChange`): KiCad's TransferDataFromWindow does both after every other edit, whatever changed, and sets the net
  // only when the box is not indeterminate.
  if (cmds.length > 0 || changed("net")) cmds.push({ op: "track_via_nets", ids: all, net: form.net ?? null });
  return { cmds };
}

/** The answers to the questions `track_via_nets` asks, put on the batch's last command: "Apply Anyway" and "Change Nets" / "Leave Nets Unchanged". */
export function withNetAnswers(cmds: readonly Cmd[], answers: { apply_anyway?: boolean; change_pads?: boolean }): Cmd[] {
  return cmds.map((c) => (c.op === "track_via_nets" ? { ...c, ...answers } : c));
}
