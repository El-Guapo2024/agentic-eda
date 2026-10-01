// Converts this app's *display* shapes (BoardState's Track/Via/Zone/
// Shape/BoardText, `[x, y]` tuple points) into the Cmd-ready IR shapes
// `paste_items` actually wants (CmdTrack/CmdVia/CmdZone/CmdShape/CmdText,
// `{x, y}` PointXY points) -- api/types.ts's own header comment on why
// these are two different shapes for the same data. Used by Cmd+C
// (state/store.tsx's `copySelection`, which snapshots this converted
// form as the clipboard) and Cmd+V (`pasteClipboard`, which sends it
// straight back as-is).
import type { BoardState, BoardText, CmdShape, CmdText, CmdTrack, CmdVia, CmdZone, Shape, Track, Via, Zone } from "../../api/types";

export interface ClipboardContents {
  tracks: CmdTrack[];
  vias: CmdVia[];
  zones: CmdZone[];
  shapes: CmdShape[];
  texts: CmdText[];
}

const pt = ([x, y]: readonly [number, number]) => ({ x, y });

function trackToCmd(t: Track): CmdTrack {
  return { net: t.net, layer: t.layer, width: t.width, pts: t.pts.map(pt) };
}
function viaToCmd(v: Via): CmdVia {
  return { net: v.net, at: { x: v.x, y: v.y }, drill: v.drill, diameter: v.d, from_layer: v.from, to_layer: v.to };
}
function zoneToCmd(z: Zone): CmdZone {
  // Every ZONE_SETTINGS field forwarded explicitly, not just net/layer/
  // outline -- otherwise a copy/paste or duplicate of a zone with
  // customized clearance/priority/hatch/etc. would silently reset it to
  // KiCad's defaults the moment it round-trips through `paste_items`.
  return {
    net: z.net,
    layer: z.layer,
    outline: z.outline.map(pt),
    clearance: z.clearance,
    min_thickness: z.min_thickness,
    thermal_gap: z.thermal_gap,
    thermal_spoke_width: z.thermal_spoke_width,
    pad_connection: z.pad_connection,
    priority: z.priority,
    island_removal_mode: z.island_removal_mode,
    min_island_area: z.min_island_area,
    fill_mode: z.fill_mode,
    hatch_thickness: z.hatch_thickness,
    hatch_gap: z.hatch_gap,
    hatch_orientation_mdeg: z.hatch_orientation_mdeg,
    hatch_smoothing_level: z.hatch_smoothing_level,
    hatch_smoothing_value: z.hatch_smoothing_value,
    hatch_hole_min_area: z.hatch_hole_min_area,
    hatch_border_algorithm: z.hatch_border_algorithm,
  };
}
function shapeToCmd(s: Shape): CmdShape {
  const common = { layer: s.layer, stroke_width: s.stroke_width, filled: s.filled };
  switch (s.kind) {
    case "segment":
      return { kind: "segment", ...common, start: pt(s.start), end: pt(s.end) };
    case "arc":
      return { kind: "arc", ...common, start: pt(s.start), mid: pt(s.mid), end: pt(s.end) };
    case "rect":
      return { kind: "rect", ...common, start: pt(s.start), end: pt(s.end) };
    case "circle":
      return { kind: "circle", ...common, center: pt(s.center), end: pt(s.end) };
    case "polygon":
      return { kind: "polygon", ...common, pts: s.pts.map(pt) };
  }
}
function textToCmd(t: BoardText): CmdText {
  return { content: t.content, at: { x: t.x, y: t.y }, angle: t.angle, layer: t.layer, size_um: t.size, stroke_width: t.stroke_width, justify: t.justify, mirror: t.mirror };
}

/** Every id in `ids` that names a track/via/zone/shape/text, converted to the clipboard's Cmd-ready shape. Footprints are never copyable this way -- same scope line as `Cmd::Duplicate` (crates/ops/src/lib.rs). */
export function collectClipboardContents(board: BoardState, ids: ReadonlySet<string>): ClipboardContents | null {
  const tracks = (board.routing?.tracks ?? []).filter((t) => ids.has(t.id)).map(trackToCmd);
  const vias = (board.routing?.vias ?? []).filter((v) => ids.has(v.id)).map(viaToCmd);
  const zones = (board.routing?.zones ?? []).filter((z) => ids.has(z.id)).map(zoneToCmd);
  const shapes = (board.drawings?.shapes ?? []).filter((s) => ids.has(s.id)).map(shapeToCmd);
  const texts = (board.drawings?.texts ?? []).filter((t) => ids.has(t.id)).map(textToCmd);
  if (tracks.length === 0 && vias.length === 0 && zones.length === 0 && shapes.length === 0 && texts.length === 0) return null;
  return { tracks, vias, zones, shapes, texts };
}

/** Every track/via/zone/shape/text id currently on the board -- used to diff before/after a duplicate or paste to find out what's new (neither Cmd's reply says so directly; see state/store.tsx). */
export function allItemIds(board: BoardState): Set<string> {
  const ids = new Set<string>();
  for (const t of board.routing?.tracks ?? []) ids.add(t.id);
  for (const v of board.routing?.vias ?? []) ids.add(v.id);
  for (const z of board.routing?.zones ?? []) ids.add(z.id);
  for (const s of board.drawings?.shapes ?? []) ids.add(s.id);
  for (const t of board.drawings?.texts ?? []) ids.add(t.id);
  return ids;
}
