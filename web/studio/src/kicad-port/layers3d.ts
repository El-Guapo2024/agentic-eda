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
import type { BoardState, BoardTable, BoardText, Dimension, FieldInfo, FillZone, MaskPaste, Pad, Part, Shape, Track, Via, Zone } from "../api/types";
import type { BoardBarcode, BoardTextBox } from "../api/pcbItemTypes";
import { bezierPolyline } from "./bezierPoly";
import { endingInks, hasEndings, kicadArc, shortenArc, shortenBezier, shortenSegment } from "./lineEnding";
import { DEFAULT_DASH_RATIOS, dashPieces, isDashStyle, strokeArc, strokeSegment, type DashPiece, type DashRatios, type DashStyle } from "./strokeDash";
import { fieldStrokes } from "./textHit";
import { boardTextAsField } from "./textCoverage";
import { frameToWorld, knockoutBox, layoutKicadText, layoutLineAt, layoutSegments, resolveFont, runGlyphs, runInk, type KicadTextLayout } from "./kicadText";
import { dimensionTextSpec, fieldSpec } from "./textSpecs";
import { textBoxKicadLayout } from "./pcbTextBox";
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
  /** The custom rules' margins, when the board's rules have such constraints. */
  rules?: RuleMargins | null;
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
function addTrack(out: Prim[], t: Pick<Track, "pts" | "width">, margin: number | ((segment: number) => number)): void {
  const w = (i: number) => t.width + (typeof margin === "number" ? margin : margin(i)) * 2;
  if (t.pts.length === 1) out.push({ k: "seg", a: t.pts[0]!, b: t.pts[0]!, w: w(0) });
  for (let i = 0; i + 1 < t.pts.length; i++) out.push({ k: "seg", a: t.pts[i]!, b: t.pts[i + 1]!, w: w(i) });
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

/** `PCB_SHAPE::GetSolderMaskExpansion`: the custom rules' (`rule`, not undefined when the rules have the kind: their opt, none being 0), else the shape's own margin, else the
 *  board's; "Ensure the resulting mask opening has a non-negative size" of a shape that is not filled. */
function shapeMaskExpansion(s: Pick<Shape, "solder_mask_margin" | "stroke_width" | "filled">, mp: MaskPaste | undefined, rule?: number | null): number {
  let margin = rule !== undefined ? (rule ?? 0) : (s.solder_mask_margin ?? mp?.expansion_nm ?? 0);
  if (margin < 0 && !s.filled) margin = Math.max(margin, -s.stroke_width / 2);
  return margin;
}

/** `PCB_TRACK::GetSolderMaskExpansion`: the custom rules' (as above), else the track's own margin, else the board's, "ensure the resulting mask opening has a non-negative size". */
function trackMaskExpansion(t: Track, mp: MaskPaste | undefined, rule?: number | null): number {
  let margin = rule !== undefined ? (rule ?? 0) : (t.mask?.margin ?? mp?.expansion_nm ?? 0);
  if (margin < 0) margin = Math.max(margin, -t.width / 2);
  return margin;
}

/**
 * `GET /api/board3d`'s `rule_margins` (crates/cli/src/board3d_api.rs `rule_margins`): what the board's custom rules make of the mask expansions and paste margins (each
 * constraint's opt, or null), for the kinds the rules have: a pad's by its id (`REF.NUM`, `REF.NUM#k`) per side, a track's per segment, a board shape's by its id, a
 * footprint's graphics in their order.
 */
export interface RuleMargins {
  mask: boolean;
  paste_abs: boolean;
  paste_rel: boolean;
  pads: Record<string, { mask: [number | null, number | null] | null; abs: [number | null, number | null] | null; rel: [number | null, number | null] | null }>;
  tracks: Record<string, Array<number | null>>;
  shapes: Record<string, number | null>;
  fp_shapes: Record<string, Array<number | null>>;
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

/** An arc's body once its endings have taken their part (`CalcArcAngles`, `ShortenArcForEndings`): its centre, its first point and its sweep in degrees; null when nothing is left. */
function arcBody(s: Extract<Shape, { kind: "arc" }>, k: NonNullable<ReturnType<typeof kicadArc>>): { c: Pt; from: Pt; sweepDeg: number } | null {
  const c: Pt = [k.centre[0], k.centre[1]];
  const r = Math.hypot(k.start[0] - c[0], k.start[1] - c[1]);
  const a0 = (Math.atan2(k.start[1] - c[1], k.start[0] - c[0]) * 180) / Math.PI;
  let a1 = (Math.atan2(k.end[1] - c[1], k.end[0] - c[0]) * 180) / Math.PI;
  if (a1 === a0) a1 = a0 + 360;
  while (a1 < a0) a1 += 360;
  const body = shortenArc(a0, a1 - a0, r, s.start_ending, s.end_ending, s.stroke_width);
  if (!body) return null;
  const from: Pt = body[0] === a0 ? [k.start[0], k.start[1]] : [c[0] + r * Math.cos((body[0] * Math.PI) / 180), c[1] + r * Math.sin((body[0] * Math.PI) / 180)];
  return { c, from, sweepDeg: body[1] };
}

/** A curve's body as KiCad 25ec2335 flattens it (`ShortenedBezierPolyline`): what its endings leave, at the board's error. */
function bezierBody(s: Extract<Shape, { kind: "bezier" }>): Pt[] {
  const b = shortenBezier({ start: s.start, c1: s.c1, c2: s.c2, end: s.end }, s.start_ending, s.end_ending, s.stroke_width);
  return b ? bezierPolyline(b.start, b.c1, b.c2, b.end, MAX_ERROR_NM) : [];
}

/**
 * `BOARD_ADAPTER::addShape( const PCB_SHAPE*, .., aLayer )`: a shape on `layer`. On a mask layer, a shape on outer copper with a solder mask opening is grown by its expansion,
 * solid and filled if hatched; a solid line is drawn as round segments of its width (a circle as a ring, an arc in `createArcSegments`' pieces); a dashed one as
 * `STROKE_PARAMS::Stroke` cuts it (kicad-port/strokeDash.ts) with Board Setup's ratios; a filled shape as its polygon, without its line width when it is dashed; a
 * hatched one with its hatch lines; a line, an arc or a curve shortened for its endings and the endings drawn (KiCad 25ec2335).
 */
export function addShape(ctx: Ctx, out: Prim[], s: Shape, layer: string, rule?: number | null): void {
  let linewidth = s.stroke_width;
  let margin = 0;
  let dashed = isDashStyle(s.dash);
  let solidFill = s.filled;
  let hatched = !!s.fill && !s.filled;
  if (isMaskLayer(layer) && s.solder_mask && (s.layer === "F.Cu" || s.layer === "B.Cu")) {
    margin = shapeMaskExpansion(s, ctx.maskPaste, rule);
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
        // KiCad 25ec2335: `ShortenArcForEndings( startAngle, arcAngle, radius, linewidth )`, the start turned by what the start ending took
        const body = arcBody(s, k);
        if (body) arcSegs(out, body.c, body.from, body.sweepDeg, linewidth);
        break;
      }
      case "segment": {
        // KiCad 25ec2335: `EDA_SHAPE::ShortenSegmentForEndings( segStart, segEnd, startEnding, endEnding, linewidth )`
        const body = shortenSegment(s.start, s.end, s.start_ending, s.end_ending, s.stroke_width);
        if (body) out.push({ k: "seg", a: body[0], b: body[1], w: linewidth });
        break;
      }
      case "bezier": {
        // KiCad 25ec2335: `ShortenedBezierPolyline( linewidth )`, `BEZIER_POLY::GetPoly( pts, std::max( 1, getMaxError() ) )`
        const pts = bezierBody(s);
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
    // `STROKE_PARAMS::Stroke` of `MakeEffectiveShapesForStroking( linewidth )`: the body the endings leave (KiCad 25ec2335)
    const ratios = ctx.ratios ?? DEFAULT_DASH_RATIOS;
    const style = s.dash as DashStyle;
    const pieces: DashPiece[] = [];
    if (s.kind === "segment" && hasEndings(s)) {
      const body = shortenSegment(s.start, s.end, s.start_ending, s.end_ending, s.stroke_width);
      if (body) strokeSegment(body[0], body[1], style, s.stroke_width, ratios, pieces);
    } else if (s.kind === "arc" && hasEndings(s)) {
      const k = kicadArc(s.start, s.mid, s.end);
      const body = k ? arcBody(s, k) : null;
      if (body) strokeArc(body.c, body.from, body.sweepDeg, style, s.stroke_width, ratios, pieces);
    } else if (s.kind === "bezier") {
      const pts = bezierBody(s);
      for (let i = 1; i < pts.length; i++) strokeSegment(pts[i - 1]!, pts[i]!, style, s.stroke_width, ratios, pieces);
    } else pieces.push(...dashPieces(s, { style, width: s.stroke_width, ratios }, MAX_ERROR_NM));
    for (const [a, b] of pieces) out.push({ k: "seg", a, b, w: linewidth });
  }
  // KiCad 25ec2335's line endings (`LINE_ENDING::GetShapes` at `GetLineEndingEndpoints`, turned by `GetEndingTangents`): an open arrow as round segments of its own stroke
  // or the line's, a closed one as its polygon, outlined with round segments when it has a stroke of its own.
  if ((s.kind === "segment" || s.kind === "arc" || s.kind === "bezier") && hasEndings(s)) {
    for (const ink of endingInks(s)) {
      if (!ink.fill) {
        for (let i = 0; i + 1 < ink.pts.length; i++) out.push({ k: "seg", a: ink.pts[i]!, b: ink.pts[i + 1]!, w: ink.stroke });
        continue;
      }
      if (ink.stroke > 0) outlineSegs(out, ink.pts, ink.stroke);
      out.push({ k: "poly", contours: [ink.pts] });
    }
  }
  // `ConvertPolygonToTriangles( aShape->GetHatching() )`: the hatch lines `UpdateHatching` made, each its width wide.
  if (hatched && s.hatch_lines) {
    const w = s.hatch_width ?? s.stroke_width;
    for (const [x1, y1, x2, y2] of s.hatch_lines) out.push({ k: "seg", a: [x1, y1], b: [x2, y2], w });
  }
}

/**
 * `BOARD_ADAPTER::addText` of a laid-out text (kicad-port/kicadText.ts `layoutKicadText`): a stroke-font text's pen strokes as round segments of its pen width
 * (`CALLBACK_GAL`'s stroke callback); an outline font's glyphs as their polygons (the triangulation callback: `OUTLINE_GLYPH::Triangulate`, kicad-port/sfnt.ts
 * `sfntGlyphPolys`), a run's ink box only while its face's file is not read; a knockout text (`knockout`: its size) the box of what it draws grown by
 * `GetKnockoutTextMargin` less its strokes and glyphs (`PCB_TEXT::TransformTextToPolySet`), the server's subtraction.
 */
export function addTextLayout(ctx: Ctx, out: Prim[], l: KicadTextLayout, knockout?: readonly [number, number]): void {
  const segs = layoutSegments(l);
  const glyphs: Array<{ outer: Contour; holes: Contour[] }> = [];
  for (const r of l.runs) {
    const polys = runGlyphs(l, r);
    if (polys) {
      for (const g of polys) glyphs.push({ outer: g.outer.map(([x, y]) => frameToWorld(l, x, y)), holes: g.holes.map((h) => h.map(([x, y]) => frameToWorld(l, x, y))) });
      continue;
    }
    const ink = runInk(l, r);
    if (ink) glyphs.push({ outer: [frameToWorld(l, ink.x0, ink.y0), frameToWorld(l, ink.x1, ink.y0), frameToWorld(l, ink.x1, ink.y1), frameToWorld(l, ink.x0, ink.y1)], holes: [] });
  }
  if (knockout) {
    const box = knockoutBox(l, knockout);
    if (!box) return;
    const ring: Contour = [frameToWorld(l, box.x0, box.y0), frameToWorld(l, box.x1, box.y0), frameToWorld(l, box.x1, box.y1), frameToWorld(l, box.x0, box.y1)];
    const strokes = segs.map(([x0, y0, x1, y1]) => ({ outer: x0 === x1 && y0 === y1 ? circlePolygon([x0, y0], l.pen / 2, MAX_ERROR_NM) : ovalPolygon([x0, y0], [x1, y1], l.pen, MAX_ERROR_NM), holes: [] as Contour[] }));
    out.push(job(ctx, { a: [{ outer: ring, holes: [] }], b: [...strokes, ...glyphs] }));
    return;
  }
  for (const [x0, y0, x1, y1] of segs) out.push({ k: "seg", a: [x0, y0], b: [x1, y1], w: l.pen });
  for (const g of glyphs) out.push({ k: "poly", contours: [g.outer, ...g.holes] });
}

/** `addText` of a footprint's field or text, or a board text made one (`FieldInfo`): its spec (kicad-port/textSpecs.ts `fieldSpec`) laid out in its face. */
export function addText(ctx: Ctx, out: Prim[], f: FieldInfo): void {
  const { spec, face } = fieldSpec(f);
  addTextLayout(ctx, out, layoutKicadText(spec, resolveFont(face, spec.bold, spec.italic)), f.knockout ? spec.size : undefined);
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

/** The board's text boxes, tables and dimensions on `layer` (`createLayers`' `PCB_TEXTBOX_T`, `PCB_TABLE_T` and `PCB_DIM_*` cases, on copper and on the technical layers). */
function addBoardItems(ctx: Ctx, out: Prim[], board: BoardState, layer: string): void {
  for (const b of board.drawings?.text_boxes ?? []) if (b.layer === layer) tagged(out, "text-box", () => addTextBox(ctx, out, b, true));
  for (const t of board.drawings?.tables ?? []) if (t.layer === layer) tagged(out, "table", () => addTable(ctx, out, t));
  for (const d of (board.drawings?.dimensions ?? []) as Dimension[]) if (d.layer === layer) tagged(out, "dimension", () => addDimension(ctx, out, d));
}

/**
 * `BOARD_ADAPTER::addShape( const PCB_TEXTBOX*, .., aOwner )`: its text (`addText( aTextBox, aContainer, aOwner )`: a knockout box of the board -- its own owner -- is the box,
 * with its border, less the text, `PCB_TEXTBOX::TransformTextToPolySet`; a footprint's box is owned by the footprint, never a knockout), then its border when it has one: a
 * rectangle through `addShape( PCB_SHAPE )` (its line style kept), a turned box's outline as its strokes (`PCB_SHAPE::TransformShapeToPolygon`).
 */
function addTextBox(ctx: Ctx, out: Prim[], b: BoardTextBox, ownedByItself: boolean): void {
  const { layout } = textBoxKicadLayout(b);
  const outline = textBoxOutline(b);
  const width = b.stroke_width;
  if (ownedByItself && b.knockout) {
    // `TransformShapeToPolygon` of the box: its corners and, with a border, each side as an oval of the line's width
    const a: Array<{ outer: Contour; holes: Contour[] }> = [{ outer: outline, holes: [] }];
    if (b.border && width > 0) for (let i = 0; i < outline.length; i++) a.push({ outer: ovalPolygon(outline[i]!, outline[(i + 1) % outline.length]!, width, MAX_ERROR_NM), holes: [] });
    const glyphs: Prim[] = [];
    addTextLayout(ctx, glyphs, layout);
    const b2 = glyphs.flatMap((p) => (p.k === "poly" ? [{ outer: p.contours[0]!, holes: p.contours.slice(1) }] : primContours(p).map((c) => ({ outer: c, holes: [] as Contour[] }))));
    out.push(job(ctx, { a, b: b2 }));
  } else if ((b.shown ?? b.text) !== "") addTextLayout(ctx, out, layout);
  if (!b.border) return;
  if (b.pts.length === 0) {
    const style = b.line_style === "default" ? undefined : b.line_style;
    addShape(ctx, out, { id: b.id, kind: "rect", layer: b.layer, start: b.start, end: b.end, stroke_width: width, filled: false, ...(style && style !== "solid" ? { dash: style } : {}) } as Shape, "");
  } else if (width > 0) outlineSegs(out, outline, width);
}

/** A text box's outline: its corners (`GetRectCorners`), or the four of a box turned by an angle that is no multiple of 90 degrees. */
function textBoxOutline(b: Pick<BoardTextBox, "start" | "end" | "pts">): Contour {
  if (b.pts.length >= 3) return b.pts.map(([x, y]) => [x, y] as Pt);
  return rectCorners({ start: b.start, end: b.end });
}

/** `BOARD_ADAPTER::addShape( const PCB_DIMENSION_BASE* )`: its text, then each of its shapes -- a segment as a round segment of the line's width, a circle as a ring. */
function addDimension(ctx: Ctx, out: Prim[], d: Dimension): void {
  if (d.text) {
    const { spec, face } = dimensionTextSpec(d);
    addTextLayout(ctx, out, layoutKicadText(spec, resolveFont(face, spec.bold, spec.italic)));
  }
  const w = d.stroke_width;
  for (const [a, b] of d.lines) out.push(a[0] === b[0] && a[1] === b[1] ? { k: "circle", c: a, r: w / 2 } : { k: "seg", a, b, w });
  for (const [c, r] of d.circles ?? []) {
    const ri = r - w / 2;
    const ro = r + w / 2;
    // `addRING_2D`: "if( aOuterRadius > aInnerRadius && aInnerRadius > 0.0f )"
    if (ro > ri && ri > 0) out.push({ k: "ring", c, ri, ro });
  }
}

/**
 * `BOARD_ADAPTER::addTable`: `DrawBorders`' lines as round segments of their stroke's width (solid: the 3D view does not dash a table), then every cell another does not
 * cover, its text (`addText( cell, .. )`) laid out as the canvas lays it out (components/canvas/tablePaint.ts: the server's lines in the stroke font, `GetDrawPos` in an
 * outline face).
 */
function addTable(ctx: Ctx, out: Prim[], t: BoardTable): void {
  for (const b of t.borders) {
    if (b.width < 0) continue;
    out.push({ k: "seg", a: b.a, b: b.b, w: b.width });
  }
  for (const c of t.cells) {
    if (c.col_span === 0 || c.row_span === 0) continue;
    const font = resolveFont(c.face, c.bold, c.italic);
    const size: [number, number] = [c.size_w || c.size_h, c.size_h];
    const look = { size, pen: c.pen, angleDeg: t.angle / 1000, mirror: c.mirror, italic: c.italic, bold: c.bold };
    if (font.kind === "outline" && c.draw_pos) {
      addTextLayout(ctx, out, layoutKicadText({ ...look, text: c.shown ?? c.text, pos: c.draw_pos, halign: c.h_align, valign: c.v_align, lineSpacing: (c.line_spacing || 1000) / 1000 }, font));
      continue;
    }
    for (const line of c.lines) if (line.text) addTextLayout(ctx, out, layoutLineAt(line.text, [line.x, line.y], look));
  }
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
  // the studio's pad ids (kicad-port/pcbItems.ts `padIds`): what the custom rules' margins are keyed by
  const seen = new Map<string, number>();
  const ids = (part.pads ?? []).map((pad) => {
    const n = (seen.get(pad.num) ?? 0) + 1;
    seen.set(pad.num, n);
    return n === 1 ? `${part.ref}.${pad.num}` : `${part.ref}.${pad.num}#${n}`;
  });
  const side = layer.startsWith("B.") ? 1 : 0;
  for (const [pi, pad] of (part.pads ?? []).entries()) {
    const rule = ctx.rules?.pads[ids[pi]!];
    if (isCopperLayer(layer)) {
      const layers = Array.isArray(pad.stack_layers) ? pad.stack_layers.flatMap((g) => g.layers) : padCopperLayers(pad, part, layerOrder);
      if (!layers.includes(layer)) continue;
      const contours = padCopperOn(pad, part, layer);
      if (contours && contours.length > 0) out.push({ k: "poly", contours, tag: "pad", meta: { padShape: pad.shape ?? (pad.round ? "circle" : "rect"), corners: contours.reduce((n, c) => n + c.length, 0) } });
      continue;
    }
    if (!padOnTechLayer(pad, part, layer)) continue;
    if (isMaskLayer(layer)) {
      const m = padMaskMargin3d(pad, part, ctx.maskPaste, ctx.rules?.mask ? (rule?.mask?.[side] ?? null) : undefined);
      addPadWithMargin(ctx, out, pad, part, [m, m]);
    } else if (layer === "F.Paste" || layer === "B.Paste") {
      const m = padHasCopper(pad) ? pasteMarginOf(pad, part, ctx.maskPaste, { abs: ctx.rules?.paste_abs ? rule?.abs?.[side] : null, rel: ctx.rules?.paste_rel ? rule?.rel?.[side] : null }) : ([0, 0] as Pt);
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
  for (const b of part.items?.text_boxes ?? []) if (b.layer === layer) tagged(out, "text-box", () => addTextBox(ctx, out, b, false));
  for (const t of part.items?.tables ?? []) if (t.layer === layer) tagged(out, "table", () => addTable(ctx, out, t));
  for (const d of part.items?.dimensions ?? []) if (d.layer === layer) tagged(out, "dimension", () => addDimension(ctx, out, d));
  for (const b of part.items?.barcodes ?? []) if (b.layer === layer) tagged(out, "barcode", () => addBarcode(out, b, barcodePaths(b)));
  for (const [gi, g] of graphicsOf(part).entries()) {
    for (const l of layersOf(g)) {
      const s = l === g.layer ? g : { ...g, layer: l };
      if (shapeOnLayer(s, layer)) tagged(out, "fp-shape", () => addShape(ctx, out, s, layer, ctx.rules?.mask ? (ctx.rules.fp_shapes[part.ref]?.[gi] ?? null) : undefined));
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

/** `techLayerList`: the technical layers `createLayers` builds, in its order (KiCad 25ec2335: the fab and courtyard layers after the four named user layers). */
export const TECH_LAYERS: readonly string[] = ["B.Adhes", "F.Adhes", "B.Paste", "F.Paste", "B.SilkS", "F.SilkS", "B.Mask", "F.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "F.Fab", "B.Fab", "F.CrtYd", "B.CrtYd", ...Array.from({ length: 45 }, (_, i) => `User.${i + 1}`)];

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
  /** `GET /api/board3d`'s `rule_margins`, once it has answered. */
  ruleMargins?: RuleMargins | null;
}

/** `BOARD_ADAPTER::createLayers` and the hole and post-machining geometry `RENDER_3D_OPENGL::bgWorker` makes from it (`backfillPostMachine`, `generateViaBarrels`, `generateViaCovers`). */
export function buildLayers3d(board: BoardState, input: Layers3dInput, opts: Layers3dOptions): Layers3d {
  const layerOrder = input.layerOrder;
  const mp = board.board_rules?.mask_paste;
  const ctx: Ctx = { board, jobs: [], ratios: dashRatiosOf(board), maskPaste: mp, rules: input.ruleMargins ?? null };
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
    for (const s of drawings?.shapes ?? []) if (shapeOnLayer(s, layer)) tagged(c, "shape", () => addShape(ctx, c, s, layer, ctx.rules?.mask ? (ctx.rules.shapes[s.id] ?? null) : undefined));
    for (const t of drawings?.texts ?? []) if (t.layer === layer) tagged(c, "text", () => addBoardText(ctx, c, t));
    addBoardItems(ctx, c, board, layer);
    for (const b of drawings?.barcodes ?? []) if (b.layer === layer) tagged(c, "barcode", () => addBarcode(c, b, barcodePaths(b)));
    if (opts.showZones) for (const z of zones) if (zoneLayers(z).includes(layer)) tagged(c, "zone-fill", () => addZone(c, z, layer, input.zoneFills));
  }

  // ---- The technical layers (`techLayerList`); the masks are built for the silkscreen and the plated copper even when hidden.
  for (const layer of TECH_LAYERS) {
    const forced = isMaskLayer(layer) && (opts.subtractMaskFromSilk || opts.differentiatePlatedCopper) && layerEnabled(board, layer, layerOrder);
    if (!enabled3d(layer) && !forced) continue;
    const c: Prim[] = [];
    out.layers.set(layer, c);
    for (const s of drawings?.shapes ?? []) if (shapeOnLayer(s, layer)) tagged(c, "shape", () => addShape(ctx, c, s, layer, ctx.rules?.mask ? (ctx.rules.shapes[s.id] ?? null) : undefined));
    for (const t of drawings?.texts ?? []) if (t.layer === layer) tagged(c, "text", () => addBoardText(ctx, c, t));
    addBoardItems(ctx, c, board, layer);
    for (const b of drawings?.barcodes ?? []) if (b.layer === layer) tagged(c, "barcode", () => addBarcode(c, b, barcodePaths(b)));
    if (isMaskLayer(layer)) {
      const copper = layer === "F.Mask" ? "F.Cu" : "B.Cu";
      for (const t of tracks) {
        if (!t.mask || t.layer !== copper) continue;
        // each segment's own expansion (`createTrackWithMargin( track, .., track->GetSolderMaskExpansion() )`, a track being its segments to KiCad)
        const perSeg = ctx.rules?.mask ? ctx.rules.tracks[t.id] : undefined;
        tagged(c, "track", () => addTrack(c, t, (i) => trackMaskExpansion(t, mp, ctx.rules?.mask ? (perSeg?.[i] ?? null) : undefined)));
      }
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
