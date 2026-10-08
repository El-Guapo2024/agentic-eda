// The pure logic behind the board-control actions (pcbnew's `PCB_CONTROL`, `BOARD_INSPECTION_TOOL`, `ZONE_FILLER_TOOL`):
// which ratsnest lines are drawn, how a net highlight is chosen and toggled back, and which zones are filled. No React, no DOM,
// so `npm run test:unit` covers it; `actions/boardControlActions.ts` is the thin layer that reads the store and dispatches.

import type { FillReport, RatsnestEdge } from "../api/types";
import { padIds } from "./pcbItems";

// ------------------------------------------------------------------- ratsnest

/** `RATSNEST_MODE`: lines for every copper layer, or only between items on a layer that is shown. */
export type RatsnestMode = "all" | "visible";

/**
 * `PCB_CONTROL::RatsnestModeCycle`: off -> all layers -> visible layers -> off. (From off it goes to all; from visible it
 * goes off with the mode left as it was.)
 */
export function ratsnestModeCycle(show: boolean, mode: RatsnestMode): { show: boolean; mode: RatsnestMode } {
  if (!show) return { show: true, mode: "all" };
  if (mode === "all") return { show: true, mode: "visible" };
  return { show: false, mode };
}

export interface RatsnestView {
  /** `m_ShowGlobalRatsnest` (View > Show Ratsnest). */
  showGlobal: boolean;
  mode: RatsnestMode;
  /** `rs->GetHiddenNets()` (`hideNetInRatsnest`). */
  hiddenNets: ReadonlySet<string>;
  /** Pads whose local-ratsnest flag differs from the global state (the Local Ratsnest tool's clicks), as `REF.NUMBER`. */
  flippedPads: ReadonlySet<string>;
  /** Copper layers on show, as indexes into `board.layers` (`aView->IsLayerVisible`). */
  visibleLayers: ReadonlySet<number>;
}

/** An item's `GetLocalRatsnestVisible()`: everything follows the global state, except a pad the tool has clicked. */
function localFlag(id: string | undefined, v: RatsnestView): boolean {
  const flipped = id !== undefined && v.flippedPads.has(id);
  return v.showGlobal ? !flipped : flipped;
}

function onAVisibleLayer(span: readonly [number, number] | undefined, visible: ReadonlySet<number>): boolean {
  if (!span) return true; // an older backend names no layers: never filter on what is unknown
  for (let layer = span[0]; layer <= span[1]; layer++) if (visible.has(layer)) return true;
  return false;
}

/**
 * `RATSNEST_VIEW_ITEM::ViewDraw`'s decision, edge by edge: a hidden net draws nothing; with the global ratsnest on a line
 * is drawn when BOTH its ends' local flags are on (so either end can turn it off), with it off when EITHER end's is (so
 * either can turn it on); and in "visible layers" mode both ends must be on a layer that is shown.
 */
export function displayedRatsnest<E extends RatsnestEdge>(edges: readonly E[], v: RatsnestView): E[] {
  return edges.filter((e) => {
    if (v.hiddenNets.has(e.net)) return false;
    const [a, b] = [localFlag(e.from_id, v), localFlag(e.to_id, v)];
    if (!(v.showGlobal ? a && b : a || b)) return false;
    return v.mode !== "visible" || (onAVisibleLayer(e.from_layers, v.visibleLayers) && onAVisibleLayer(e.to_layers, v.visibleLayers));
  });
}

/**
 * `LocalRatsnestTool`'s click on one pad: `pad->SetLocalRatsnestVisible( !pad->GetLocalRatsnestVisible() )` -- in terms of the
 * set of pads that differ from the global state, the pad changes sides.
 */
export function toggleLocalRatsnestPad(flipped: readonly string[], pad: string): string[] {
  return flipped.includes(pad) ? flipped.filter((p) => p !== pad) : [...flipped, pad];
}

