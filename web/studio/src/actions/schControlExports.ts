// The control actions that hand the browser a file or the clipboard something: Save Current Sheet Copy As... (the sheet being shown as a `.kicad_sch`), Export Drawing
// to Clipboard (the sheet as a picture) and Export Symbol as SVG (a symbol plotted by kicad-cli). Plain async functions over the action context.
import type { SchControlContext } from "./schControlActions";
import { fetchHierarchy, fetchSchematicFiles, postSymbolSvg } from "../api/schControlClient";
import { fileStem } from "../kicad-port/saveAs";
import { samePath } from "../kicad-port/sheetPages";
import { saveBlob, saveTextFile } from "../api/libraryClient";
import { layerColor } from "../components/canvas/layers";
import { drawPageAndFrame, drawTitleBlock, drawZoneReferences, PAGE_HEIGHT_UM, PAGE_WIDTH_UM } from "../components/schematic/drawingSheet";
import { paintSchematic } from "../components/schematic/painter";
import { DEFAULT_SCH_DISPLAY } from "../components/schematic/displayOptions";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * `SCH_EDITOR_CONTROL::SaveCurrSheetCopyAs`: `m_frame->saveSchematicFile( curr_sheet, newFilename )` writes the one screen being shown to a new
 * file -- the root sheet under the board's name, a sub-sheet under its own file name (the sheet blocks that place it name it).
 */
export async function saveSheetCopy(ctx: SchControlContext): Promise<void> {
  const toast = (text: string, kind: "info" | "error") => ctx.dispatch({ type: "TOAST", message: text, kind });
  try {
    const [files, hierarchy] = await Promise.all([fetchSchematicFiles(), fetchHierarchy()]);
    const path = ctx.state.currentSheetPath;
    const sheet = hierarchy.find((h) => samePath(h.path, path));
    const atRoot = path.length === 0;
    const source = atRoot ? files[0] : files.find((f) => f.name === sheet?.file);
    if (!source) return toast("This sheet has no file to copy.", "error");
    const name = atRoot ? `${fileStem(ctx.state.board?.name ?? "board")}.kicad_sch` : source.name;
    saveTextFile(source.text, name);
    toast(`Saved a copy of this sheet as ${name}`, "info");
  } catch (e) {
    toast(`Could not save the sheet: ${message(e)}`, "error");
  }
}

/** Pixels per millimetre of the sheet picture: A4 comes out 2376 x 1680, sharp enough to paste into a document. */
const SHEET_PX_PER_MM = 8;

/**
 * The whole page of the sheet being shown -- drawing sheet, title block and every item, nothing selected -- on a canvas of its own (`renderSelectionToImageForClipboard`
 * over `BOX2I( 0, 0, page size )`): the same painters the editor draws with, at a fixed scale instead of the view's.
 */
export function renderSheetImage(ctx: SchControlContext): HTMLCanvasElement | null {
  const sch = ctx.state.schematic;
  if (!sch) return null;
  const scale = SHEET_PX_PER_MM / 1000;
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(PAGE_WIDTH_UM * scale);
  canvas.height = Math.round(PAGE_HEIGHT_UM * scale);
  const g = canvas.getContext("2d");
  if (!g) return null;
  const view = { x: 0, y: 0, scale };
  g.fillStyle = layerColor("LAYER_SCHEMATIC_BACKGROUND");
  g.fillRect(0, 0, canvas.width, canvas.height);
  g.save();
  // like the editor's own canvas: the context carries the scale, and the painters get the same view to size their hairlines by
  g.scale(scale, scale);
  drawPageAndFrame(g, view);
  drawZoneReferences(g, view);
  const tb = sch.title_block;
  drawTitleBlock(g, view, {
    title: tb?.title || ctx.state.board?.name || "untitled",
    date: tb?.date ?? new Date().toISOString().slice(0, 10),
    rev: tb?.rev ?? "",
    company: tb?.company,
    fileName: `${ctx.state.board?.name || "schematic"}.kicad_sch`,
    sheetPath: "/",
  });
  paintSchematic(g, view, sch, { selection: new Set(), netHighlight: null, display: ctx.control.display ?? DEFAULT_SCH_DISPLAY });
  g.restore();
  return canvas;
}

/**
 * `SCH_EDITOR_CONTROL::DrawSheetOnClipboard`: the sheet being shown, as a picture, onto the clipboard. "Cannot create the schematic image" when there is no picture to make;
 * the browser may also refuse the clipboard (it wants the page in focus and, in some browsers, a permission).
 */
export async function copySheetImage(ctx: SchControlContext): Promise<void> {
  const toast = (text: string, kind: "info" | "error") => ctx.dispatch({ type: "TOAST", message: text, kind });
  const canvas = renderSheetImage(ctx);
  const blob = canvas ? await new Promise<Blob | null>((done) => canvas.toBlob(done, "image/png")) : null;
  if (!blob) return toast("Cannot create the schematic image", "error");
  try {
    await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
    toast("Copied the sheet drawing to the clipboard.", "info");
  } catch (e) {
    // A browser that keeps the clipboard from a page (no permission, no focus, no image support): the picture is saved as a file instead.
    const name = `${fileStem(ctx.state.board?.name ?? "schematic")}.png`;
    saveBlob(blob, name);
    toast(`The browser would not put the picture on the clipboard (${message(e)}), so it was saved as ${name}.`, "info");
  }
}

/**
 * `SYMBOL_EDITOR_CONTROL::ExportSymbolAsSVG`: the symbol open in the Symbol Editor, in the unit and body style being edited, plotted to `<symbol name>.svg`. The plot is
 * kicad-cli's (`kicad-cli sym export svg` through the server's lane); the browser saves the SVG it wrote.
 */
export async function exportSymbolSvg(ctx: SchControlContext): Promise<void> {
  const toast = (text: string, kind: "info" | "error") => ctx.dispatch({ type: "TOAST", message: text, kind });
  const st = ctx.symApi.getState();
  const symbol = st.symbol;
  if (!symbol) return toast("No symbol to export", "error");
  try {
    const r = await postSymbolSvg({ lib_id: symbol.lib_id, unit: Math.max(st.activeUnit, 1), body_style: symbol.has_alternate_body_style && st.activeBodyStyle === 2 ? 2 : 1 });
    if (!r.ok || !r.svg) return toast(r.message ?? "kicad-cli wrote no SVG", "error");
    saveTextFile(r.svg.text, r.svg.name, "image/svg+xml");
    toast(`Saved ${r.svg.name}`, "info");
  } catch (e) {
    toast(`Could not export the symbol: ${message(e)}`, "error");
  }
}
