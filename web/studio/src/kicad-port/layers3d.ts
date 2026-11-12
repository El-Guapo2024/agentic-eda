// The 3D viewer's layers, as KiCad's `BOARD_ADAPTER::createLayers` builds them (KiCad 25ec2335, 3d-viewer/3d_canvas/create_layer_items.cpp) out of the 2D objects
// of 3d-viewer/3d_canvas/create_3Dgraphic_brd_items.cpp: a list of primitives per board layer (the `m_layerMap` containers), the holes the OpenGL renderer culls each
// layer with (`m_TH_ODs`, `m_viaAnnuli`, `m_layerHoleMap`), the off-board pads (`m_offboardPadsFront`, `m_offboardPadsBack`), and what `RENDER_3D_OPENGL::bgWorker`
// takes out of the body for the post-machining (`m_frontCounterborePolys` ..). components/viewer3d/scene.ts turns each list into geometry and culls it as
// `OPENGL_RENDER_LIST::DrawCulled` does, through the stencil buffer.
//
// The primitives are KiCad's: `ROUND_SEGMENT_2D` (a segment of a width with round ends), `FILLED_CIRCLE_2D`, `RING_2D` and the triangles of a polygon
// (`ConvertPolygonToTriangles`), all in board nanometres. Where KiCad does polygon arithmetic the page cannot (`Inflate` of a custom pad's paste or a filled shape's
// mask opening, a knockout text's box less its strokes), the primitive is a job the server answers (`POST /api/board3d/polys`, crates/cli/src/board3d_api.rs): the
// scene draws it once the answer is in.
//
// Pure: no three.js, no React (compiled by `npm run test:unit`).
import type { BoardState, BoardText, FieldInfo, FillZone, MaskPaste, Pad, Part, Shape, Track, Via, Zone } from "../api/types";
import type { BoardBarcode } from "../api/pcbItemTypes";
import { bezierPolyline } from "./bezierPoly";
import { kicadArc } from "./lineEnding";
import { dashPieces, isDashStyle, type DashRatios } from "./strokeDash";
import { fieldStrokes } from "./textHit";
import { boardTextAsField, strokesBoxRing } from "./textCoverage";
import { artTextAsField, graphicsOf, layersOf } from "./fpArt";
import { padHasCopper, padMaskMargin3d, pasteMarginOf } from "./footprint3d";
import { MAX_ERROR_NM, arcToSegmentCount, circlePolygon, nestContours, ovalPolygon, padContours, padPlacement, toBoard, type Contour, type PadPlacement, type Pt } from "./padShape3d";
import { zoneLayers } from "./zoneLayers";
import { boardOutlinePolygons } from "./pcbOutline";

/** One 2D object of a layer, board nanometres, with what it is (`tag`: "track", "via", "pad", "zone-fill", "part-ref" ..: the scene names its pieces by it). */
export type Prim = PrimShape & { tag?: string; meta?: { padShape?: string; corners?: number } };

export type PrimShape =
  /** `ROUND_SEGMENT_2D`: from `a` to `b`, `w` wide, round ends. */
  | { k: "seg"; a: Pt; b: Pt; w: number }
  /** `FILLED_CIRCLE_2D`. */
  | { k: "circle"; c: Pt; r: number }
  /** `RING_2D`. */
  | { k: "ring"; c: Pt; ri: number; ro: number }
  /** A polygon's triangles (`ConvertPolygonToTriangles`): contours filled with the even-odd rule. */
  | { k: "poly"; contours: Contour[] }
  /** A polygon the server works out (`jobs`), drawn once it has answered. */
  | { k: "job"; key: string };

/** `POST /api/board3d/polys`' job: `a` (outlines with holes) unioned, inflated by `inflate` (round corners, or sharp ones with `acute`), less `b`. */
export interface PolyJob {
  key: string;
  a: Array<{ outer: Contour; holes: Contour[] }>;
  inflate?: number;
  acute?: boolean;
  b?: Array<{ outer: Contour; holes: Contour[] }>;
}

/** The Render Options and rows `createLayers` reads (`EDA_3D_VIEWER_SETTINGS::RENDER_SETTINGS`, `GetVisibleLayers`). */
export interface Layers3dOptions {
  /** `Is3dLayerEnabled( layer, visibilityFlags )`: whether a board layer's row is on (the copper rows, the silkscreen, mask, paste, adhesive and user rows; an inner copper layer as the PCB editor shows it). */
  layerShown: (layer: string) => boolean;
  /** `aFlags.test( LAYER_FP_TEXT )`, `LAYER_FP_REFERENCES`, `LAYER_FP_VALUES`. */
  fpText: boolean;
  references: boolean;
  values: boolean;
  /** `render.show_zones`. */
  showZones: boolean;
  /** `render.opengl_show_off_board_silk`. */
  showOffBoardSilk: boolean;
  /** `render.clip_silk_on_via_annuli`. */
  clipSilkOnViaAnnuli: boolean;
  /** `render.subtract_mask_from_silk`. */
  subtractMaskFromSilk: boolean;
  /** `RENDER_SETTINGS::DifferentiatePlatedCopper()`. */
  differentiatePlatedCopper: boolean;
  /** `GetHolePlatingThickness()`, nm. */
  plating: number;
}

/** A via's covering disk or dimple (`RENDER_3D_OPENGL::generateViaCovers`), drawn with the solder mask of its side. */
export interface ViaCover {
  c: Pt;
  r: number;
  /** `generateDimple`'s depth (a plugged via that is not filled), else 0 (`generateDisk`). */
  depth: number;
}

/** A ring of board material or of plating between two heights (`generateCylinder`, `generateInvCone`), heights as layer positions the scene resolves. */
export interface Cylinder {
  c: Pt;
  ri: number;
  ro: number;
  /** Where it runs: from `top` down to `bottom`, each a copper layer's face ("F.Cu:top" ..), or a depth below a face (`{ face, depth }`). */
  top: ZRef;
  bottom: ZRef;
  /** A countersink's half-angle, tenths of a degree (`generateInvCone`): the top is a cone from the inner radius. */
  coneDdeg?: number;
  /** A countersink's plating (`appendPostMachiningGeometry`): a cone wall from `ri` at its narrow end to `ro` at its wide end (`wideAt`), `plating` thick. */
  coneWall?: number;
  wideAt?: "top" | "bottom";
}

/** A height in the stack: a copper layer's top or bottom face, moved by `depth` (nm) into the board. */
export interface ZRef {
  layer: string;
  face: "top" | "bottom";
  depth?: number;
}

