// What the painter takes from the Appearance panel (kicad-port/appearance.ts) besides plain visibility, and the objects only the panel can switch on:
// the opacities, the colour a copper item or a ratsnest line takes from its net or net class, and the overlays of pcb_painter.cpp that were not drawn
// before -- footprint anchors (`LAYER_ANCHOR`), the shadow on locked items (`LAYER_LOCKED_ITEM_SHADOW`), the colliding courtyards of a move
// (`LAYER_CONFLICTS_SHADOW`, drc_interactive_courtyard_clearance.cpp), the board area shadow (`LAYER_BOARD_OUTLINE_AREA`) and the drawing sheet
// (`LAYER_DRAWINGSHEET`). painter.ts calls these; it keeps every other drawing decision.

import type { BoardState, Part, Zone } from "../../api/types";
import type { ViewTransform } from "../../state/store";
import { hairlineUm } from "./view";
import { layerColor } from "./layers";
import { drawStrokeText } from "../text/strokeFont";
import { DEFAULT_OPACITY, netPalette, objectOn, type AppearanceState, type NetColorMode, type ObjectId, type Opacity, type OpacityKey, type Rgba } from "../../kicad-port/appearance";
import type { NetsContext } from "../../kicad-port/appearanceNets";
import { brighten, darken, HIGHLIGHT_FACTOR } from "../../kicad-port/netHighlight";
import { carryPoint, splitCarried, type CarryPreview } from "../../kicad-port/pcbCarry";
import { scaleToZoomFactor } from "../../kicad-port/zoomFit";

/** Everything the painter needs from the panel's settings, assembled once per change (components/canvas/Canvas.tsx). */
export interface PaintAppearance {
  opacity: Opacity;
  /** HIDDEN inactive-layer mode: what is not on the active layer is not drawn at all. */
  contrastHidden: boolean;
  netColorMode: NetColorMode;
  /** The colour of each net that has one of its own or through its net class (`netPalette`). */
  palette: ReadonlyMap<string, Rgba>;
  /** The locked-item shadow's margin, µm (`m_lockedShadowMargin`: four times the silkscreen line width). */
  lockedMargin: number;
  /** The board's paper for the drawing sheet, µm, and its title block. */
  sheet: { widthUm: number; heightUm: number; paper: string; title: string; date: string; rev: string; company: string; comments: string[]; file: string } | null;
}

export const PLAIN_APPEARANCE: PaintAppearance = { opacity: DEFAULT_OPACITY, contrastHidden: false, netColorMode: "ratsnest", palette: new Map(), lockedMargin: 400, sheet: null };

/** The painter's view of the panel's settings for this board: the opacities, the net colours resolved per net, the locked shadow margin (four silkscreen line widths) and the sheet. */
export function buildPaintAppearance(a: AppearanceState, nets: NetsContext, board: BoardState | null): PaintAppearance {
  const paper = board?.page?.size_um ?? ([297_000, 210_000] as const);
  const tb = board?.title_block;
  const silk = board?.board_rules?.text_graphics?.silk.line_width_um ?? 100;
  return {
    opacity: a.opacity,
    contrastHidden: a.contrastHidden,
    netColorMode: a.netColorMode,
    palette: netPalette(a, nets.nets, nets.classOf),
    lockedMargin: silk * 4,
    sheet: board
      ? { widthUm: paper[0], heightUm: paper[1], paper: board.page?.paper ?? "A4", title: tb?.title ?? "", date: tb?.date ?? "", rev: tb?.rev ?? "", company: tb?.company ?? "", comments: tb?.comments ?? [], file: `${board.name}.kicad_pcb` }
      : null,
  };
}

export function opacityOf(a: PaintAppearance | undefined, key: OpacityKey): number {
  return (a ?? PLAIN_APPEARANCE).opacity[key];
}

const rgbaText = (c: Rgba): string => `rgba(${c.r}, ${c.g}, ${c.b}, ${c.a})`;

/**
 * `PCB_RENDER_SETTINGS::GetColor`'s net branch for copper: the item's colour is its net's (or its net class's) when the net colour mode is "All" and the net
 * has one, else the layer's `base`; a highlighted net brightens it and every other net darkens it by the same 0.5 (`m_highlightFactor`).
 */
