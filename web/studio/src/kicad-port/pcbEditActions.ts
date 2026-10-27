// Pure logic behind a batch of pcbnew actions the studio had no handler for
// (docs/parity/UI-ACTIONS.md, "pcbnew: not handled, hotkeyed first"). Each
// function names the KiCad source function it ports; the action handlers
// themselves (actions/useActionRunner.ts, the "pcbnew parity" block) only
// gather state, call these, and send the resulting `/api/cmd` verbs.
//
// Everything here is pure (no store, no DOM) so it is unit-tested in
// pcbEditActions.test.ts.

import type { ExtraKind } from "./pcbExtraItems";
import { ancestors, type GroupLike } from "./groupTree";

// ------------------------------------------------------------------ lock

/**
 * `BOARD_EDITOR_CONTROL::modifyLockSelected` (board_editor_control.cpp),
 * TOGGLE mode: "aMode = ON; for each item: if IsLocked() { aMode = OFF;
 * break; }" -- so a selection with ANY locked item unlocks everything, and
 * only an entirely unlocked selection locks. Returns the `locked` value to
 * send in `set_locked`.
 */
export function resolveToggleLock(ids: readonly string[], lockedIds: ReadonlySet<string>, groups: readonly GroupLike[] = []): boolean {
  // `BOARD_ITEM::IsLocked`: an item's own lock, or a locked group above it (round 6 E6-8).
  for (const id of ids) if (lockedIds.has(id) || ancestors(groups, id).some((g) => lockedIds.has(g.id))) return false;
  return true;
}

// ------------------------------------------------- unconnected footprints

export interface UnconnectedPad {
  num: string;
  net: string | null;
  x: number;
  y: number;
}
export interface UnconnectedPart {
  ref: string;
  placed: boolean;
  pads?: readonly UnconnectedPad[];
}
export interface UnconnectedEdge {
  net: string;
  from: readonly [number, number];
  to: readonly [number, number];
}

/** The pad of a placed part sitting exactly at `pt` on `net` -- how a ratsnest edge endpoint (an anchor position) is mapped back to its `PAD`. */
function padAt(parts: readonly UnconnectedPart[], pt: readonly [number, number], net: string): { part: UnconnectedPart; pad: UnconnectedPad } | null {
  for (const part of parts) {
    if (!part.placed) continue;
    for (const pad of part.pads ?? []) if (pad.net === net && pad.x === pt[0] && pad.y === pt[1]) return { part, pad };
  }
  return null;
}

/**
 * The pads `selectUnconnected` and `grabUnconnected` start from, in selection order: every pad of a selected footprint, and a selected pad itself
 * (`REF.NUMBER`, `#k` for the k-th pad of a number the footprint repeats; round 6 E6-4: `else if( item->Type() == PCB_PAD_T ) pads.push_back( pad )`).
 */
function selectedPads(parts: readonly UnconnectedPart[], selected: readonly string[]): { part: UnconnectedPart; pad: UnconnectedPad }[] {
  const out: { part: UnconnectedPart; pad: UnconnectedPad }[] = [];
  const byRef = new Map(parts.filter((p) => p.placed).map((p) => [p.ref, p] as const));
  for (const id of selected) {
    const whole = byRef.get(id);
    if (whole) {
      for (const pad of whole.pads ?? []) out.push({ part: whole, pad });
      continue;
    }
    const dot = id.indexOf(".");
    const part = dot > 0 ? byRef.get(id.slice(0, dot)) : undefined;
    if (!part) continue;
    const [num, k] = id.slice(dot + 1).split("#");
    const nth = k ? Number(k) : 1;
    const pad = (part.pads ?? []).filter((q) => q.num === num)[nth - 1];
    if (pad) out.push({ part, pad });
  }
  return out;
}

/**
 * `GetRatsnestForPad` for one pad, each edge resolved to the pad at its far end (when the far end is a pad -- an edge ending on a track end or a via has
 * `Parent()->Type() != PCB_PAD_T` and is skipped, as in source).
 */
function farPadsOfPad(parts: readonly UnconnectedPart[], edges: readonly UnconnectedEdge[], pad: UnconnectedPad): { far: { part: UnconnectedPart; pad: UnconnectedPad }; length: number }[] {
  const out: { far: { part: UnconnectedPart; pad: UnconnectedPad }; length: number }[] = [];
  if (!pad.net) return out;
  for (const e of edges) {
    if (e.net !== pad.net) continue;
    let otherPt: readonly [number, number] | null = null;
    if (e.from[0] === pad.x && e.from[1] === pad.y) otherPt = e.to;
    else if (e.to[0] === pad.x && e.to[1] === pad.y) otherPt = e.from;
    if (!otherPt) continue;
    const far = padAt(parts, otherPt, e.net);
    if (!far) continue;
    out.push({ far, length: Math.hypot(e.from[0] - e.to[0], e.from[1] - e.to[1]) });
  }
  return out;
}

