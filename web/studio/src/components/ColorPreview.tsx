// The Colors pages' previews (UI-LEFT-3 C3-12): KiCad shows the page's colours on a little board or sheet of its own at the right of the swatches, not on the editor's
// canvas.
//   * PCB Editor > Colors (`PANEL_PCBNEW_COLOR_SETTINGS::createPreviewItems`): `g_previewBoard` -- the board text of pcbnew/dialogs/panel_pcbnew_color_settings.cpp --
//     loaded by `PCB_IO_KICAD_SEXPR::DoLoad` into a `FOOTPRINT_PREVIEW_PANEL`, on a 6000 x 5000 mil page titled "Color Preview", fitted to the board's box with a
//     tenth to spare (`zoomFitPreview`: `scale / 1.1`, centred on the box).
//   * Schematic Editor > Colors (`PANEL_EESCHEMA_COLOR_SETTINGS::createPreviewItems`): the wires, buses, notes, labels, OPA604 and sheet it builds in code (LABEL_{0}
//     selected), on the same page, fitted to the page with a fifth to spare (`scale * 0.8`, centred on the drawing sheet).
// Both are what the studio's own server reads of them (src/kicad/color_preview.json, tools/make-color-preview.sh: the board through `eda import-kicad`, its zone
// filled by kicad-cli; the sheet written out as a `.kicad_sch` by tools/extract-color-preview.mjs), painted by the editors' own painters in the page's colours
// (`updatePreview`: `LoadColors( m_currentSettings )`; here `setEditorThemePreview`, which the page sets while the dialog is up, and `paintingIn( editor )`).
// The Footprint Editor's and the Symbol Editor's pages have no preview, as in KiCad.
import { useEffect, useRef } from "react";
import previewData from "../kicad/color_preview.json";
import type { BoardState, FillReport, Schematic } from "../api/types";
import type { ColorTheme } from "../kicad-port/colorTheme";
import { paintingIn } from "../state/colorThemeStore";
import { layerColor } from "./canvas/layers";
import { paintBoard, type PaintOptions } from "./canvas/painter";
import { PLAIN_APPEARANCE } from "./canvas/appearancePaint";
import { paintSchematic } from "./schematic/painter";
import { drawPaper, drawSchematicDrawingSheet, pageOf } from "./schematic/drawingSheet";
import { schematicSheetPage } from "./drawingSheet/sheetInfo";

const DATA = previewData as unknown as { board: BoardState; fill: FillReport; sheet: Schematic };

/** `m_page->SetWidthMils( 6000 )`, `SetHeightMils( 5000 )`, nm. */
const PAGE_NM: [number, number] = [6000 * 25_400, 5000 * 25_400];

/** The preview's size on the page (`m_preview` fills the panel at the right of the swatches). */
export const PREVIEW_SIZE = { width: 420, height: 400 };

/** `wxDateTime::Now().FormatDate()` of the title block: today. */
const today = () => new Date().toISOString().slice(0, 10);

/** `BOARD::GetBoundingBox` of the preview board: its outline, footprints (their pads and positions), tracks and vias. */
function boardBox(b: BoardState): [number, number, number, number] {
  const xs: number[] = [];
  const ys: number[] = [];
  const add = (x: number, y: number) => (xs.push(x), ys.push(y));
  for (const [x, y] of b.outline ?? []) add(x, y);
  for (const p of b.parts ?? []) {
    if (p.at) add(p.at[0], p.at[1]);
    for (const pad of p.pads ?? []) add(pad.x, pad.y);
  }
  for (const t of b.routing?.tracks ?? []) for (const [x, y] of t.pts) add(x, y);
  for (const v of b.routing?.vias ?? []) add(v.x, v.y);
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
}

/** `zoomFitPreview`: the box fitted to the canvas (`scale = 1 / max( w / W, h / H )`) divided by `margin`, centred on the box's centre. */
function fitView(box: [number, number, number, number], width: number, height: number, margin: number) {
  const [x0, y0, x1, y1] = box;
  const scale = 1 / Math.max((x1 - x0) / width, (y1 - y0) / height) / margin;
  return { scale, x: width / 2 - ((x0 + x1) / 2) * scale, y: height / 2 - ((y0 + y1) / 2) * scale };
}

function useCanvas(paint: (ctx: CanvasRenderingContext2D, width: number, height: number) => void, deps: unknown[]) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    const { width, height } = PREVIEW_SIZE;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.save();
    ctx.scale(dpr, dpr);
    paint(ctx, width, height);
    ctx.restore();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return ref;
}

