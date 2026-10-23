// The file-in actions: Import Graphics (File > Import > Graphics... in the board editor, `DRAWING_TOOL::PlaceImportedGraphics`; the schematic's and the symbol editor's twin is
// `SCH_DRAWING_TOOLS::ImportGraphics`) and the drops that reach them (`DdImportGraphics`: a DXF or SVG dropped on the window). Append Board and its drop are `appendBoardActions`
// below. A browser drop gives the page a file's text and not its path, so the drop actions take the file as their parameter -- `{ name, text }` -- where KiCad's takes a file name.
import { openImportGraphics, type PickedFile } from "../state/importDialogs";
import { graphicsFormatOf } from "../kicad-port/importGraphics";
import { appendBoardCmd, describeAppended, isKicadBoardName } from "../kicad-port/appendBoard";
import { acceptFor, asForeignParam, describeImport, describeIssues, describeRules, issuesOf, type ForeignFile, type ForeignParam, type ImportKind } from "../kicad-port/foreignImport";
import { readAppendBoard } from "../api/importClient";
import { pickBinaryFile, postImportForeign } from "../api/foreignImportClient";
import { postCmd } from "../api/client";
import { pickTextFile } from "../api/libraryClient";
import { askLayerMapping, importKeepKiCadLayerNames, importShowIssues, showImportReport } from "../state/foreignImport";
import { boundsOfPoints, fitTransform } from "../components/canvas/view";
import { schematicBounds } from "../components/schematic/schHit";
import type { Action, StudioApi } from "../state/store";
import type { Dispatch } from "react";
import type { ActionHandler } from "./commonActions";
import { describeExport, describeImport as describeSessionImport, SESSION_ACCEPT, specctraImportCmd } from "../kicad-port/specctra";
import { exportSpecctraDsn } from "../api/specctraClient";

/** A drop action's parameter: the file the drop host read. */
export function asPickedFile(param: unknown): PickedFile | null {
  if (!param || typeof param !== "object") return null;
  const f = param as Partial<PickedFile>;
  return typeof f.name === "string" && typeof f.text === "string" ? { name: f.name, text: f.text } : null;
}

export interface ImportActionContext {
  tab: string;
  dispatch: Dispatch<Action>;
  api: StudioApi;
}

/**
 * `PCB_CONTROL::AppendBoard`: the file's items are put on the board (the paste of its text, `Cmd::PasteClipboard`, one undo step) with the top left item at the cursor and carried by the
 * Move tool until a click drops them; Escape takes them back. What the paste leaves out or changes on this board is said afterwards.
 */
async function appendBoardFile(ctx: ImportActionContext, file: PickedFile): Promise<void> {
  const toast = (message: string, kind: "error" | "info") => ctx.dispatch({ type: "TOAST", message, kind });
  if (!isKicadBoardName(file.name)) return toast("Only KiCad boards (.kicad_pcb files) can be appended.", "error");
  let info;
  try {
    info = await readAppendBoard(file);
  } catch (e) {
    return toast(e instanceof Error ? e.message : String(e), "error");
  }
  if (!info.ok || !info.reference) return toast(info.message ?? "Error loading board.", "error");
  const reference = info.reference;
  const placed = await ctx.api.placeNewItems((at) => appendBoardCmd(file.text, reference, at), true);
  if (!placed) return; // the refusal is on screen as a toast
  const notes = info.notes ?? [];
  toast(`Appended ${describeAppended(info.counts)}.${notes.length > 0 ? ` ${notes.join(" ")}` : ""}`, "info");
}

/** The view is fitted to what was imported (the canvas fits by itself only until the view has been moved by hand, and an imported board is elsewhere). */
function fitToImport(ctx: ImportActionContext, kind: ImportKind): void {
  const rect = document.querySelector(".pcb-canvas-container")?.getBoundingClientRect();
  if (!rect || rect.width < 50 || rect.height < 50) return;
  const state = ctx.api.getState();
  if (kind === "schematic") {
    const bounds = state.schematic ? boundsOfPoints(schematicBounds(state.schematic)) : null;
    if (bounds) ctx.dispatch({ type: "SET_SCHEMATIC_VIEW", view: fitTransform(bounds, rect.width, rect.height, 80) });
    return;
  }
  const bounds = state.board?.outline ? boundsOfPoints(state.board.outline) : null;
  if (bounds) ctx.dispatch({ type: "SET_VIEW", view: fitTransform(bounds, rect.width, rect.height) });
}

