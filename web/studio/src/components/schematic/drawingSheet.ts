// The sheet frame, zone-reference grid and title block -- KiCad calls
// this whole thing the "drawing sheet" (common/drawing_sheet in
// kicad-src). This session could not read that C++ source directly
// (kicad-mirror's tree isn't checked out on disk here, and this
// worktree-isolated sandbox refuses git access to it even via a
// subprocess -- see tools/lib/kicadSource.js's own comment on exactly
// this limitation) -- but every geometry number below is NOT a guess:
// it's read straight off a real Eeschema-SVG export already sitting in
// this repo's own render path (`crates/render`, which already ports
// KiCad's default A4 worksheet template), captured for this task at
// scratchpad/kicad-ref/ours/mcu_board_30plus.svg. That file's <path>
// elements for the frame/ticks/dividers and <text> elements for every
// label were read directly (grep on the raw SVG, not eyeballed), so the
// margins, division pitch, row bands and column splits below are a
// faithful, source-verified port of KiCad's real default sheet, not an
// approximation. Colors are equally real (src/kicad/colors.json, from
// common/settings/builtin_color_themes.h): LAYER_SCHEMATIC_DRAWINGSHEET
// for all of the frame/title-block ink.
import type { ViewTransform } from "../../state/store";
import { layerColor } from "../canvas/layers";

/** A4 landscape, KiCad's default schematic page size -- um (mm * 1000). */
export const PAGE_WIDTH_UM = 297_000;
export const PAGE_HEIGHT_UM = 210_000;

// Double-rule frame: an outer line 10mm from the paper edge, an inner
// line 12mm from it (both in LAYER_SCHEMATIC_DRAWINGSHEET red, not a
// dashed "page limit" -- the real export has no dashed line at all).
const OUTER_MARGIN_UM = 10_000;
const INNER_MARGIN_UM = 12_000;
const FRAME_STROKE_UM = 152; // 0.1524mm in the source SVG

// Zone-reference grid: fixed 50mm pitch from the outer frame's corner,
// however many whole+remainder cells that yields (6 columns, 4 rows for
// A4 landscape) -- not "divide evenly into a fixed count".
const ZONE_PITCH_UM = 50_000;
const ZONE_LABEL_SIZE_UM = 1_733;
const ZONE_TICK_UM = 2_000; // outer margin (10mm) minus inner margin (12mm) -- the tick spans exactly that gap

// Title block: flush with the inner frame's bottom-right corner.
const TB_WIDTH_UM = 108_000;
const TB_HEIGHT_UM = 32_000;
// Row band bottom edges, measured from the block's own top edge (y0).
const TB_ROW_COMMENTS_BOTTOM = 15_507;
const TB_ROW_FILESHEET_BOTTOM = 21_507;
const TB_ROW_TITLE_BOTTOM = 25_507;
const TB_ROW_SIZEDATEREV_BOTTOM = 28_507;
// Column split offsets, measured from the block's own left edge (x0).
const TB_COL_SIZE_DATE_SPLIT = 20_000;
const TB_COL_DATE_REV_SPLIT = 84_000;

export interface TitleBlockInfo {
  title: string;
  date: string;
  rev: string;
  fileName: string;
  sheetPath: string;
}

function frameLine(ctx: CanvasRenderingContext2D, hair: number) {
  ctx.strokeStyle = layerColor("LAYER_SCHEMATIC_DRAWINGSHEET");
  ctx.lineWidth = Math.max(FRAME_STROKE_UM, hair);
}

/** Cell boundary offsets from 0 to `totalUm`, `pitchUm` apart, with a shorter final cell absorbing the remainder -- matches the real export's fixed-pitch (not fixed-count) zone-reference grid. */
function divisions(totalUm: number, pitchUm: number): number[] {
  const bounds = [0];
  let x = 0;
  while (x + pitchUm < totalUm) {
    x += pitchUm;
    bounds.push(x);
  }
  bounds.push(totalUm);
  return bounds;
}

