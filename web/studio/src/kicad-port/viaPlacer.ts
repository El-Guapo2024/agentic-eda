// The board's Draw Via tool (Ctrl+Shift+X): `DRAWING_TOOL::DrawVia`'s `VIA_PLACER` (pcbnew/tools/drawing_tool.cpp at 8303b2ad), the parts that read the canvas:
// where the via snaps to (`SnapItem`) and which net it takes (`PlaceItem`'s choice, with `findStitchedZoneNet`). The rest of `PlaceItem` -- the DRC refusal
// and the split of the track it landed on -- is the backend's `place_via` (crates/ops/src/via_place.rs), one undo step.
//
// - `CreateItem`: `bds.m_CurrentViaType`, which nothing in KiCad sets from THROUGH, so a through via (`SetLayerPair( B_Cu, F_Cu )`) of the current via size.
// - `SnapItem`, with snapping on (no Shift): onto the nearest track under the via (`AlignToSegment` on its chord) when the magnetic tracks setting is not
//   "never"; else onto the pad under the cursor (its position) when the magnetic pads one is not; else, with "snap to graphics", onto the copper graphic
//   under it (a filled one's position when it is less than four vias' area, a line's nearest grid point, an arc's nearer end, a polygon's nearest edge,
//   anything else its position). Else the via stays where the grid put it.
// - `PlaceItem`'s net: the track's, the pad's, a copper graphic's of a net, else the zone fill's under the via (`findStitchedZoneNet`): on the active layer in
//   high-contrast mode, then on the visible layers of the via; one net is taken, several are asked ("Select Net:"), none is no net (a free via).
import type { Shape } from "../api/types";
import { shapeHitDistance, shapeArea } from "../components/canvas/itemHitTest";
import { queryScene, sceneOf, type PadItem, type SceneBoard, type ShapeItem, type SnapScene, type TrackItem } from "./snapScene";
import { box, type Pt } from "./snapGeom";
import type { MagneticOption } from "./snapAnchors";

/** The title of `selectPossibleNetsByPopupMenu`'s menu, word for word. */
export const SELECT_NET_TITLE = "Select Net:";

/** What the placer reads of the board: the scene's items, and the pads' and tracks' nets. */
export type ViaPlacerBoard = SceneBoard;

/** One zone fill, on one layer: what `ZONE::HitTestFilledArea( layer, position )` tests (the fill's fragments, fractured outlines). */
export interface ViaFill {
  net: string;
  layer: string;
  fragments: ReadonlyArray<ReadonlyArray<readonly [number, number]>>;
  /** A rule area has no fill and no net: `findStitchedZoneNet` skips it. */
  ruleArea?: boolean;
}

export interface ViaPlacerInput {
  board: ViaPlacerBoard;
  /** The via's diameter (`bds.GetCurrentViaSize()`) and the copper layers it spans (a through via: all of them). */
  diameter: number;
  layers: readonly string[];
  /** `MAGNETIC_SETTINGS`: `tracks`, `pads` (`NO_EFFECT` is "never"), `graphics`. */
  magnetic: { tracks: MagneticOption; pads: MagneticOption; graphics: boolean };
  /** `m_gridHelper.GetSnap()`: false while Shift is held. */
  snap: boolean;
  /** `AlignToSegment( position, seg )` of the grid helper (pcbGridHelper.ts): the grid point on the segment nearest the cursor, or an end. */
  alignToSegment: (p: Pt, a: Pt, b: Pt) => Pt;
  fills: readonly ViaFill[];
  activeLayer: string;
  /** `HIGH_CONTRAST_MODE::DIMMED` or `HIDDEN`. */
  highContrast: boolean;
  /** `m_board->GetVisibleLayers()`. */
  isLayerVisible: (layer: string) => boolean;
}

const overlaps = (a: readonly string[], b: readonly string[]) => a.some((l) => b.includes(l));

/** Whether an item is shown at all. */
export type ItemShown = (item: { type: string; layers: readonly string[]; th?: boolean }) => boolean;