/**
 * `PCB_SELECTION_TOOL::selectUnconnected` ("O", Select All Unconnected
 * Footprints): the current selection plus every footprint at the other end
 * of a ratsnest line from one of its pads (`select()` only ever adds).
 * Selection order is preserved, new refs appended in discovery order.
 */
export function selectUnconnectedFootprints(parts: readonly UnconnectedPart[], edges: readonly UnconnectedEdge[], selected: readonly string[]): string[] {
  const result = [...selected];
  const have = new Set(result);
  for (const { pad } of selectedPads(parts, selected)) {
    for (const { far } of farPadsOfPad(parts, edges, pad)) {
      if (!have.has(far.part.ref)) {
        have.add(far.part.ref);
        result.push(far.part.ref);
      }
    }
  }
  return result;
}

/**
 * `PCB_SELECTION_TOOL::grabUnconnected` ("Shift+O", Grab Nearest Unconnected
 * Footprints): the selection is CLEARED, then for each pad of the old
 * selection the single nearest footprint over its ratsnest edges is
 * selected (edges looping back onto the same footprint skipped, strict
 * `<` so the first of equal-length edges wins). The caller then runs
 * Move Individually on the result.
 */
export function grabNearestUnconnectedFootprints(parts: readonly UnconnectedPart[], edges: readonly UnconnectedEdge[], selected: readonly string[]): string[] {
  const result: string[] = [];
  const have = new Set<string>();
  for (const { part, pad } of selectedPads(parts, selected)) {
    let best = Infinity;
    let nearest: string | null = null;
    for (const c of farPadsOfPad(parts, edges, pad)) {
      if (c.far.part.ref === part.ref) continue; // "This edge is a loop on the same footprint"
      if (c.length < best) {
        best = c.length;
        nearest = c.far.part.ref;
      }
    }
    if (nearest && !have.has(nearest)) {
      have.add(nearest);
      result.push(nearest);
    }
  }
  return result;
}

// --------------------------------------------------------- move anchors

/** Every kind of item the move tools carry: a footprint, track, via, zone, graphic, text, dimension or group. */
export type MovableKind = "part" | "track" | "via" | "zone" | "shape" | "text" | "dimension" | "group" | ExtraKind;

type XY = readonly [number, number];

export interface AnchorBoard {
  parts: readonly { ref: string; placed: boolean; at?: XY }[];
  routing?: {
    vias: readonly { id: string; x: number; y: number }[];
    tracks?: readonly { id: string; pts: readonly XY[] }[];
    zones?: readonly { id: string; outline: readonly XY[] }[];
  } | null;
  drawings?: {
    shapes: readonly ({ id: string } & ({ kind: "segment" | "rect" | "arc" | "bezier"; start: XY } | { kind: "circle"; center: XY } | { kind: "polygon"; pts: readonly XY[] }))[];
    texts: readonly { id: string; x: number; y: number }[];
    dimensions?: readonly { id: string; start: XY }[];
    groups?: readonly { id: string; member_ids: readonly string[] }[];
    text_boxes?: readonly { id: string; start: XY; pts: readonly XY[] }[];
    images?: readonly { id: string; x: number; y: number }[];
    points?: readonly { id: string; x: number; y: number }[];
    barcodes?: readonly { id: string; x: number; y: number }[];
  } | null;
}

/**
 * What `BOARD_ITEM::GetPosition()` is for any item the move tools carry: a footprint's anchor, a track's start, a via's centre, a zone's
 * first corner, a shape's first defining point (`PCB_SHAPE::GetPosition()` is its start/centre), a text's position, a dimension's first
 * feature point, a group's box centre (here the centre of the box around its members' own anchors). `Move Individually` glues each item's
 * anchor to the cursor, and `Position Relative` measures from it. Null = not something the move tool carries (an unplaced footprint, an
 * unknown id; a pad moves with its footprint).
 */