export interface Layers3d {
  /** `m_layerMap`: each board layer's primitives, by its name (the copper layers, F.SilkS, F.Mask, F.Paste, F.Adhes .., the user layers). */
  layers: Map<string, Prim[]>;
  /** `m_TH_ODs`: the through holes, a plated one grown by half the plating, a non-plated one as drilled; every layer but the user ones is culled by it. */
  thODs: Prim[];
  /** `m_viaAnnuli` (with `clip_silk_on_via_annuli`): every through via's annulus and every plated pad's hole grown by half the plating; the silkscreen is culled by it instead. */
  viaAnnuli: Prim[];
  /** `m_layerHoleMap`: each copper layer's own holes (a blind via's, a post-machining's, a backdrill's). */
  layerHoles: Map<string, Prim[]>;
  /** `m_offboardPadsFront` / `Back` (with `show_off_board_silk`): the pads of the footprints wholly off the board. */
  offboardPads: { front: Prim[]; back: Prim[] };
  /** What `bgWorker` takes out of the body besides the holes: the counterbores, countersinks and backdrills (`m_frontCounterborePolys` .., `m_SecondarydrillPolys`). */
  bodyCuts: Contour[];
  /** `backfillPostMachine`'s plugs of board material under a counterbore and beside a backdrill. */
  plugs: Cylinder[];
  /** `generateViaBarrels`' barrels of a post-machined or backdrilled via (the plated part) and the plating of the counterbores and countersinks. */
  barrels: Cylinder[];
  /** The through vias `generateViaBarrels` leaves to the plated hole shells: their barrels are the usual ones. Ids of the vias drawn here instead. */
  barrelVias: Set<string>;
  /** `generateViaCovers`. */
  covers: { front: ViaCover[]; back: ViaCover[] };
  /** The server's polygon jobs the primitives name. */
  jobs: PolyJob[];
}

// -------------------------------------------------------------------------------------------------------------------------------- helpers

const kiRound = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v));

const isCopperLayer = (l: string): boolean => /\.Cu$/.test(l);
const isMaskLayer = (l: string): boolean => l === "F.Mask" || l === "B.Mask";
const isSilkLayer = (l: string): boolean => l === "F.SilkS" || l === "B.SilkS";

/** The layer of a footprint's own graphic or pad, named as for a top-side footprint, on the board (`FOOTPRINT::Flip` swaps F and B). */
function fpLayer(layer: string, side: Part["side"]): string {
  if (side !== "bottom") return layer;
  if (layer.startsWith("F.")) return `B.${layer.slice(2)}`;
  if (layer.startsWith("B.")) return `F.${layer.slice(2)}`;
  return layer;
}

/** `PAD::IsOnLayer` of a technical layer: the pad's `(layers ..)` in its footprint's frame (`*.Mask`, `F&B.Mask`, `F.Paste` ..), else its kind's (an SMD pad its side's paste and mask, a hole both masks). */
export function padOnTechLayer(pad: Pick<Pad, "layers" | "th" | "kind">, part: Pick<Part, "side">, layer: string): boolean {
  const kind = layer.slice(2);
  const layers = pad.layers ?? (pad.th ? ["*.Mask"] : ["F.Paste", "F.Mask"]);
  for (const l of layers) {
    if (l === `*.${kind}` || l === `F&B.${kind}`) {
      if (layer === `F.${kind}` || layer === `B.${kind}`) return true;
    } else if (fpLayer(l, part.side) === layer) return true;
  }
  return false;
}

/**
 * `PAD::IsOnCopperLayer` for a non-plated pad: "If their annular ring size is 0 or negative, then they have no annular ring either" -- a round or oval pad that is no bigger
 * than its hole, with no offset, has no copper.
 */
export function npthHasRing(pad: Pad): boolean {
  const [sx, sy] = pad.size ?? [pad.w, pad.h];
  const [dx, dy] = pad.slot ?? [pad.drill ?? 0, pad.drill ?? 0];
  const noOffset = !pad.offset || (pad.offset[0] === 0 && pad.offset[1] === 0);
  if (pad.shape === "circle" && noOffset && sx <= dx) return false;
  if (pad.shape === "oval" && noOffset && sx <= dx && sy <= dy) return false;
  return true;
}

/**
 * The copper layers a pad has copper on (`addPads`: `pad->IsOnLayer( aLayerId )`, `IsOnCopperLayer`, `FlashLayer`): the layers Remove Unused Pads left it (`flashed`); else its
 * `(layers ..)` (in the footprint's frame: `F.Cu` is the footprint's side, `*.Cu` every copper layer); else its kind's (an SMD pad its side's outer layer, a hole every copper
 * layer). A non-plated pad with no ring has none.
 */
export function padCopperLayers(pad: Pad, part: Pick<Part, "side">, layerOrder: readonly string[]): string[] {
  if (pad.kind === "non_plated_hole" && !npthHasRing(pad)) return [];
  if (pad.flashed) return pad.flashed;
  const front = part.side === "bottom" ? "B.Cu" : "F.Cu";
  const back = part.side === "bottom" ? "F.Cu" : "B.Cu";
  if (!pad.layers) return pad.th ? [...layerOrder] : [front];
  const out = new Set<string>();
  for (const l of pad.layers) {
    if (l === "*.Cu") layerOrder.forEach((x) => out.add(x));
    else if (l === "F&B.Cu") [front, back].forEach((x) => out.add(x));
    else if (l === "F.Cu") out.add(front);
    else if (l === "B.Cu") out.add(back);
    else if (/^In\d+\.Cu$/.test(l)) out.add(l);
  }
  return [...out].filter((l) => layerOrder.includes(l));
}

/** `PAD::GetEffectiveHoleShape` on the board: the slot's segment (its ends) and width, or null without a hole (`if( !drillsize.x || !drillsize.y )`). */
export function padHoleSegment(pad: Pad, part: Pick<Part, "rot" | "side">): { a: Pt; b: Pt; w: number } | null {
  const c: Pt = [pad.px ?? pad.x, pad.py ?? pad.y];
  if (pad.slot) {
    const [sx, sy] = pad.slot;
    if (!sx || !sy) return null;
    const hx = Math.trunc(sx / 2);
    const hy = Math.trunc(sy / 2);
    const hw = Math.min(hx, hy);
    const place: PadPlacement = { x: c[0], y: c[1], padRotDeg: pad.size ? (pad.rot ?? 0) : 0, fpRotDeg: pad.size ? (part.rot ?? 0) : 0, mirror: !!pad.size && part.side === "bottom" };
    return { a: toBoard(place, [-(hx - hw), -(hy - hw)]), b: toBoard(place, [hx - hw, hy - hw]), w: 2 * hw };
  }
  if (pad.drill) return { a: c, b: c, w: pad.drill };
  return null;
}

/** `createPadHoleShape( pad, container, inflate )`: the hole as a round segment `inflate` wider on each side. */
function padHolePrim(pad: Pad, part: Part, inflate: number): Prim | null {
  const h = padHoleSegment(pad, part);
  return h ? { k: "seg", a: h.a, b: h.b, w: h.w + inflate * 2 } : null;
}

/** `PAD::TransformHoleToPolygon( aBuffer, inflate, aError, ERROR_INSIDE )`: the oval of the hole, `inflate` wider on each side. */
export function padHoleContour(pad: Pad, part: Part, inflate: number): Contour | null {
  const h = padHoleSegment(pad, part);
  if (!h) return null;
  if (h.a[0] === h.b[0] && h.a[1] === h.b[1]) return circlePolygon(h.a, Math.trunc(h.w / 2) + inflate, MAX_ERROR_NM);
  return ovalPolygon(h.a, h.b, h.w + 2 * inflate, MAX_ERROR_NM);
}

/** A primitive as contours (even-odd), for the server's jobs and the tests. */
export function primContours(p: Prim): Contour[] {
  switch (p.k) {
    case "seg":
      if (p.w <= 0) return [];
      return [p.a[0] === p.b[0] && p.a[1] === p.b[1] ? circlePolygon(p.a, p.w / 2, MAX_ERROR_NM) : ovalPolygon(p.a, p.b, p.w, MAX_ERROR_NM)];
    case "circle":
      return p.r > 0 ? [circlePolygon(p.c, p.r, MAX_ERROR_NM)] : [];
    case "ring":
      return p.ro > 0 ? [circlePolygon(p.c, p.ro, MAX_ERROR_NM), ...(p.ri > 0 ? [circlePolygon(p.c, p.ri, MAX_ERROR_NM)] : [])] : [];
    case "poly":
      return p.contours;
    case "job":
      return [];
  }
}

