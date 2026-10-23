// KiCad 10's grid items on the canvas (kicad-port/gridItem.ts has the geometry).
//
//   pcbnew/pcb_draw_panel_gal.cpp  PCB_DRAW_PANEL_GAL::prepareGridSources: one grid source per grid item, while Grids (`LAYER_SUBGRIDS`) is shown, in the Grids colour of the theme,
//                                  highlighted when selected, in the editor grid's style
//   common/gal/cairo/cairo_gal.cpp CAIRO_GAL_BASE::drawGridSources / drawGridSource: the sources back to front (the winner of an overlap on top), the selected ones last; inside each the
//                                  lines (circles and spokes of a polar grid), dots or small crosses of the thinned-out lattice, the major ones twice as wide, then the coverage's
//                                  outline a little darker
//   pcbnew/pcb_painter.cpp         PCB_PAINTER::draw( PCB_GRID_ITEM ): a selected grid's outline in the selection colour and its centre's cross in the anchor colour
//
// KiCad draws the grid sources behind every item, the editor grid among them, and each source first "clears" its coverage with the background, so the editor grid does not show
// through it. This canvas draws its editor grid in the grid's own tier, above the copper; the grid items are drawn in that tier too, and the editor grid leaves out the points a
// grid item covers (`gridItemCovers`) instead of a wash, which would hide the items under it here.
import type { BoardGridItem } from "../../api/pcbItemTypes";
import type { BoardState } from "../../api/types";
import type { ViewTransform } from "../../state/store";
import { geometryContains, gridGeometry, gridLattice, gridOutline, gridsInPrecedence, isMajor, selectedLayerColor, GRID_EDGE_DARKEN, GRID_SELECTED_BRIGHTEN, type GridGeometry } from "../../kicad-port/gridItem";
import { DEFAULT_GRID_MIN_SPACING_PX, DEFAULT_GRID_TICK, type GridStyle } from "../../kicad-port/grid";
import { brighten as brightenRgb, darken, hexToRgb, rgbToHex } from "../../kicad-port/netHighlight";
import { hairlineUm } from "./view";
import { layerColor } from "./layers";

/** The grid items that are drawn: all of them, while Grids is shown. */
export function shownGridItems(board: BoardState, layerVisible: Readonly<Record<string, boolean>>): BoardGridItem[] {
  if (layerVisible["obj:subgrids"] === false) return [];
  return board.drawings?.grid_items ?? [];
}

/** Whether a shown grid item covers `(x, y)`: the editor grid leaves the point out there. */
export function gridItemCovers(geoms: readonly GridGeometry[], x: number, y: number): boolean {
  for (const g of geoms) if (geometryContains(g, [x, y])) return true;
  return false;
}

/** `COLOR4D::Brighten( f )` / `Darkened( f )` on an `#rrggbb[aa]` colour. */
const brighten = (hex: string, f: number): string => rgbToHex(brightenRgb(hexToRgb(hex), f));
const darkened = (hex: string, f: number): string => rgbToHex(darken(hexToRgb(hex), f));

/** The coverage of a grid in its own frame (`GAL::drawGridCoverageShape`): the rectangle, a full circle, or the wedge in at least 16 segments. */
function coveragePath(ctx: CanvasRenderingContext2D, g: GridGeometry): void {
  const [rMax, phiMax] = g.extent;
  ctx.beginPath();
  if (g.kind === "cartesian") {
    ctx.rect(-g.extent[0], -g.extent[1], 2 * g.extent[0], 2 * g.extent[1]);
  } else if (rMax > 0 && phiMax >= 2 * Math.PI - 1e-6) {
    ctx.arc(0, 0, rMax, 0, 2 * Math.PI);
  } else if (rMax > 0 && phiMax > 0) {
    const segments = Math.max(16, Math.trunc(phiMax / (Math.PI / 16)));
    ctx.moveTo(0, 0);
    for (let i = 0; i <= segments; i++) {
      const phi = (phiMax * i) / segments;
      ctx.lineTo(rMax * Math.cos(phi), rMax * Math.sin(phi));
    }
    ctx.closePath();
  }
}

/** Clips the canvas to the outside of every grid's coverage (each one an even-odd path of a huge rectangle around its coverage, the clips intersected). */
function clipOutCoverages(ctx: CanvasRenderingContext2D, grids: readonly BoardGridItem[]): void {
  if (grids.length === 0) return;
  const base = ctx.getTransform();
  const far = 1e12;
  for (const item of grids) {
    const h = gridGeometry(item);
    ctx.setTransform(base);
    ctx.translate(h.origin[0], h.origin[1]);
    ctx.rotate(-h.orientation);
    coveragePath(ctx, h);
    ctx.rect(-far, -far, 2 * far, 2 * far);
    ctx.clip("evenodd");
  }
  ctx.setTransform(base);
}