export function movableItem(board: AnchorBoard, id: string): { kind: MovableKind; at: [number, number] } | null {
  const part = board.parts.find((p) => p.ref === id);
  if (part?.placed && part.at) return { kind: "part", at: [part.at[0], part.at[1]] };
  const track = board.routing?.tracks?.find((t) => t.id === id);
  if (track?.pts[0]) return { kind: "track", at: [track.pts[0][0], track.pts[0][1]] };
  const via = board.routing?.vias.find((v) => v.id === id);
  if (via) return { kind: "via", at: [via.x, via.y] };
  const zone = board.routing?.zones?.find((z) => z.id === id);
  if (zone?.outline[0]) return { kind: "zone", at: [zone.outline[0][0], zone.outline[0][1]] };
  const shape = board.drawings?.shapes.find((s) => s.id === id);
  if (shape) {
    const p = shape.kind === "circle" ? shape.center : shape.kind === "polygon" ? shape.pts[0] : shape.start;
    return p ? { kind: "shape", at: [p[0], p[1]] } : null;
  }
  const text = board.drawings?.texts.find((t) => t.id === id);
  if (text) return { kind: "text", at: [text.x, text.y] };
  const dim = board.drawings?.dimensions?.find((d) => d.id === id);
  if (dim) return { kind: "dimension", at: [dim.start[0], dim.start[1]] };
  // The text boxes, images, points and barcodes (GAPS.md item 24): `PCB_TEXTBOX::GetPosition()` is the shape's, its start (the first corner of a polygon).
  const textBox = board.drawings?.text_boxes?.find((t) => t.id === id);
  if (textBox) return { kind: "textbox", at: [(textBox.pts[0] ?? textBox.start)[0], (textBox.pts[0] ?? textBox.start)[1]] };
  // `PCB_REFERENCE_IMAGE::GetPosition()` is the image's centre.
  const image = board.drawings?.images?.find((i) => i.id === id);
  if (image) return { kind: "image", at: [image.x, image.y] };
  // `PCB_POINT::GetPosition()`
  const point = board.drawings?.points?.find((q) => q.id === id);
  if (point) return { kind: "point", at: [point.x, point.y] };
  // `PCB_BARCODE::GetPosition()` is the barcode's centre.
  const barcode = board.drawings?.barcodes?.find((q) => q.id === id);
  if (barcode) return { kind: "barcode", at: [barcode.x, barcode.y] };
  const group = board.drawings?.groups?.find((g) => g.id === id);
  if (group) {
    const anchors = group.member_ids.map((m) => (m === id ? null : movableItem(board, m))).filter((m): m is NonNullable<typeof m> => m != null);
    if (anchors.length === 0) return null;
    const xs = anchors.map((a) => a.at[0]);
    const ys = anchors.map((a) => a.at[1]);
    return { kind: "group", at: [(Math.min(...xs) + Math.max(...xs)) / 2, (Math.min(...ys) + Math.max(...ys)) / 2] };
  }
  return null;
}

/**
 * `LEADER_MODE::DEG90` (`POLYGON_GEOM_MANAGER`/`EDA_SHAPE` leader line): the
 * end point snaps onto whichever axis the cursor is further along -- a
 * strictly horizontal or vertical run from `from`.
 */
export function posture90(from: readonly [number, number], to: readonly [number, number]): [number, number] {
  return Math.abs(to[0] - from[0]) >= Math.abs(to[1] - from[1]) ? [to[0], from[1]] : [from[0], to[1]];
}

// ------------------------------------------------------ track layer step

/**
 * `PCB_ACTIONS::layerNext`/`layerPrev` as `EDIT_TOOL::ChangeTrackLayer`
 * invokes them (changeTrackLayerNext/Prev, Ctrl++/Ctrl+-): the next enabled
 * copper layer in stack order, skipping hidden ones, wrapping. The active
 * layer jumps to B.Cu (next) / F.Cu (prev) when it is not a copper layer
 * at all. Returns null when every other copper layer is hidden (source:
 * `wxBell()`, active layer unchanged -- and ChangeTrackLayer then returns
 * early because `newLayer == origLayer`).
 */
export function stepCopperLayer(layers: readonly string[], layerVisible: Readonly<Record<string, boolean>>, active: string | null, dir: 1 | -1): string | null {
  if (layers.length === 0) return null;
  if (active == null || !layers.includes(active)) return (dir === 1 ? layers[layers.length - 1] : layers[0]) ?? null;
  const i = layers.indexOf(active);
  for (let step = 1; step <= layers.length; step++) {
    const j = (((i + dir * step) % layers.length) + layers.length) % layers.length;
    const l = layers[j]!;
    if (layerVisible[l] !== false) return l === active ? null : l;
  }
  return null;
}