/** `KIGFX::PCB_RENDER_SETTINGS::SetDashLengthRatio( plotParams.GetDashedLineDashRatio() )`, `SetGapLengthRatio`: Board Setup > Formatting's dashed lines. */
function dashRatiosOf(board: BoardState): DashRatios | undefined {
  const f = board.board_rules?.formatting;
  return f ? { dash: f.dash_length_ratio, gap: f.gap_length_ratio } : undefined;
}

// -------------------------------------------------------------------------------------------------------------------------------- items

/** Tags what `fn` adds to `out`. */
function tagged(out: Prim[], tag: string, fn: () => void): void {
  const from = out.length;
  fn();
  for (let i = from; i < out.length; i++) out[i]!.tag ??= tag;
}

/** The builder's state: the jobs it has asked for. */
interface Ctx {
  board: BoardState;
  jobs: PolyJob[];
  ratios: DashRatios | undefined;
  maskPaste: MaskPaste | undefined;
}

/** A job for the server, named by what it holds (so the same polygon is asked for once and its answer kept across builds). */
export function makeJob(j: Omit<PolyJob, "key">): PolyJob {
  return { key: `${j.inflate ?? 0}|${j.acute ? 1 : 0}|${hashPolys(j.a)}|${hashPolys(j.b ?? [])}`, ...j };
}

function job(ctx: Ctx, j: Omit<PolyJob, "key">): Prim {
  const pj = makeJob(j);
  if (!ctx.jobs.some((x) => x.key === pj.key)) ctx.jobs.push(pj);
  return { k: "job", key: pj.key };
}

function hashPolys(ps: ReadonlyArray<{ outer: Contour; holes: Contour[] }>): string {
  let h = 2166136261;
  const mix = (v: number) => {
    h ^= v | 0;
    h = Math.imul(h, 16777619);
  };
  for (const p of ps) {
    mix(-1);
    for (const c of [p.outer, ...p.holes]) {
      mix(-2);
      for (const [x, y] of c) {
        mix(Math.round(x));
        mix(Math.round(y));
      }
    }
  }
  return (h >>> 0).toString(36) + ":" + ps.length;
}

/** `createTrackWithMargin( track, container, layer, margin )`: each segment of a track (an arc's tessellation is its `pts`) a round segment `width + 2 margin` wide. */
function addTrack(out: Prim[], t: Pick<Track, "pts" | "width">, margin: number): void {
  const w = t.width + margin * 2;
  if (t.pts.length === 1) out.push({ k: "seg", a: t.pts[0]!, b: t.pts[0]!, w });
  for (let i = 0; i + 1 < t.pts.length; i++) out.push({ k: "seg", a: t.pts[i]!, b: t.pts[i + 1]!, w });
}

/** `PCB_VIA::GetWidth( aLayer )`: the padstack's diameter of the layer (a front / inner / back stack, or one per layer), else the via's. */
function viaWidth(v: Via, layer: string, layerOrder: readonly string[]): number {
  const ps = v.props?.padstack;
  if (!ps) return v.d;
  if (ps.mode === "front_inner_back") {
    if (layer === layerOrder[0] || layer === "F.Cu") return v.d;
    if (layer === "B.Cu") return ps.sizes["B.Cu"] ?? v.d;
    return ps.sizes["Inner"] ?? v.d;
  }
  return ps.sizes[layer] ?? v.d;
}

/** `PCB_VIA::IsTented( aLayer )`: the via's own setting, else Board Setup's (`m_TentViasFront` / `Back`). */
export function viaTented(v: Via, front: boolean, mp: Pick<MaskPaste, "tent_vias_front" | "tent_vias_back"> | undefined): boolean {
  const own = front ? v.props?.front_tenting : v.props?.back_tenting;
  if (own !== undefined) return own;
  return front ? (mp?.tent_vias_front ?? true) : (mp?.tent_vias_back ?? true);
}

/** The copper layers a via is on (`IsOnLayer`: `LAYER_RANGE( start, end )`), and the ones its ring is drawn on (`FlashLayer`). */
function viaSpan(v: Via, layerOrder: readonly string[]): string[] {
  const from = layerOrder.indexOf(v.from);
  const to = layerOrder.indexOf(v.to);
  if (!v.type || from < 0 || to < 0) return [...layerOrder];
  return layerOrder.slice(Math.min(from, to), Math.max(from, to) + 1);
}

/** `PCB_SHAPE::GetSolderMaskExpansion`: the shape's own margin, else the board's (custom DRC rules are not read). */
function shapeMaskExpansion(s: Pick<Shape, "solder_mask_margin">, mp: MaskPaste | undefined): number {
  return s.solder_mask_margin ?? mp?.expansion_nm ?? 0;
}

/** `PCB_TRACK::GetSolderMaskExpansion`: the track's own margin, else the board's, "ensure the resulting mask opening has a non-negative size". */
function trackMaskExpansion(t: Track, mp: MaskPaste | undefined): number {
  let margin = t.mask?.margin ?? mp?.expansion_nm ?? 0;
  if (margin < 0) margin = Math.max(margin, -t.width / 2);
  return margin;
}

/** `PCB_SHAPE::IsOnLayer`: its layer, and its side's mask when it has a solder mask opening on outer copper. */
function shapeOnLayer(s: Pick<Shape, "layer" | "solder_mask">, layer: string): boolean {
  if (s.layer === layer) return true;
  return !!s.solder_mask && ((layer === "F.Mask" && s.layer === "F.Cu") || (layer === "B.Mask" && s.layer === "B.Cu"));
}

/** The segments of an arc as `createArcSegments` lays them (`GetCircleSegmentCount` of the arc's box), from a KiCad arc's centre, start and angle. */
function arcSegs(out: Prim[], centre: Pt, start: Pt, angleDeg: number, width: number): void {
  if (width === 0) return;
  const r = Math.hypot(start[0] - centre[0], start[1] - centre[1]);
  const n = Math.max(1, Math.ceil((arcToSegmentCount(r, MAX_ERROR_NM, 360) * Math.abs(angleDeg)) / 360));
  const a0 = Math.atan2(start[1] - centre[1], start[0] - centre[0]);
  const sweep = (angleDeg * Math.PI) / 180;
  let prev = start;
  for (let i = 1; i <= n; i++) {
    const a = a0 + (sweep * i) / n;
    const p: Pt = [centre[0] + r * Math.cos(a), centre[1] + r * Math.sin(a)];
    out.push({ k: "seg", a: prev, b: p, w: width });
    prev = p;
  }
}

/** A rectangle's corners (`GetRectCorners`): start, (end.x, start.y), end, (start.x, end.y). */
const rectCorners = (s: { start: Pt; end: Pt }): Pt[] => [s.start, [s.end[0], s.start[1]], s.end, [s.start[0], s.end[1]]];

