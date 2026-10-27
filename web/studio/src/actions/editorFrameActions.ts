// What the toolbars of the Footprint Editor and the Symbol Editor need from the action registry beyond the library actions (`libraryEditorActions.ts`): the
// frame-wide actions KiCad's `COMMON_TOOLS` / `COMMON_CONTROL` run in every editor frame -- zoom in/out/fit, the grid toggle, Save, Print, the library
// tree toggle, the select tool -- done on THIS frame's canvas and store, and the one app-specific action (`studio.SymbolEditor.updateOnBoard`).
//
// The registry is one map of handlers per action name, and the board and schematic editors register most of these names for their own canvases; while
// the Footprint or Symbol tab is showing, this overrides them with the frame's own (`registerEditorFrameActions` is called last, with the tab).
// Every action on those two toolbars that is NOT supported (src/kicad/editor_toolbar_support.json, with the reason for each) is unregistered on the
// tab, so the toolbar, the menus and the hotkey table never show it live while it would do nothing.
import type { Dispatch } from "react";
import support from "../kicad/editor_toolbar_support.json";
import { datasheetTarget } from "../kicad-port/itemText";
import { footprintPropertiesTarget, symbolPropertiesTarget } from "../kicad-port/libEditorEdit";
import { libFieldName } from "../kicad-port/symFieldLayout";
import { rotateCarriedPad } from "../kicad-port/fpPadPlacer";
import { newPadFromMaster } from "../kicad-port/padSettings";
import { worldToScreen, zoomAbout, type ViewTransform } from "../kicad-port/view";
import { getDockLayout, setDockColumnCollapsed } from "../state/dockLayoutStore";
import type { FootprintEditorApi, FpAction } from "../state/footprintEditorStore";
import type { SymAction, SymbolEditorApi } from "../state/symbolEditorStore";

/** common_tools.cpp doZoomInOut: "Step must be AT LEAST 1.3" -- the factor of one Zoom In / Zoom Out step. */
const ZOOM_STEP = 1.3;

export interface EditorFrameContext {
  tab: string;
  fpApi: FootprintEditorApi;
  fpDispatch: Dispatch<FpAction>;
  symApi: SymbolEditorApi;
  symDispatch: Dispatch<SymAction>;
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

  // Save / Save All: every edit of a library entry is stored in design.json as it is made (an undoable verb); there is no separate library file to write.
  // Export (File menu) writes the derived .kicad_mod / .kicad_sym.
  const saved = () => toast(fp ? "Footprint saved: every edit is stored in the design as you make it (File > Export writes a .kicad_mod)." : "Symbol saved: every edit is stored in the design as you make it (File > Export writes a .kicad_sym).");
  m.set("common.Control.save", saved);
  m.set("common.Control.saveAll", saved);
  // Print: the browser's print of the frame (styles/layout.css reduces the page to the canvas column).
  m.set("common.Control.print", () => window.print());

  // Show Library Tree (`ACTIONS::showLibraryTree`): the Libraries column.
  m.set("common.Control.showLibraryTree", () => setDockColumnCollapsed("tree", !getDockLayout().treeCollapsed));