/** PCB Editor > Colors' preview board, in `working`'s colours (the page sets them as the PCB editor's preview theme). */
export function PcbColorPreview({ working }: { working: ColorTheme }) {
  const ref = useCanvas(
    (ctx, width, height) =>
      paintingIn("pcb", () => {
        const board = DATA.board;
        const view = fitView(boardBox(board), width, height, 1.1);
        ctx.fillStyle = layerColor("background");
        ctx.fillRect(0, 0, width, height);
        ctx.translate(view.x, view.y);
        ctx.scale(view.scale, view.scale);
        paintBoard(ctx, view, width, height, board, {
          selection: new Set(),
          hot: new Set(),
          netHighlight: null,
          showRatsnest: true,
          ratsnestCurved: false,
          ratsnestEdges: [],
          layerVisible: {},
          layerOpacity: {},
          activeLayer: "F.Cu",
          highContrast: false,
          gridNm: 1_270_000,
          gridVisible: false,
          movePreview: null,
          drawState: null,
          cursorNm: null,
          activeTool: "select",
          sketchPads: false,
          sketchTracks: false,
          sketchVias: false,
          drcViolations: null,
          drcSelected: null,
          currentViaPreset: null,
          units: "mm",
          zoneCornerPreview: null,
          shapePointPreview: null,
          zoneFill: DATA.fill,
          zoneDisplayMode: "filled",
          textRecordTag: "color-preview",
          // the preview's own page: "Color Preview", today, 6000 x 5000 mils (`DS_PROXY_VIEW_ITEM` with `LAYER_DRAWINGSHEET` and `LAYER_PAGE_LIMITS`)
          appearance: { ...PLAIN_APPEARANCE, sheet: { widthNm: PAGE_NM[0], heightNm: PAGE_NM[1], paper: "User", title: "Color Preview", date: today(), rev: "", company: "", comments: [], file: "preview" } },
        } satisfies PaintOptions);
        if (typeof window !== "undefined") (window as unknown as { __edaColorPreview?: Record<string, unknown> }).__edaColorPreview = { ...((window as unknown as { __edaColorPreview?: Record<string, unknown> }).__edaColorPreview ?? {}), pcb: { background: layerColor("background"), at: Date.now() } };
      }),
    [working]
  );
  return <canvas ref={ref} data-testid="pcb-colors-preview" aria-label="Preview" style={{ width: PREVIEW_SIZE.width, height: PREVIEW_SIZE.height, flexShrink: 0, border: "1px solid var(--chrome-border)" }} />;
}

/** Schematic Editor > Colors' preview sheet, in `working`'s colours (the page sets them as the schematic's preview theme). */
export function SchColorPreview({ working }: { working: ColorTheme }) {
  const ref = useCanvas(
    (ctx, width, height) =>
      paintingIn("schematic", () => {
        const sch: Schematic = { ...DATA.sheet, title_block: DATA.sheet.title_block ? { ...DATA.sheet.title_block, date: today() } : DATA.sheet.title_block };
        const view = fitView([0, 0, PAGE_NM[0], PAGE_NM[1]], width, height, 1 / 0.8);
        ctx.fillStyle = layerColor("LAYER_SCHEMATIC_BACKGROUND");
        ctx.fillRect(0, 0, width, height);
        ctx.translate(view.x, view.y);
        ctx.scale(view.scale, view.scale);
        drawPaper(ctx, pageOf(sch.paper));
        drawSchematicDrawingSheet(ctx, view, schematicSheetPage(sch, "preview"), null);
        // `t2->SetSelected()`: LABEL_{0} drawn selected
        const selected = new Set(sch.labels.filter((l) => l.net === "LABEL_{0}").map((l) => l.id));
        paintSchematic(ctx, view, sch, { selection: selected, netHighlight: null });
        if (typeof window !== "undefined") (window as unknown as { __edaColorPreview?: Record<string, unknown> }).__edaColorPreview = { ...((window as unknown as { __edaColorPreview?: Record<string, unknown> }).__edaColorPreview ?? {}), schematic: { background: layerColor("LAYER_SCHEMATIC_BACKGROUND"), wire: layerColor("LAYER_WIRE"), at: Date.now() } };
      }),
    [working]
  );
  return <canvas ref={ref} data-testid="sch-colors-preview" aria-label="Preview" style={{ width: PREVIEW_SIZE.width, height: PREVIEW_SIZE.height, flexShrink: 0, border: "1px solid var(--chrome-border)" }} />;
}