/** A rounded rectangle's outline (`ROUNDRECT::TransformToPolygon`), its corners arcs at the board's error. */
function roundRectOutline(s: { start: Pt; end: Pt }, radius: number): Contour {
  const [x0, x1] = [Math.min(s.start[0], s.end[0]), Math.max(s.start[0], s.end[0])];
  const [y0, y1] = [Math.min(s.start[1], s.end[1]), Math.max(s.start[1], s.end[1])];
  const r = Math.min(radius, (x1 - x0) / 2, (y1 - y0) / 2);
  const n = Math.max(1, Math.ceil(arcToSegmentCount(r, MAX_ERROR_NM, 360) / 4));
  const out: Contour = [];
  const corner = (cx: number, cy: number, from: number) => {
    for (let i = 0; i <= n; i++) {
      const a = from + (Math.PI / 2) * (i / n);
      out.push([cx + r * Math.cos(a), cy + r * Math.sin(a)]);
    }
  };
  corner(x1 - r, y1 - r, 0);
  corner(x0 + r, y1 - r, Math.PI / 2);
  corner(x0 + r, y0 + r, Math.PI);
  corner(x1 - r, y0 + r, (3 * Math.PI) / 2);
  return out;
}

/** The contours of a closed shape's area (`TransformShapeToPolygon( .., ignoreLineWidth = true )`): a rectangle, a rounded one, a circle, a polygon with its holes. */
function shapeArea(s: Shape): Contour[] | null {
  switch (s.kind) {
    case "rect":
      return [s.corner_radius && s.corner_radius > 0 ? roundRectOutline(s, s.corner_radius) : rectCorners(s)];
    case "circle":
      return [circlePolygon(s.center, Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1]), MAX_ERROR_NM)];
    case "polygon":
      return [s.pts.map(([x, y]) => [x, y] as Pt), ...(s.holes ?? []).map((h) => h.map(([x, y]) => [x, y] as Pt))];
    default:
      return null;
  }
}

/** The edges of a closed shape's outline, for its line (`addROUND_SEGMENT_2D` along each). */
function outlineSegs(out: Prim[], contour: Contour, width: number): void {
  for (let i = 0; i < contour.length; i++) out.push({ k: "seg", a: contour[i]!, b: contour[(i + 1) % contour.length]!, w: width });
}

/**
 * `BOARD_ADAPTER::addShape( const PCB_SHAPE*, .., aLayer )`: a shape on `layer`. On a mask layer, a shape on outer copper with a solder mask opening is grown by its expansion,
 * solid and filled if hatched; a solid line is drawn as round segments of its width (a circle as a ring, an arc in `createArcSegments`' pieces); a dashed one as
 * `STROKE_PARAMS::Stroke` cuts it (kicad-port/strokeDash.ts) with Board Setup's ratios; a filled shape as its polygon, without its line width when it is dashed; a
 * hatched one with its hatch lines. Line endings are not drawn (`aShape->GetStartEnding()`).
 */
export function addShape(ctx: Ctx, out: Prim[], s: Shape, layer: string): void {
  let linewidth = s.stroke_width;
  let margin = 0;
  let dashed = isDashStyle(s.dash);
  let solidFill = s.filled;
  let hatched = !!s.fill && !s.filled;
  if (isMaskLayer(layer) && s.solder_mask && (s.layer === "F.Cu" || s.layer === "B.Cu")) {
    margin = shapeMaskExpansion(s, ctx.maskPaste);
    linewidth += margin * 2;
    dashed = false;
    if (hatched) {
      solidFill = true;
      hatched = false;
    }
  }
  if (!dashed || solidFill) {
    switch (s.kind) {
      case "circle": {
        const r = Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1]);
        const inner = r - linewidth / 2;
        const outer = r + linewidth / 2;
        if (solidFill || inner <= 0) out.push({ k: "circle", c: s.center, r: dashed ? r : outer });
        else out.push({ k: "ring", c: s.center, ri: inner, ro: outer });
        break;
      }
      case "rect":
        if (solidFill) {
          const area = shapeArea(s)!;
          // `TransformShapeToPolygon` with the line (the rectangle grown by half the line, round corners), then `Inflate( margin, ROUND_ALL_CORNERS )`: the area and its
          // edges as round segments of the line plus twice the margin.
          out.push({ k: "poly", contours: area });
          const w = (dashed ? 0 : s.stroke_width) + 2 * margin;
          if (w > 0) outlineSegs(out, area[0]!, w);
        } else if (s.corner_radius && s.corner_radius > 0) {
          outlineSegs(out, roundRectOutline(s, s.corner_radius), linewidth);
        } else {
          outlineSegs(out, rectCorners(s), linewidth);
        }
        break;
      case "arc": {
        const k = kicadArc(s.start, s.mid, s.end);
        if (!k) {
          out.push({ k: "seg", a: s.start, b: s.end, w: linewidth });
          break;
        }
        const c: Pt = [k.centre[0], k.centre[1]];
        const a0 = Math.atan2(k.start[1] - c[1], k.start[0] - c[0]);
        let a1 = Math.atan2(k.end[1] - c[1], k.end[0] - c[0]);
        while (a1 <= a0) a1 += Math.PI * 2;
        arcSegs(out, c, [k.start[0], k.start[1]], ((a1 - a0) * 180) / Math.PI, linewidth);
        break;
      }
      case "segment":
        out.push({ k: "seg", a: s.start, b: s.end, w: linewidth });
        break;
      case "bezier": {
        const pts = bezierPolyline(s.start, s.c1, s.c2, s.end, 5);
        for (let i = 0; i + 1 < pts.length; i++) out.push({ k: "seg", a: pts[i]!, b: pts[i + 1]!, w: linewidth });
        break;
      }
      case "polygon": {
        const area = shapeArea(s)!;
        if (solidFill) {
          if (margin < 0) {
            // `polyList.Inflate( margin, ALLOW_ACUTE_CORNERS )` of the polygon with its line: the server's.
            const width = dashed ? 0 : s.stroke_width;
            const nested = nestContours(area);
            out.push(job(ctx, { a: nested, inflate: width / 2 + margin, acute: true }));
          } else {
            out.push({ k: "poly", contours: area });
            const w = (dashed ? 0 : s.stroke_width) + 2 * margin;
            if (w > 0) for (const c of area) outlineSegs(out, c, w);
          }
        } else {
          for (const c of area) outlineSegs(out, c, linewidth);
        }
        break;
      }
    }
  }
  if (dashed) {
    for (const [a, b] of dashPieces(s, { style: s.dash!, width: s.stroke_width, ...(ctx.ratios ? { ratios: ctx.ratios } : {}) }, 5)) out.push({ k: "seg", a, b, w: linewidth });
  }
  // `ConvertPolygonToTriangles( aShape->GetHatching() )`: the hatch lines `UpdateHatching` made, each its width wide.
  if (hatched && s.hatch_lines) {
    const w = s.hatch_width ?? s.stroke_width;
    for (const [x1, y1, x2, y2] of s.hatch_lines) out.push({ k: "seg", a: [x1, y1], b: [x2, y2], w });
  }
}

/**
 * `BOARD_ADAPTER::addText`: a stroke-font text's pen strokes as round segments of its pen width (`CALLBACK_GAL`'s stroke callback); an outline font's runs as their ink's
 * boxes (kicad-port/textHit.ts `fieldStrokes`: the page has no glyph triangles); a knockout text the box of its strokes grown by `GetKnockoutTextMargin` less its strokes
 * (`PCB_TEXT::TransformTextToPolySet`), the server's subtraction.
 */