/** One grid source (`CAIRO_GAL_BASE::drawGridSource` for a bounded source). */
function drawGridSource(
  ctx: CanvasRenderingContext2D,
  view: ViewTransform,
  item: BoardGridItem,
  highlighted: boolean,
  style: GridStyle,
  visibleBox: [number, number, number, number],
  washedBy: readonly BoardGridItem[] = []
): void {
  const g = gridGeometry(item);
  const base = layerColor("LAYER_SUBGRIDS");
  const color = highlighted ? brighten(base, GRID_SELECTED_BRIGHTEN) : base;
  const lattice = gridLattice(g, item.tick_interval, view.scale, DEFAULT_GRID_MIN_SPACING_PX, DEFAULT_GRID_TICK, style === "small-cross");
  // `max( 1.0f, m_gridLineWidth ) / m_worldScale`: one pixel, two for a major line.
  const marker = hairlineUm(view, 1);
  const double = 2 * marker;
  ctx.save();
  // `drawGridSource` first fills a bounded grid's coverage with the clear colour, hiding the grids drawn before it; the canvas has the board's items under the grids, so the grids
  // drawn after this one (`washedBy`, not highlighted: a highlighted one's wash is only `GRID_DIM_ALPHA`) clip it out of their coverage instead of painting over it.
  clipOutCoverages(ctx, washedBy);
  ctx.translate(g.origin[0], g.origin[1]);
  // "GAL Rotate is math-convention; grid orientation is screen-convention": the canvas turns clockwise on the screen for a positive angle, KiCad's orientation the other way.
  ctx.rotate(-g.orientation);
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineCap = "butt";
  const [dx, dy] = lattice.pitch;
  const dot = (x: number, y: number, majorX: boolean, majorY: boolean) => {
    const w = majorX ? double : marker;
    const h = majorY ? double : marker;
    ctx.fillRect(x - w / 2, y - h / 2, w, h);
  };
  const cross = (x: number, y: number, major: boolean, armAngle: number) => {
    const w = major ? double : marker;
    const len = 2 * w;
    const a1: [number, number] = [Math.cos(armAngle) * len, Math.sin(armAngle) * len];
    ctx.lineWidth = w;
    ctx.beginPath();
    ctx.moveTo(x - a1[0], y - a1[1]);
    ctx.lineTo(x + a1[0], y + a1[1]);
    ctx.moveTo(x + a1[1], y - a1[0]);
    ctx.lineTo(x - a1[1], y + a1[0]);
    ctx.stroke();
  };
  if (g.kind === "polar" && lattice.extent[0] > 0 && lattice.extent[1] > 0) {
    const [rMax, phiMax] = lattice.extent;
    if (style === "lines") {
      let rIdx = 1;
      for (let r = dx; r <= rMax + 1e-6; r += dx, ++rIdx) {
        ctx.lineWidth = isMajor(lattice, rIdx) ? double : marker;
        ctx.beginPath();
        ctx.arc(0, 0, r, 0, phiMax);
        ctx.stroke();
      }
      let pIdx = 0;
      for (let phi = 0; phi <= phiMax + 1e-6; phi += dy, ++pIdx) {
        ctx.lineWidth = isMajor(lattice, pIdx) ? double : marker;
        ctx.beginPath();
        ctx.moveTo(0, 0);
        ctx.lineTo(rMax * Math.cos(phi), rMax * Math.sin(phi));
        ctx.stroke();
      }
    } else {
      let rIdx = 0;
      for (let r = 0; r <= rMax + 1e-6; r += dx, ++rIdx) {
        let pIdx = 0;
        for (let phi = 0; phi <= phiMax + 1e-6; phi += dy, ++pIdx) {
          const x = r * Math.cos(phi);
          const y = r * Math.sin(phi);
          if (style === "dots") dot(x, y, isMajor(lattice, rIdx), isMajor(lattice, pIdx));
          else cross(x, y, isMajor(lattice, rIdx) && isMajor(lattice, pIdx), phi);
        }
      }
    }
  } else if (g.kind === "cartesian") {
    // `gridScreenBBox`: the screen in the grid's frame, grown by a pitch so lines just off screen still paint; then cut to the grid's extent.
    const corners: [number, number][] = [
      [visibleBox[0], visibleBox[1]],
      [visibleBox[2], visibleBox[1]],
      [visibleBox[0], visibleBox[3]],
      [visibleBox[2], visibleBox[3]],
    ];
    const co = Math.cos(g.orientation);
    const so = Math.sin(g.orientation);
    let xMin = Infinity,
      xMax = -Infinity,
      yMin = Infinity,
      yMax = -Infinity;
    for (const [wx, wy] of corners) {
      const ox = wx - g.origin[0];
      const oy = wy - g.origin[1];
      const lx = co * ox - so * oy;
      const ly = so * ox + co * oy;
      xMin = Math.min(xMin, lx);
      xMax = Math.max(xMax, lx);
      yMin = Math.min(yMin, ly);
      yMax = Math.max(yMax, ly);
    }
    xMin -= dx;
    xMax += dx;
    yMin -= dy;
    yMax += dy;
    let ixMin = Math.floor(xMin / dx);
    let ixMax = Math.ceil(xMax / dx);
    let iyMin = Math.floor(yMin / dy);
    let iyMax = Math.ceil(yMax / dy);
    xMin = Math.max(xMin, -g.extent[0]);
    xMax = Math.min(xMax, g.extent[0]);
    yMin = Math.max(yMin, -g.extent[1]);
    yMax = Math.min(yMax, g.extent[1]);
    ixMin = Math.max(ixMin, -Math.trunc(g.extent[0] / dx));
    ixMax = Math.min(ixMax, Math.trunc(g.extent[0] / dx));
    iyMin = Math.max(iyMin, -Math.trunc(g.extent[1] / dy));
    iyMax = Math.min(iyMax, Math.trunc(g.extent[1] / dy));
    if (style === "lines") {
      for (let ix = ixMin; ix <= ixMax; ++ix) {
        ctx.lineWidth = isMajor(lattice, ix) ? double : marker;
        ctx.beginPath();
        ctx.moveTo(ix * dx, yMin);
        ctx.lineTo(ix * dx, yMax);
        ctx.stroke();
      }
      for (let iy = iyMin; iy <= iyMax; ++iy) {
        ctx.lineWidth = isMajor(lattice, iy) ? double : marker;
        ctx.beginPath();
        ctx.moveTo(xMin, iy * dy);
        ctx.lineTo(xMax, iy * dy);
        ctx.stroke();
      }
    } else {
      for (let ix = ixMin; ix <= ixMax; ++ix) {
        for (let iy = iyMin; iy <= iyMax; ++iy) {
          if (style === "dots") dot(ix * dx, iy * dy, isMajor(lattice, ix), isMajor(lattice, iy));
          else cross(ix * dx, iy * dy, isMajor(lattice, ix) && isMajor(lattice, iy), 0);
        }
      }
    }
  }
  // "Outline the coverage, so a grid too sparse to render still reads as an area."
  ctx.strokeStyle = darkened(color, GRID_EDGE_DARKEN);
  ctx.lineWidth = hairlineUm(view, 1);
  coveragePath(ctx, g);
  ctx.stroke();
  ctx.restore();
}

