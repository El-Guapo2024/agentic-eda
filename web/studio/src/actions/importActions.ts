// The file-in actions: Import Graphics (File > Import > Graphics... in the board editor, `DRAWING_TOOL::PlaceImportedGraphics`; the schematic's and the symbol editor's twin is
// `SCH_DRAWING_TOOLS::ImportGraphics`) and the drops that reach them (`DdImportGraphics`: a DXF or SVG dropped on the window). Append Board and its drop are `appendBoardActions`
// below. A browser drop gives the page a file's text and not its path, so the drop actions take the file as their parameter -- `{ name, text }` -- where KiCad's takes a file name.
import { markRescueOnLoad } from "../state/rescueOnLoad";
import { LEGACY_SCHEMATIC_FORMAT } from "../kicad-port/foreignImport";
import { openImportGraphics, type PickedFile } from "../state/importDialogs";
import { graphicsFormatOf } from "../kicad-port/importGraphics";
import { appendBoardCmd, describeAppended, isKicadBoardName } from "../kicad-port/appendBoard";
import { getDesignBlocksUi } from "../state/designBlocks";
import {
  acceptFor,
  appendSourceLayers,
  asForeignParam,
  boardIndexOf,
  describeImport,
  describeIssues,
  describeRules,
  issuesOf,
  PCB_MODIFIED_MESSAGE,
  projectToSelectorRows,
  REPLACE_SCHEMATIC_QUESTION,
  rowsOfBoards,
  SAVE_CHANGES,
  SCHEMATIC_MODIFIED_MESSAGE,
  type ConvertKind,
  type ForeignFile,
  type ForeignParam,
  type ImportKind,
} from "../kicad-port/foreignImport";
import { easyEdaProject } from "../kicad-port/zipEntry";
import { readAppendBoard } from "../api/importClient";
import { pickBinaryFile, pickBinaryFiles, postImportForeign } from "../api/foreignImportClient";
import { fetchState, postCarry, postCmd } from "../api/client";
import { isContentModified, markContentSaved, type ModifiedEditor } from "../state/contentModified";
import { pickTextFile } from "../api/libraryClient";
import { askChooseProject, askImportQuestion, askLayerMapping, importKeepKiCadLayerNames, importShowIssues, showImportReport } from "../state/foreignImport";
import { boundsOfPoints, fitTransform } from "../components/canvas/view";
import { schematicBounds } from "../components/schematic/schHit";
import type { Action, StudioApi } from "../state/store";
import type { Dispatch } from "react";
import type { ActionHandler } from "./commonActions";
import { describeExport, describeImport as describeSessionImport, SESSION_ACCEPT, specctraImportCmd } from "../kicad-port/specctra";
import { exportSpecctraDsn } from "../api/specctraClient";
import { createElement } from "react";
import { openSweepDialog } from "./pcbSweepDialogs";
import { ImportNetlistDialog, type ImportNetlistHost } from "../components/ImportNetlistDialog";
import { spreadBySheet, type SpreadItem } from "../kicad-port/importNetlist";
import { footprintBBox } from "../kicad-port/packFootprints";

/** The project import's parameter: a list of files, or `{ files: [..], layerMap, .. }` with the options for the first. */
function asProjectParam(param: unknown): ForeignParam[] | null {
  if (Array.isArray(param)) {
    const files = param.map(asForeignParam).filter((f): f is ForeignParam => !!f);
    return files.length > 0 ? files : null;
  }
  if (param && typeof param === "object" && Array.isArray((param as { files?: unknown }).files)) {
    const { files, ...options } = param as { files: unknown[] } & Record<string, unknown>;
    const list = files.map(asForeignParam).filter((f): f is ForeignParam => !!f);
    if (list.length === 0) return null;
    list[0] = { ...list[0]!, ...(options as Partial<ForeignParam>) };
    return list;
  }
  return null;
}

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
  /** Run another action of the registry (a legacy schematic opens Remap Symbols after its import). */
  run?: (id: string) => void;
}