export function addText(ctx: Ctx, out: Prim[], f: FieldInfo): void {
  const s = fieldStrokes(f);
  if (f.knockout) {
    const margin = Math.max(kiRound(s.pen / 2), kiRound(f.h / 9));
    const box = strokesBoxRing(s, margin);
    if (!box) return;
    const strokes = s.segs.map(([x0, y0, x1, y1]) => ({ outer: x0 === x1 && y0 === y1 ? circlePolygon([x0, y0], s.pen / 2, MAX_ERROR_NM) : ovalPolygon([x0, y0], [x1, y1], s.pen, MAX_ERROR_NM), holes: [] }));
    for (const a of s.areas) strokes.push({ outer: a.map(([x, y]) => [x, y] as Pt), holes: [] });
    out.push(job(ctx, { a: [{ outer: box.map(([x, y]) => [x, y] as Pt), holes: [] }], b: strokes }));
    return;
  }
  for (const [x0, y0, x1, y1] of s.segs) out.push({ k: "seg", a: [x0, y0], b: [x1, y1], w: s.pen });
  for (const a of s.areas) out.push({ k: "poly", contours: [a.map(([x, y]) => [x, y] as Pt)] });
}

/** A board text (`PCB_TEXT`) through [`addText`]. */
function addBoardText(ctx: Ctx, out: Prim[], t: BoardText): void {
  addText(ctx, out, boardTextAsField(t));
}

/** `addBarCode`: the barcode's polygons (`TransformShapeToPolySet`), here the shape kicad-cli plots of it (kicad-port/barcodeShapes.ts), turned onto the board; nothing until it has arrived. */
function addBarcode(out: Prim[], b: BoardBarcode, paths: ReadonlyArray<ReadonlyArray<ReadonlyArray<readonly [number, number]>>> | null): void {
  if (!paths) return;
  // The plot's micrometres about the barcode's centre, already turned and mirrored.
  for (const path of paths) out.push({ k: "poly", contours: path.map((ring) => ring.map(([x, y]) => [b.x + x * 1000, b.y + y * 1000] as Pt)) });
}

/**
 * `BOARD_ADAPTER::createPadWithMargin( pad, container, layer, margin )`: the pad's copper, or its paste or mask opening grown by `margin` (kicad-port/padShape3d.ts); a custom
 * pad's outline `Inflate`d by the margin's x with round corners (the server's), as KiCad applies the margin to "its final polygon shape".
 */
function addPadWithMargin(ctx: Ctx, out: Prim[], pad: Pad, part: Part, margin: Pt): void {
  if (pad.shape === "custom" && (margin[0] !== 0 || margin[1] !== 0)) {
    const outline = (pad.outline ?? []).map((c) => c.map(([x, y]) => [x, y] as Pt));
    if (outline.length === 0) return;
    out.push(job(ctx, { a: nestContours(outline), inflate: margin[0] }));
    return;
  }
  const contours = padContours(pad, padPlacement(pad, part), margin);
  if (contours.length > 0) out.push({ k: "poly", contours });
}

/** One copper layer of a padstack pad whose copper differs from layer to layer (`PADSTACK::ForEachUniqueLayer`), or the pad itself. */
function padCopperOn(pad: Pad, part: Part, layer: string): Contour[] | null {
  if (Array.isArray(pad.stack_layers)) {
    const g = pad.stack_layers.find((x) => x.layers.includes(layer)) as (Pad["stack_layers"] & object)[number] & { size?: [number, number] | null } | undefined;
    if (!g) return null;
    const place = g.size ? padPlacement(pad, part, [g.x, g.y]) : padPlacement({ x: g.x, y: g.y, rot: 0, size: undefined }, part);
    return padContours({ shape: g.shape, size: g.size ?? null, w: g.w, h: g.h, round: g.round, ratio: g.ratio, outline: g.outline }, place);
  }
  return padContours(pad, padPlacement(pad, part));
}

/** `BOARD_ADAPTER::addPads( footprint, container, layer )`: the footprint's pads on `layer` -- on a copper layer those with copper there; on a mask layer grown by `GetSolderMaskExpansion`, on a paste layer by `GetSolderPasteMargin`. */
function addPads(ctx: Ctx, out: Prim[], part: Part, layer: string, layerOrder: readonly string[]): void {
  for (const pad of part.pads ?? []) {
    if (isCopperLayer(layer)) {
      const layers = Array.isArray(pad.stack_layers) ? pad.stack_layers.flatMap((g) => g.layers) : padCopperLayers(pad, part, layerOrder);
      if (!layers.includes(layer)) continue;
      const contours = padCopperOn(pad, part, layer);
      if (contours && contours.length > 0) out.push({ k: "poly", contours, tag: "pad", meta: { padShape: pad.shape ?? (pad.round ? "circle" : "rect"), corners: contours.reduce((n, c) => n + c.length, 0) } });
      continue;
    }
    if (!padOnTechLayer(pad, part, layer)) continue;
    if (isMaskLayer(layer)) {
      const m = padMaskMargin3d(pad, part, ctx.maskPaste);
      addPadWithMargin(ctx, out, pad, part, [m, m]);
    } else if (layer === "F.Paste" || layer === "B.Paste") {
      const m = padHasCopper(pad) ? pasteMarginOf(pad, part, ctx.maskPaste) : ([0, 0] as Pt);
      addPadWithMargin(ctx, out, pad, part, m);
    } else {
      addPadWithMargin(ctx, out, pad, part, [0, 0]);
    }
  }
}

/** `buildPadOutlineAsSegments`: a pad on a silkscreen layer as its outline, `width` wide (a round pad as a ring). */
function addPadOutline(out: Prim[], pad: Pad, part: Part, width: number): void {
  if (pad.shape === "circle") {
    const r = (pad.size ?? [pad.w, pad.h])[0] / 2;
    out.push({ k: "ring", c: [pad.x, pad.y], ri: r - width / 2, ro: r + width / 2 });
    return;
  }
  const contour = padContours(pad, padPlacement(pad, part))[0];
  if (contour) outlineSegs(out, contour, width);
}

/** `BOARD_ADAPTER::addFootprintShapes( footprint, container, layer, flags )`: its visible fields and its texts on `layer` (as the text rows allow), its text boxes, barcodes and shapes. */
function addFootprintShapes(ctx: Ctx, out: Prim[], part: Part, layer: string, opts: Layers3dOptions, barcodePaths: (b: BoardBarcode) => BarcodePaths | null): void {
  if (opts.fpText) {
    for (const f of part.fields ?? []) {
      if (f.name === "Reference" && !opts.references) continue;
      if (f.name === "Value" && !opts.values) continue;
      if (f.layer === layer && f.visible) tagged(out, f.name === "Reference" ? "part-ref" : f.name === "Value" ? "part-value" : "part-field", () => addText(ctx, out, f));
    }
    for (const t of part.texts ?? []) {
      if (t.role === "reference" && !opts.references) continue;
      if (t.role === "value" && !opts.values) continue;
      if (t.layer === layer && t.text) tagged(out, t.role === "reference" ? "part-ref" : t.role === "value" ? "part-value" : "fp-text", () => addText(ctx, out, artTextAsField(t)));
    }
  }
  for (const b of part.items?.barcodes ?? []) if (b.layer === layer) tagged(out, "barcode", () => addBarcode(out, b, barcodePaths(b)));
  for (const g of graphicsOf(part)) {
    for (const l of layersOf(g)) {
      const s = l === g.layer ? g : { ...g, layer: l };
      if (shapeOnLayer(s, layer)) tagged(out, "fp-shape", () => addShape(ctx, out, s, layer));
    }
  }
}

