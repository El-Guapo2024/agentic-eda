// KiCad 10's microvia stack (`PCB_VIA_STACK`, pcbnew/generators/pcb_via_stack.cpp of KiCad 25ec2335; UI-LEFT-3 X3-1): the parts the studio works out itself --
// Place Microvia Stack's settings, cursor and preview (`DRAWING_TOOL::PlaceMicroviaStack`), the net a stack takes (`FindNetAtPosition`), the toolbar's preset
// selector (`UpdateViaStackSelectBox`, `SelectViaStack_Event`) and the read-only rows of the Properties panel (`PCB_VIA_STACK_DESC`). The board edit is the
// backend's (crates/ops/src/via_stack.rs: `set_via_stack`, `delete_via_stack`, `make_via_stack`), where the stack's vias and traces are made (`Regenerate`).
//
//   placement     the preset chosen in the toolbar (`presets[clamp( idx )]`), or the Microvia Stack dialog on a new stack when the board has none; the span checked
//                 ("The microvia stack layers are not present on this board."); a staggered stack with no pitch takes twice the via (`dia * 2`)
//   stacked       one click places the whole stack where the snapped cursor is (`snapToCopper`: a track's line, a pad's centre, else the grid)
//   staggered     a click per hop: the first is snapped like a stacked one, every later hop is "locked onto a circle of radius = pitch around the previous hop", at the
//                 line mode's 45 or 90 degrees (Ctrl: any angle); the preview shows the hops placed and the one being steered; the last hop places the stack
//   Undo / Delete take back the last hop ("Swallowed for the whole loop like the router does"); Esc leaves.
import type { BoardState, Cmd, NetClass } from "../api/types";
import type { BoardViaStack } from "../api/pcbItemTypes";
import type { ViaStackPreset } from "../api/types";
import { classesOfNet } from "./boardSetupRules";
import { findPad, findTrack, shownBy, stitchedZoneNets, type ViaPlacerInput } from "./viaPlacer";
import { sceneOf } from "./snapScene";
import type { Pt } from "./snapGeom";

/** `PCB_VIA_STACK::DISPLAY_NAME`. */
export const VIA_STACK_NAME = "Microvia Stack";
/** `PlaceMicroviaStack`'s info bar message for a span the board does not have. */
export const STACK_LAYERS_MISSING = "The microvia stack layers are not present on this board.";
/** `MakeViaStackFromSelection`'s fallback message. */
export const MAKE_STACK_REFUSED = "A microvia stack needs two or more microvias on the same net, each spanning one layer step, with no gap between them.";

/** The board's copper layers, front to back. */
export function copperOf(board: Pick<BoardState, "layers"> | null | undefined): string[] {
  return (board?.layers ?? ["F.Cu", "B.Cu"]).filter((l) => l.endsWith(".Cu"));
}

/** `LAYER_RANGE( start, end )`: the copper layers from `start` to `end` in physical order (either way); empty when either is not on the board. */
export function layerRange(copper: readonly string[], start: string, end: string): string[] {
  const a = copper.indexOf(start);
  const b = copper.indexOf(end);
  if (a < 0 || b < 0) return [];
  return a <= b ? copper.slice(a, b + 1) : copper.slice(b, a + 1).reverse();
}

/** `IsSpanValid`: both ends copper layers of the board, and different. */
export function isSpanValid(copper: readonly string[], start: string, end: string): boolean {
  return copper.includes(start) && copper.includes(end) && start !== end;
}

/** `PCB_VIA_STACK( board, F_Cu )`: F.Cu to In1.Cu, stacked, filled, no sizes. */
export function newViaStack(at: PointLike): BoardViaStack {
  return { id: "", name: VIA_STACK_NAME, start_layer: "F.Cu", end_layer: "In1.Cu", style: "stacked", pitch_nm: 0, filled: true, capped: false, via_size_nm: 0, via_drill_nm: 0, use_netclass: false, preset: "", origin: { x: at.x, y: at.y } };
}

type PointLike = { x: number; y: number };

/** `ApplyPreset`. */
export function withPreset(s: BoardViaStack, p: ViaStackPreset): BoardViaStack {
  return { ...s, start_layer: p.start_layer, end_layer: p.end_layer, style: p.staggered ? "staggered" : "stacked", via_size_nm: p.via_size_nm, via_drill_nm: p.via_drill_nm, use_netclass: p.use_netclass, filled: p.filled, capped: p.capped, pitch_nm: p.pitch_nm, preset: p.name };
}

/** `ToPreset`. */
export function presetOf(s: BoardViaStack): ViaStackPreset {
  return { name: s.preset ?? "", start_layer: s.start_layer, end_layer: s.end_layer, staggered: s.style === "staggered", via_size_nm: s.via_size_nm ?? 0, via_drill_nm: s.via_drill_nm ?? 0, use_netclass: s.use_netclass ?? false, filled: s.filled ?? true, capped: s.capped ?? false, pitch_nm: s.pitch_nm ?? 0 };
}