/**
 * `PCB_CONTROL::AppendBoard`: the file is loaded into the board (`place_design_block` with no block, one undo step: its layers, nets and items, in one group named after the file
 * when "Place as group" is on) with the top left item at the cursor and carried by the Move tool until a click drops them; Escape takes them back. What the append added or
 * changed on this board is said afterwards.
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
  // KiCad 25ec2335 `remapAppendedLayers`: when a layer of the file is not on the board, or has another name there, the parser asks `DIALOG_MAP_LAYERS` where each layer
  // goes. Cancel answers with nothing (`RunModal` returns `{}`), which keeps every layer on its own: the append goes on as it would have.
  let text = file.text;
  if (info.layers && info.layers.length > 0) {
    const permitted = [...new Set(info.layers.flatMap((l) => l.permitted))];
    const answer = await askLayerMapping({ name: file.name, format: "KiCad", layers: appendSourceLayers(info.layers), keepNames: importKeepKiCadLayerNames(), mode: "append", permitted });
    if (answer && Object.keys(answer.map).length > 0) {
      try {
        info = await readAppendBoard(file, answer.map);
      } catch (e) {
        return toast(e instanceof Error ? e.message : String(e), "error");
      }
      if (!info.ok || !info.reference) return toast(info.message ?? "Error loading board.", "error");
      if (typeof info.text === "string") text = info.text;
    }
  }
  const reference = info.reference;
  // `AppendBoard` reads the Design Blocks pane's "Place as group" for a plain board too (`cfg->m_DesignBlockChooserPanel.place_as_group`)
  const asGroup = getDesignBlocksUi().options.asGroup;
  const placed = await ctx.api.placeNewItems((at) => appendBoardCmd(text, reference, at, file.name, asGroup), true);
  if (!placed) return; // the refusal is on screen as a toast
  const notes = info.notes ?? [];
  // what the append did, in the board's info bar as KiCad says its append's news there (KiCad 25ec2335's "Board changed from %d to %d copper layers, stackup updated.",
  // `ShowInfoBarMsg`): the information icon, 8 s
  ctx.dispatch({ type: "SHOW_PCB_INFOBAR", message: `Appended ${describeAppended(info.counts)}.${notes.length > 0 ? ` ${notes.join(" ")}` : ""}`, kind: "info", ms: 8000, close: false });
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

/** `HandleUnsavedChanges`: "Save changes?" with Save / Discard Changes / Cancel. Save is the studio's File > Save (design.json is written as each edit is made, so it only
 * clears the flag and says so); false is Cancel, which stops the import. */
async function handleUnsavedChanges(ctx: ImportActionContext, editor: ModifiedEditor, message: string): Promise<boolean> {
  if (!isContentModified(editor)) return true;
  const answer = await askImportQuestion({ title: SAVE_CHANGES.title, message, detail: SAVE_CHANGES.detail, buttons: [SAVE_CHANGES.save, SAVE_CHANGES.discard, SAVE_CHANGES.cancel] });
  if (answer === null) return false;
  if (answer === SAVE_CHANGES.save) {
    markContentSaved(editor);
    ctx.dispatch({ type: "TOAST", message: "File 'design.json' saved.", kind: "info" });
  }
  return true;
}

/** Does the schematic in view hold anything (`Schematic().RootScreen()->Items().empty()`), which `OnImportProject` asks before it replaces it? */
function schematicHasItems(ctx: ImportActionContext): boolean {
  const sch = ctx.api.getState().schematic;
  if (!sch) return false;
  const s = sch as unknown as Record<string, unknown>;
  return ["symbols", "wires", "labels", "sheets", "texts", "lines", "power_symbols", "junctions", "no_connects", "bus_entries"].some((k) => Array.isArray(s[k]) && (s[k] as unknown[]).length > 0);
}

/**
 * `DIALOG_IMPORT_CHOOSE_PROJECT`: a project file of several boards (an EasyEDA Pro project) asks which. The rows are `ProjectToSelectorDialog`'s, read from the file's
 * `project.json` here (the board import asks with `aPcbOnly`); a file this cannot read shows the boards kicad-cli named. Resolves to the board's place in kicad-cli's list,
 * or `null` when the dialog is closed (KiCad then imports nothing).
 */
async function chooseBoard(file: ForeignFile, boards: string[], pcbOnly: boolean): Promise<number | null> {
  const project = await easyEdaProject(file.data);
  const rows = project ? projectToSelectorRows(project, pcbOnly, false).filter((r) => r.pcbId) : [];
  if (rows.length === 0) {
    const i = await askChooseProject(rowsOfBoards(boards));
    return i;
  }
  const i = await askChooseProject(rows);
  if (i === null) return null;
  const index = boardIndexOf(project, rows[i]!.pcbId);
  return index >= 0 ? index : null;
}