type BarcodePaths = ReadonlyArray<ReadonlyArray<ReadonlyArray<readonly [number, number]>>>;

/** `zone->GetFilledPolysList( layer )` (`addSolidAreasShapes`): the fill kicad-cli made of the zone on the layer; a zone on no copper layer (`fillNonCopperZone`) its outline when it has no fill. */
function addZone(out: Prim[], zone: Zone, layer: string, fills: readonly FillZone[] | undefined): void {
  if (zone.is_rule_area) return;
  const fill = fills?.find((f) => f.id === zone.id && f.layer === layer);
  if (fill) {
    if (!fill.filled) return;
    for (const ring of fill.fragments) out.push({ k: "poly", contours: [ring.map(([x, y]) => [x, y] as Pt)] });
    return;
  }
  if (!isCopperLayer(layer) || !fills) out.push({ k: "poly", contours: [zone.outline.map(([x, y]) => [x, y] as Pt)] });
}

/** Whether a board layer is in the board (`m_board->IsLayerEnabled`): a copper layer of its stack, a technical or user layer of Board Setup's Board Editor Layers (all of them on an older backend). */
function layerEnabled(board: BoardState, layer: string, layerOrder: readonly string[]): boolean {
  if (isCopperLayer(layer)) return layerOrder.includes(layer);
  const enabled = board.board_rules?.layer_setup?.enabled;
  return !enabled || enabled.length === 0 || enabled.includes(layer);
}

/** `LSET::AllNonCuMask().Seq( { B_Adhes, F_Adhes, .. } )`: the technical layers `createLayers` builds, in its order (the fab and courtyard layers are not 3D rows in the studio). */
export const TECH_LAYERS: readonly string[] = ["B.Adhes", "F.Adhes", "B.Paste", "F.Paste", "B.SilkS", "F.SilkS", "B.Mask", "F.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", ...Array.from({ length: 45 }, (_, i) => `User.${i + 1}`)];

/** The box of a footprint (`FOOTPRINT::GetBoundingBox`): its pads, its graphics and its shown texts. Null for a footprint with none. */
function footprintBox(part: Part): [number, number, number, number] | null {
  let box: [number, number, number, number] | null = null;
  const take = (x: number, y: number) => {
    box = box ? [Math.min(box[0], x), Math.min(box[1], y), Math.max(box[2], x), Math.max(box[3], y)] : [x, y, x, y];
  };
  for (const p of part.pads ?? []) {
    take(p.x - p.w / 2, p.y - p.h / 2);
    take(p.x + p.w / 2, p.y + p.h / 2);
  }
  for (const g of graphicsOf(part)) {
    const segs: Prim[] = [];
    addShape({ board: {} as BoardState, jobs: [], ratios: undefined, maskPaste: undefined }, segs, { ...g, dash: undefined, fill: undefined, filled: false, hatch_lines: undefined }, g.layer);
    for (const s of segs) for (const c of primContours(s)) for (const [x, y] of c) take(x, y);
  }
  for (const f of part.fields ?? []) {
    if (!f.visible) continue;
    const s = fieldStrokes(f);
    for (const [x0, y0, x1, y1] of s.segs) {
      take(x0, y0);
      take(x1, y1);
    }
  }
  return box;
}

// -------------------------------------------------------------------------------------------------------------------------------- the builder

/** What `buildLayers3d` reads besides the board. */
export interface Layers3dInput {
  /** The copper layers in stack order (F.Cu first). */
  layerOrder: readonly string[];
  zoneFills?: readonly FillZone[];
  /** A barcode's polygons once they are in (kicad-port/barcodeShapes.ts `readyPaths`). */
  barcodePaths?: (b: BoardBarcode) => BarcodePaths | null;
}

