// What the toolbars of the Footprint Editor and the Symbol Editor need from the action registry beyond the library actions (`libraryEditorActions.ts`): the
// frame-wide actions KiCad's `COMMON_TOOLS` / `COMMON_CONTROL` run in every editor frame -- zoom in/out/fit, the grid toggle, Save, Print, the library
// tree toggle, the select tool -- done on THIS frame's canvas and store, and the one app-specific action (`studio.SymbolEditor.updateOnBoard`).
//
// The registry is one map of handlers per action name, and the board and schematic editors register most of these names for their own canvases; while
// the Footprint or Symbol tab is showing, this overrides them with the frame's own (`registerEditorFrameActions` is called last, with the tab).
// Every action on those two toolbars that is NOT supported (src/kicad/editor_toolbar_support.json, with the reason for each) is unregistered on the
// tab, so the toolbar, the menus and the hotkey table never show it live while it would do nothing.
import { openEmbeddedDocument } from "../api/embeddedClient";
import type { Dispatch } from "react";
import support from "../kicad/editor_toolbar_support.json";
import { datasheetTarget } from "../kicad-port/itemText";
import { symbolPropertiesTarget } from "../kicad-port/libEditorEdit";
import { libFieldName } from "../kicad-port/symFieldLayout";
import { rotateCarriedPad } from "../kicad-port/fpPadPlacer";
import { newPadFromMaster } from "../kicad-port/padSettings";
import { worldToScreen, zoomAbout, type ViewTransform } from "../kicad-port/view";
import { revertFootprint, revertSymbol, saveAllSymbols, saveFootprint, saveSymbol } from "./librarySaveOps";
import { fpEditTarget, type Snap } from "../kicad-port/fpItems";
import { getDockLayout, setDockColumnCollapsed, toggleDockPane } from "../state/dockLayoutStore";
import type { FootprintEditorApi, FpAction, FpToolId } from "../state/footprintEditorStore";
import { asPickedImage } from "./imageFiles";
import { fpChooseImage } from "../components/footprint/fpItemTools";
import type { SymAction, SymbolEditorApi } from "../state/symbolEditorStore";
import type { Preferences } from "../kicad-port/preferences";
import { showSymFind, symFindNext, symReplaceAll, symReplaceAndFindNext } from "./symFindOps";

/** common_tools.cpp doZoomInOut: "Step must be AT LEAST 1.3" -- the factor of one Zoom In / Zoom Out step. */
const ZOOM_STEP = 1.3;

import { ARC_EDIT_MODE_LABEL, type ArcEditMode } from "../kicad-port/pcbPointEdit";
import { setSymArcEditMode } from "../kicad-port/symPointEdit";

export interface EditorFrameContext {
  tab: string;
  fpApi: FootprintEditorApi;
  fpDispatch: Dispatch<FpAction>;
  symApi: SymbolEditorApi;
  symDispatch: Dispatch<SymAction>;
  /** The studio's preferences (the Symbol Editor's Repeated Items: `SYMBOL_EDITOR_SETTINGS::m_Repeat`); the defaults when absent. */
  prefs?: Pick<Preferences, "symRepeatPinStepMils" | "symRepeatLabelDelta" | "fpRotationStepDeg">;
}

/** The canvas of the editor frame on screen (only one editor is mounted at a time). */
function canvasRect(): DOMRect | null {
  return document.querySelector(".editor-canvas-col .pcb-canvas-container")?.getBoundingClientRect() ?? null;
}