/**
 * File > Import > Non-KiCad Board File... (`BOARD_EDITOR_CONTROL::OpenNonKicadBoard`), Non-KiCad Schematic File... (`SCH_EDIT_FRAME::OnImportProject`) and the studio's Import
 * Non-KiCad Project... (the project manager's, `IMPORT_PROJ_HELPER::ImportFiles`: a board and its schematic together): a file of another EDA format replaces the board (or the
 * schematic, or both) that is open -- one command, or for a project one step of two commands, so one undo step; KiCad cannot take it back.
 *
 *   1. the schematic's import asks first, before the file is chosen, when the sheet holds anything (`IsOK`: "This operation replaces the contents of the current schematic,
 *      which will be permanently lost."); the board's asks "The current PCB has been modified.  Save changes?" after it (`OpenProjectFiles`' `HandleUnsavedChanges`), when the
 *      board was modified since the page opened or was saved (state/contentModified.ts);
 *   2. the file is chosen (a browser gives the page its bytes, not its path) and kicad-cli converts it (`POST /api/import_foreign`; nothing on the board changes yet); a project
 *      file of several boards asks which (`DIALOG_IMPORT_CHOOSE_PROJECT`);
 *   3. an importer that asks which KiCad layer each of its layers goes to (Altium, Eagle, PADS, CADSTAR, ...) is answered by the layer dialog (DIALOG_MAP_LAYERS), and the file is
 *      converted again with the layers the person moved and without the ones left unmatched;
 *   4. `replace_board` / `replace_schematic` makes it the board or the schematic;
 *   5. what the importers said is the toast and, with "Show import issues", a report.
 *
 * `given` is the file (or a project's files) when the action was run with one (`window.__eda.run(id, { name, data })`) instead of through the chooser.
 */