/**
 * `LocalRatsnestTool`'s click on a footprint: `enable = !firstPad->GetLocalRatsnestVisible()`, then every pad of it is set to
 * `enable` -- so a footprint whose first pad shows its lines hides all of them, and the other way round.
 */
export function toggleLocalRatsnestFootprint(flipped: readonly string[], pads: readonly string[], showGlobal: boolean): string[] {
  if (pads.length === 0) return [...flipped];
  const flag = (p: string) => (showGlobal ? !flipped.includes(p) : flipped.includes(p));
  const enable = !flag(pads[0]!);
  // A pad whose flag should be `enable` differs from the global state exactly when `enable !== showGlobal`.
  const wantFlipped = enable !== showGlobal;
  const rest = flipped.filter((p) => !pads.includes(p));
  return wantFlipped ? [...rest, ...pads] : rest;
}

/** `hideNetInRatsnest` / `showNetInRatsnest` (`doHideRatsnestNet`): the hidden set after hiding or showing `nets`. */
export function setNetsHidden(hidden: readonly string[], nets: readonly string[], hide: boolean): string[] {
  const set = new Set(hidden);
  for (const n of nets) {
    if (hide) set.add(n);
    else set.delete(n);
  }
  return [...set];
}

// ------------------------------------------------------------------ highlight

/** Every net on show: the highlight's first net and the rest of a multi-net highlight. */
export function highlightedNets(primary: string | null, more: readonly string[]): string[] {
  return primary ? [primary, ...more.filter((n) => n !== primary)] : [];
}

/** Whether `net` is among the highlighted ones (`netcodes.count( net )`); `highlight` is the first net alone or the whole list. */
export function isHighlighted(net: string | null | undefined, highlight: string | readonly string[] | null): boolean {
  if (!net || !highlight) return false;
  return typeof highlight === "string" ? net === highlight : highlight.includes(net);
}

export interface BoardNets {
  parts: ReadonlyArray<{ ref: string; pads?: ReadonlyArray<{ num: string; net: string | null }> }>;
  routing: { tracks: ReadonlyArray<{ id: string; net: string }>; vias: ReadonlyArray<{ id: string; net: string }>; zones: ReadonlyArray<{ id: string; net: string }> } | null;
}

/**
 * `BOARD_INSPECTION_TOOL::highlightNet( ..., aUseSelection = true )`: the nets of the selected connected items (pads, tracks, vias,
 * zones; a selected footprint stands for its pads here, where the studio's selection is whole footprints), without the unnamed
 * net, in first-seen order. Empty = nothing connected is selected.
 */
export function netsOfSelection(selection: Iterable<string>, board: BoardNets): string[] {
  const nets: string[] = [];
  const add = (n: string | null | undefined) => {
    if (n && !nets.includes(n)) nets.push(n);
  };
  const part = new Map(board.parts.map((p) => [p.ref, p]));
  const tracks = new Map((board.routing?.tracks ?? []).map((t) => [t.id, t.net]));
  const vias = new Map((board.routing?.vias ?? []).map((v) => [v.id, v.net]));
  const zones = new Map((board.routing?.zones ?? []).map((z) => [z.id, z.net]));
  // A selected pad is its own item (`REF.NUMBER`, kicad-port/pcbItems.ts `padIds`): its own net alone.
  const pads = new Map<string, string | null>();
  for (const p of board.parts) padIds(p).forEach((id, i) => pads.set(id, p.pads![i]!.net));
  for (const id of selection) {
    const p = part.get(id);
    if (p) p.pads?.forEach((pad) => add(pad.net));
    else if (pads.has(id)) add(pads.get(id));
    else add(tracks.get(id) ?? vias.get(id) ?? zones.get(id));
  }
  return nets;
}

/** `toggleLastNetHighlight`: the highlighted set and the last one trade places (`m_lastHighlighted`). */
export function swapHighlight(current: readonly string[], last: readonly string[]): { current: string[]; last: string[] } {
  return { current: [...last], last: [...current] };
}