export function copperColor(base: string, net: string | null | undefined, a: PaintAppearance | undefined, highlighted: boolean | null): string {
  const own = net && a && a.netColorMode === "all" ? a.palette.get(net) : undefined;
  if (!own) return base;
  if (highlighted === null) return rgbaText(own);
  const rgb = highlighted ? brighten(own, HIGHLIGHT_FACTOR) : darken(own, HIGHLIGHT_FACTOR);
  return rgbaText({ ...rgb, a: own.a });
}

/** The ratsnest line's colour: the net's (or its class's) in every mode but "None" (`RATSNEST_VIEW_ITEM::ViewDraw`'s `colorByNet`), else the ratsnest colour. */
export function ratsnestColor(net: string, a: PaintAppearance | undefined): string | null {
  const own = a && a.netColorMode !== "off" ? a.palette.get(net) : undefined;
  return own ? rgbaText(own) : null;
}

/** True when the object is switched on (and not at opacity 0), by the `obj:<id>` keys of the layer visibility record. */
export const on = (layerVisible: Readonly<Record<string, boolean>>, id: ObjectId): boolean => objectOn(layerVisible, id);

// ------------------------------------------------------------------------------------------------------------------------------------ overlays

type Opts = { layerVisible: Record<string, boolean>; appearance?: PaintAppearance };

/** `PCB_PAINTER::draw( const FOOTPRINT* )` on `LAYER_ANCHOR`: a cross five pixels either way, one pixel wide, at the footprint's origin -- only where its side shows (`FOOTPRINT::ViewGetLOD`) and zoomed in enough (the item's level of detail 1.5). */
export function drawAnchors(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, opts: Opts): void {
  if (!on(opts.layerVisible, "footprint_anchors") || scaleToZoomFactor(view.scale) < 1.5) return;
  const r = hairlineUm(view, 5);
  ctx.save();
  ctx.strokeStyle = layerColor("LAYER_ANCHOR");
  ctx.lineWidth = hairlineUm(view, 1);
  ctx.beginPath();
  for (const p of board.parts) {
    if (!p.placed || !p.at || !footprintShown(opts.layerVisible, p)) continue;
    const [x, y] = p.at;
    ctx.moveTo(x - r, y);
    ctx.lineTo(x + r, y);
    ctx.moveTo(x, y - r);
    ctx.lineTo(x, y + r);
  }
  ctx.stroke();
  ctx.restore();
}

/** Whether the footprint's side is shown (`LAYER_FOOTPRINTS_FR` / `LAYER_FOOTPRINTS_BK`). */
export function footprintShown(layerVisible: Readonly<Record<string, boolean>>, p: Pick<Part, "side">): boolean {
  return on(layerVisible, p.side === "bottom" ? "footprints_back" : "footprints_front");
}

/**
 * The shadow on locked items (`PCB_PAINTER::draw` on `LAYER_LOCKED_ITEM_SHADOW`): a locked footprint's box and a margin around it, a track a margin wider, a via
 * a ring of the margin's width around it, a zone's outline and a graphic's stroke a margin wider.
 */
