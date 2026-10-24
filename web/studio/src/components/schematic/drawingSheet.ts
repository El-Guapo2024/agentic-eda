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

/** A4 landscape, KiCad's default schematic page size -- nm (mm * 1e6). */
export const PAGE_WIDTH_NM = 297_000_000;
export const PAGE_HEIGHT_NM = 210_000_000;

/** A sheet's paper, landscape, um: what the frame, the zone references, the grid and the title block are drawn for. */
export interface PageSize {
  name: string;
  widthNm: number;
  heightNm: number;
}

export const A4_PAGE: PageSize = { name: "A4", widthNm: PAGE_WIDTH_NM, heightNm: PAGE_HEIGHT_NM };

/** The paper `GET /api/schematic` reports for a sheet (`paper`), A4 when it reports none (a backend from before sheets chose their own). */
export function pageOf(paper: { name: string; width_nm: number; height_nm: number } | null | undefined): PageSize {
  return paper ? { name: paper.name, widthNm: paper.width_nm, heightNm: paper.height_nm } : A4_PAGE;
}

// Double-rule frame: an outer line 10mm from the paper edge, an inner
// line 12mm from it (both in LAYER_SCHEMATIC_DRAWINGSHEET red, not a
// dashed "page limit" -- the real export has no dashed line at all).
const OUTER_MARGIN_NM = 10_000_000;
const INNER_MARGIN_NM = 12_000_000;
const FRAME_STROKE_NM = 152_000; // 0.1524mm in the source SVG

// Zone-reference grid: fixed 50mm pitch from the outer frame's corner,
// however many whole+remainder cells that yields (6 columns, 4 rows for
// A4 landscape) -- not "divide evenly into a fixed count".
const ZONE_PITCH_NM = 50_000_000;
const ZONE_LABEL_SIZE_NM = 1_733_000;
/** Approximates Canvas2D's old textBaseline:"middle" for stroke text -- see painter.ts's MIDDLE_OFFSET_FACTOR for the same constant and its derivation. */
const ZONE_LABEL_MIDDLE_OFFSET = 0.35;
const ZONE_TICK_NM = 2_000_000; // outer margin (10mm) minus inner margin (12mm) -- the tick spans exactly that gap

// Title block: flush with the inner frame's bottom-right corner.
const TB_WIDTH_NM = 108_000_000;
const TB_HEIGHT_NM = 32_000_000;
// Row band bottom edges, measured from the block's own top edge (y0).
const TB_ROW_COMMENTS_BOTTOM = 15_507_000;
const TB_ROW_FILESHEET_BOTTOM = 21_507_000;
const TB_ROW_TITLE_BOTTOM = 25_507_000;
const TB_ROW_SIZEDATEREV_BOTTOM = 28_507_000;
// Column split offsets, measured from the block's own left edge (x0).
const TB_COL_SIZE_DATE_SPLIT = 20_000_000;
const TB_COL_DATE_REV_SPLIT = 84_000_000;

export interface TitleBlockInfo {
  title: string;
  date: string;
  rev: string;
  fileName: string;
  sheetPath: string;
  /** `${COMPANY}`, drawn bold above the comments. */
  company?: string;
  /** `${COMMENT1}`..`${COMMENT4}`: the default drawing sheet shows the first four comments (`drawing_sheet_default_description.cpp`). */
  comments?: readonly string[];
}

function frameLine(ctx: CanvasRenderingContext2D, hair: number) {
  ctx.strokeStyle = layerColor("LAYER_SCHEMATIC_DRAWINGSHEET");
  ctx.lineWidth = Math.max(FRAME_STROKE_NM, hair);
}

/** Cell boundary offsets from 0 to `totalNm`, `pitchNm` apart, with a shorter final cell absorbing the remainder -- matches the real export's fixed-pitch (not fixed-count) zone-reference grid. */
function divisions(totalNm: number, pitchNm: number): number[] {
  const bounds = [0];
  let x = 0;
  while (x + pitchNm < totalNm) {
    x += pitchNm;
    bounds.push(x);
  }
  bounds.push(totalNm);
  return bounds;
}

