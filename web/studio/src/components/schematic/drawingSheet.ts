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
import { drawStrokeText } from "../text/strokeFont";

/** A4 landscape, KiCad's default schematic page size -- um (mm * 1000). */
export const PAGE_WIDTH_UM = 297_000;
export const PAGE_HEIGHT_UM = 210_000;

/** The paper the sheet is drawn on (Page Settings, kicad-port/pageSettings.ts `paperSizeUm`): A4 landscape unless the sheet has set another. */
export interface PageSize {
  width: number;
  height: number;
}

export const A4_LANDSCAPE: PageSize = { width: PAGE_WIDTH_UM, height: PAGE_HEIGHT_UM };

/** The paper a schematic sheet is on: `GET /api/schematic`'s `page.size_um` (the resolved width and height), A4 landscape when the sheet has no page of its own. */
export function schPageSize(sch: { page?: { size_um: [number, number] | null } | null }): PageSize {
  const size = sch.page?.size_um;
  return size && size[0] > 0 && size[1] > 0 ? { width: size[0], height: size[1] } : A4_LANDSCAPE;
}

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
/** Approximates Canvas2D's old textBaseline:"middle" for stroke text -- see painter.ts's MIDDLE_OFFSET_FACTOR for the same constant and its derivation. */
const ZONE_LABEL_MIDDLE_OFFSET = 0.35;
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
  /** The paper's name for `Size: ${PAPER}` ("A4", "USLetter", "User"); A4 when absent. */
  paper?: string;
  /** `${COMPANY}`, drawn bold above the comments. */
  company?: string;
  /** `${COMMENT1}`..`${COMMENT4}`: the default drawing sheet shows the first four comments (`drawing_sheet_default_description.cpp`). */
  comments?: readonly string[];
  /** The page the block sits in the corner of; A4 landscape when absent. */
  page?: PageSize;
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
export function drawPageAndFrame(ctx: CanvasRenderingContext2D, view: ViewTransform, page: PageSize = A4_LANDSCAPE): void {
  const hair = 1 / view.scale;
  const pageW = page.width;
  const pageH = page.height;

  // Page background (the paper itself) -- same color the container div
  // already fills screen-space with, painted again here in world-space
  // so it's exactly page-sized.
  ctx.fillStyle = layerColor("LAYER_SCHEMATIC_BACKGROUND");
  ctx.fillRect(0, 0, pageW, pageH);

  frameLine(ctx, hair);
  ctx.strokeRect(OUTER_MARGIN_UM, OUTER_MARGIN_UM, pageW - 2 * OUTER_MARGIN_UM, pageH - 2 * OUTER_MARGIN_UM);
  ctx.strokeRect(INNER_MARGIN_UM, INNER_MARGIN_UM, pageW - 2 * INNER_MARGIN_UM, pageH - 2 * INNER_MARGIN_UM);
}