// -------------------------------------------------------- unroute segment

export interface UnrouteTrack {
  id: string;
  net: string;
  pts: readonly (readonly [number, number])[];
}
export interface UnrouteVia {
  id: string;
  net: string;
  x: number;
  y: number;
}

function endpointsOfTrack(t: UnrouteTrack): [number, number][] {
  const first = t.pts[0];
  const last = t.pts[t.pts.length - 1];
  return first && last ? [[first[0], first[1]], [last[0], last[1]]] : [];
}

/**
 * The selection `PCB_SELECTION_TOOL::unrouteSegment` ("Unroute Segment",
 * Backspace) leaves behind: it deletes the selected tracks/vias, collects the
 * items connected to them with `selectAllConnectedTracks(...,
 * STOP_AT_SEGMENT)` (one hop: whatever copper touches a deleted item's
 * endpoint on the same net), and re-selects those that were not themselves
 * deleted ("so the user can continue backing up as desired").
 */
export function unrouteSegmentReselect(tracks: readonly UnrouteTrack[], vias: readonly UnrouteVia[], deletedIds: ReadonlySet<string>): string[] {
  const pointsByNet = new Map<string, Set<string>>();
  const add = (net: string, x: number, y: number) => {
    let s = pointsByNet.get(net);
    if (!s) pointsByNet.set(net, (s = new Set()));
    s.add(`${x},${y}`);
  };
  for (const t of tracks) if (deletedIds.has(t.id)) for (const [x, y] of endpointsOfTrack(t)) add(t.net, x, y);
  for (const v of vias) if (deletedIds.has(v.id)) add(v.net, v.x, v.y);

  const out: string[] = [];
  for (const t of tracks) {
    if (deletedIds.has(t.id)) continue;
    const pts = pointsByNet.get(t.net);
    if (pts && endpointsOfTrack(t).some(([x, y]) => pts.has(`${x},${y}`))) out.push(t.id);
  }
  for (const v of vias) {
    if (deletedIds.has(v.id)) continue;
    const pts = pointsByNet.get(v.net);
    if (pts?.has(`${v.x},${v.y}`)) out.push(v.id);
  }
  return out;
}

// ------------------------------------------------------ position relative

export interface PositionedItem {
  isFootprint: boolean;
  /** A pad selected on its own (`PCB_PAD_T`): the middle tier of the anchor. */
  isPad?: boolean;
  x: number;
  y: number;
}

/**
 * `PCB_SELECTION::GetTopLeftItem( aFootprintsOnly )`: the leftmost item
 * (ties: highest, i.e. smallest y). With `footprintsOnly` non-footprints are
 * skipped; returns null when there is nothing to pick.
 */
export function topLeftItem<T extends PositionedItem>(items: readonly T[], footprintsOnly: boolean): T | null {
  let best: T | null = null;
  for (const it of items) {
    if (footprintsOnly && !it.isFootprint) continue;
    if (best == null || it.x < best.x || (it.x === best.x && it.y < best.y)) best = it;
  }
  return best;
}

/**
 * `POSITION_RELATIVE_TOOL::PositionRelative`'s `m_selectionAnchor`: "We
 * prefer footprints, then pads, then anything else" -- the top-left
 * footprint if the selection has one, else the top-left pad (a pad selected
 * on its own: its position, while the move takes its footprint), else the
 * top-left item of any kind.
 */
export function positionRelativeSelectionAnchor(items: readonly PositionedItem[]): { x: number; y: number } | null {
  const it = topLeftItem(items, true) ?? topLeftItem(items.filter((i) => i.isPad), false) ?? topLeftItem(items, false);
  return it ? { x: it.x, y: it.y } : null;
}

/**
 * `POSITION_RELATIVE_TOOL::RelativeItemSelectionMove`:
 * `aggregateTranslation = aPosAnchor + aTranslation - GetSelectionAnchorPosition()`
 * -- every selected item moves by this one vector, so the selection's anchor
 * lands at (reference + offset).
 */
export function relativeMoveVector(referenceAnchor: { x: number; y: number }, translation: { x: number; y: number }, selectionAnchor: { x: number; y: number }): { x: number; y: number } {
  return { x: Math.round(referenceAnchor.x + translation.x - selectionAnchor.x), y: Math.round(referenceAnchor.y + translation.y - selectionAnchor.y) };
}