// -------------------------------------------------------------------- zone fill

/**
 * Which zones are filled. `null` = all of them (what Fill All and the live refresh keep), a list = only those (Draft Fill on
 * a selection): `zoneFill` itself (the last fill fetched) stays the whole report, painted only for these.
 */
export type FilledZones = readonly string[] | null;

/** The zones with a fill showing: none when nothing was ever filled (or it was just unfilled). */
export function filledNow(all: readonly string[], filled: FilledZones, hasFill: boolean): string[] {
  if (!hasFill) return [];
  return filled === null ? [...all] : all.filter((id) => filled.includes(id));
}

/** `ZoneFill` ("Draft Fill Selected Zone(s)"): the filled set after filling `ids` too. All filled again is `null`. */
export function afterFill(all: readonly string[], filled: FilledZones, hasFill: boolean, ids: readonly string[]): FilledZones {
  const next = new Set([...filledNow(all, filled, hasFill), ...ids.filter((id) => all.includes(id))]);
  return next.size === all.length ? null : [...next];
}

/** `ZoneUnfill`: the filled set after `UnFill()` on `ids`. An empty list means nothing is filled any more. */
export function afterUnfill(all: readonly string[], filled: FilledZones, hasFill: boolean, ids: readonly string[]): string[] {
  return filledNow(all, filled, hasFill).filter((id) => !ids.includes(id));
}

/** The fill report, kept to the zones in `ids` (`null` = all of it). */
export function keepFilled(report: FillReport, ids: FilledZones): FillReport {
  return ids === null ? report : { ...report, zones: report.zones.filter((z) => ids.includes(z.id)) };
}

// ------------------------------------------------------------------------ zones

/** `ZONE_FILLER_TOOL::ZoneFill`/`ZoneUnfill`: the copper zones among the selection (a rule area has no fill). */
export function selectedCopperZones(selection: Iterable<string>, zones: ReadonlyArray<{ id: string; is_rule_area?: boolean; teardrop?: boolean }>): string[] {
  const copper = new Set(zones.filter((z) => !z.is_rule_area).map((z) => z.id));
  return [...selection].filter((id) => copper.has(id));
}

// --------------------------------------------------------------- zone manager

/** `ZONE::HigherPriority`: the higher priority first, ties broken by the id (KiCad's uuid), also higher first. */
export function zoneOrder(zones: ReadonlyArray<{ id: string; priority: number }>): string[] {
  return [...zones].sort((a, b) => (a.priority !== b.priority ? b.priority - a.priority : a.id < b.id ? 1 : a.id > b.id ? -1 : 0)).map((z) => z.id);
}

/** `ZONE_SETTINGS_BAG`: the manager shows and assigns consecutive priorities, the top of the list the highest (`size - 1` .. 0). */
export function rankPriorities(order: readonly string[]): Record<string, number> {
  const out: Record<string, number> = {};
  order.forEach((id, i) => (out[id] = order.length - 1 - i));
  return out;
}

export type ZoneMove = "top" | "up" | "down" | "bottom";

/**
 * `MODEL_ZONES_OVERVIEW::MoveZoneIndex`: move zone `id` within the rows now shown (`visible`, a filtered subsequence of `order`).
 * Up and down trade priorities with the neighbouring row, top and bottom do that repeatedly -- so the zone changes places with the
 * shown rows only, whatever the filter hides between them. Returns the new full order (`order` itself when nothing moves).
 */