/** `GetNetclassViaSize( board, net )`: the net's classes' microvia diameter and drill (the first that sets each, then the Default class's), else the current via size. */
export function netclassViaSize(board: Pick<BoardState, "board_rules"> | null | undefined, net: string, current: { diameter: number; drill: number }): { size: number; drill: number } {
  const rules = board?.board_rules;
  const classes: NetClass[] = [...classesOfNet(rules?.net_classes ?? [], net), ...(rules?.default_class ? [rules.default_class] : [])];
  const first = (f: (c: NetClass) => number | undefined) => classes.map(f).find((v) => v !== undefined);
  return { size: first((c) => c.microvia_diameter) ?? current.diameter, drill: first((c) => c.microvia_drill) ?? current.drill };
}

/** One member `BuildMembers` makes, as the preview draws it. */
export type PreviewMember = { kind: "via"; at: Pt; size: number; drill: number; top: string; bottom: string } | { kind: "trace"; a: Pt; b: Pt; layer: string; width: number };

/** `hopPos`: where hop `i` is. */
export function hopPos(s: BoardViaStack, i: number): Pt {
  if (s.style !== "staggered") return [s.origin.x, s.origin.y];
  const h = s.hops ?? [];
  if (i < h.length) return [h[i]!.x, h[i]!.y];
  if (h.length > 1) {
    const last = h.length - 1;
    const dx = h[last]!.x - h[last - 1]!.x;
    const dy = h[last]!.y - h[last - 1]!.y;
    const len = Math.hypot(dx, dy);
    if (len > 0) {
      const k = (s.pitch_nm ?? 0) / len;
      const step: Pt = [Math.round(dx * k), Math.round(dy * k)];
      if (step[0] !== 0 || step[1] !== 0) return [h[last]!.x + step[0] * (i - last), h[last]!.y + step[1] * (i - last)];
    }
  }
  return [s.origin.x + (s.pitch_nm ?? 0) * i, s.origin.y];
}

/**
 * `BuildMembers( board, net )` as the placement preview draws it (`settings.BuildMembers( board, 0 )`): a microvia per adjacent pair of the span (the stack's size,
 * else the current via size; the net class's with "Use netclass values"), and a staggered stack's traces of `traceWidth` on the landing layers. The rules' resizing
 * (`SetSizeFromRules`, the track width rule) is the backend's.
 */
export function buildMembers(s: BoardViaStack, copper: readonly string[], sizes: { current: { diameter: number; drill: number }; netclass: { size: number; drill: number }; traceWidth: number }): PreviewMember[] {
  const out: PreviewMember[] = [];
  if (!isSpanValid(copper, s.start_layer, s.end_layer)) return out;
  const layers = layerRange(copper, s.start_layer, s.end_layer);
  if (layers.length < 2) return out;
  const size = s.use_netclass ? sizes.netclass.size : (s.via_size_nm ?? 0) > 0 ? s.via_size_nm! : sizes.current.diameter;
  const drill = s.use_netclass ? sizes.netclass.drill : (s.via_drill_nm ?? 0) > 0 ? s.via_drill_nm! : sizes.current.drill;
  for (let i = 0; i + 1 < layers.length; i++) {
    const [a, b] = [layers[i]!, layers[i + 1]!];
    const [top, bottom] = copper.indexOf(a) <= copper.indexOf(b) ? [a, b] : [b, a];
    out.push({ kind: "via", at: hopPos(s, i), size, drill, top, bottom });
  }
  if (s.style === "staggered") {
    for (let i = 0; i + 2 < layers.length; i++) {
      const a = hopPos(s, i);
      const b = hopPos(s, i + 1);
      if (a[0] === b[0] && a[1] === b[1]) continue;
      out.push({ kind: "trace", a, b, layer: layers[i + 1]!, width: sizes.traceWidth });
    }
  }
  return out;
}

/**
 * The settings Place Microvia Stack starts from: "Place silently with the preset currently selected in the toolbar" (`presets[ clamp( idx ) ]`), null when the board has none
 * ("Only fall back to the properties dialog when no presets are defined yet").
 */
export function placementPreset(presets: readonly ViaStackPreset[] | undefined, index: number): ViaStackPreset | null {
  if (!presets || presets.length === 0) return null;
  return presets[Math.max(0, Math.min(index, presets.length - 1))]!;
}

/** "A zero pitch would pin the steering circle onto a single point": a staggered stack with none steps by twice its via (or the current via's). */
export function placementPitch(s: BoardViaStack, currentViaSize: number): number {
  if (s.style !== "staggered" || (s.pitch_nm ?? 0) > 0) return s.pitch_nm ?? 0;
  return ((s.via_size_nm ?? 0) > 0 ? s.via_size_nm! : currentViaSize) * 2;
}

