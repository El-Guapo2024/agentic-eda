// Place Microvia Stack on the canvas (KiCad 25ec2335 `DRAWING_TOOL::PlaceMicroviaStack`, UI-LEFT-3 X3-1): the loop's motion, click and undo over the placement held in
// state/viaStackTool.ts, and the preview's drawing. The rules of the tool are kicad-port/viaStack.ts's; Canvas.tsx calls in here.
import type { BoardState } from "../../api/types";
import type { BoardViaStack } from "../../api/pcbItemTypes";
import { buildMembers, layerRange, netAtPosition, steeredHop, type PreviewMember } from "../../kicad-port/viaStack";
import { snapViaPosition, type ViaPlacerInput } from "../../kicad-port/viaPlacer";
import type { Pt } from "../../kicad-port/snapGeom";
import { getViaStackTool, setViaStackPlacement, type ViaStackPlacement } from "../../state/viaStackTool";
import { netclassViaSize } from "../../kicad-port/viaStack";

/** What the loop reads besides the placement: the board, the via placer's view of it, the current via size, the line mode and Ctrl. */
export interface StackToolInput {
  board: BoardState;
  /** `VIA_PLACER`'s view (the canvas's `viaPlacerFor`): the snapping and the zone fills. */
  placer: ViaPlacerInput;
  current: { diameter: number; drill: number };
  angleMode: "direct" | "deg45" | "deg90";
  ctrl: boolean;
}

function sizes(inp: StackToolInput, s: BoardViaStack) {
  const net = s.net ?? "";
  const rules = inp.board.board_rules;
  // `nc->HasTrackWidth() ? nc->GetTrackWidth() : bds.GetCurrentTrackWidth()`: the preview's trace (the backend sizes it by the rules)
  const width = rules?.default_class?.track_width ?? rules?.track_width ?? 200_000;
  return { current: inp.current, netclass: netclassViaSize(inp.board, net, inp.current), traceWidth: width };
}

/**
 * Where the cursor is for the stack: the first hop (or a stacked stack) snapped onto copper of the span (`snapToCopper`: `FindSnapTrack` / `FindSnapPad` with the probe
 * of the stack's via, else the grid point `grid`), a later hop of a staggered stack steered round the previous one (`steeredHop`).
 */
export function stackCursor(p: ViaStackPlacement, inp: StackToolInput, raw: Pt, grid: Pt): Pt {
  const s = p.settings;
  if (s.style === "staggered" && p.hops.length > 0) return steeredHop(p.hops[p.hops.length - 1]!, raw, s.pitch_nm ?? 0, inp.angleMode, inp.ctrl);
  const dia = (s.via_size_nm ?? 0) > 0 ? s.via_size_nm! : inp.current.diameter;
  // "A stack may land on any layer it spans, so the probe carries the whole span rather than a single layer." -- tracks and pads, not graphics
  const probe: ViaPlacerInput = { ...inp.placer, diameter: dia, layers: p.span, magnetic: { ...inp.placer.magnetic, graphics: false } };
  return snapViaPosition(probe, raw, grid);
}

/** `updatePreview( cursor )`: the hops placed and the one being steered (a staggered stack's span cut to them), or the whole stack at the cursor. */
export function previewAt(p: ViaStackPlacement, inp: StackToolInput, cursor: Pt): ViaStackPlacement {
  const s = p.settings;
  let shown: BoardViaStack;
  if (s.style === "staggered") {
    const hops = [...p.hops, cursor].map(([x, y]) => ({ x, y }));
    const n = Math.min(p.hops.length + 1, p.span.length - 1);
    shown = { ...s, hops, origin: p.hops.length > 0 ? { x: p.hops[0]![0], y: p.hops[0]![1] } : { x: cursor[0], y: cursor[1] }, end_layer: p.span[n]! };
  } else shown = { ...s, origin: { x: cursor[0], y: cursor[1] } };
  return { ...p, preview: buildMembers(shown, inp.board.layers.filter((l) => l.endsWith(".Cu")), sizes(inp, s)) };
}