/**
 * `DIALOG_POSITION_RELATIVE::ToPolarDeg` (`r = hypot`, `q = atan2( y, x )` in board, Y-down, coordinates, 0 for r == 0) and the
 * angle field's `SetAngleValue( q )`: the field is `REL_Y_COORD`, and `PCB_ORIGIN_TRANSFORMS::ToDisplay( const EDA_ANGLE&, .. )`
 * negates a relative angle while the Y axis is not inverted ("if( !invertYAxis() && ( aCoordType == REL_X_COORD || .. REL_Y_COORD ) )
 * return -aValue.AsDegrees();"), so the angle shown runs counter-clockwise on the screen: (0, -1000), straight up, is 90.
 * (DIALOG_OFFSET_ITEM's polar fields are the same.)
 */
export function toPolarDeg(x: number, y: number): { r: number; deg: number } {
  const r = Math.hypot(x, y);
  return { r, deg: r === 0 ? 0 : -(Math.atan2(y, x) * 180) / Math.PI + 0 };
}

/**
 * `DIALOG_POSITION_RELATIVE::getTranslationInIU`, polar branch: `q = m_yOffset.GetAngleValue()`, which is `FromDisplay` of the angle typed
 * (negated, as in [`toPolarDeg`]), then `x = KiROUND(r cos q), y = KiROUND(r sin q)`: 90 degrees moves up the screen.
 */
export function polarTranslation(r: number, deg: number): { x: number; y: number } {
  const q = (-deg * Math.PI) / 180;
  return { x: Math.round(r * Math.cos(q)) + 0, y: Math.round(r * Math.sin(q)) + 0 };
}

// ----------------------------------------------------------- route selected

export interface RouteAnchor {
  /** Where the route starts. */
  at: [number, number];
  /** The ratsnest line's far end -- where `Attempt Finish` aims. */
  target: [number, number];
  net: string;
  /** Pad layer hint: `th` pads reach every copper layer. */
  th: boolean;
  side: "top" | "bottom";
}

export interface RouteSelectedPart extends UnconnectedPart {
  side?: "top" | "bottom";
  pads?: readonly (UnconnectedPad & { th?: boolean })[];
}

/**
 * `ROUTER_TOOL::RouteSelected`'s anchor collection: for every selected
 * footprint's pads (in selection order), and every selected track/via end,
 * each ratsnest edge of that item's net with one end on the item yields an
 * anchor (the item's own end) plus its far end as the target. Edges are
 * walked in the order the ratsnest lists them.
 */
export function routeSelectedAnchors(parts: readonly RouteSelectedPart[], edges: readonly UnconnectedEdge[], tracks: readonly UnrouteTrack[], vias: readonly UnrouteVia[], selection: readonly string[]): RouteAnchor[] {
  const out: RouteAnchor[] = [];
  const partByRef = new Map(parts.map((p) => [p.ref, p]));
  const pushFor = (x: number, y: number, net: string | null, th: boolean, side: "top" | "bottom") => {
    if (!net) return;
    for (const e of edges) {
      if (e.net !== net) continue;
      if (e.from[0] === x && e.from[1] === y) out.push({ at: [x, y], target: [e.to[0], e.to[1]], net, th, side });
      else if (e.to[0] === x && e.to[1] === y) out.push({ at: [x, y], target: [e.from[0], e.from[1]], net, th, side });
    }
  };
  for (const id of selection) {
    const part = partByRef.get(id);
    if (part?.placed) {
      for (const pad of part.pads ?? []) pushFor(pad.x, pad.y, pad.net, pad.th ?? false, part.side ?? "top");
      continue;
    }
    const t = tracks.find((tt) => tt.id === id);
    if (t) {
      for (const [x, y] of endpointsOfTrack(t)) pushFor(x, y, t.net, true, "top");
      continue;
    }
    const v = vias.find((vv) => vv.id === id);
    if (v) pushFor(v.x, v.y, v.net, true, "top");
  }
  return out;
}

/**
 * `ROUTER_TOOL::RouteSelected` "Try to return to the original layer as
 * indicating the user's preferred layer": the active layer when the
 * anchor's pad can sit on it (through-hole, or the SMD pad's own side),
 * else the pad's own side layer (first copper layer = top, last = bottom).
 */
export function routeStartLayer(anchor: Pick<RouteAnchor, "th" | "side">, layers: readonly string[], active: string | null): string {
  const sideLayer = anchor.side === "bottom" ? layers[layers.length - 1] : layers[0];
  if (active && layers.includes(active) && (anchor.th || active === sideLayer)) return active;
  return sideLayer ?? layers[0] ?? "F.Cu";
}