async function importForeign(ctx: ImportActionContext, kind: ConvertKind, given: ForeignParam | ForeignParam[] | null): Promise<void> {
  const toast = (message: string, tone: "error" | "info") => ctx.dispatch({ type: "TOAST", message, kind: tone });
  try {
    if (kind === "schematic" && schematicHasItems(ctx)) {
      const yes = await askImportQuestion({ title: "Confirmation", message: REPLACE_SCHEMATIC_QUESTION, buttons: ["Yes", "No"] });
      if (yes !== "Yes") return;
    }
    // A schematic may come with the files beside it: a KiCad 5 legacy schematic's sheets, cache library and libraries (a browser hands the page files, not a folder).
    const files: ForeignFile[] = given ? (Array.isArray(given) ? given : [given]) : kind !== "board" ? await pickBinaryFiles(acceptFor(kind)) : [await pickBinaryFile(acceptFor(kind))].filter((f): f is ForeignFile => !!f);
    if (files.length === 0) return;
    const first = (Array.isArray(given) ? given[0] : given) ?? null;
    const file = files[0]!;
    const shown = files.map((f) => f.name).join(" + ");
    if (kind !== "schematic" && !(await handleUnsavedChanges(ctx, "pcb", PCB_MODIFIED_MESSAGE))) return;
    if (kind === "project" && !(await handleUnsavedChanges(ctx, "schematic", SCHEMATIC_MODIFIED_MESSAGE))) return;
    const target = kind === "project" || (kind === "schematic" && files.length > 1) ? files : file;
    let board: number | undefined = first?.board;
    let reply = await postImportForeign(kind, target, first?.layerMap, first?.dropLayers, board);
    if (reply.ok && reply.choose_board) {
      const chosen = board ?? (await chooseBoard(file, reply.boards ?? [], kind !== "project"));
      if (chosen === null || chosen === undefined) return toast(`Import of '${shown}' cancelled.`, "info");
      board = chosen;
      reply = await postImportForeign(kind, target, first?.layerMap, first?.dropLayers, board);
    }
    if (!reply.ok) return toast(reply.message ?? `The file '${shown}' could not be imported.`, "error");
    let keepNames = first?.keepKicadLayerNames ?? importKeepKiCadLayerNames();
    if (kind !== "schematic" && reply.maps_layers && !first?.layerMap && !first?.dropLayers && !first?.skipLayerMapping && reply.layers) {
      // `PCB_IO_EAGLE::mapEagleLayersToKicad`, `ALTIUM_PCB::...`: the importer asks, and the person answers.
      const answer = await askLayerMapping({ name: reply.name ?? file.name, format: reply.format ?? "", layers: reply.layers, keepNames });
      if (!answer) return toast(`Import of '${shown}' cancelled.`, "info");
      keepNames = answer.keepNames;
      if (Object.keys(answer.map).length > 0 || answer.drop.length > 0) {
        reply = await postImportForeign(kind, target, answer.map, answer.drop, board);
        if (!reply.ok) return toast(reply.message ?? `The file '${shown}' could not be imported.`, "error");
      }
    }
    // The board comes with the project kicad-cli made of it: the design rules and the net classes the importer set are in that file and not in the board's.
    const boardCmd = { op: "replace_board", text: reply.text ?? "", name: reply.name ?? file.name, keep_kicad_layer_names: keepNames, ...(reply.project ? { project: reply.project } : {}), ...(reply.rules ? { rules: reply.rules } : {}) } as const;
    const schCmd = { op: "replace_schematic", content: reply.content, name: kind === "project" ? (files.find((f) => f.name !== reply.name)?.name ?? file.name) : file.name } as const;
    // An import is not a move the gates judge: Strict is off for it, as for a paste. A project's two commands are one step: the board's holds the step open and the
    // schematic's ends it (crates/cli/src/carry.rs), so one Undo, from either tab, takes both back.
    let done;
    if (kind === "project") {
      done = await postCmd(boardCmd, false, "begin");
      if (done.ok) {
        const second = await postCmd(schCmd, false, "commit");
        if (!second.ok) await postCarry("cancel");
        done = second;
      }
    } else {
      done = await postCmd(kind === "board" ? boardCmd : schCmd, false);
    }
    if (!done.ok) {
      await ctx.api.refresh();
      return toast(done.message, "error");
    }
    await ctx.api.refresh();
    if (kind !== "board") await ctx.api.navigateToSheet([]);
    fitToImport(ctx, kind === "schematic" ? "schematic" : "board");
    const issues = describeIssues(reply.findings);
    const rules = describeRules(reply);
    const notes = (reply.notes ?? []).filter((n) => kind !== "schematic" && n);
    toast(`${describeImport(reply)}${issues ? ` ${issues}` : ""}${rules ? ` ${rules}` : ""}${notes.length > 0 ? ` ${notes.join(" ")}` : ""} Undo brings the ${kind === "board" ? "old board" : kind === "project" ? "old board and schematic" : "old schematic"} back.`, "info");
    if (issuesOf(reply.findings).length > 0 && !first?.skipReport && importShowIssues()) {
      showImportReport({ name: shown, format: reply.format ?? "", summary: `${describeImport(reply)} ${issues}`.trim(), findings: issuesOf(reply.findings), notes: [...(reply.notes ?? []), ...(reply.schematic_notes ?? [])] });
    }
    // A legacy schematic none of whose symbols names a library: `OpenProjectFiles` opens Remap Symbols (`DIALOG_SYMBOL_REMAP`) as the project loads
    // ("Convert old projects over to use symbol library table").
    if (kind === "schematic" && reply.remap) ctx.run?.("eeschema.EditorControl.remapSymbols");
    // ... else, with no cache library beside it and "Never show Rescue Symbols tool" off: Rescue Symbols, not on demand (`if( ( !cfg || !cfg->m_RescueNeverShow ) &&
    // !cacheExists ) RescueSymbolLibTableProject( false )`; UI-LEFT-3 S3-26)
    else if (kind === "schematic" && reply.format === LEGACY_SCHEMATIC_FORMAT && reply.cache_found === false && !ctx.api.getState().prefs.schRescueNeverShow) {
      markRescueOnLoad();
      ctx.run?.("eeschema.EditorControl.rescueSymbols");
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

/**
 * File > Import > Netlist... (`PCB_EDIT_FRAME::InstallNetlistFrame`): the dialog (components/ImportNetlistDialog.tsx). The footprints Update PCB added are spread as
 * `OnNetlistChanged` spreads them (`SpreadFootprints( &newFootprints, { 0, 0 }, true )`) and picked up by the Move tool when the dialog closes, inside the step the
 * update held open: the drop ends it (judged by the Strict switch with the import); Escape ends the move with the footprints where they were spread, as KiCad's
 * cancelled move leaves them (the update and its spread were pushed before the move began), and the held step with them (UI-LEFT-3 P3-29).
 */
function openImportNetlist(ctx: ImportActionContext, given: PickedFile | null): void {
  const host: ImportNetlistHost = {
    strict: () => ctx.api.getState().strict,
    refresh: () => ctx.api.refresh(),
    toast: (message, kind) => ctx.dispatch({ type: "TOAST", message, kind }),
    carry: async (refs, paths) => {
      // The server's board: the page's state follows a refresh only once React has drawn it, and a Close right after Update PCB came before that (the new footprints
      // were not in it yet, nothing was carried, and the step held open was later settled unjudged).
      const board = await fetchState().catch(() => null);
      const parts = board?.parts ?? ctx.api.getState().board?.parts ?? [];
      const items: SpreadItem[] = [];
      refs.forEach((ref, i) => {
        const part = parts.find((p) => p.ref === ref);
        const bbox = part ? footprintBBox(part) : null;
        if (part && bbox) items.push({ ref, bbox, path: paths[i] ?? "" });
      });
      const first = parts.find((p) => p.ref === items[0]?.ref);
      if (!first?.at) {
        // nothing to carry after all: the held step ends here, judged by the Strict switch (a refusal takes the import back)
        const done = await postCarry("commit", ctx.api.getState().strict);
        if (!done.ok) ctx.dispatch({ type: "TOAST", message: done.message, kind: "error" });
        await ctx.api.refresh();
        return;
      }
      await ctx.api.refresh();
      // Escape cancels the move, not the update: the footprints stay spread (UI-LEFT-3 P3-29)
      ctx.api.carryPlaced(items.map((i) => i.ref), { x: first.at[0], y: first.at[1] }, spreadBySheet(items), true);
    },
  };
  openSweepDialog({ kind: "element", element: createElement(ImportNetlistDialog, { host, given }) });
}

export function registerImportActions(m: Map<string, ActionHandler>, ctx: ImportActionContext): void {
  if (ctx.tab === "pcb") {
    // PCB_ACTIONS::openNonKicadBoard ("Import Non-KiCad Board File..."): a file chooser; the parameter `{ name, data }` is a file the page already has.
    m.set("pcbnew.EditorControl.openNonKicadBoard", (param) => void importForeign(ctx, "board", asForeignParam(param)));
    // PCB_ACTIONS::importSpecctraSession ("Import Specctra Session...") and PCB_ACTIONS::exportSpecctraDSN ("Export Specctra DSN..."); the import's parameter `{ name, text }`
    // is a session file the page already has.
    m.set("pcbnew.EditorControl.importSpecctraSession", (param) => void importSpecctraSession(ctx, asPickedFile(param)));
    m.set("pcbnew.EditorControl.exportSpecctraDSN", () => void exportSpecctra(ctx));
    // PCB_ACTIONS::importNetlist ("Import Netlist..."): DIALOG_IMPORT_NETLIST; the parameter `{ name, text }` is a netlist file the page already has.
    m.set("pcbnew.EditorControl.importNetlist", (param) => openImportNetlist(ctx, asPickedFile(param)));
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
  if (ctx.tab === "footprint") {
    // PCB_ACTIONS::placeImportedGraphics in the Footprint Editor (File > Import > Graphics..., UI-LEFT F8): the same dialog, the items the footprint's.
    m.set("pcbnew.InteractiveDrawing.placeImportedGraphics", () => openImportGraphics("footprint"));
    m.set("pcbnew.InteractiveDrawing.ddImportGraphics", (param) => {
      const file = asPickedFile(param);
      if (file && !graphicsFormatOf(file.name)) return ctx.dispatch({ type: "TOAST", message: "There is no plugin to handle this file type.", kind: "error" });
      openImportGraphics("footprint", file);
    });
  }
  if (ctx.tab === "schematic") {
    // SCH_ACTIONS::importNonKicadSchematic ("Import Non-KiCad Schematic..."): the same, for the schematic.
    // The parameter may be several files: a KiCad 5 legacy schematic with its sheets, cache library and libraries.
    m.set("eeschema.EditorControl.importNonKicadSchematic", (param) => void importForeign(ctx, "schematic", asProjectParam(param) ?? asForeignParam(param)));
  }
  if (ctx.tab === "pcb" || ctx.tab === "schematic") {
    // The project manager's File > Import Non-KiCad Project... (`KICAD_MANAGER_FRAME::ImportNonKiCadProject`, `IMPORT_PROJ_HELPER::ImportFiles`): a board and its schematic
    // (an Eagle `.brd` and `.sch`, a CADSTAR `.cpa` and `.csa`, an EasyEDA Pro project) come in together. The studio has no project manager, so it is an entry of both editors'
    // File > Import (kicad/menuExtras.ts); the parameter is the files (`[{ name, data }, ..]`, or `{ files: [..] }` with the options of the first).
    m.set("studio.Import.nonKicadProject", (param) => void importForeign(ctx, "project", asProjectParam(param)));
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