/** `PCB_PAINTER::draw( PCB_GRID_ITEM )` for a selected grid: the outline (`m_outlineWidth`) in the selection colour, and a 5 px cross at the centre in the anchor colour. */
function drawSelectedGridItem(ctx: CanvasRenderingContext2D, view: ViewTransform, item: BoardGridItem): void {
  const pts = gridOutline(item);
  ctx.save();
  // `m_pcbSettings.GetColor( aGridItem, LAYER_SUBGRIDS )` of a selected item: the Grids colour as `RENDER_SETTINGS::update` brightens it for a selection.
  ctx.strokeStyle = selectedLayerColor(layerColor("LAYER_SUBGRIDS"));
  ctx.lineWidth = hairlineUm(view, 1);
  ctx.beginPath();
  pts.forEach(([x, y], i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
  ctx.closePath();
  ctx.stroke();
  const a = hairlineUm(view, 5);
  ctx.strokeStyle = layerColor("anchor");
  ctx.beginPath();
  ctx.moveTo(item.x - a, item.y);
  ctx.lineTo(item.x + a, item.y);
  ctx.moveTo(item.x, item.y - a);
  ctx.lineTo(item.x, item.y + a);
  ctx.stroke();
  ctx.restore();
}

/** A grid item that is not on the board yet (Place Sub-grid's preview: `griditem->SetSelected()`), drawn as a selected grid. */
export function drawGridItemPreview(ctx: CanvasRenderingContext2D, view: ViewTransform, item: BoardGridItem): void {
  if (item.extent[0] <= 0) return;
  drawGridSource(ctx, view, item, true, "dots", [-Infinity, -Infinity, Infinity, Infinity]);
  drawSelectedGridItem(ctx, view, item);
}

/**
 * The board's grid items (`drawGridSources`): the ones not selected first, the selected (highlighted) ones after, each set back to front so the grid that takes precedence where
 * two overlap is drawn last; then the selection decorations. `visibleBox` is the screen in board coordinates.
 */
export function drawGridItems(ctx: CanvasRenderingContext2D, view: ViewTransform, board: BoardState, layerVisible: Readonly<Record<string, boolean>>, selection: ReadonlySet<string>, style: GridStyle, visibleBox: [number, number, number, number]): void {
  const grids = shownGridItems(board, layerVisible);
  if (grids.length === 0) return;
  const ordered = gridsInPrecedence(grids).reverse();
  const drawn = [...ordered.filter((g) => !selection.has(g.id)), ...ordered.filter((g) => selection.has(g.id))];
  drawn.forEach((g, i) => {
    const highlighted = selection.has(g.id);
    drawGridSource(ctx, view, g, highlighted, style, visibleBox, drawn.slice(i + 1).filter((h) => !selection.has(h.id)));
  });
  for (const g of grids) if (selection.has(g.id)) drawSelectedGridItem(ctx, view, g);
}