export function drawLockedShadows(ctx: CanvasRenderingContext2D, board: BoardState, opts: Opts): void {
  const locked = new Set(board.locked ?? []);
  if (locked.size === 0 || !on(opts.layerVisible, "locked_item_shadows")) return;
  const m = (opts.appearance ?? PLAIN_APPEARANCE).lockedMargin;
  const color = layerColor("LAYER_LOCKED_ITEM_SHADOW");
  ctx.save();
  ctx.fillStyle = color;
  ctx.strokeStyle = color;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  for (const p of board.parts) {
    if (!locked.has(p.ref) || !p.placed || !p.courtyard || !footprintShown(opts.layerVisible, p)) continue;
    const [x0, y0, x1, y1] = p.courtyard;
    ctx.fillRect(x0 - m / 2, y0 - m / 2, x1 - x0 + m, y1 - y0 + m);
  }
  if (on(opts.layerVisible, "tracks")) {
    for (const t of board.routing?.tracks ?? []) {
      if (!locked.has(t.id) || t.pts.length < 2) continue;
      ctx.lineWidth = t.width + m;
      ctx.beginPath();
      t.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.stroke();
    }
  }
  if (on(opts.layerVisible, "vias")) {
    for (const v of board.routing?.vias ?? []) {
      if (!locked.has(v.id)) continue;
      ctx.lineWidth = m;
      ctx.beginPath();
      ctx.arc(v.x, v.y, (v.d + m) / 2, 0, Math.PI * 2);
      ctx.stroke();
    }
  }
  if (on(opts.layerVisible, "zones")) {
    for (const z of board.routing?.zones ?? []) {
      if (!locked.has(z.id) || z.outline.length < 3) continue;
      ctx.lineWidth = m;
      ctx.beginPath();
      z.outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.closePath();
      ctx.stroke();
    }
  }
  for (const s of board.drawings?.shapes ?? []) {
    if (!locked.has(s.id)) continue;
    ctx.lineWidth = s.stroke_width + m;
    ctx.beginPath();
    if (s.kind === "segment") {
      ctx.moveTo(s.start[0], s.start[1]);
      ctx.lineTo(s.end[0], s.end[1]);
    } else if (s.kind === "rect") {
      ctx.rect(s.start[0], s.start[1], s.end[0] - s.start[0], s.end[1] - s.start[1]);
    } else if (s.kind === "circle") {
      ctx.arc(s.center[0], s.center[1], Math.hypot(s.end[0] - s.center[0], s.end[1] - s.center[1]), 0, Math.PI * 2);
    } else if (s.kind === "polygon") {
      s.pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.closePath();
    } else {
      continue;
    }
    ctx.stroke();
  }
  ctx.restore();
}

/** `PCB_PAINTER::draw( const PCB_BOARD_OUTLINE* )`: the board's outline filled with the board area colour (hidden by default). */
export function drawBoardArea(ctx: CanvasRenderingContext2D, board: BoardState, opts: Opts): void {
  const outline = board.outline;
  if (!outline || outline.length < 3 || !on(opts.layerVisible, "board_outline_area")) return;
  ctx.save();
  ctx.fillStyle = layerColor("LAYER_BOARD_OUTLINE_AREA");
  ctx.beginPath();
  outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  ctx.closePath();
  ctx.fill();
  ctx.restore();
}

// ------------------------------------------------------------------------------------------------------------------------------ colliding courtyards

type Box = readonly [number, number, number, number];

function boxesOverlap(a: Box, b: Box): boolean {
  return a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1];
}

/** The courtyard box of `p` after the move in progress: the corners go through the same transform the painter draws the footprint with, the box is theirs. */
function movedBox(p: Part, preview: CarryPreview & { kind?: string; perRefOffsetUm?: Record<string, [number, number]> }): Box | null {
  if (!p.courtyard) return null;
  const [x0, y0, x1, y1] = p.courtyard;
  const corners: [number, number][] = [
    [x0, y0],
    [x1, y0],
    [x1, y1],
    [x0, y1],
  ];
  let pts: [number, number][];
  if (preview.kind === "pcb") {
    pts = corners.map((c) => carryPoint(preview, c));
  } else {
    // `drawFootprint`'s own transform: turn / flip about the anchor, then move (Pack and Move adds its per-footprint shift).
    const own = preview.perRefOffsetUm?.[p.ref];
    const [dx, dy] = [preview.dxUm + (own?.[0] ?? 0), preview.dyUm + (own?.[1] ?? 0)];
    const [ax, ay] = p.at ?? [(x0 + x1) / 2, (y0 + y1) / 2];
    const theta = (-(preview.rotateQuarterTurns ?? 0) * Math.PI) / 2;
    const [cos, sin] = [Math.round(Math.cos(theta)), Math.round(Math.sin(theta))];
    pts = corners.map(([x, y]) => {
      let u = x - ax;
      const v = y - ay;
      if (preview.flipped) u = -u;
      return [ax + u * cos - v * sin + dx, ay + u * sin + v * cos + dy];
    });
  }
  const xs = pts.map((q) => q[0]);
  const ys = pts.map((q) => q[1]);
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
}