/**
 * `VIEW::Query` (`common/view/view.cpp`): "ignore layers that do not contain actual items" -- `if( ( *i )->displayOnly || !( *i )->visible ) continue;`.
 * `findTrack`, `findPad`, `findGraphic` and `checkDRCViolation` collect their candidates through it, so an item none of whose view layers is shown is not
 * snapped to, gives no net, is not split and does not refuse the via (round 6 C6-1). A track or a graphic lives on its own layer (`PCB_TRACK::ViewGetLayers`:
 * the layer, its net-name and clearance layers, which follow it); a plated pad also on the hole layers (`LAYER_PAD_PLATEDHOLES`, ...), which no copper layer
 * hides. (A hidden object class -- the Tracks, Pads or Graphics row -- is a level-of-detail choice, `ViewGetLOD`, which `Query` does not look at.)
 */
export function shownBy(isLayerVisible: (layer: string) => boolean): ItemShown {
  return (item) => (item.type === "pad" && item.th === true) || item.layers.some(isLayerVisible);
}

const allShown: ItemShown = () => true;
const d2 = (p: Pt, q: Pt) => (p[0] - q[0]) ** 2 + (p[1] - q[1]) ** 2;

/** `SEG::NearestPoint`. */
function nearestOnSeg(p: Pt, a: Pt, b: Pt): Pt {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  if (len2 === 0) return a;
  const t = Math.min(1, Math.max(0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2));
  return [a[0] + t * dx, a[1] + t * dy];
}

/** The via's box at `pos` (`getEffectiveBoundingBox`): what `view->Query` is asked about. */
const viaBox = (pos: Pt, diameter: number) => box(pos[0] - diameter / 2, pos[1] - diameter / 2, pos[0] + diameter / 2, pos[1] + diameter / 2);

/**
 * `findTrack`: the track segment (or arc) under the via at `pos` on one of its layers -- a segment within half its width and half the via's
 * (`TestSegmentHit`), an arc within half the via (`PCB_ARC::HitTest`) -- the nearest by its chord (`SEG( start, end ).NearestPoint`).
 */
export function findTrack(scene: SnapScene, pos: Pt, diameter: number, layers: readonly string[], shown: ItemShown = allShown): TrackItem | null {
  let best: TrackItem | null = null;
  let bestD = Infinity;
  for (const item of queryScene(scene, viaBox(pos, diameter))) {
    if (item.type !== "track" || !overlaps(item.layers, layers) || !shown(item)) continue;
    const t = item as TrackItem;
    const reach = (t.width + diameter) / 2;
    const hit = t.arc ? arcDistance(pos, t) <= reach : Math.sqrt(d2(pos, nearestOnSeg(pos, t.start, t.end))) <= reach;
    if (!hit) continue;
    const d = Math.round(Math.sqrt(d2(nearestOnSeg(pos, t.start, t.end), pos)));
    if (d < bestD) {
      bestD = d;
      best = t;
    }
  }
  return best;
}

/** The distance of `p` from an arc track's centre line (an end when it is outside the arc's sweep). */
function arcDistance(p: Pt, t: TrackItem): number {
  const a = t.arc!;
  if (Math.min(d2(p, t.start), d2(p, t.end)) <= ((t.width / 2) ** 2)) return 0;
  const ang = Math.atan2(p[1] - a.c[1], p[0] - a.c[0]);
  const twoPi = Math.PI * 2;
  const rel = (((ang - a.a0) % twoPi) + twoPi) % twoPi;
  const within = a.da >= 0 ? rel <= a.da : twoPi - rel <= -a.da || rel === 0;
  if (within) return Math.abs(Math.hypot(p[0] - a.c[0], p[1] - a.c[1]) - a.r);
  return Math.sqrt(Math.min(d2(p, t.start), d2(p, t.end)));
}