  if (fp) {
    // Select (`ACTIONS::selectSetRect`, the first button of the right toolbar's Selection modes group): back to the select tool.
    m.set("common.Interactive.selectSetRect", () => void ctx.fpApi.setTool("select"));
    // Rotate Counterclockwise / Clockwise (R / Shift+R): the selected pads, a quarter turn. (The verbs rotate pads only: graphics and text have none.)
    // With the Pad tool armed they turn the pad it carries instead (`doInteractiveItemPlacement`, IPO_ROTATE: `newItem->Rotate( newItem->GetPosition(), rotationAngle )`,
    // the Footprint Editor's 90 degree step), which the placed pad then hands on to the default pad (kicad-port/fpPadPlacer.ts).
    const rotate = (quarterTurns: number) => () => {
      const st = ctx.fpApi.getState();
      if (st.activeTool === "pad") {
        ctx.fpDispatch({ type: "SET_CARRIED_PAD", pad: rotateCarriedPad(st.carriedPad ?? newPadFromMaster(st.defaultPad, "", { x: 0, y: 0 }), quarterTurns * 90) });
        return;
      }
      const pads = [...ctx.fpApi.getState().selection].filter((id) => ctx.fpApi.padById(id));
      if (pads.length === 0) return toast("Select one or more pads to rotate: pads are the only items this editor rotates so far.");
      for (const id of pads) void ctx.fpApi.rotatePad(id, quarterTurns);
    };
    m.set("pcbnew.InteractiveEdit.rotateCcw", rotate(1));
    m.set("pcbnew.InteractiveEdit.rotateCw", rotate(-1));
    // Properties (E): `EDIT_TOOL::Properties` -- the one selected pad's Pad Properties, Footprint Properties with nothing selected (kicad-port/libEditorEdit.ts).
    // The window-level hotkey runs it, and so do the Edit menu and the context menu; the canvas no longer reads E itself.
    const footprintProperties = m.get("pcbnew.ModuleEditor.footprintProperties");
    m.set("pcbnew.InteractiveEdit.properties", () => {
      const st = ctx.fpApi.getState();
      if (st.activeTool !== "select") return;
      const selection = [...st.selection].map((id) => ({ id, kind: ctx.fpApi.padById(id) ? "pad" : "other" }));
      const target = footprintPropertiesTarget(selection);
      if (target?.dialog === "pad") ctx.fpDispatch({ type: "SET_PAD_PROPERTIES_ID", id: target.id });
      else if (target?.dialog === "footprint") footprintProperties?.();
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
        if (!g || g.kind === "text") return;
        if (g.kind === "text_box") ctx.symDispatch({ type: "SET_TEXT_BOX_DIALOG", dialog: { id: target.id, box: g } });
        else ctx.symDispatch({ type: "SET_SHAPE_DIALOG_ID", id: target.id });
      }
    });
    // Rotate (R / Shift+R, `SYMBOL_EDITOR_EDIT_TOOL::Rotate`): the selected pins a quarter turn, counter-clockwise or clockwise, about the pin itself or the
    // selection's half-grid centre, one undo step (shapes do not turn here yet).
    const rotatePins = (ccw: boolean) => () => {
      const st = ctx.symApi.getState();
      if (st.activeTool !== "select") return;
      void ctx.symApi.rotateSelection(ccw);
    };
    m.set("eeschema.InteractiveEdit.rotateCCW", rotatePins(true));
    m.set("eeschema.InteractiveEdit.rotateCW", rotatePins(false));
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
    m.set("eeschema.InteractiveEdit.repeatDrawItem", () => void ctx.symApi.repeatLastPin());
    // Selection tool (`ACTIONS::selectionTool`).
    m.set("common.InteractiveSelection.selectionTool", () => ctx.symDispatch({ type: "SET_ACTIVE_TOOL", tool: "select" }));
    // Show Datasheet (`ACTIONS::showDatasheet`): the open symbol's datasheet field, opened like the schematic's does.
    m.set("common.Control.showDatasheet", () => {
      const sym = ctx.symApi.getState().symbol;
      if (!sym) return toast("Open a symbol first.");
      const target = datasheetTarget(sym.datasheet);
      if (target.kind === "none") toast("No datasheet defined.", "error");
      else if (target.kind === "url") window.open(target.url, "_blank", "noopener,noreferrer");
      else toast(`Cannot open datasheet '${target.text}': only absolute http(s)/file URLs can be opened from a browser.`, "error");
    });
  }

  // Everything on this editor's toolbars that is not supported here is not registered, so nothing reads as live that would do nothing.
  const table = (fp ? support.footprint : support.symbol) as Record<string, string>;
  for (const [name, state] of Object.entries(table)) if (state !== "supported") m.delete(name);
}