/** The stack a click places: the full span, a staggered stack at its hops, a stacked one at the cursor; on the net its first hop lands on (`FindNetAtPosition`). */
export function stackToPlace(p: ViaStackPlacement, inp: StackToolInput, cursor: Pt): BoardViaStack {
  const s = p.settings;
  const end = p.span[p.span.length - 1]!;
  const placed: BoardViaStack =
    s.style === "staggered" ? { ...s, end_layer: end, hops: p.hops.map(([x, y]) => ({ x, y })), origin: { x: p.hops[0]![0], y: p.hops[0]![1] } } : { ...s, end_layer: end, hops: undefined, origin: { x: cursor[0], y: cursor[1] } };
  return { ...placed, net: netAtPosition(inp.placer, [placed.origin.x, placed.origin.y], p.span) };
}

/**
 * A click of the loop: a staggered stack takes the hop (and places itself at the last, `place` returning whether the board took it -- "Blocked by DRC, let the user
 * reposition the final hop and retry"), a stacked one places itself. Resolves when done.
 */
export async function stackClick(inp: StackToolInput, cursor: Pt, place: (s: BoardViaStack) => Promise<boolean>): Promise<void> {
  const p = getViaStackTool().placement;
  if (!p) return;
  const nHops = p.span.length - 1;
  if (p.settings.style === "staggered") {
    const hops = [...p.hops, cursor];
    if (hops.length >= nHops) {
      const ok = await place(stackToPlace({ ...p, hops }, inp, cursor));
      const now = getViaStackTool().placement;
      if (!now) return;
      setViaStackPlacement(ok ? { ...now, hops: [], preview: [] } : { ...now, hops: hops.slice(0, -1) });
      return;
    }
    // "Show the next hop right away, continuing in the last direction, instead of waiting for the mouse to move."
    const prev = p.hops.length > 0 ? p.hops[p.hops.length - 1]! : null;
    const lastDir: Pt = prev ? [cursor[0] - prev[0], cursor[1] - prev[1]] : p.lastDir;
    const len = Math.hypot(lastDir[0], lastDir[1]) || 1;
    const pitch = p.settings.pitch_nm ?? 0;
    const next: Pt = [cursor[0] + Math.round((lastDir[0] / len) * pitch), cursor[1] + Math.round((lastDir[1] / len) * pitch)];
    setViaStackPlacement(previewAt({ ...p, hops, lastDir }, inp, next));
    return;
  }
  if (await place(stackToPlace(p, inp, cursor))) {
    const now = getViaStackTool().placement;
    if (now) setViaStackPlacement({ ...now, preview: [] });
  }
}

/** Undo / Delete in the loop: a staggered stack's last hop taken back (`lastDir` reset to +X); swallowed otherwise. */
export function stackUndo(): void {
  const p = getViaStackTool().placement;
  if (!p || p.settings.style !== "staggered" || p.hops.length === 0) return;
  setViaStackPlacement({ ...p, hops: p.hops.slice(0, -1), lastDir: [1, 0] });
}

/** A new placement of `settings` (the span as the board has it). */
export function newPlacement(settings: BoardViaStack, copper: readonly string[]): ViaStackPlacement {
  return { settings, span: layerRange(copper, settings.start_layer, settings.end_layer), hops: [], lastDir: [1, 0], preview: [] };
}

/** The preview: each via a disc of the via colour, each trace a line of its width, half transparent (`m_view->Add( &preview )`). */
export function paintViaStackPreview(ctx: CanvasRenderingContext2D, preview: readonly PreviewMember[], viaColor: string, traceColor: string): void {
  ctx.save();
  ctx.globalAlpha = 0.6;
  for (const m of preview) {
    if (m.kind !== "trace") continue;
    ctx.strokeStyle = traceColor;
    ctx.lineWidth = m.width;
    ctx.lineCap = "round";
    ctx.beginPath();
    ctx.moveTo(m.a[0], m.a[1]);
    ctx.lineTo(m.b[0], m.b[1]);
    ctx.stroke();
  }
  for (const m of preview) {
    if (m.kind !== "via") continue;
    ctx.fillStyle = viaColor;
    ctx.beginPath();
    ctx.arc(m.at[0], m.at[1], m.size / 2, 0, Math.PI * 2);
    ctx.fill();
    ctx.globalAlpha = 0.9;
    ctx.fillStyle = "#000";
    ctx.beginPath();
    ctx.arc(m.at[0], m.at[1], m.drill / 2, 0, Math.PI * 2);
    ctx.fill();
    ctx.globalAlpha = 0.6;
  }
  ctx.restore();
}