/**
 * File > Import > Non-KiCad Board File... (`BOARD_EDITOR_CONTROL::OpenNonKicadBoard`) and Non-KiCad Schematic File... (`SCH_EDIT_FRAME::OnImportProject`): a file of another EDA format
 * replaces the board (or the schematic) that is open -- one command, so one undo step; KiCad cannot take it back, and the schematic's asks first because of that.
 *
 *   1. the file is chosen (a browser gives the page its bytes, not its path) and kicad-cli converts it (`POST /api/import_foreign`; nothing on the board changes yet);
 *   2. an importer that asks which KiCad layer each of its layers goes to (Altium, Eagle, PADS, CADSTAR, ...) is answered by the layer dialog (DIALOG_MAP_LAYERS), and the file is
 *      converted again with the layers the person moved (`--layer-map`);
 *   3. `replace_board` / `replace_schematic` makes it the board or the schematic;
 *   4. what the importers said is the toast and, with "Show import issues", a report.
 *
 * `given` is the file when the action was run with one (`window.__eda.run(id, { name, data })`) instead of through the chooser.
 */
async function importForeign(ctx: ImportActionContext, kind: ImportKind, given: ForeignParam | null): Promise<void> {
  const toast = (message: string, tone: "error" | "info") => ctx.dispatch({ type: "TOAST", message, kind: tone });
  try {
    const file: ForeignFile | null = given ?? (await pickBinaryFile(acceptFor(kind)));
    if (!file) return;
    let reply = await postImportForeign(kind, file, given?.layerMap);
    if (!reply.ok) return toast(reply.message ?? `The file '${file.name}' could not be imported.`, "error");
    let keepNames = given?.keepKicadLayerNames ?? importKeepKiCadLayerNames();
    if (kind === "board" && reply.maps_layers && !given?.layerMap && !given?.skipLayerMapping && reply.layers) {
      // `PCB_IO_EAGLE::mapEagleLayersToKicad`, `ALTIUM_PCB::...`: the importer asks, and the person answers.
      const answer = await askLayerMapping({ name: file.name, format: reply.format ?? "", layers: reply.layers, keepNames });
      if (!answer) return toast(`Import of '${file.name}' cancelled.`, "info");
      keepNames = answer.keepNames;
      if (Object.keys(answer.map).length > 0) {
        reply = await postImportForeign(kind, file, answer.map);
        if (!reply.ok) return toast(reply.message ?? `The file '${file.name}' could not be imported.`, "error");
      }
    }
    // The board comes with the project kicad-cli made of it: the design rules and the net classes the importer set are in that file and not in the board's.
    const cmd = kind === "board" ? ({ op: "replace_board", text: reply.text ?? "", name: file.name, keep_kicad_layer_names: keepNames, ...(reply.project ? { project: reply.project } : {}) } as const) : ({ op: "replace_schematic", content: reply.content, name: file.name } as const);
    // An import is not a move the gates judge: Strict is off for it, as for a paste.
    const done = await postCmd(cmd, false);
    if (!done.ok) return toast(done.message, "error");
    await ctx.api.refresh();
    if (kind === "schematic") await ctx.api.navigateToSheet([]);
    fitToImport(ctx, kind);
    const issues = describeIssues(reply.findings);
    const rules = describeRules(reply);
    toast(`${describeImport(reply)}${issues ? ` ${issues}` : ""}${rules ? ` ${rules}` : ""} Undo brings the ${kind === "board" ? "old board" : "old schematic"} back.`, "info");
    if (issuesOf(reply.findings).length > 0 && !given?.skipReport && importShowIssues()) {
      showImportReport({ name: file.name, format: reply.format ?? "", summary: `${describeImport(reply)} ${issues}`.trim(), findings: issuesOf(reply.findings), notes: reply.notes ?? [] });
    }
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), "error");
  }
}

/**
 * File > Import > Specctra Session... (`BOARD_EDITOR_CONTROL::ImportSpecctraSession`, `PCB_EDIT_FRAME::ImportSpecctraSession`): a session file an outside autorouter wrote is
 * chosen (`given`, `{ name, text }`, when the action is run with one), and the `specctra` command puts its wires and vias on the board in place of the unlocked ones, and the
 * footprints where its placement says -- one undo step, where KiCad clears the undo list. An import is not a move the gates judge: Strict is off for it.
 */