/** The A4 page background + the double-rule frame border. Drawn first, under everything else including the grid. */
export function drawPageAndFrame(ctx: CanvasRenderingContext2D, view: ViewTransform): void {
  const hair = 1 / view.scale;

  // Page background (the paper itself) -- same color the container div
  // already fills screen-space with, painted again here in world-space
  // so it's exactly page-sized.
  ctx.fillStyle = layerColor("LAYER_SCHEMATIC_BACKGROUND");
  ctx.fillRect(0, 0, PAGE_WIDTH_UM, PAGE_HEIGHT_UM);

  frameLine(ctx, hair);
  ctx.strokeRect(OUTER_MARGIN_UM, OUTER_MARGIN_UM, PAGE_WIDTH_UM - 2 * OUTER_MARGIN_UM, PAGE_HEIGHT_UM - 2 * OUTER_MARGIN_UM);
  ctx.strokeRect(INNER_MARGIN_UM, INNER_MARGIN_UM, PAGE_WIDTH_UM - 2 * INNER_MARGIN_UM, PAGE_HEIGHT_UM - 2 * INNER_MARGIN_UM);
}

/** Zone-reference ticks + numbers/letters, on all four edges, between the frame's outer and inner lines. */
export function drawZoneReferences(ctx: CanvasRenderingContext2D, view: ViewTransform): void {
  const hair = 1 / view.scale;
  const innerW = PAGE_WIDTH_UM - 2 * OUTER_MARGIN_UM;
  const innerH = PAGE_HEIGHT_UM - 2 * OUTER_MARGIN_UM;
  const cols = divisions(innerW, ZONE_PITCH_UM);
  const rows = divisions(innerH, ZONE_PITCH_UM);

  frameLine(ctx, hair);
  ctx.font = `${ZONE_LABEL_SIZE_UM}px sans-serif`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillStyle = layerColor("LAYER_SCHEMATIC_DRAWINGSHEET");

  // Numbers, left-to-right (the real export's "1" sits at the left/near
  // corner, ascending rightward), along the top and bottom edges.
  for (let i = 0; i < cols.length - 1; i++) {
    const cx = OUTER_MARGIN_UM + (cols[i]! + cols[i + 1]!) / 2;
    const label = String(i + 1);
    ctx.fillText(label, cx, OUTER_MARGIN_UM / 2);
    ctx.fillText(label, cx, PAGE_HEIGHT_UM - OUTER_MARGIN_UM / 2);
    if (i > 0) {
      const x = OUTER_MARGIN_UM + cols[i]!;
      ctx.beginPath();
      ctx.moveTo(x, OUTER_MARGIN_UM - ZONE_TICK_UM);
      ctx.lineTo(x, OUTER_MARGIN_UM);
      ctx.moveTo(x, PAGE_HEIGHT_UM - OUTER_MARGIN_UM);
      ctx.lineTo(x, PAGE_HEIGHT_UM - OUTER_MARGIN_UM + ZONE_TICK_UM);
      ctx.stroke();
    }
  }

  // Letters, top-to-bottom, along the left and right edges.
  for (let i = 0; i < rows.length - 1; i++) {
    const cy = OUTER_MARGIN_UM + (rows[i]! + rows[i + 1]!) / 2;
    const label = String.fromCharCode(65 + i);
    ctx.fillText(label, OUTER_MARGIN_UM / 2, cy);
    ctx.fillText(label, PAGE_WIDTH_UM - OUTER_MARGIN_UM / 2, cy);
    if (i > 0) {
      const y = OUTER_MARGIN_UM + rows[i]!;
      ctx.beginPath();
      ctx.moveTo(OUTER_MARGIN_UM - ZONE_TICK_UM, y);
      ctx.lineTo(OUTER_MARGIN_UM, y);
      ctx.moveTo(PAGE_WIDTH_UM - OUTER_MARGIN_UM, y);
      ctx.lineTo(PAGE_WIDTH_UM - OUTER_MARGIN_UM + ZONE_TICK_UM, y);
      ctx.stroke();
    }
  }
}

/**
 * The title block: 108x32mm, flush with the inner frame's bottom-right
 * corner. Row/column split positions and every field's exact position
 * are read off the real export (see header comment) -- the 4 "Comment"
 * rows above File/Sheet are left blank (this app has nothing meaningful
 * to put in them, and a real KiCad sheet with unset comments looks
 * exactly like this: present, ruled, empty).
 */
