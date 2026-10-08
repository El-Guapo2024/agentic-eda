// Move, Drag, Rotate and Mirror for every kind of schematic item: the commands the studio sends and how a preview is laid over the sheet.
//
// The work is done by one backend verb family (`{ op: "sch_move", verb: ... }`, crates/ops/src/sch_move.rs, ported from `SCH_MOVE_TOOL` and
// `SCH_EDIT_TOOL::Rotate` / `Mirror`): a symbol, wire, label, power symbol, text, text box, junction, no-connect, bus entry or sheet moves,
// drags (wires stretch, bends are added where KiCad adds them), turns and mirrors through the same commands, each one undo step. This module is
// the studio's half: which command a held selection commits, which points of a wire a click picks (`STARTPOINT` / `ENDPOINT`), and the
// merge of the backend's preview patch over the sheet while the items are held.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd, Schematic, SchematicWire } from "../api/types";
import type { SchMovePatch, SchTurn } from "../api/schEditTypes";
import { distPointSegment } from "./schItemGeom";

export type { SchTurn };

/** A selection held by the cursor: what `M`, `G` or a click-drag is carrying, and how far. */
export interface SchHeld {
  /** `move` leaves wires where they are; `drag` stretches them. */
  mode: "move" | "drag";
  ids: readonly string[];
  /** The picked points of a wire (a click near a wire's end picks that end); a wire not named is picked whole. */
  vertices?: Readonly<Record<string, readonly number[]>>;
  dxUm: number;
  dyUm: number;
  /** Turns made while holding, in order. */
  turns?: readonly SchTurn[];
  /** The point the items are held at (where they were picked up plus the offset): turns are about it. */
  holdUm?: readonly [number, number];
}

const vertexMap = (v: SchHeld["vertices"]): Record<string, number[]> | undefined => {
  if (!v) return undefined;
  const out: Record<string, number[]> = {};
  for (const [k, list] of Object.entries(v)) out[k] = [...list];
  return Object.keys(out).length > 0 ? out : undefined;
};

/** The one command a drop commits (the move or drag with the turns made on the way), or null when the items did not go anywhere and were not turned. */
export function heldCmd(h: SchHeld, opts: { ortho?: boolean } = {}): Cmd | null {
  const turns = h.turns ?? [];
  if (h.dxUm === 0 && h.dyUm === 0 && turns.length === 0) return null;
  const about = h.holdUm ? { x: h.holdUm[0], y: h.holdUm[1] } : undefined;
  const ids = [...h.ids];
  const held = turns.length > 0 ? { turns: [...turns], about } : {};
  if (h.mode === "move") return { op: "sch_move", verb: "move", ids, dx: h.dxUm, dy: h.dyUm, ...held };
  return { op: "sch_move", verb: "drag", ids, vertices: vertexMap(h.vertices), dx: h.dxUm, dy: h.dyUm, ortho: opts.ortho ?? true, ...held };
}

/** `R`, Shift+`R`, `X`, `Y` on a selection that is not being held. */
export function turnCmd(ids: readonly string[], turn: SchTurn, vertices?: SchHeld["vertices"]): Cmd {
  const base = { op: "sch_move" as const, ids: [...ids], vertices: vertexMap(vertices) };
  switch (turn) {
    case "rot_ccw":
      return { ...base, verb: "rotate", ccw: true };
    case "rot_cw":
      return { ...base, verb: "rotate", ccw: false };
    case "mirror_h":
      return { ...base, verb: "mirror", vertical: false };
    case "mirror_v":
      return { ...base, verb: "mirror", vertical: true };
  }
}

/**
 * Which points of a wire a click at `point` picks (`SCH_SELECTION_TOOL::selectPoint`): within `thresholdUm` of the wire's own first or last end only
 * that end (`STARTPOINT` / `ENDPOINT`), so dragging it stretches the wire; anywhere else on a segment the two ends of that segment, so dragging it
 * drags that segment and its neighbours follow. Null when the point is not on the wire.
 */
export function wirePick(wire: Pick<SchematicWire, "pts">, point: readonly [number, number], thresholdUm: number): number[] | null {
  let best: { k: number; d: number } | null = null;
  for (let k = 0; k + 1 < wire.pts.length; k++) {
    const d = distPointSegment(point, wire.pts[k]!, wire.pts[k + 1]!);
    if (d <= thresholdUm && (!best || d < best.d)) best = { k, d };
  }
  if (!best) return null;
  const a = wire.pts[best.k]!;
  const b = wire.pts[best.k + 1]!;
  if (Math.hypot(point[0] - a[0], point[1] - a[1]) <= thresholdUm) return [best.k];
  if (Math.hypot(point[0] - b[0], point[1] - b[1]) <= thresholdUm) return [best.k + 1];
  return [best.k, best.k + 1];
}