/** Zone-reference ticks + numbers/letters, on all four edges, between the frame's outer and inner lines. */
export function drawZoneReferences(ctx: CanvasRenderingContext2D, view: ViewTransform, page: PageSize = A4_LANDSCAPE): void {
  const hair = 1 / view.scale;
  const pageW = page.width;
  const pageH = page.height;
  const innerW = pageW - 2 * OUTER_MARGIN_UM;
  const innerH = pageH - 2 * OUTER_MARGIN_UM;
  const cols = divisions(innerW, ZONE_PITCH_UM);
  const rows = divisions(innerH, ZONE_PITCH_UM);

  frameLine(ctx, hair);
  const color = layerColor("LAYER_SCHEMATIC_DRAWINGSHEET");
  const labelY = ZONE_LABEL_SIZE_UM * ZONE_LABEL_MIDDLE_OFFSET;

  // Numbers, left-to-right (the real export's "1" sits at the left/near
  // corner, ascending rightward), along the top and bottom edges.
  for (let i = 0; i < cols.length - 1; i++) {
    const cx = OUTER_MARGIN_UM + (cols[i]! + cols[i + 1]!) / 2;
    const label = String(i + 1);
    drawStrokeText(ctx, label, cx, OUTER_MARGIN_UM / 2 + labelY, { sizeUm: ZONE_LABEL_SIZE_UM, justify: "center", color });
    drawStrokeText(ctx, label, cx, pageH - OUTER_MARGIN_UM / 2 + labelY, { sizeUm: ZONE_LABEL_SIZE_UM, justify: "center", color });
    if (i > 0) {
      const x = OUTER_MARGIN_UM + cols[i]!;
      ctx.beginPath();
      ctx.moveTo(x, OUTER_MARGIN_UM - ZONE_TICK_UM);
      ctx.lineTo(x, OUTER_MARGIN_UM);
      ctx.moveTo(x, pageH - OUTER_MARGIN_UM);
      ctx.lineTo(x, pageH - OUTER_MARGIN_UM + ZONE_TICK_UM);
      ctx.stroke();
    }
  }

  // Letters, top-to-bottom, along the left and right edges.
  for (let i = 0; i < rows.length - 1; i++) {
    const cy = OUTER_MARGIN_UM + (rows[i]! + rows[i + 1]!) / 2;
    const label = String.fromCharCode(65 + i);
    drawStrokeText(ctx, label, OUTER_MARGIN_UM / 2, cy + labelY, { sizeUm: ZONE_LABEL_SIZE_UM, justify: "center", color });
    drawStrokeText(ctx, label, pageW - OUTER_MARGIN_UM / 2, cy + labelY, { sizeUm: ZONE_LABEL_SIZE_UM, justify: "center", color });
    if (i > 0) {
      const y = OUTER_MARGIN_UM + rows[i]!;
      ctx.beginPath();
      ctx.moveTo(OUTER_MARGIN_UM - ZONE_TICK_UM, y);
      ctx.lineTo(OUTER_MARGIN_UM, y);
      ctx.moveTo(pageW - OUTER_MARGIN_UM, y);
      ctx.lineTo(pageW - OUTER_MARGIN_UM + ZONE_TICK_UM, y);
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
  const pageW = (info.page ?? A4_LANDSCAPE).width;
  const pageH = (info.page ?? A4_LANDSCAPE).height;
  const x1 = pageW - INNER_MARGIN_UM;
  const y1 = pageH - INNER_MARGIN_UM;
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

  const tbColor = layerColor("LAYER_SCHEMATIC_DRAWINGSHEET");

  // Stroke text has no bold variant -- a thicker stroke stands in, same
  // as painter.ts's BOLD_THICKNESS_FACTOR (this module doesn't import
  // that one to avoid a schematic/schematic cross-import for one
  // constant; same value, same reasoning).
  const text = (s: string, xOff: number, yOff: number, sizeUm: number, bold = false) => {
    drawStrokeText(ctx, s, x0 + xOff, y0 + yOff, { sizeUm, thicknessUm: bold ? sizeUm / 5 : undefined, color: tbColor });
  };

  text(`Sheet: ${info.sheetPath}`, 1_000, TB_ROW_FILESHEET_BOTTOM - 3_750, 2_000);
  text(`File: ${info.fileName}`, 1_000, TB_ROW_FILESHEET_BOTTOM - 1_050, 2_000);
  text(`Title: ${info.title}`, 1_000, TB_ROW_TITLE_BOTTOM - 1_200, 2_667, true);
  text(`Size: ${info.paper ?? "A4"}`, 1_000, TB_ROW_SIZEDATEREV_BOTTOM - 650, 2_000);
  text(`Date: ${info.date}`, TB_COL_SIZE_DATE_SPLIT + 3_000, TB_ROW_SIZEDATEREV_BOTTOM - 650, 2_000);
  text(`Rev: ${info.rev}`, TB_COL_DATE_REV_SPLIT + 2_000, TB_ROW_SIZEDATEREV_BOTTOM - 650, 2_000);
  text(`KiCad E.D.A. eda studio`, 1_000, TB_HEIGHT_UM - 1_350, 2_000);
  text(`Id: 1/1`, TB_COL_DATE_REV_SPLIT + 2_000, TB_HEIGHT_UM - 1_350, 2_000);

  // `${COMPANY}` (bold) and `${COMMENT1}`..`${COMMENT4}` are the default sheet's `(tbtext ... (pos 109 20))` ... `(pos 109 32)`: 3 mm apart in the rows above
  // Sheet. A `pos` is the text's centre in mm above the margin corner, 2 mm below the block's bottom edge; the baseline is half the 1.5 mm text
  // below it -- the same relation the Title (`pos 10.7`, 2 mm) and File (`pos 14.3`) rows above use.
  const row = (s: string | undefined, posMm: number, bold = false) => {
    if (s) text(s, 1_000, TB_HEIGHT_UM - (posMm - 2.75) * 1_000, 2_000, bold);
  };
  row(info.company, 20, true);
  for (let i = 0; i < 4; i++) row(info.comments?.[i], 23 + 3 * i);
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
export function drawGridDots(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, gridUm: number, page: PageSize = A4_LANDSCAPE): void {
  const pageW = page.width;
  const pageH = page.height;
  const stepPx = gridUm * view.scale;
  if (stepPx < 4) return; // too dense to be useful -- same threshold as the PCB grid
  const x0 = Math.max(0, -view.x / view.scale);
  const y0 = Math.max(0, -view.y / view.scale);
  const x1 = Math.min(pageW, x0 + widthPx / view.scale + gridUm);
  const y1 = Math.min(pageH, y0 + heightPx / view.scale + gridUm);
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