export function registerEditorFrameActions(m: Map<string, () => void>, ctx: EditorFrameContext): void {
  // The symbol editor's explicit "push this library definition to every placed instance" -- the studio's own action (no KiCad equivalent: KiCad saves the
  // library and the schematic offers the update), reachable from the Symbol Editor's File menu (kicad/menuExtras.ts).
  if (ctx.tab === "symbol") m.set("studio.SymbolEditor.updateOnBoard", () => void ctx.symApi.updateOnBoard());

  if (ctx.tab !== "footprint" && ctx.tab !== "symbol") return;
  const fp = ctx.tab === "footprint";

  const view = (): ViewTransform => (fp ? ctx.fpApi.getState().view : ctx.symApi.getState().view);
  const cursor = () => (fp ? ctx.fpApi.getState().cursorNm : ctx.symApi.getState().cursorNm);
  const setView = (v: ViewTransform) => (fp ? ctx.fpDispatch({ type: "SET_VIEW", view: v }) : ctx.symDispatch({ type: "SET_VIEW", view: v }));
  const toast = (message: string, kind: "info" | "error" = "info") => (fp ? ctx.fpDispatch({ type: "TOAST", message, kind }) : ctx.symDispatch({ type: "TOAST", message, kind }));

  // Zoom In / Out (F1, F2: about the cursor) and Zoom In / Out Center: one step of 1.3 about the cursor or the middle of the canvas.
  const zoom = (factor: number, atCursor: boolean) => () => {
    const rect = canvasRect();
    if (!rect) return;
    const c = atCursor ? cursor() : null;
    const [px, py] = c ? worldToScreen(view(), c.x, c.y) : [rect.width / 2, rect.height / 2];
    setView(zoomAbout(view(), px, py, factor));
  };
  m.set("common.Control.zoomInCenter", zoom(ZOOM_STEP, false));
  m.set("common.Control.zoomOutCenter", zoom(1 / ZOOM_STEP, false));
  m.set("common.Control.zoomIn", zoom(ZOOM_STEP, true));
  m.set("common.Control.zoomOut", zoom(1 / ZOOM_STEP, true));
  // Zoom to Fit: the canvas fits the open footprint/symbol again (it already does so once on opening; this is for after the view was moved by hand).
  const fit = () => (fp ? ctx.fpDispatch({ type: "REQUEST_FIT" }) : ctx.symDispatch({ type: "REQUEST_FIT" }));
  m.set("common.Control.zoomFitScreen", fit);
  m.set("common.Control.zoomFitObjects", fit);
  // Redraw: this app repaints from current state every time; nothing to invalidate (the board's own zoomRedraw says the same).
  m.set("common.Control.zoomRedraw", () => {});

  // Show Grid.
  m.set("common.Control.toggleGrid", () => (fp ? ctx.fpDispatch({ type: "TOGGLE_GRID_VISIBLE" }) : ctx.symDispatch({ type: "TOGGLE_GRID_VISIBLE" })));

  // Save / Save All / Revert (actions/librarySaveOps.ts): a library's symbol or footprint is edited as the design's copy (KiCad's library buffer); Save writes it
  // into the library's file the table names (`SYMBOL_EDIT_FRAME::Save`, `FOOTPRINT_EDITOR_CONTROL::Save`), Revert puts the file's version back. The project
  // library's edits are stored in the design as they are made. Export (File menu) writes the derived .kicad_mod / .kicad_sym.
  const failed = (e: unknown) => toast(e instanceof Error ? e.message : String(e), "error");
  if (fp) {
    m.set("common.Control.save", () => void saveFootprint(ctx.fpApi, toast).catch(failed));
    m.set("common.Control.saveAll", () => void saveFootprint(ctx.fpApi, toast).catch(failed));
    m.set("common.Control.revert", () => void revertFootprint(ctx.fpApi, toast).catch(failed));
  } else {
    m.set("common.Control.save", () => void saveSymbol(ctx.symApi, toast).catch(failed));
    m.set("common.Control.saveAll", () => void saveAllSymbols(toast).catch(failed));
    m.set("common.Control.revert", () => void revertSymbol(ctx.symApi, toast).catch(failed));
  }
  // Print: the browser's print of the frame (styles/layout.css reduces the page to the canvas column).
  m.set("common.Control.print", () => window.print());

  // Show Library Tree (`ACTIONS::showLibraryTree`): the Libraries column.
  m.set("common.Control.showLibraryTree", () => setDockColumnCollapsed("tree", !getDockLayout().treeCollapsed));

  if (fp) {
    // Select (`ACTIONS::selectSetRect`, the first button of the right toolbar's Selection modes group): back to the select tool.
    m.set("common.Interactive.selectSetRect", () => void ctx.fpApi.setTool("select"));
    // `updateModificationPoint`'s grid snap of a several-item pivot (`grid.BestSnapAnchor( refPt, nullptr )`), on the footprint editor's grid.
    const gridSnap: Snap = (p) => {
      const g = ctx.fpApi.getState().gridNm || 1;
      return [Math.round(p[0] / g) * g, Math.round(p[1] / g) * g];
    };
    // Rotate Counterclockwise / Clockwise (R / Shift+R, `EDIT_TOOL::Rotate`): the selection -- pads, graphics, texts, fields, dimensions, zones, text boxes ..., groups -- a turn of the
    // footprint editor's 90 degrees about its pivot (kicad-port/fpItems.ts `fpRotationPivot`), one undo step.
    // With the Pad tool armed they turn the pad it carries instead (`doInteractiveItemPlacement`, IPO_ROTATE: `newItem->Rotate( newItem->GetPosition(), rotationAngle )`,
    // the Footprint Editor's 90 degree step), which the placed pad then hands on to the default pad (kicad-port/fpPadPlacer.ts).
    // The step is the Footprint Editor's own (`FOOTPRINT_EDITOR_SETTINGS::m_RotationAngle`, Preferences > Footprint Editor > Editing Options, UI-LEFT F8), 90 degrees by default.
    const rotate = (quarterTurns: number) => () => {
      const st = ctx.fpApi.getState();
      const step = ctx.prefs?.fpRotationStepDeg ?? 90;
      if (st.activeTool === "pad") {
        ctx.fpDispatch({ type: "SET_CARRIED_PAD", pad: rotateCarriedPad(st.carriedPad ?? newPadFromMaster(st.defaultPad, "", { x: 0, y: 0 }), quarterTurns * step) });
        return;
      }
      if (st.selection.size === 0) return;
      void ctx.fpApi.rotateSelection(quarterTurns, gridSnap, step);
    };
    m.set("pcbnew.InteractiveEdit.rotateCcw", rotate(1));
    m.set("pcbnew.InteractiveEdit.rotateCw", rotate(-1));
    // Mirror Horizontally / Vertically (`EDIT_TOOL::Mirror`, no default hotkey): the mirrorable items of the selection about the modification point, on their own layers.
    m.set("pcbnew.InteractiveEdit.mirrorHoriontally", () => ctx.fpApi.getState().selection.size > 0 && void ctx.fpApi.mirrorSelection("left_right", gridSnap));
    m.set("pcbnew.InteractiveEdit.mirrorVertically", () => ctx.fpApi.getState().selection.size > 0 && void ctx.fpApi.mirrorSelection("top_bottom", gridSnap));
    // Show Properties (`ACTIONS::showProperties`, `FOOTPRINT_EDITOR_CONTROL::ToggleProperties`): the frame's `PCB_PROPERTIES_PANEL` beside the Libraries tree (UI-LEFT Y3).
    m.set("common.Control.showProperties", () => toggleDockPane("fpProperties"));
    // Group / Ungroup (Ctrl+G / Ctrl+Shift+G, `PCB_GROUP_TOOL::Group` in the footprint editor, `GROUP_TOOL::Ungroup`).
    m.set("common.Interactive.group", () => void ctx.fpApi.groupSelection());
    m.set("common.Interactive.ungroup", () => void ctx.fpApi.ungroupSelection());
    // Line modes (`FOOTPRINT_EDITOR_CONTROL::ChangeLineMode`): the drawing tools' angle constraint, the editor's own setting.
    m.set("pcbnew.EditorControl.lineModeFree", () => ctx.fpDispatch({ type: "SET_LINE_MODE", mode: "direct" }));
    m.set("pcbnew.EditorControl.lineModeOrthonal", () => ctx.fpDispatch({ type: "SET_LINE_MODE", mode: "90" }));
    m.set("pcbnew.EditorControl.lineMode45", () => ctx.fpDispatch({ type: "SET_LINE_MODE", mode: "45" }));
    // The right toolbar's tools for the footprint's dimensions, rule areas, text boxes, points, tables, barcodes and reference images: `DRAWING_TOOL`'s on the footprint
    // (components/footprint/fpItemTools.ts has their clicks). `if( m_isFootprintEditor && !m_frame->GetModel() ) return 0;`; choosing the tool in force again leaves it.
    const armItemTool = (tool: FpToolId) => () => {
      const st = ctx.fpApi.getState();
      if (!st.name) return toast("Open a footprint first.");
      void ctx.fpApi.setTool(st.activeTool === tool ? "select" : tool);
    };
    m.set("pcbnew.InteractiveDrawing.alignedDimension", armItemTool("dim_aligned"));
    m.set("pcbnew.InteractiveDrawing.orthogonalDimension", armItemTool("dim_orthogonal"));
    m.set("pcbnew.InteractiveDrawing.centerDimension", armItemTool("dim_center"));
    m.set("pcbnew.InteractiveDrawing.radialDimension", armItemTool("dim_radial"));
    m.set("pcbnew.InteractiveDrawing.leader", armItemTool("dim_leader"));
    m.set("pcbnew.InteractiveDrawing.ruleArea", armItemTool("rule_area"));
    m.set("pcbnew.InteractiveDrawing.textbox", armItemTool("textbox"));
    m.set("pcbnew.InteractiveDrawing.placePoint", armItemTool("point"));
    m.set("pcbnew.InteractiveDrawing.drawTable", armItemTool("table"));
    m.set("pcbnew.InteractiveDrawing.barcode", armItemTool("barcode"));
    // Place Reference Image asks for the file at once (the tool is primed with a click); a parameter that is a picked image ({ name, data }) is the file already chosen.
    const m2 = m as Map<string, (param?: unknown) => void>;
    m2.set("pcbnew.InteractiveDrawing.placeReferenceImage", (param?: unknown) => {
      const st = ctx.fpApi.getState();
      if (!st.name) return toast("Open a footprint first.");
      if (st.activeTool === "ref_image" && !param) return void ctx.fpApi.setTool("select");
      void ctx.fpApi.setTool("ref_image").then(() => fpChooseImage({ dispatch: ctx.fpDispatch }, asPickedImage(param)));
    });
    // The display modes of the left toolbar (`PCB_VIEWER_TOOLS::ShowPadNumbers` family: `PadDisplayMode`, `GraphicOutlines`, `TextOutlines`).
    m.set("pcbnew.Control.padDisplayMode", () => ctx.fpDispatch({ type: "SET_DISPLAY", patch: { sketchPads: !ctx.fpApi.getState().sketchPads } }));
    m.set("pcbnew.Control.graphicOutlines", () => ctx.fpDispatch({ type: "SET_DISPLAY", patch: { sketchGraphics: !ctx.fpApi.getState().sketchGraphics } }));
    m.set("pcbnew.Control.textOutlines", () => ctx.fpDispatch({ type: "SET_DISPLAY", patch: { sketchText: !ctx.fpApi.getState().sketchText } }));
    // Show Appearance Manager (`FOOTPRINT_EDITOR_CONTROL::ToggleLayersManager`): the frame's Appearance pane.
    m.set("pcbnew.Control.showLayersManager", () => ctx.fpDispatch({ type: "SET_APPEARANCE_SHOWN", shown: !ctx.fpApi.getState().appearanceShown }));
    // Properties (E): `EDIT_TOOL::Properties` -> `FOOTPRINT_EDIT_FRAME::OnEditItemRequest` -- the one selected item's dialog (a pad's, a text's or a field's, a shape's, ...),
    // Footprint Properties with nothing selected (kicad-port/fpItems.ts `fpEditTarget`).
    // The window-level hotkey runs it, and so do the Edit menu and the context menu; the canvas no longer reads E itself.
    const footprintProperties = m.get("pcbnew.ModuleEditor.footprintProperties");
    m.set("pcbnew.InteractiveEdit.properties", () => {
      const st = ctx.fpApi.getState();
      if (st.activeTool !== "select" || !st.footprint) return;
      const target = fpEditTarget(st.footprint, [...st.selection]);
      if (!target) return;
      if (target.dialog === "pad") ctx.fpDispatch({ type: "SET_PAD_PROPERTIES_ID", id: target.id });
      else if (target.dialog === "footprint") footprintProperties?.();
      else if (target.dialog === "field" || target.dialog === "text") ctx.fpDispatch({ type: "SET_TEXT_EDIT_ID", id: target.id });
      else if (target.dialog === "shape") ctx.fpDispatch({ type: "SET_SHAPE_DIALOG_ID", id: target.id });
      else ctx.fpDispatch({ type: "SET_ITEM_DIALOG", dialog: { id: target.id, kind: target.dialog } });
    });
    // Delete (`EDIT_TOOL::Remove` in the footprint editor): the selected pads, graphics and texts in one commit, one undo step. The Delete key reaches it through the
    // hotkey table (the canvas keeps Backspace, which the table does not bind).
    m.set("common.Interactive.delete", () => {
      const st = ctx.fpApi.getState();
      if (st.activeTool !== "select" || st.selection.size === 0) return;
      void ctx.fpApi.deleteItems([...st.selection]);
    });
  } else {
    // Properties (E): `SYMBOL_EDITOR_EDIT_TOOL::Properties` -- a pin's Pin Properties, a shape's own dialog, a field's row of Symbol Properties, Symbol
    // Properties with nothing selected (kicad-port/libEditorEdit.ts).
    m.set("eeschema.InteractiveEdit.properties", () => {
      const st = ctx.symApi.getState();
      if (st.activeTool !== "select") return;
      const kind = (id: string) => (ctx.symApi.pinById(id) ? "pin" : libFieldName(id) ? "field" : ctx.symApi.graphicById(id) ? "graphic" : "other");
      const target = symbolPropertiesTarget([...st.selection].map((id) => ({ id, kind: kind(id) })));
      if (!target) return;
      if (target.dialog === "pin") ctx.symDispatch({ type: "SET_PIN_PROPERTIES_ID", id: target.id });
      else if (target.dialog === "symbol") ctx.symDispatch({ type: "SET_PROPERTIES_OPEN", open: true, field: target.field ? (libFieldName(target.field) ?? undefined) : undefined });
      else {
        const g = ctx.symApi.graphicById(target.id);
        if (!g) return;
        // `editTextProperties` for a text, `editTextBoxProperties` for a text box, `editShapeProperties` for the rest
        if (g.kind === "text") ctx.symDispatch({ type: "SET_TEXT_DIALOG", dialog: { id: target.id } });
        else if (g.kind === "text_box") ctx.symDispatch({ type: "SET_TEXT_BOX_DIALOG", dialog: { id: target.id, box: g } });
        else ctx.symDispatch({ type: "SET_SHAPE_DIALOG_ID", id: target.id });
      }
    });
    // Rotate (R / Shift+R, `SYMBOL_EDITOR_EDIT_TOOL::Rotate`) and Mirror Horizontally / Vertically (`Mirror`): every selected pin, graphic and field a quarter turn or
    // mirrored, about the lone item's position or the selection's half-grid centre, one undo step (kicad-port/symTransform.ts).
    // (with something carried -- a pin or a text being placed, Paste's or Duplicate's items -- they turn what is carried, `item->IsMoving()`)
    const transform = (run: () => Promise<void>) => () => {
      const st = ctx.symApi.getState();
      if (st.activeTool !== "select" && !st.carry) return;
      void run();
    };
    m.set("eeschema.InteractiveEdit.rotateCCW", transform(() => ctx.symApi.rotateSelection(true)));
    m.set("eeschema.InteractiveEdit.rotateCW", transform(() => ctx.symApi.rotateSelection(false)));
    m.set("eeschema.InteractiveEdit.mirrorH", transform(() => ctx.symApi.mirrorSelection("h")));
    m.set("eeschema.InteractiveEdit.mirrorV", transform(() => ctx.symApi.mirrorSelection("v")));
    // Delete (`SYMBOL_EDITOR_EDIT_TOOL::DoDelete`): the selected pins and graphics, with the pins' twins in Synchronized Pins Mode, one undo step.
    m.set("common.Interactive.delete", () => {
      const st = ctx.symApi.getState();
      if (st.activeTool !== "select" || st.selection.size === 0) return;
      void ctx.symApi.deleteItems([...st.selection]);
    });
    // The Symbol Editor's own clipboard and repeat (the schematic's handlers of these names act on the schematic): `SYMBOL_EDITOR_EDIT_TOOL::Cut` / `Copy` / `Paste`
    // / `Duplicate` and `SYMBOL_EDITOR_DRAWING_TOOLS::RepeatDrawItem` (Insert: `RepeatPin`), each one undo step (kicad-port/symEditItems.ts).
    const idle = () => ctx.symApi.getState().activeTool === "select";
    m.set("common.Interactive.cut", () => idle() && void ctx.symApi.copySelection(true));
    m.set("common.Interactive.copy", () => idle() && void ctx.symApi.copySelection(false));
    m.set("common.Interactive.paste", () => void ctx.symApi.pasteItems());
    m.set("common.Interactive.duplicate", () => idle() && void ctx.symApi.duplicateSelection());
    // (`m_Repeat.pin_step` in mils and `m_Repeat.label_delta`, Preferences > Symbol Editor > Editing Options)
    m.set("eeschema.InteractiveEdit.repeatDrawItem", () => void ctx.symApi.repeatLastPin((ctx.prefs?.symRepeatPinStepMils ?? 100) * 0.0254, ctx.prefs?.symRepeatLabelDelta ?? 1));

    // Selection tool (`ACTIONS::selectionTool`).
    m.set("common.InteractiveSelection.selectionTool", () => ctx.symDispatch({ type: "SET_ACTIVE_TOOL", tool: "select" }));
    // Show Properties (`ACTIONS::showProperties`, `SYMBOL_EDIT_FRAME::ToggleProperties`): the frame's own Properties pane, below the Libraries tree
    m.set("common.Control.showProperties", () => toggleDockPane("symProperties"));
    // Show Datasheet (`ACTIONS::showDatasheet`): the open symbol's datasheet field, opened like the schematic's does.
    m.set("common.Control.showDatasheet", () => {
      const sym = ctx.symApi.getState().symbol;
      if (!sym) return toast("Open a symbol first.");
      const target = datasheetTarget(sym.datasheet);
      if (target.kind === "none") toast("No datasheet defined.", "error");
      else if (target.kind === "url") window.open(target.url, "_blank", "noopener,noreferrer");
      // `filesStack.push_back( entry )`: the library symbol's files
      else if (target.kind === "embedded") {
        const libId = ctx.symApi.getState().libId;
        void openEmbeddedDocument(libId ? [{ kind: "library_symbol", lib_id: libId }] : [], target.link);
      }
      else toast(`Cannot open datasheet '${target.text}': only absolute http(s)/file URLs can be opened from a browser.`, "error");
    });

    // `SCH_POINT_EDITOR::changeArcEditMode` in the Symbol Editor: Ctrl+Space (`cycleArcEditMode`) and the three `pointEditorArcKeep*` actions set the Symbol Editor's own
    // `m_ArcEditMode`, the mode its arc point editor drags in (kicad-port/symPointEdit.ts; UI-LEFT-2 YY1)
    const arcMode = (mode: ArcEditMode | "cycle") => () => toast(`Arc editing: ${ARC_EDIT_MODE_LABEL[setSymArcEditMode(mode)]}`);
    m.set("common.Interactive.cycleArcEditMode", arcMode("cycle"));
    m.set("pcbnew.PointEditor.arcKeepCenter", arcMode("keep_center_adjust_angle_radius"));
    m.set("pcbnew.PointEditor.arcKeepEndpoint", arcMode("keep_endpoints_or_start_direction"));
    m.set("pcbnew.PointEditor.arcKeepRadius", arcMode("keep_center_ends_adjust_angle"));

    // Find / Find and Replace (`SCH_FIND_REPLACE_TOOL`, which the Symbol Editor registers too): over the symbol being edited, with its own dialog and cursor
    // (actions/symFindOps.ts) -- the schematic's handlers of the same names are replaced on this tab.
    m.set("common.Interactive.find", () => showSymFind(false, ctx.symApi));
    m.set("common.Interactive.findAndReplace", () => showSymFind(true, ctx.symApi));
    m.set("common.Interactive.findNext", () => void symFindNext(ctx.symApi, ctx.symDispatch, false));
    m.set("common.Interactive.findPrevious", () => void symFindNext(ctx.symApi, ctx.symDispatch, true));
    m.set("common.Interactive.replaceAndFindNext", () => void symReplaceAndFindNext(ctx.symApi, ctx.symDispatch));
    m.set("common.Interactive.replaceAll", () => void symReplaceAll(ctx.symApi));
  }

  // Everything on this editor's toolbars that is not supported here is not registered, so nothing reads as live that would do nothing.
  const table = (fp ? support.footprint : support.symbol) as Record<string, string>;
  for (const [name, state] of Object.entries(table)) if (state !== "supported") m.delete(name);
}