/**
 * Where a staggered stack's next hop goes: "Cursor is locked onto a circle of radius = pitch around the previous hop. It steers direction only, following the editor angle
 * mode" -- 90 or 45 degree steps unless the line mode is free or Ctrl is held (`LEADER_MODE::DIRECT`).
 */
export function steeredHop(prev: Pt, raw: Pt, pitch: number, mode: "direct" | "deg45" | "deg90", ctrl: boolean): Pt {
  let dx = raw[0] - prev[0];
  let dy = raw[1] - prev[1];
  if (Math.hypot(dx, dy) < 1) [dx, dy] = [1, 0];
  const snapMode = ctrl ? "direct" : mode;
  if (snapMode !== "direct") {
    const step = snapMode === "deg90" ? Math.PI / 2 : Math.PI / 4;
    const ang = Math.round(Math.atan2(dy, dx) / step) * step;
    return [prev[0] + Math.round(Math.cos(ang) * pitch), prev[1] + Math.round(Math.sin(ang) * pitch)];
  }
  const len = Math.hypot(dx, dy);
  return [prev[0] + Math.round((dx / len) * pitch), prev[1] + Math.round((dy / len) * pitch)];
}

/**
 * `FindNetAtPosition( board, pos, span )`: on each layer of the span in stack order, the net of the pad under `pos` (a mechanical hole skipped), else the track's, else the
 * zone fill's; "" when nothing is hit.
 */
export function netAtPosition(input: Pick<ViaPlacerInput, "board" | "fills">, pos: Pt, layers: readonly string[]): string {
  const scene = sceneOf(input.board);
  const all = shownBy(() => true);
  for (const layer of layers) {
    const pad = findPad(scene, pos, 0, [layer], all);
    if (pad) {
      const part = input.board.parts.find((p) => p.ref === pad.footprint);
      const num = pad.id.slice(pad.footprint.length + 1).split("#")[0];
      const net = part?.pads?.find((p) => p.num === num)?.net ?? "";
      if (net) return net;
      continue;
    }
    const track = findTrack(scene, pos, 0, [layer], all);
    if (track) {
      const net = input.board.routing?.tracks.find((t) => t.id === track.owner)?.net ?? "";
      if (net) return net;
      continue;
    }
    const nets = stitchedZoneNets({ ...(input as ViaPlacerInput), layers: [layer], highContrast: false, activeLayer: layer, isLayerVisible: () => true }, pos).filter((n) => n !== "");
    if (nets.length > 0) return nets[0]!;
  }
  return "";
}

/** The `set_via_stack` verb (`net` and `member_ids` are the server's to keep; a new stack's net is what it lands on). */
export function viaStackCmd(id: string | null, s: BoardViaStack, extra: { current?: { via_size_nm: number; via_drill_nm: number }; allow_drc_violations?: boolean; hidden_layers?: string[] } = {}): Cmd {
  const { id: _drop, member_ids: _members, ...stack } = s;
  void _drop;
  void _members;
  return { op: "pcb_item", verb: "set_via_stack", ...(id ? { id } : {}), stack, ...extra } as Cmd;
}

/** `UpdateViaStackSelectBox`: "Microvia stack: <name>" per preset (or "Microvia stack: none defined"), "---", "Edit Pre-defined Microvia Stacks...". */
export function viaStackBoxLabels(presets: readonly ViaStackPreset[] | undefined): string[] {
  const names = presets && presets.length > 0 ? presets.map((p) => `Microvia stack: ${p.name}`) : ["Microvia stack: none defined"];
  return [...names, "---", "Edit Pre-defined Microvia Stacks..."];
}

/** `SelectViaStack_Event`: the separator keeps the index, the last entry opens Board Setup's Microvia Stacks page, a preset is chosen. */
export function viaStackBoxPick(selected: number, count: number, presets: number): { kind: "none" } | { kind: "edit" } | { kind: "index"; index: number } {
  if (selected === count - 2) return { kind: "none" };
  if (selected === count - 1) return { kind: "edit" };
  if (selected >= 0 && selected < presets) return { kind: "index", index: selected };
  return { kind: "none" };
}

/** The stack of the board `id` names. */
export function viaStackById(board: BoardState | null | undefined, id: string): BoardViaStack | undefined {
  return board?.drawings?.via_stacks?.find((s) => s.id === id);
}

/** The ids `make_via_stack` takes of a selection: its vias and tracks (`MakeViaStackFromSelection`'s filter: `PCB_VIA_T` and `PCB_TRACE_T`). */
export function makeStackIds(board: BoardState, selection: Iterable<string>): string[] {
  const vias = new Set((board.routing?.vias ?? []).map((v) => v.id));
  const tracks = new Set((board.routing?.tracks ?? []).filter((t) => !t.arc_mid).map((t) => t.id));
  return [...selection].filter((id) => vias.has(id) || tracks.has(id));
}