export function moveZone(order: readonly string[], visible: readonly string[], id: string, how: ZoneMove): string[] {
  const row = visible.indexOf(id);
  if (row < 0 || visible.length < 2) return [...order];
  const target = how === "top" ? 0 : how === "bottom" ? visible.length - 1 : how === "up" ? row - 1 : row + 1;
  if (target < 0 || target >= visible.length || target === row) return [...order];
  const shown = [...visible];
  if (how === "up" || how === "down") {
    [shown[row], shown[target]] = [shown[target]!, shown[row]!];
  } else {
    shown.splice(row, 1);
    shown.splice(target, 0, id);
  }
  // The shown rows keep the slots they had in the full order; only their sequence changes.
  const slots = order.map((z, i) => (visible.includes(z) ? i : -1)).filter((i) => i >= 0);
  const next = [...order];
  slots.forEach((slot, k) => (next[slot] = shown[k]!));
  return next;
}

/** `MODEL_ZONES_OVERVIEW::ApplyFilter`: the zones whose name and/or net contain the text (case-insensitive) and that are on `layer` (null = any). */
export function filterZones<Z extends { id: string; net: string; layer: string }>(zones: readonly Z[], text: string, byName: boolean, byNet: boolean, layer: string | null): Z[] {
  const needle = text.trim().toLowerCase();
  return zones.filter((z) => {
    if (layer && z.layer !== layer) return false;
    if (!needle) return true;
    return (byName && z.id.toLowerCase().includes(needle)) || (byNet && z.net.toLowerCase().includes(needle));
  });
}

// ------------------------------------------------------------------ track width

/**
 * `pcbnew.EditorControl.autoTrackWidth` (`BOARD_DESIGN_SETTINGS::m_UseConnectedTrackWidth`): a route started from the end of an
 * existing track takes that track's width instead of the current one. `from` is `findRouteAnchor`'s description of what it found
 * (`track <id>`, `via <id>`, `REF.PAD`); null when it is not a track end (the router uses the current width then).
 */
export function connectedTrackWidth(from: string | undefined, tracks: ReadonlyArray<{ id: string; width: number }>): number | null {
  if (!from?.startsWith("track ")) return null;
  const id = from.slice("track ".length);
  return tracks.find((t) => t.id === id)?.width ?? null;
}

/** `ZoneDuplicate`: "If the new zone is on the same layer(s) as the initial zone, offset it a bit so it can more easily be picked" -- 1 mm each way. */
export const DUPLICATE_ZONE_OFFSET_UM = 1000;

export function duplicatedZoneOutline(outline: ReadonlyArray<readonly [number, number]>, sameLayer: boolean): [number, number][] {
  const d = sameLayer ? DUPLICATE_ZONE_OFFSET_UM : 0;
  return outline.map(([x, y]) => [x + d, y + d]);
}

// ------------------------------------------------------------------ flipped view

/**
 * `pcbnew.Control.flipBoard` (`PCB_CONTROL::FlipPcbView` -> `view->SetMirror( m_FlipBoardView, ... )`): the board seen from its other
 * side is the picture mirrored about the middle of the canvas, so the world point at the centre stays the centre. `x` is a pointer
 * x measured from the canvas's left edge; the mirror maps it to the x the unmirrored view would have at the same world point (and back:
 * it is its own inverse).
 */
export function flipLocalX(flipped: boolean, canvasWidth: number, x: number): number {
  return flipped ? canvasWidth - x : x;
}

/**
 * A drag that pans the view moves the picture by the pointer's travel on screen. In a mirrored picture a rightward drag moves the
 * unmirrored view leftward (`WX_VIEW_CONTROLS` pans through the mirrored matrix the same way).
 */
export function panDeltaX(flipped: boolean, dxScreen: number): number {
  return flipped ? 0 - dxScreen : dxScreen; // `0 -`, not a unary minus: no negative zero for a drag that has not moved
}

/** A view that `before` was panned to (`after`) by a handler that knows nothing of the mirror (the wheel's pan): in a flipped view its x travel goes the other way. */
export function flipPan<V extends { x: number }>(flipped: boolean, before: V, after: V): V {
  return flipped ? { ...after, x: 2 * before.x - after.x } : after;
}
