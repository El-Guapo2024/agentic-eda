// Pure logic behind a batch of pcbnew actions the studio had no handler for
// (docs/parity/UI-ACTIONS.md, "pcbnew: not handled, hotkeyed first"). Each
// function names the KiCad source function it ports; the action handlers
// themselves (actions/useActionRunner.ts, the "pcbnew parity" block) only
// gather state, call these, and send the resulting `/api/cmd` verbs.
//
// Everything here is pure (no store, no DOM) so it is unit-tested in
// pcbEditActions.test.ts.

// ------------------------------------------------------------------ lock

/**
 * `BOARD_EDITOR_CONTROL::modifyLockSelected` (board_editor_control.cpp),
 * TOGGLE mode: "aMode = ON; for each item: if IsLocked() { aMode = OFF;
 * break; }" -- so a selection with ANY locked item unlocks everything, and
 * only an entirely unlocked selection locks. Returns the `locked` value to
 * send in `set_locked`.
 */
export function resolveToggleLock(ids: readonly string[], lockedIds: ReadonlySet<string>): boolean {
  for (const id of ids) if (lockedIds.has(id)) return false;
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
 * `GetRatsnestForPad` + the loop both `PCB_SELECTION_TOOL::selectUnconnected`
 * and `::grabUnconnected` share: for every pad of every selected footprint,
 * the ratsnest edges touching that pad, each resolved to the pad at its far
 * end (when the far end is a pad -- an edge ending on a track end/via has
 * `Parent()->Type() != PCB_PAD_T` and is skipped, as in source).
 */
function farPadsOf(parts: readonly UnconnectedPart[], edges: readonly UnconnectedEdge[], selected: readonly string[]): { padOwner: string; far: { part: UnconnectedPart; pad: UnconnectedPad }; length: number }[] {
  const out: { padOwner: string; far: { part: UnconnectedPart; pad: UnconnectedPad }; length: number }[] = [];
  const sel = new Set(selected);
  for (const part of parts) {
    if (!part.placed || !sel.has(part.ref)) continue;
    for (const pad of part.pads ?? []) {
      if (!pad.net) continue;
      for (const e of edges) {
        if (e.net !== pad.net) continue;
        let otherPt: readonly [number, number] | null = null;
        if (e.from[0] === pad.x && e.from[1] === pad.y) otherPt = e.to;
        else if (e.to[0] === pad.x && e.to[1] === pad.y) otherPt = e.from;
        if (!otherPt) continue;
        const far = padAt(parts, otherPt, e.net);
        if (!far) continue;
        out.push({ padOwner: part.ref, far, length: Math.hypot(e.from[0] - e.to[0], e.from[1] - e.to[1]) });
      }
    }
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
  for (const { far } of farPadsOf(parts, edges, selected)) {
    if (!have.has(far.part.ref)) {
      have.add(far.part.ref);
      result.push(far.part.ref);
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
  const sel = new Set(selected);
  const result: string[] = [];
  const have = new Set<string>();
  for (const part of parts) {
    if (!part.placed || !sel.has(part.ref)) continue;
    for (const pad of part.pads ?? []) {
      if (!pad.net) continue;
      // `farPadsOf` restricted to this single pad: reuse it on a one-pad part.
      const one: UnconnectedPart = { ref: part.ref, placed: true, pads: [pad] };
      const rest = parts.map((p) => (p.ref === part.ref ? one : p));
      let best = Infinity;
      let nearest: string | null = null;
      for (const c of farPadsOf(rest, edges, [part.ref])) {
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
  }
  return result;
}

// --------------------------------------------------------- move anchors

export type MovableKind = "part" | "via" | "shape" | "text";

export interface AnchorBoard {
  parts: readonly { ref: string; placed: boolean; at?: readonly [number, number] }[];
  routing?: { vias: readonly { id: string; x: number; y: number }[] } | null;
  drawings?: {
    shapes: readonly ({ id: string } & ({ kind: "segment" | "rect" | "arc" | "bezier"; start: readonly [number, number] } | { kind: "circle"; center: readonly [number, number] } | { kind: "polygon"; pts: readonly (readonly [number, number])[] }))[];
    texts: readonly { id: string; x: number; y: number }[];
  } | null;
}

/**
 * What `BOARD_ITEM::GetPosition()` is for the kinds this app can move
 * (tracks and zones have no move Cmd, so they are not movable here): a
 * footprint's anchor, a via/text position, a shape's first defining point
 * (`PCB_SHAPE::GetPosition()` is its start/center). `Move Individually`
 * glues each item's anchor to the cursor, so it needs this. Null = not an
 * item the move tool carries here (dimensions included: the pointer-move
 * path builds no dimension move preview).
 */
export function movableItem(board: AnchorBoard, id: string): { kind: MovableKind; at: [number, number] } | null {
  const part = board.parts.find((p) => p.ref === id);
  if (part?.placed && part.at) return { kind: "part", at: [part.at[0], part.at[1]] };
  const via = board.routing?.vias.find((v) => v.id === id);
  if (via) return { kind: "via", at: [via.x, via.y] };
  const shape = board.drawings?.shapes.find((s) => s.id === id);
  if (shape) {
    const p = shape.kind === "circle" ? shape.center : shape.kind === "polygon" ? shape.pts[0] : shape.start;
    return p ? { kind: "shape", at: [p[0], p[1]] } : null;
  }
  const text = board.drawings?.texts.find((t) => t.id === id);
  if (text) return { kind: "text", at: [text.x, text.y] };
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
 * footprint if the selection has one, else the top-left item of any kind.
 * (This app has no free pads, so the middle tier never applies.)
 */
export function positionRelativeSelectionAnchor(items: readonly PositionedItem[]): { x: number; y: number } | null {
  const it = topLeftItem(items, true) ?? topLeftItem(items, false);
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

/** `DIALOG_POSITION_RELATIVE::ToPolarDeg` -- `r = hypot`, angle = atan2(y, x) in board (Y-down) coordinates, 0 for r == 0. */
export function toPolarDeg(x: number, y: number): { r: number; deg: number } {
  const r = Math.hypot(x, y);
  return { r, deg: r === 0 ? 0 : (Math.atan2(y, x) * 180) / Math.PI };
}

/** `DIALOG_POSITION_RELATIVE::getTranslationInIU`, polar branch: `x = KiROUND(r cos q), y = KiROUND(r sin q)`. */
export function polarTranslation(r: number, deg: number): { x: number; y: number } {
  const q = (deg * Math.PI) / 180;
  return { x: Math.round(r * Math.cos(q)), y: Math.round(r * Math.sin(q)) };
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

/**
 * `PCB_ACTIONS::routerContinueFromEnd` for a route that has not fixed
 * anything yet (`HasPlacedAnything() == false`): the route's own start
 * moves to the far end of the ratsnest line that began at the current
 * start. Returns that far end, or null if the start is not on a ratsnest
 * line (source: `ContinueFromEnd` fails, "Route From Other End" error).
 */
export function otherEndOfStart(start: readonly [number, number], net: string, edges: readonly UnconnectedEdge[]): [number, number] | null {
  for (const e of edges) {
    if (e.net !== net) continue;
    if (e.from[0] === start[0] && e.from[1] === start[1]) return [e.to[0], e.to[1]];
    if (e.to[0] === start[0] && e.to[1] === start[1]) return [e.from[0], e.from[1]];
  }
  return null;
}