/** The A4 page background + the double-rule frame border. Drawn first, under everything else including the grid. */
export function drawPageAndFrame(ctx: CanvasRenderingContext2D, view: ViewTransform, page: PageSize = A4_PAGE): void {
  const hair = 1 / view.scale;

  // Page background (the paper itself) -- same color the container div
  // already fills screen-space with, painted again here in world-space
  // so it's exactly page-sized.
  ctx.fillStyle = layerColor("LAYER_SCHEMATIC_BACKGROUND");
  ctx.fillRect(0, 0, page.widthNm, page.heightNm);

  frameLine(ctx, hair);
  ctx.strokeRect(OUTER_MARGIN_NM, OUTER_MARGIN_NM, page.widthNm - 2 * OUTER_MARGIN_NM, page.heightNm - 2 * OUTER_MARGIN_NM);
  ctx.strokeRect(INNER_MARGIN_NM, INNER_MARGIN_NM, page.widthNm - 2 * INNER_MARGIN_NM, page.heightNm - 2 * INNER_MARGIN_NM);
}

/** Zone-reference ticks + numbers/letters, on all four edges, between the frame's outer and inner lines. */
export function drawZoneReferences(ctx: CanvasRenderingContext2D, view: ViewTransform, page: PageSize = A4_PAGE): void {
  const hair = 1 / view.scale;
  const innerW = page.widthNm - 2 * OUTER_MARGIN_NM;
  const innerH = page.heightNm - 2 * OUTER_MARGIN_NM;
  const cols = divisions(innerW, ZONE_PITCH_NM);
  const rows = divisions(innerH, ZONE_PITCH_NM);

  frameLine(ctx, hair);
  const color = layerColor("LAYER_SCHEMATIC_DRAWINGSHEET");
  const labelY = ZONE_LABEL_SIZE_NM * ZONE_LABEL_MIDDLE_OFFSET;

  // Numbers, left-to-right (the real export's "1" sits at the left/near
  // corner, ascending rightward), along the top and bottom edges.
  for (let i = 0; i < cols.length - 1; i++) {
    const cx = OUTER_MARGIN_NM + (cols[i]! + cols[i + 1]!) / 2;
    const label = String(i + 1);
    drawStrokeText(ctx, label, cx, OUTER_MARGIN_NM / 2 + labelY, { sizeNm: ZONE_LABEL_SIZE_NM, justify: "center", color });
    drawStrokeText(ctx, label, cx, page.heightNm - OUTER_MARGIN_NM / 2 + labelY, { sizeNm: ZONE_LABEL_SIZE_NM, justify: "center", color });
    if (i > 0) {
      const x = OUTER_MARGIN_NM + cols[i]!;
      ctx.beginPath();
      ctx.moveTo(x, OUTER_MARGIN_NM - ZONE_TICK_NM);
      ctx.lineTo(x, OUTER_MARGIN_NM);
      ctx.moveTo(x, page.heightNm - OUTER_MARGIN_NM);
      ctx.lineTo(x, page.heightNm - OUTER_MARGIN_NM + ZONE_TICK_NM);
      ctx.stroke();
    }
  }

  // Letters, top-to-bottom, along the left and right edges.
  for (let i = 0; i < rows.length - 1; i++) {
    const cy = OUTER_MARGIN_NM + (rows[i]! + rows[i + 1]!) / 2;
    const label = String.fromCharCode(65 + i);
    drawStrokeText(ctx, label, OUTER_MARGIN_NM / 2, cy + labelY, { sizeNm: ZONE_LABEL_SIZE_NM, justify: "center", color });
    drawStrokeText(ctx, label, page.widthNm - OUTER_MARGIN_NM / 2, cy + labelY, { sizeNm: ZONE_LABEL_SIZE_NM, justify: "center", color });
    if (i > 0) {
      const y = OUTER_MARGIN_NM + rows[i]!;
      ctx.beginPath();
      ctx.moveTo(OUTER_MARGIN_NM - ZONE_TICK_NM, y);
      ctx.lineTo(OUTER_MARGIN_NM, y);
      ctx.moveTo(page.widthNm - OUTER_MARGIN_NM, y);
      ctx.lineTo(page.widthNm - OUTER_MARGIN_NM + ZONE_TICK_NM, y);
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
export function drawTitleBlock(ctx: CanvasRenderingContext2D, view: ViewTransform, info: TitleBlockInfo, page: PageSize = A4_PAGE): void {
  const hair = 1 / view.scale;
  const x1 = page.widthNm - INNER_MARGIN_NM;
  const y1 = page.heightNm - INNER_MARGIN_NM;
  const x0 = x1 - TB_WIDTH_NM;
  const y0 = y1 - TB_HEIGHT_NM;

  frameLine(ctx, hair);
  ctx.strokeRect(x0, y0, TB_WIDTH_NM, TB_HEIGHT_NM);
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
  ctx.lineTo(x0 + TB_COL_DATE_REV_SPLIT, y0 + TB_HEIGHT_NM);
  ctx.stroke();

  const tbColor = layerColor("LAYER_SCHEMATIC_DRAWINGSHEET");

  // Stroke text has no bold variant -- a thicker stroke stands in, same
  // as painter.ts's BOLD_THICKNESS_FACTOR (this module doesn't import
  // that one to avoid a schematic/schematic cross-import for one
  // constant; same value, same reasoning).
  const text = (s: string, xOff: number, yOff: number, sizeNm: number, bold = false) => {
    drawStrokeText(ctx, s, x0 + xOff, y0 + yOff, { sizeNm, thicknessNm: bold ? sizeNm / 5 : undefined, color: tbColor });
  };

  text(`Sheet: ${info.sheetPath}`, 1_000_000, TB_ROW_FILESHEET_BOTTOM - 3_750_000, 2_000_000);
  text(`File: ${info.fileName}`, 1_000_000, TB_ROW_FILESHEET_BOTTOM - 1_050_000, 2_000_000);
  text(`Title: ${info.title}`, 1_000_000, TB_ROW_TITLE_BOTTOM - 1_200_000, 2_667_000, true);
  text(`Size: ${page.name}`, 1_000_000, TB_ROW_SIZEDATEREV_BOTTOM - 650_000, 2_000_000);
  text(`Date: ${info.date}`, TB_COL_SIZE_DATE_SPLIT + 3_000_000, TB_ROW_SIZEDATEREV_BOTTOM - 650_000, 2_000_000);
  text(`Rev: ${info.rev}`, TB_COL_DATE_REV_SPLIT + 2_000_000, TB_ROW_SIZEDATEREV_BOTTOM - 650_000, 2_000_000);
  text(`KiCad E.D.A. eda studio`, 1_000_000, TB_HEIGHT_NM - 1_350_000, 2_000_000);
  text(`Id: 1/1`, TB_COL_DATE_REV_SPLIT + 2_000_000, TB_HEIGHT_NM - 1_350_000, 2_000_000);

  // `${COMPANY}` (bold) and `${COMMENT1}`..`${COMMENT4}` are the default sheet's `(tbtext ... (pos 109 20))` ... `(pos 109 32)`: 3 mm apart in the rows above
  // Sheet. A `pos` is the text's centre in mm above the margin corner, 2 mm below the block's bottom edge; the baseline is half the 1.5 mm text
  // below it -- the same relation the Title (`pos 10.7`, 2 mm) and File (`pos 14.3`) rows above use.
  const row = (s: string | undefined, posMm: number, bold = false) => {
    if (s) text(s, 1_000_000, TB_HEIGHT_NM - (posMm - 2.75) * 1_000_000, 2_000_000, bold);
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
export function drawGridDots(ctx: CanvasRenderingContext2D, view: ViewTransform, widthPx: number, heightPx: number, gridNm: number, page: PageSize = A4_PAGE): void {
  const stepPx = gridNm * view.scale;
  if (stepPx < 4) return; // too dense to be useful -- same threshold as the PCB grid
  const x0 = Math.max(0, -view.x / view.scale);
  const y0 = Math.max(0, -view.y / view.scale);
  const x1 = Math.min(page.widthNm, x0 + widthPx / view.scale + gridNm);
  const y1 = Math.min(page.heightNm, y0 + heightPx / view.scale + gridNm);
  const firstX = Math.floor(x0 / gridNm) * gridNm;
  const firstY = Math.floor(y0 / gridNm) * gridNm;
  ctx.fillStyle = layerColor("LAYER_SCHEMATIC_GRID");
  const r = Math.max(120_000, (stepPx < 8 ? 0.6 : 0.9) / view.scale);
  for (let x = firstX; x <= x1; x += gridNm) {
    for (let y = firstY; y <= y1; y += gridNm) {
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
    }
  }
}