/** The sheet with a preview patch laid over it: the moved items where they would end up. */
export function applyPatch(sch: Schematic, p: SchMovePatch): Schematic {
  const symbolAt = new Map(p.symbols.map((s) => [`${s.id}#${s.unit}`, s]));
  const power = new Map(p.power_symbols.map((s) => [s.id, s]));
  const wiresOld = new Map(sch.wires.map((w) => [w.id, w]));
  const labels = new Map(p.labels.map((l) => [l.id, l]));
  const texts = new Map(p.texts.map((t) => [t.id, t]));
  const lines = new Map(p.lines.map((l) => [l.id, l]));
  const sheets = new Map(p.sheets.map((s) => [s.id, s]));
  const ncOld = new Map(sch.no_connects.map((n) => [n.id, n]));
  return {
    ...sch,
    symbols: sch.symbols.map((s) => {
      const q = symbolAt.get(`${s.id}#${s.unit}`);
      return q ? { ...s, at: q.at, rot: q.rot, mirror: q.mirror } : s;
    }),
    power_symbols: sch.power_symbols.map((s) => {
      const q = power.get(s.id);
      return q ? { ...s, at: q.at, rot: q.rot } : s;
    }),
    wires: p.wires.map((w) => {
      const old = wiresOld.get(w.id);
      return old ? { ...old, pts: w.pts, net: w.net, bus: w.bus } : { id: w.id, net: w.net, pins: [], pts: w.pts, bus: w.bus };
    }),
    labels: sch.labels.map((l) => {
      const q = labels.get(l.id);
      return q ? { ...l, at: q.at, spin: q.spin } : l;
    }),
    texts: sch.texts.map((t) => {
      const q = texts.get(t.id);
      return q ? { ...t, at: q.at, angle: q.angle } : t;
    }),
    no_connects: p.no_connects.map((n) => ({ ...(ncOld.get(n.id) ?? { id: n.id }), id: n.id, at: n.at })),
    bus_entries: p.bus_entries.map((b) => ({ id: b.id, at: b.at, size: b.size })),
    junctions: p.junctions.map((j) => ({ id: j.id, at: j.at })),
    lines: (sch.lines ?? []).map((l) => {
      const q = lines.get(l.id);
      return q ? { ...l, pts: q.pts } : l;
    }),
    graphics: p.graphics ?? sch.graphics,
    sheets: sch.sheets.map((s) => {
      const q = sheets.get(s.id);
      if (!q) return s;
      const pins = new Map(q.pins.map((x) => [x.id, x.at]));
      return { ...s, at: q.at, size: q.size, pins: s.pins.map((pin) => ({ ...pin, at: pins.get(pin.id) ?? pin.at })) };
    }),
  };
}

/** The request body of `POST /api/sch/move_preview`: the commands a drop would send, for the sheet in view. */
export function previewBody(cmd: Cmd, sheetPath: readonly string[]): { sheet: string; cmds: Cmd[] } {
  return { sheet: sheetPath.join("/"), cmds: [cmd] };
}

/**
 * The point a held selection is at: where it was picked up (snapped to the grid, as `GetCursorPosition` is) plus how far it has gone -- the point
 * R, Shift+R, X and Y turn it about (`selection.GetReferencePoint()`). Undefined when it is not known where it was picked up.
 */
export function holdPoint(origin: { x: number; y: number } | null, dxUm: number, dyUm: number, grid = 1270): [number, number] | undefined {
  if (!origin) return undefined;
  const snap = (v: number) => Math.round(v / grid) * grid;
  return [snap(origin.x) + dxUm, snap(origin.y) + dyUm];
}

/** The points of a wire the last click picked, when the selection is still exactly that wire (what `G` and a drag start from). */
export function pickedVertices(state: { selection: ReadonlySet<string>; schWirePick: { id: string; vertices: number[] } | null }): Record<string, number[]> | undefined {
  const pick = state.schWirePick;
  if (!pick || state.selection.size !== 1 || !state.selection.has(pick.id)) return undefined;
  return { [pick.id]: pick.vertices };
}
