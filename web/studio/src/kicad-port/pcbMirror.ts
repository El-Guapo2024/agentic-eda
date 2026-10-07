// Port of EDIT_TOOL::Mirror (pcbnew/tools/edit_tool.cpp, "Mirror
// Horizontally" / "Mirror Vertically", no default hotkey; `mirrorH` flips
// LEFT_RIGHT, `mirrorV` TOP_BOTTOM) for the items this IR has.
//
// `EDIT_TOOL::MirrorableItems` has shapes, text, zones, pads, tracks, arcs,
// vias, groups -- NOT footprints (those are Flipped, not Mirrored) -- and
// the mirror point is `updateModificationPoint`'s: the item's own
// `GetPosition()` for a single item, else the (grid-snapped) centre of the
// selection's bounding box. Each item mirrors as its own `Mirror()` does:
//   PCB_SHAPE (EDA_SHAPE::flip)   every point; an arc swaps start and end so
//                                 it keeps its direction
//   PCB_TEXT::Mirror              the position only, and the horizontal
//                                 justification flips when the text is
//                                 horizontal (left-right) / vertical (top-bottom)
//   ZONE::Mirror                  the outline
//   PCB_TRACK/PCB_ARC/PCB_VIA     start, end (and mid) / the position
//   PCB_GROUP::Mirror             every member
// Output is a list of `Cmd`s (one `batch` = one undo step).

import type { BoardState, Cmd, CmdRouteTrack, CmdShape, Shape } from "../api/types";
import { itemKind, itemPosition, itemBounds, unionBounds } from "./pcbItems";
import { arcRouteTrack, lineRouteTrack, type P } from "./trackArc";

export type FlipDirection = "leftRight" | "topBottom";

/** `MIRROR( pt, centre, direction )`. */
export function mirrorPoint(p: readonly [number, number], centre: readonly [number, number], dir: FlipDirection): P {
  return dir === "leftRight" ? [2 * centre[0] - p[0], p[1]] : [p[0], 2 * centre[1] - p[1]];
}

const cmdPt = (p: P): { x: number; y: number } => ({ x: p[0], y: p[1] });

/** `EDA_SHAPE::flip` -> the mirrored shape as an `add_shape` payload. */
export function mirrorShape(s: Shape, centre: readonly [number, number], dir: FlipDirection): CmdShape {
  const m = (p: readonly [number, number]) => cmdPt(mirrorPoint(p, centre, dir));
  const base = { layer: s.layer, stroke_width: s.stroke_width, filled: s.filled };
  switch (s.kind) {
    case "segment":
      return { kind: "segment", ...base, start: m(s.start), end: m(s.end) };
    case "rect":
      return { kind: "rect", ...base, start: m(s.start), end: m(s.end) };
    case "circle":
      return { kind: "circle", ...base, center: m(s.center), end: m(s.end) };
    case "arc":
      // MIRROR( start ), MIRROR( end ), MIRROR( centre ), swap( start, end )
      return { kind: "arc", ...base, start: m(s.end), mid: m(s.mid), end: m(s.start) };
    case "polygon":
      return { kind: "polygon", ...base, pts: s.pts.map(m) };
    case "bezier":
      return { kind: "bezier", ...base, start: m(s.start), c1: m(s.c1), c2: m(s.c2), end: m(s.end) };
  }
}

/** `PCB_TEXT::Mirror`: the horizontal justification flips when the text is horizontal (left-right) or vertical (top-bottom). */
export function mirroredJustify(justify: "left" | "center" | "right", angleMdeg: number, dir: FlipDirection): "left" | "center" | "right" {
  const a = ((angleMdeg % 360000) + 360000) % 360000;
  const flips = dir === "topBottom" ? a === 90000 : a === 0;
  if (!flips) return justify;
  return justify === "left" ? "right" : justify === "right" ? "left" : "center";
}

/** The mirror point `updateModificationPoint` picks: the item's position when one item is selected, else the centre of the union bounding box (the caller snaps it to the grid). */
export function mirrorReferencePoint(board: BoardState, ids: readonly string[]): [number, number] | null {
  if (ids.length === 1) return itemPosition(board, ids[0]!);
  const b = unionBounds(ids.map((id) => itemBounds(board, id)));
  return b ? [(b[0] + b[2]) / 2, (b[1] + b[3]) / 2] : null;
}

/** Items `Mirror` acts on (`MirrorableItems`, with groups expanded to their members). */
export function mirrorableIds(board: BoardState, ids: readonly string[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  const visit = (id: string): void => {
    if (seen.has(id)) return;
    seen.add(id);
    const kind = itemKind(board, id);
    if (kind === "group") {
      for (const member of board.drawings?.groups.find((g) => g.id === id)?.member_ids ?? []) visit(member);
    } else if (kind === "track" || kind === "via" || kind === "zone" || kind === "shape" || kind === "text") {
      out.push(id);
    }
  };
  for (const id of ids) visit(id);
  return out;
}

export interface MirrorPlan {
  cmds: Cmd[];
  /** Ids removed by the plan (shapes and tracks are re-created, so they come back under new ids). */
  removed: string[];
}

/** The commands that mirror `ids` about `centre`. Locked items must already have been filtered out by the caller. */
export function planMirror(board: BoardState, ids: readonly string[], centre: readonly [number, number], dir: FlipDirection): MirrorPlan {
  const cmds: Cmd[] = [];
  const removed: string[] = [];
  const removeTracks: string[] = [];
  const addTracks: CmdRouteTrack[] = [];

  for (const id of mirrorableIds(board, ids)) {
    const kind = itemKind(board, id);
    if (kind === "shape") {
      const s = board.drawings!.shapes.find((q) => q.id === id)!;
      cmds.push({ op: "delete_shape", id }, { op: "add_shape", shape: mirrorShape(s, centre, dir) });
      removed.push(id);
    } else if (kind === "track") {
      const t = board.routing!.tracks.find((q) => q.id === id)!;
      removeTracks.push(id);
      removed.push(id);
      const pts = t.pts.map((p) => mirrorPoint(p, centre, dir));
      if (t.arc_mid) addTracks.push(arcRouteTrack(t.net, t.layer, t.width, pts[0]!, mirrorPoint(t.arc_mid, centre, dir), pts[pts.length - 1]!));
      else addTracks.push(lineRouteTrack(t.net, t.layer, t.width, pts));
    } else if (kind === "via") {
      const v = board.routing!.vias.find((q) => q.id === id)!;
      const [x, y] = mirrorPoint([v.x, v.y], centre, dir);
      cmds.push({ op: "move_via", id, x, y });
    } else if (kind === "zone") {
      const z = board.routing!.zones.find((q) => q.id === id)!;
      cmds.push({ op: "set_zone_outline", id, outline: z.outline.map((p) => cmdPt(mirrorPoint(p, centre, dir))) });
    } else if (kind === "text") {
      const t = board.drawings!.texts.find((q) => q.id === id)!;
      const justify = mirroredJustify(t.justify, t.angle, dir);
      if (justify !== t.justify) {
        cmds.push({ op: "edit_text", id, content: t.content, angle: t.angle, layer: t.layer, size_um: t.size, stroke_width: t.stroke_width, justify, mirror: t.mirror });
      }
      const [x, y] = mirrorPoint([t.x, t.y], centre, dir);
      cmds.push({ op: "move_text", id, x, y });
    }
  }
  if (removeTracks.length > 0) cmds.push({ op: "commit_route", remove_track_ids: removeTracks, remove_via_ids: [], tracks: addTracks });
  return { cmds, removed };
}