/** `findPad`: the pad on one of the via's layers that `pos` is inside (`PAD::HitTest`). */
export function findPad(scene: SnapScene, pos: Pt, diameter: number, layers: readonly string[], shown: ItemShown = allShown): PadItem | null {
  for (const item of queryScene(scene, viaBox(pos, diameter))) {
    if (item.type === "pad" && overlaps(item.layers, layers) && shown(item as PadItem) && item.hit(pos)) return item as PadItem;
  }
  return null;
}

/** `EDA_SHAPE::getPosition`: an arc's centre, a polygon's first corner, else the start (a circle's centre). */
function shapePosition(s: Shape): Pt {
  switch (s.kind) {
    case "arc": {
      const c = circumcentre(s.start, s.mid, s.end);
      return c ?? [s.start[0], s.start[1]];
    }
    case "polygon":
      return s.pts[0] ? [s.pts[0][0], s.pts[0][1]] : [0, 0];
    case "circle":
      return [s.center[0], s.center[1]];
    default:
      return [s.start[0], s.start[1]];
  }
}

function circumcentre(a: readonly [number, number], b: readonly [number, number], c: readonly [number, number]): Pt | null {
  const d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
  if (d === 0) return null;
  const sq = (p: readonly [number, number]) => p[0] * p[0] + p[1] * p[1];
  return [(sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d, (sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d];
}

/** `findGraphic`: the copper graphic on one of the via's layers within half the via of `pos` (`HitTest( pos, viaWidth / 2 )`), the nearest by its position. */
export function findGraphic(scene: SnapScene, pos: Pt, diameter: number, layers: readonly string[], shown: ItemShown = allShown): ShapeItem | null {
  const copper = layers.filter((l) => l.endsWith(".Cu"));
  let best: ShapeItem | null = null;
  let bestD = Infinity;
  for (const item of queryScene(scene, viaBox(pos, diameter))) {
    if (item.type !== "shape" || !overlaps(item.layers, copper) || !shown(item)) continue;
    const s = (item as ShapeItem).shape;
    if (shapeHitDistance(s, pos[0], pos[1]) > diameter / 2 + s.stroke_width / 2) continue;
    const d = Math.round(Math.sqrt(d2(shapePosition(s), pos)));
    if (d < bestD) {
      bestD = d;
      best = item as ShapeItem;
    }
  }
  return best;
}

/** `VIA_PLACER::SnapItem`: where the via goes, from the cursor `mouse` (not grid-snapped) and `gridPos` (the via as the grid put it). */
export function snapViaPosition(input: ViaPlacerInput, mouse: Pt, gridPos: Pt): Pt {
  const scene = sceneOf(input.board);
  const { diameter, layers } = input;
  const shown = shownBy(input.isLayerVisible);
  if (input.magnetic.tracks !== "never" && input.snap) {
    const track = findTrack(scene, mouse, diameter, layers, shown);
    if (track) return input.alignToSegment(mouse, track.start, track.end);
  }
  if (input.magnetic.pads !== "never" && input.snap) {
    const pad = findPad(scene, mouse, diameter, layers, shown);
    if (pad) return pad.at;
  }
  if (input.magnetic.graphics && input.snap) {
    const item = findGraphic(scene, mouse, diameter, layers, shown);
    if (item) {
      const s = item.shape;
      if (s.filled) {
        // "Is this shape something to be replaced by the via, or something to be stitched by multiple vias? Use an area-based test to make a guess."
        const r = diameter / 2;
        return Math.PI * r * r * 4 > shapeArea(s) ? shapePosition(s) : gridPos;
      }
      switch (s.kind) {
        case "segment":
          return input.alignToSegment(mouse, s.start, s.end);
        case "arc":
          return d2(s.end, mouse) < d2(s.start, mouse) ? [s.end[0], s.end[1]] : [s.start[0], s.start[1]];
        case "polygon": {
          if (s.pts.length < 2) return shapePosition(s);
          let bestSeg: [Pt, Pt] | null = null;
          let bestD = Infinity;
          for (let i = 0; i < s.pts.length; i++) {
            const a: Pt = s.pts[i]!;
            const b: Pt = s.pts[(i + 1) % s.pts.length]!;
            const d = d2(nearestOnSeg(mouse, a, b), mouse);
            if (d < bestD) {
              bestD = d;
              bestSeg = [a, b];
            }
          }
          return bestSeg ? input.alignToSegment(mouse, bestSeg[0], bestSeg[1]) : gridPos;
        }
        default:
          return shapePosition(s);
      }
    }
  }
  return gridPos;
}

/** Even-odd inside test of a fill fragment. */
function insideFragment(p: Pt, pts: ReadonlyArray<readonly [number, number]>): boolean {
  let inside = false;
  for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
    const [xi, yi] = pts[i]!;
    const [xj, yj] = pts[j]!;
    if (yi > p[1] !== yj > p[1] && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

/** `findStitchedZoneNet`: the nets of the zone fills under `pos` -- the active layer's alone in high-contrast mode when that is one net, else the visible layers'. */
export function stitchedZoneNets(input: ViaPlacerInput, pos: Pt): string[] {
  const hit = (f: ViaFill) => f.fragments.some((frag) => insideFragment(pos, frag));
  const nets = new Set<string>();
  if (input.highContrast && input.layers.includes(input.activeLayer)) {
    for (const f of input.fills) if (f.layer === input.activeLayer && hit(f)) nets.add(f.net);
  }
  if (nets.size === 1) return [...nets];
  for (const f of input.fills) {
    if (f.ruleArea || !input.layers.includes(f.layer) || !input.isLayerVisible(f.layer)) continue;
    if (hit(f)) nets.add(f.net);
  }
  return [...nets];
}

/** What `PlaceItem` gives the via at `pos`: a net (empty for none) and the track it splits, or the nets to choose from. */
export type ViaNetChoice = { kind: "net"; net: string; track: string | null } | { kind: "choose"; nets: string[] };

/** `PlaceItem`'s net: the track's (and that track is split), the pad's, a copper graphic's of a net, else `findStitchedZoneNet`. */
export function viaNet(input: ViaPlacerInput, pos: Pt): ViaNetChoice {
  const scene = sceneOf(input.board);
  const { diameter, layers } = input;
  const shown = shownBy(input.isLayerVisible);
  const track = findTrack(scene, pos, diameter, layers, shown);
  if (track) {
    const t = input.board.routing?.tracks.find((x) => x.id === track.owner);
    // `if( track && m_gridHelper.GetSnap() )`: the split; Shift (no snap) leaves the track whole.
    return { kind: "net", net: t?.net ?? "", track: input.snap ? track.owner : null };
  }
  const pad = findPad(scene, pos, diameter, layers, shown);
  if (pad) {
    const part = input.board.parts.find((p) => p.ref === pad.footprint);
    const num = pad.id.slice(pad.footprint.length + 1).split("#")[0];
    return { kind: "net", net: part?.pads?.find((p) => p.num === num)?.net ?? "", track: null };
  }
  const shape = findGraphic(scene, pos, diameter, layers, shown);
  if (shape?.shape.net) return { kind: "net", net: shape.shape.net, track: null };
  const nets = stitchedZoneNets(input, pos);
  if (nets.length > 1) return { kind: "choose", nets };
  return { kind: "net", net: nets[0] ?? "", track: null };
}

/** `NO_NET` (`pcbnew/netinfo.h`, 25ec2335): the menu's name for net 0. */
export const NO_NET = "<no net>";

/**
 * `selectPossibleNetsByPopupMenu`'s entries: "&1  GND" for the first nine (the accelerator shown as its digit), the bare name after; a pour of no net is
 * `NO_NET` (25ec2335: `wxString menuText = netcode ? netInfo.GetNetItem( netcode )->GetNetname() : NO_NET;`; round 6 C6-8).
 */
export function selectNetLabels(nets: readonly string[]): string[] {
  return nets.map((n, i) => {
    const name = n === "" ? NO_NET : n;
    return i < 9 ? `${i + 1}  ${name}` : name;
  });
}