function polygonHitsBox(poly: readonly (readonly [number, number])[], b: Box): boolean {
  const inside = (x: number, y: number) => {
    let c = false;
    for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
      const [xi, yi] = poly[i]!;
      const [xj, yj] = poly[j]!;
      if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) c = !c;
    }
    return c;
  };
  if (poly.some(([x, y]) => x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3])) return true;
  if (inside(b[0], b[1]) || inside(b[2], b[1]) || inside(b[2], b[3]) || inside(b[0], b[3])) return true;
  const edges: [number, number, number, number][] = [
    [b[0], b[1], b[2], b[1]],
    [b[2], b[1], b[2], b[3]],
    [b[2], b[3], b[0], b[3]],
    [b[0], b[3], b[0], b[1]],
  ];
  const ccw = (ax: number, ay: number, bx: number, by: number, cx: number, cy: number) => (cy - ay) * (bx - ax) > (by - ay) * (cx - ax);
  const cross = (a: readonly [number, number], b2: readonly [number, number], c: readonly [number, number], d: readonly [number, number]) =>
    ccw(a[0], a[1], c[0], c[1], d[0], d[1]) !== ccw(b2[0], b2[1], c[0], c[1], d[0], d[1]) && ccw(a[0], a[1], b2[0], b2[1], c[0], c[1]) !== ccw(a[0], a[1], b2[0], b2[1], d[0], d[1]);
  for (let i = 0; i < poly.length; i++) {
    const [p, q] = [poly[i]!, poly[(i + 1) % poly.length]!];
    if (edges.some(([x0, y0, x1, y1]) => cross(p, q, [x0, y0], [x1, y1]))) return true;
  }
  return false;
}

/**
 * `DRC_INTERACTIVE_COURTYARD_CLEARANCE::testCourtyardClearances` for a move in progress, over the studio's rectangular courtyards: a footprint being
 * moved and one that is not conflict when their courtyards on the same side overlap (clearance 0); a footprint being moved and a rule area that
 * keeps footprints out conflict when the area meets the courtyard. Both parties of a conflict are highlighted (`UpdateConflicts( view, true )`).
 * Returns the refs of the footprints in conflict and the ids of the rule areas.
 */
export function courtyardConflicts(board: BoardState, preview: (CarryPreview & { kind?: string; perRefOffsetUm?: Record<string, [number, number]> }) | null): { parts: Set<string>; zones: Set<string> } {
  const parts = new Set<string>();
  const zones = new Set<string>();
  if (!preview) return { parts, zones };
  const split = splitCarried(board, preview.refs);
  const moving = split.moving.parts.filter((p) => p.placed && p.courtyard);
  const still = split.still.parts.filter((p) => p.placed && p.courtyard);
  const keepouts = (board.routing?.zones ?? []).filter((z: Zone) => z.is_rule_area && z.keepout_footprints && z.outline.length >= 3);
  for (const m of moving) {
    const mb = movedBox(m, preview);
    if (!mb) continue;
    for (const o of still) {
      if ((o.side === "bottom") !== (m.side === "bottom")) continue;
      if (boxesOverlap(mb, o.courtyard as Box)) {
        parts.add(m.ref);
        parts.add(o.ref);
      }
    }
    for (const z of keepouts) {
      if (polygonHitsBox(z.outline, mb)) {
        parts.add(m.ref);
        zones.add(z.id);
      }
    }
  }
  return { parts, zones };
}