/** `BOARD_ADAPTER::createLayers` and the hole and post-machining geometry `RENDER_3D_OPENGL::bgWorker` makes from it (`backfillPostMachine`, `generateViaBarrels`, `generateViaCovers`). */
export function buildLayers3d(board: BoardState, input: Layers3dInput, opts: Layers3dOptions): Layers3d {
  const layerOrder = input.layerOrder;
  const mp = board.board_rules?.mask_paste;
  const ctx: Ctx = { board, jobs: [], ratios: dashRatiosOf(board), maskPaste: mp };
  const barcodePaths = input.barcodePaths ?? (() => null);
  const plating = opts.plating;
  const halfPlating = plating / 2;
  const tracks = board.routing?.tracks ?? [];
  const vias = board.routing?.vias ?? [];
  const parts = board.parts.filter((p) => p.placed && p.side);
  const drawings = board.drawings;
  const zones = board.routing?.zones ?? [];
  const out: Layers3d = {
    layers: new Map(),
    thODs: [],
    viaAnnuli: [],
    layerHoles: new Map(),
    offboardPads: { front: [], back: [] },
    bodyCuts: [],
    plugs: [],
    barrels: [],
    barrelVias: new Set(),
    covers: { front: [], back: [] },
    jobs: ctx.jobs,
  };
  const enabled3d = (layer: string) => layerEnabled(board, layer, layerOrder) && opts.layerShown(layer);
  const copperIds = layerOrder.filter(enabled3d);
  const holesOf = (layer: string): Prim[] => {
    let h = out.layerHoles.get(layer);
    if (!h) out.layerHoles.set(layer, (h = []));
    return h;
  };

  // ---- Copper: tracks and via rings (`createTrackWithMargin`), only on the layers shown.
  for (const layer of copperIds) {
    const c: Prim[] = [];
    out.layers.set(layer, c);
    for (const t of tracks) if (t.layer === layer) tagged(c, "track", () => addTrack(c, t, 0));
    for (const v of vias) {
      const flashed = v.flashed ?? viaSpan(v, layerOrder);
      if (viaSpan(v, layerOrder).includes(layer) && flashed.includes(layer)) c.push({ k: "circle", c: [v.x, v.y], r: viaWidth(v, layer, layerOrder) / 2, tag: "via" });
    }
  }

  // ---- The holes of the vias (`m_TH_ODs`, `m_viaAnnuli`, `m_layerHoleMap`): a through via's once, a blind one on each of its layers; a post-machining's and a backdrill's.
  const first = copperIds[0];
  const last = copperIds[copperIds.length - 1];
  for (const v of vias) {
    const r = v.drill / 2;
    const span = viaSpan(v, layerOrder);
    const capped = v.props?.capping === true;
    for (const layer of copperIds) {
      if (!span.includes(layer)) continue;
      if (v.type) holesOf(layer).push({ k: "circle", c: [v.x, v.y], r: r + halfPlating });
      else if (layer === first) {
        // "if( !capped || !( IsFrontLayer( layer ) || IsBackLayer( layer ) ) )"
        if (!capped || !(layer === "F.Cu" || layer === "B.Cu")) out.thODs.push({ k: "circle", c: [v.x, v.y], r: r + halfPlating });
        if (opts.clipSilkOnViaAnnuli && v.d > 0) out.viaAnnuli.push({ k: "circle", c: [v.x, v.y], r: viaWidth(v, layer, layerOrder) / 2 });
      }
      for (const [pm, front] of [[v.props?.front_post_machining, true], [v.props?.back_post_machining, false]] as const) {
        if (!pm || pm.size / 2 <= r) continue;
        // "only add once for front layer" (back: the last); every layer of the via gets the hole
        if (front ? layer === first : layer === last) out.bodyCuts.push(circlePolygon([v.x, v.y], pm.size / 2, MAX_ERROR_NM));
        holesOf(layer).push({ k: "circle", c: [v.x, v.y], r: pm.size / 2 + halfPlating });
      }
      for (const d of [v.props?.backdrill, v.props?.tertiary_drill]) {
        if (!d || d.size / 2 <= r + halfPlating) continue;
        const a = layerOrder.indexOf(d.start);
        const b = layerOrder.indexOf(d.end);
        if (a < 0 || b < 0) continue;
        const i = layerOrder.indexOf(layer);
        if (i >= Math.min(a, b) && i <= Math.max(a, b)) holesOf(layer).push({ k: "circle", c: [v.x, v.y], r: d.size / 2 });
      }
    }
    // `m_SecondarydrillPolys` / `m_TertiarydrillPolys`, out of the body: "backdrillOuterRadiusBIU > holeOuterRadiusBIU", on the drill's layers.
    for (const d of [v.props?.backdrill, v.props?.tertiary_drill]) if (d && d.size / 2 + plating > r + plating && layerOrder.includes(d.start) && layerOrder.includes(d.end)) out.bodyCuts.push(circlePolygon([v.x, v.y], d.size / 2 + plating, MAX_ERROR_NM));
  }

  // ---- The holes of the pads.
  for (const part of parts) {
    for (const pad of part.pads ?? []) {
      const seg = padHoleSegment(pad, part);
      if (!seg) continue;
      if (pad.kind === "non_plated_hole") {
        // "Ensure the silk drawings are clipped to the NPTH hole, like other pad/via holes"
        const p = padHolePrim(pad, part, 0);
        if (p) out.thODs.push(p);
        continue;
      }
      const inflate = kiRound(plating / 2);
      const p = padHolePrim(pad, part, inflate);
      if (p) {
        out.thODs.push(p);
        if (opts.clipSilkOnViaAnnuli) out.viaAnnuli.push(p);
      }
      const dp = pad.drill_props;
      const holeRadius = kiRound((pad.slot ? (pad.slot[0] + pad.slot[1]) / 2 : (pad.drill ?? 0)) / 2);
      const centre: Pt = [pad.px ?? pad.x, pad.py ?? pad.y];
      for (const pm of [dp?.front_post_machining, dp?.back_post_machining]) if (pm && Math.trunc(pm.size / 2) > holeRadius) out.bodyCuts.push(circlePolygon(centre, Math.trunc(pm.size / 2), MAX_ERROR_NM));
      for (const d of [dp?.backdrill, dp?.tertiary_drill]) {
        if (!d) continue;
        const radius = Math.trunc((d.size + d.size) / 4);
        const a = layerOrder.indexOf(d.start);
        const b = layerOrder.indexOf(d.end);
        if (radius <= holeRadius + inflate || a < 0 || b < 0) continue;
        out.bodyCuts.push(circlePolygon(centre, radius, MAX_ERROR_NM));
        for (const layer of layerOrder.slice(Math.min(a, b), Math.max(a, b) + 1)) if (copperIds.includes(layer)) holesOf(layer).push({ k: "circle", c: centre, r: radius });
      }
    }
  }

  // ---- Copper: the footprints' pads, texts and shapes (`addPads`, `addFootprintShapes`), the board's drawings on copper, the zones.
  for (const layer of copperIds) {
    const c = out.layers.get(layer)!;
    for (const part of parts) {
      addPads(ctx, c, part, layer, layerOrder);
      addFootprintShapes(ctx, c, part, layer, opts, barcodePaths);
    }
    for (const s of drawings?.shapes ?? []) if (shapeOnLayer(s, layer)) tagged(c, "shape", () => addShape(ctx, c, s, layer));
    for (const t of drawings?.texts ?? []) if (t.layer === layer) tagged(c, "text", () => addBoardText(ctx, c, t));
    for (const b of drawings?.barcodes ?? []) if (b.layer === layer) tagged(c, "barcode", () => addBarcode(c, b, barcodePaths(b)));
    if (opts.showZones) for (const z of zones) if (zoneLayers(z).includes(layer)) tagged(c, "zone-fill", () => addZone(c, z, layer, input.zoneFills));
  }

  // ---- The technical layers (`techLayerList`); the masks are built for the silkscreen and the plated copper even when hidden.
  for (const layer of TECH_LAYERS) {
    const forced = isMaskLayer(layer) && (opts.subtractMaskFromSilk || opts.differentiatePlatedCopper) && layerEnabled(board, layer, layerOrder);
    if (!enabled3d(layer) && !forced) continue;
    const c: Prim[] = [];
    out.layers.set(layer, c);
    for (const s of drawings?.shapes ?? []) if (shapeOnLayer(s, layer)) tagged(c, "shape", () => addShape(ctx, c, s, layer));
    for (const t of drawings?.texts ?? []) if (t.layer === layer) tagged(c, "text", () => addBoardText(ctx, c, t));
    for (const b of drawings?.barcodes ?? []) if (b.layer === layer) tagged(c, "barcode", () => addBarcode(c, b, barcodePaths(b)));
    if (isMaskLayer(layer)) {
      const copper = layer === "F.Mask" ? "F.Cu" : "B.Cu";
      for (const t of tracks) if (t.mask && t.layer === copper) tagged(c, "track", () => addTrack(c, t, trackMaskExpansion(t, mp)));
      for (const v of vias) {
        // `PCB_VIA::IsOnLayer( F_Mask )`: "a via on a mask layer if not tented and if it is on the corresponding external copper layer", and flashed there.
        const span = viaSpan(v, layerOrder);
        const flashed = v.flashed ?? span;
        if (!span.includes(copper) || !flashed.includes(copper) || viaTented(v, layer === "F.Mask", mp)) continue;
        c.push({ k: "circle", c: [v.x, v.y], r: (viaWidth(v, copper, layerOrder) + 2 * (mp?.expansion_nm ?? 0)) / 2, tag: "via" });
      }
    }
    for (const part of parts) {
      if (isSilkLayer(layer)) {
        // `m_LineThickness[ LAYER_CLASS_SILK ]`
        const width = board.board_rules?.text_graphics?.silk.line_width_nm ?? 100_000;
        for (const pad of part.pads ?? []) if (padOnTechLayer(pad, part, layer)) tagged(c, "pad-outline", () => addPadOutline(c, pad, part, width));
      } else {
        tagged(c, "pad", () => addPads(ctx, c, part, layer, layerOrder));
      }
      addFootprintShapes(ctx, c, part, layer, opts, barcodePaths);
    }
    if (opts.showZones) for (const z of zones) if (zoneLayers(z).includes(layer)) tagged(c, "zone-fill", () => addZone(c, z, layer, input.zoneFills));
  }

  // ---- The off-board pads (`show_off_board_silk`): a footprint whose box misses the board's.
  if (opts.showOffBoardSilk) {
    let bb: [number, number, number, number] | null = null;
    for (const p of boardOutlinePolygons(board)) for (const [x, y] of p.outer) bb = bb ? [Math.min(bb[0], x), Math.min(bb[1], y), Math.max(bb[2], x), Math.max(bb[3], y)] : [x, y, x, y];
    for (const part of parts) {
      const fb = footprintBox(part);
      const off = !bb || !fb || fb[2] < bb[0] || fb[0] > bb[2] || fb[3] < bb[1] || fb[1] > bb[3];
      if (!off) continue;
      const bottom = part.side === "bottom";
      addPads(ctx, bottom ? out.offboardPads.back : out.offboardPads.front, part, bottom ? "B.Cu" : "F.Cu", layerOrder);
    }
  }

  buildPostMachining(out, board, layerOrder, opts, parts);
  return out;
}