async function importSpecctraSession(ctx: ImportActionContext, given: PickedFile | null): Promise<void> {
  const toast = (message: string, kind: "error" | "info") => ctx.dispatch({ type: "TOAST", message, kind });
  try {
    const file = given ?? (await pickTextFile(SESSION_ACCEPT));
    if (!file) return;
    const done = await postCmd(specctraImportCmd(file), false);
    if (done.ok) await ctx.api.refresh();
    const said = describeSessionImport(done);
    toast(said.message, said.kind);
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), "error");
  }
}

/** File > Export > Specctra DSN... (`BOARD_EDITOR_CONTROL::ExportSpecctraDSN`, `PCB_EDIT_FRAME::ExportSpecctraFile`): the board's DSN, saved as a download. */
async function exportSpecctra(ctx: ImportActionContext): Promise<void> {
  const toast = (message: string, kind: "error" | "info") => ctx.dispatch({ type: "TOAST", message, kind });
  try {
    const { reply, savedAs } = await exportSpecctraDsn(ctx.api.getState().board?.name ?? "board");
    const said = describeExport(reply, savedAs);
    toast(said.message, said.kind);
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), "error");
  }
}

export function registerImportActions(m: Map<string, ActionHandler>, ctx: ImportActionContext): void {
  if (ctx.tab === "pcb") {
    // PCB_ACTIONS::openNonKicadBoard ("Import Non-KiCad Board File..."): a file chooser; the parameter `{ name, data }` is a file the page already has.
    m.set("pcbnew.EditorControl.openNonKicadBoard", (param) => void importForeign(ctx, "board", asForeignParam(param)));
    // PCB_ACTIONS::importSpecctraSession ("Import Specctra Session...") and PCB_ACTIONS::exportSpecctraDSN ("Export Specctra DSN..."); the import's parameter `{ name, text }`
    // is a session file the page already has.
    m.set("pcbnew.EditorControl.importSpecctraSession", (param) => void importSpecctraSession(ctx, asPickedFile(param)));
    m.set("pcbnew.EditorControl.exportSpecctraDSN", () => void exportSpecctra(ctx));
    // PCB_ACTIONS::appendBoard ("Append Board..."): a file chooser; PCB_ACTIONS::ddAppendBoard: the board that was dropped on the window.
    m.set("pcbnew.EditorControl.appendBoard", () => {
      void (async () => {
        try {
          const file = await pickTextFile(".kicad_pcb");
          if (file) await appendBoardFile(ctx, file);
        } catch (e) {
          ctx.dispatch({ type: "TOAST", message: e instanceof Error ? e.message : String(e), kind: "error" });
        }
      })();
    });
    m.set("pcbnew.Control.DdAppendBoard", (param) => {
      const file = asPickedFile(param);
      if (file) void appendBoardFile(ctx, file);
    });
    // PCB_ACTIONS::placeImportedGraphics ("Import Graphics..."): the dialog; PCB_ACTIONS::ddImportGraphics: the same dialog with the dropped file in its File: field.
    m.set("pcbnew.InteractiveDrawing.placeImportedGraphics", () => openImportGraphics("pcb"));
    m.set("pcbnew.InteractiveDrawing.ddImportGraphics", (param) => {
      const file = asPickedFile(param);
      // Only the extensions a plugin handles are accepted (`m_acceptedExts`).
      if (file && !graphicsFormatOf(file.name)) return ctx.dispatch({ type: "TOAST", message: "There is no plugin to handle this file type.", kind: "error" });
      openImportGraphics("pcb", file);
    });
  }
  if (ctx.tab === "schematic") {
    // SCH_ACTIONS::importNonKicadSchematic ("Import Non-KiCad Schematic..."): the same, for the schematic.
    m.set("eeschema.EditorControl.importNonKicadSchematic", (param) => void importForeign(ctx, "schematic", asForeignParam(param)));
  }
  if (ctx.tab === "schematic" || ctx.tab === "symbol") {
    // SCH_ACTIONS::importGraphics (the schematic's and the Symbol Editor's File > Import > Graphics...) and ddImportGraphics: DIALOG_IMPORT_GFX_SCH.
    const editor = ctx.tab === "symbol" ? "symbol" : "schematic";
    m.set("eeschema.EditorControl.importGraphics", () => openImportGraphics(editor));
    m.set("eeschema.EditorControl.ddImportGraphics", (param) => {
      const file = asPickedFile(param);
      if (file && !graphicsFormatOf(file.name)) return ctx.dispatch({ type: "TOAST", message: "There is no plugin to handle this file type.", kind: "error" });
      openImportGraphics(editor, file);
    });
  }
}