/** `PCB_PAINTER::draw( const FOOTPRINT* )` / `draw( const ZONE* )` on `LAYER_CONFLICTS_SHADOW`: the courtyards (and rule areas) in conflict during a move, filled. Drawn where the part is on screen: the moved ones through the move. */
export function drawConflicts(ctx: CanvasRenderingContext2D, board: BoardState, preview: (CarryPreview & { kind?: string; perRefOffsetUm?: Record<string, [number, number]> }) | null, opts: Opts): void {
  if (!preview || !on(opts.layerVisible, "conflict_shadows")) return;
  const { parts, zones } = courtyardConflicts(board, preview);
  if (parts.size === 0 && zones.size === 0) return;
  const movingRefs = new Set(splitCarried(board, preview.refs).moving.parts.map((p) => p.ref));
  ctx.save();
  ctx.fillStyle = layerColor("LAYER_CONFLICTS_SHADOW");
  for (const p of board.parts) {
    if (!parts.has(p.ref) || !p.courtyard || !footprintShown(opts.layerVisible, p)) continue;
    const box = movingRefs.has(p.ref) ? movedBox(p, preview) : (p.courtyard as Box);
    if (box) ctx.fillRect(box[0], box[1], box[2] - box[0], box[3] - box[1]);
  }
  for (const z of board.routing?.zones ?? []) {
    if (!zones.has(z.id)) continue;
    ctx.beginPath();
    z.outline.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
    ctx.closePath();
    ctx.fill();
  }
  ctx.restore();
}

// --------------------------------------------------------------------------------------------------------------------------------- drawing sheet

const OUTER_MARGIN_UM = 10_000;
const INNER_MARGIN_UM = 12_000;
const TB_WIDTH_UM = 108_000;
const TB_HEIGHT_UM = 32_000;
const ROWS_UM = [15_507, 21_507, 25_507, 28_507];

/**
 * The drawing sheet (`LAYER_DRAWINGSHEET`, KiCad's default sheet): the double frame ten and twelve millimetres in from the paper's edge, the title block flush with
 * the inner frame's bottom right corner. Lines in the drawing sheet colour on the board's dark background -- the paper is not filled, as the board editor does not.
 */
export function drawSheet(ctx: CanvasRenderingContext2D, view: ViewTransform, opts: Opts): void {
  const sheet = opts.appearance?.sheet;
  if (!sheet || !on(opts.layerVisible, "drawing_sheet")) return;
  const { widthUm: w, heightUm: h } = sheet;
  const color = layerColor("LAYER_DRAWINGSHEET");
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = Math.max(152, hairlineUm(view, 1));
  ctx.strokeRect(OUTER_MARGIN_UM, OUTER_MARGIN_UM, w - 2 * OUTER_MARGIN_UM, h - 2 * OUTER_MARGIN_UM);
  ctx.strokeRect(INNER_MARGIN_UM, INNER_MARGIN_UM, w - 2 * INNER_MARGIN_UM, h - 2 * INNER_MARGIN_UM);
  const x1 = w - INNER_MARGIN_UM;
  const y1 = h - INNER_MARGIN_UM;
  const x0 = x1 - TB_WIDTH_UM;
  const y0 = y1 - TB_HEIGHT_UM;
  ctx.strokeRect(x0, y0, TB_WIDTH_UM, TB_HEIGHT_UM);
  ctx.beginPath();
  for (const yOff of ROWS_UM) {
    ctx.moveTo(x0, y0 + yOff);
    ctx.lineTo(x1, y0 + yOff);
  }
  ctx.stroke();
  // Text only where it can be read: the sheet is large and its text small.
  if (view.scale * 2_000 >= 5) {
    const text = (s: string, xOff: number, yOff: number, size: number, bold = false) => drawStrokeText(ctx, s, x0 + xOff, y0 + yOff, { sizeUm: size, thicknessUm: bold ? size / 5 : undefined, color });
    text(`File: ${sheet.file}`, 1_000, ROWS_UM[1]! - 1_050, 2_000);
    text(`Title: ${sheet.title}`, 1_000, ROWS_UM[2]! - 1_200, 2_667, true);
    text(`Size: ${sheet.paper}`, 1_000, ROWS_UM[3]! - 650, 2_000);
    text(`Date: ${sheet.date}`, 23_000, ROWS_UM[3]! - 650, 2_000);
    text(`Rev: ${sheet.rev}`, 86_000, ROWS_UM[3]! - 650, 2_000);
    if (sheet.company) text(sheet.company, 1_000, TB_HEIGHT_UM - 17_250, 2_000, true);
    sheet.comments.slice(0, 4).forEach((c, i) => {
      if (c) text(c, 1_000, TB_HEIGHT_UM - (23 + 3 * i - 2.75) * 1_000, 2_000);
    });
  }
  ctx.restore();
}