export function drawTitleBlock(ctx: CanvasRenderingContext2D, view: ViewTransform, info: TitleBlockInfo): void {
  const hair = 1 / view.scale;
  const x1 = PAGE_WIDTH_UM - INNER_MARGIN_UM;
  const y1 = PAGE_HEIGHT_UM - INNER_MARGIN_UM;
  const x0 = x1 - TB_WIDTH_UM;
  const y0 = y1 - TB_HEIGHT_UM;

  frameLine(ctx, hair);
  ctx.strokeRect(x0, y0, TB_WIDTH_UM, TB_HEIGHT_UM);
  for (const yOff of [TB_ROW_COMMENTS_BOTTOM, TB_ROW_FILESHEET_BOTTOM, TB_ROW_TITLE_BOTTOM, TB_ROW_SIZEDATEREV_BOTTOM]) {
    ctx.beginPath();
    ctx.moveTo(x0, y0 + yOff);
    ctx.lineTo(x1, y0 + yOff);
    ctx.stroke();
  }
  // Size|Date split -- only within the Size/Date/Rev row.
  ctx.beginPath();
  ctx.moveTo(x0 + TB_COL_SIZE_DATE_SPLIT, y0 + TB_ROW_TITLE_BOTTOM);
  ctx.lineTo(x0 + TB_COL_SIZE_DATE_SPLIT, y0 + TB_ROW_SIZEDATEREV_BOTTOM);
  // Date|Rev split -- spans both the Size/Date/Rev row and the row below it (generator | sheet-id).
  ctx.moveTo(x0 + TB_COL_DATE_REV_SPLIT, y0 + TB_ROW_TITLE_BOTTOM);
  ctx.lineTo(x0 + TB_COL_DATE_REV_SPLIT, y0 + TB_HEIGHT_UM);
  ctx.stroke();

  ctx.fillStyle = layerColor("LAYER_SCHEMATIC_DRAWINGSHEET");
  ctx.textBaseline = "alphabetic";
  ctx.textAlign = "start";

  const text = (s: string, xOff: number, yOff: number, sizeUm: number, bold = false) => {
    ctx.save();
    ctx.font = `${bold ? "bold " : ""}${sizeUm}px sans-serif`;
    ctx.fillText(s, x0 + xOff, y0 + yOff);
    ctx.restore();
  };

  text(`Sheet: ${info.sheetPath}`, 1_000, TB_ROW_FILESHEET_BOTTOM - 3_750, 2_000);
  text(`File: ${info.fileName}`, 1_000, TB_ROW_FILESHEET_BOTTOM - 1_050, 2_000);
  text(`Title: ${info.title}`, 1_000, TB_ROW_TITLE_BOTTOM - 1_200, 2_667, true);
  text(`Size: A4`, 1_000, TB_ROW_SIZEDATEREV_BOTTOM - 650, 2_000);
  text(`Date: ${info.date}`, TB_COL_SIZE_DATE_SPLIT + 3_000, TB_ROW_SIZEDATEREV_BOTTOM - 650, 2_000);
  text(`Rev: ${info.rev}`, TB_COL_DATE_REV_SPLIT + 2_000, TB_ROW_SIZEDATEREV_BOTTOM - 650, 2_000);
  text(`KiCad E.D.A. eda studio`, 1_000, TB_HEIGHT_UM - 1_350, 2_000);
  text(`Id: 1/1`, TB_COL_DATE_REV_SPLIT + 2_000, TB_HEIGHT_UM - 1_350, 2_000);
}

/**
 * Grid dots -- schematic/layout.ts's GRID (1.27mm / 50 mil, eeschema's
 * own default), in KiCad's schematic grid color. Dots, matching KiCad's
 * default schematic grid style (the PCB editor's own drawGrid in
 * canvas/painter.ts draws the same way, for the same reason: only the
 * visible viewport, clamped to the page rect, not the whole fixed page
 * unconditionally -- an A4 sheet at 1.27mm pitch is ~39k intersections,
 * not worth redrawing in full on every pan/zoom).
 */
export function drawGridDots(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, gridUm: number): void {
  const stepPx = gridUm * view.scale;
  if (stepPx < 4) return; // too dense to be useful -- same threshold as the PCB grid
  const x0 = Math.max(0, -view.x / view.scale);
  const y0 = Math.max(0, -view.y / view.scale);
  const x1 = Math.min(PAGE_WIDTH_UM, x0 + widthPx / view.scale + gridUm);
  const y1 = Math.min(PAGE_HEIGHT_UM, y0 + heightPx / view.scale + gridUm);
  const firstX = Math.floor(x0 / gridUm) * gridUm;
  const firstY = Math.floor(y0 / gridUm) * gridUm;
  ctx.fillStyle = layerColor("LAYER_SCHEMATIC_GRID");
  const r = Math.max(120, (stepPx < 8 ? 0.6 : 0.9) / view.scale);
  for (let x = firstX; x <= x1; x += gridUm) {
    for (let y = firstY; y <= y1; y += gridUm) {
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
    }
  }
}