/**
 * `backfillPostMachine` (the plugs of board material: under a counterbore or countersink down to the far face, beside a backdrill), `generateViaBarrels` (a via's plated
 * barrel cut short by its backdrill and its post-machining, and the plating of the counterbore or countersink, `appendPostMachiningGeometry`) and `generateViaCovers` (the
 * covering disk or dimple of a covered or tented via, drawn with the mask).
 */
function buildPostMachining(out: Layers3d, board: BoardState, layerOrder: readonly string[], opts: Layers3dOptions, parts: readonly Part[]): void {
  const plating = opts.plating;
  const mp = board.board_rules?.mask_paste;
  const F = layerOrder[0] ?? "F.Cu";
  const B = layerOrder[layerOrder.length - 1] ?? "B.Cu";
  for (const v of board.routing?.vias ?? []) {
    const p = v.props;
    const c: Pt = [v.x, v.y];
    const ri = v.drill / 2;
    const ro = ri + plating;
    const span = viaSpan(v, layerOrder);
    const top = span[0] ?? F;
    const bottom = span[span.length - 1] ?? B;
    const backdrill = p?.backdrill;
    const tertiary = p?.tertiary_drill;
    const r2 = (backdrill?.size ?? 0) / 2;
    const r3 = (tertiary?.size ?? 0) / 2;
    if (r2 > ro || r3 > ro) {
      const plugEnd = r2 > ro && backdrill ? backdrill.end : B;
      const plugStart = r3 > ro && tertiary ? tertiary.start : F;
      out.plugs.push({ c, ri: ro, ro: Math.max(r2, r3), top: { layer: plugStart, face: "top" }, bottom: { layer: plugEnd, face: "bottom" } });
    }
    const pmF = p?.front_post_machining;
    const pmB = p?.back_post_machining;
    if (pmF && pmF.size / 2 > ro && pmF.depth > 0) out.plugs.push({ c, ri: ro, ro: pmF.size / 2, top: { layer: top, face: "top", depth: pmF.depth }, bottom: { layer: bottom, face: "bottom" }, ...(pmF.mode === "countersink" ? { coneDdeg: pmF.angle_ddeg } : {}) });
    if (pmB && pmB.size / 2 > ro && pmB.depth > 0) out.plugs.push({ c, ri: ro, ro: pmB.size / 2, top: { layer: top, face: "top" }, bottom: { layer: bottom, face: "bottom", depth: pmB.depth }, ...(pmB.mode === "countersink" ? { coneDdeg: pmB.angle_ddeg } : {}) });
    // `generateViaBarrels`: a through via with no backdrill and no post-machining is a plated hole shell instead.
    const special = !!backdrill || !!pmF || !!pmB;
    if (!v.type && !special) continue;
    out.barrelVias.add(v.id);
    let topRef: ZRef = { layer: top, face: "top" };
    let bottomRef: ZRef = { layer: bottom, face: "bottom" };
    if (backdrill) bottomRef = { layer: backdrill.end, face: "top" };
    if (tertiary) topRef = { layer: tertiary.end, face: "bottom" };
    for (const [pm, front] of [[pmF, true], [pmB, false]] as const) {
      if (!pm || pm.size / 2 <= ri || pm.depth <= 0 || !opts.plating) continue;
      const surface: ZRef = front ? { layer: top, face: "top" } : { layer: bottom, face: "bottom" };
      const end: ZRef = { ...surface, depth: pm.depth };
      if (pm.mode === "counterbore") out.barrels.push({ c, ri: pm.size / 2, ro: pm.size / 2 + plating, top: front ? surface : end, bottom: front ? end : surface });
      else out.barrels.push({ c, ri, ro: pm.size / 2, top: front ? surface : end, bottom: front ? end : surface, coneWall: plating, wideAt: front ? "top" : "bottom" });
      if (front) topRef = end;
      else bottomRef = end;
    }
    out.barrels.push({ c, ri, ro, top: topRef, bottom: bottomRef });
  }
  // The pads' post-machining plating and plugs (`backfillPostMachine`'s and `generateViaBarrels`' pad loops: a round hole only).
  for (const part of parts) {
    for (const pad of part.pads ?? []) {
      if (pad.slot || !pad.drill) continue;
      const dp = pad.drill_props;
      const c: Pt = [pad.px ?? pad.x, pad.py ?? pad.y];
      const ri = pad.drill / 2;
      const ro = ri + plating;
      for (const [pm, front] of [[dp?.front_post_machining, true], [dp?.back_post_machining, false]] as const) {
        if (!pm || pm.depth <= 0) continue;
        if (pm.size / 2 > ro) {
          out.plugs.push(front ? { c, ri: ro, ro: pm.size / 2, top: { layer: F, face: "top", depth: pm.depth }, bottom: { layer: B, face: "bottom" }, ...(pm.mode === "countersink" ? { coneDdeg: pm.angle_ddeg } : {}) } : { c, ri: ro, ro: pm.size / 2, top: { layer: F, face: "top" }, bottom: { layer: B, face: "bottom", depth: pm.depth }, ...(pm.mode === "countersink" ? { coneDdeg: pm.angle_ddeg } : {}) });
        }
        if (pad.kind === "non_plated_hole" || pm.size / 2 <= ri) continue;
        const surface: ZRef = front ? { layer: F, face: "top" } : { layer: B, face: "bottom" };
        const end: ZRef = { ...surface, depth: pm.depth };
        if (pm.mode === "counterbore") out.barrels.push({ c, ri: pm.size / 2, ro: pm.size / 2 + plating, top: front ? surface : end, bottom: front ? end : surface });
        else out.barrels.push({ c, ri, ro: pm.size / 2, top: front ? surface : end, bottom: front ? end : surface, coneWall: plating, wideAt: front ? "top" : "bottom" });
      }
    }
  }
  // `generateViaCovers`
  for (const v of board.routing?.vias ?? []) {
    const p = v.props;
    const r = v.drill / 2 + 2 * plating;
    const depth = r * 0.3;
    const filled = p?.filling === true || p?.capping === true;
    const frontCovering = p?.front_covering === true || viaTented(v, true, mp);
    const backCovering = p?.back_covering === true || viaTented(v, false, mp);
    const frontBackdrill = p?.backdrill?.start === "F.Cu";
    const backBackdrill = p?.backdrill?.start === "B.Cu";
    if (frontCovering && !p?.front_post_machining && !frontBackdrill) out.covers.front.push({ c: [v.x, v.y], r, depth: filled || !p?.front_plugging ? 0 : depth });
    if (backCovering && !p?.back_post_machining && !backBackdrill) out.covers.back.push({ c: [v.x, v.y], r, depth: filled || !p?.back_plugging ? 0 : depth });
  }
}

/** For the tests: how many primitives of each kind a list holds. */
export function primCounts(list: readonly Prim[]): Record<string, number> {
  const out: Record<string, number> = {};
  for (const p of list) out[p.k] = (out[p.k] ?? 0) + 1;
  return out;
}
