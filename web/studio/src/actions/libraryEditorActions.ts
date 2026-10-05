// The KiCad actions of the two library editors, registered into the action runner's registry (`useActionRunner.ts` calls
// `registerLibraryEditorActions` once): the footprint editor's `pcbnew.ModuleEditor.*` and `pcbnew.PadTool.*` and the symbol editor's
// `eeschema.SymbolLibraryControl.*`, `eeschema.SymbolDrawing.*` and `eeschema.PinEditing.*`. Each handler cites the KiCad function it ports (the
// heavy ones live in `footprintLibraryOps.ts` / `symbolLibraryOps.ts`); handlers only act on their own tab (`actionTabGate.ts` keeps the
// menus and hotkeys in step).
//
// Not registered, with the reason recorded in `tools/ui-parity-missing.json`: `pcbnew.ModuleEditor.createFootprint` (the footprint wizard),
// `checkFootprint` (the footprint checker is DRC, which only kicad-cli runs), the five `pcbnew.MicrowaveTool.*` (board-only footprints and custom
// pads), `eeschema.SymbolLibraryControl.{deriveFromExistingSymbol, flattenSymbol, updateSymbolFields, showRelatedLibraryFieldsTable}` (derived
// symbols), `exportSymbolAsSVG` (kicad-cli), `nextSymbol` / `previousSymbol` (the Symbol Viewer), `showHiddenFields` and
// `eeschema.SymbolDrawing.drawSymbolTextBox`.
import type { Dispatch } from "react";
import type { Action as StudioAction } from "../state/store";
import type { FootprintEditorApi, FpAction } from "../state/footprintEditorStore";
import type { SymAction, SymbolEditorApi } from "../state/symbolEditorStore";
import * as fpLib from "./footprintLibraryOps";
import * as symLib from "./symbolLibraryOps";

export interface LibraryActionContext {
  tab: string;
  studioDispatch: Dispatch<StudioAction>;
  /** The board's parts, for Load footprint from current PCB and Insert footprint into PCB. */
  boardParts: readonly { ref: string; footprint: string | null }[];
  fpApi: FootprintEditorApi;
  fpDispatch: Dispatch<FpAction>;
  symApi: SymbolEditorApi;
  symDispatch: Dispatch<SymAction>;
}

export function registerLibraryEditorActions(m: Map<string, () => void>, ctx: LibraryActionContext): void {
  const fpc: fpLib.FpCtx = { api: ctx.fpApi, dispatch: ctx.fpDispatch };
  const sc: symLib.SymCtx = { api: ctx.symApi, dispatch: ctx.symDispatch };
  const fpTab = (fn: () => void | Promise<void>) => () => {
    if (ctx.tab === "footprint") void fn();
  };
  const symTab = (fn: () => void | Promise<void>) => () => {
    if (ctx.tab === "symbol") void fn();
  };
  const fpToast = (message: string) => ctx.fpDispatch({ type: "TOAST", message, kind: "info" });
  const symToast = (message: string) => ctx.symDispatch({ type: "TOAST", message, kind: "info" });
  const selectedPads = () => {
    const st = ctx.fpApi.getState();
    return [...st.selection].filter((id) => st.footprint?.pads.some((p) => p.id === id));
  };

  // ===================================================================================================
  // Footprint editor: FOOTPRINT_EDITOR_CONTROL (pcbnew/tools/footprint_editor_control.cpp) and PAD_TOOL (pad_tool.cpp)
  // ===================================================================================================

  // Edit Footprint -- FOOTPRINT_EDITOR_CONTROL::EditFootprint: the tree's selected footprint on the canvas.
  m.set("pcbnew.ModuleEditor.editFootprint", fpTab(() => fpLib.editFootprint(fpc)));
  // Cut / Copy / Paste / Duplicate Footprint -- CutCopyFootprint, PasteFootprint, DuplicateFootprint.
  m.set("pcbnew.ModuleEditor.cutFootprint", fpTab(() => fpLib.copyFootprint(fpc, true)));
  m.set("pcbnew.ModuleEditor.copyFootprint", fpTab(() => fpLib.copyFootprint(fpc, false)));
  m.set("pcbnew.ModuleEditor.pasteFootprint", fpTab(() => fpLib.pasteFootprint(fpc)));
  m.set("pcbnew.ModuleEditor.duplicateFootprint", fpTab(() => fpLib.duplicateFootprint(fpc)));
  // Rename Footprint... / Delete Footprint from Library -- RenameFootprint, DeleteFootprint.
  m.set("pcbnew.ModuleEditor.renameFootprint", fpTab(() => fpLib.renameFootprint(fpc)));
  m.set("pcbnew.ModuleEditor.deleteFootprint", fpTab(() => fpLib.deleteFootprint(fpc)));
  // Import Footprint... / Export Current Footprint... -- ImportFootprint, ExportFootprint (the loaded footprint, `GetFirstFootprint()`).
  m.set("pcbnew.ModuleEditor.importFootprint", fpTab(() => fpLib.importFootprint(fpc)));
  m.set("pcbnew.ModuleEditor.exportFootprint", fpTab(() => (ctx.fpApi.getState().name ? ctx.fpApi.exportKicadMod() : fpToast("There is no footprint to export."))));
  // Footprint Properties... -- Properties (`pcbnew.ModuleEditor.footprintProperties`).
  m.set("pcbnew.ModuleEditor.footprintProperties", fpTab(() => fpLib.footprintProperties(fpc)));
  // Pad Table... -- PAD_TOOL::PadTable (DIALOG_FP_EDIT_PAD_TABLE): refused while there is no footprint (`if( !footprint ) return 0`).
  m.set(
    "pcbnew.ModuleEditor.padTable",
    fpTab(() => (ctx.fpApi.getState().footprint ? ctx.fpDispatch({ type: "SET_PAD_TABLE_OPEN", open: true }) : fpToast("Open a footprint first.")))
  );
  // Repair Footprint -- RepairFootprint.
  m.set("pcbnew.ModuleEditor.repairFootprint", fpTab(() => fpLib.repairFootprint(fpc)));
  // Load footprint from current PCB -- FOOTPRINT_EDIT_FRAME::LoadFootprintFromBoard: pick a board footprint by reference (`SelectFootprintFromBoard`).
  m.set("pcbnew.ModuleEditor.loadFootprintFromBoard", fpTab(() => (ctx.boardParts.some((p) => p.footprint) ? ctx.fpDispatch({ type: "SET_LOAD_FROM_BOARD_OPEN", open: true }) : fpToast("The board has no footprint to load."))));
  // Insert footprint into PCB -- FOOTPRINT_EDIT_FRAME::SaveFootprintToBoard.
  m.set("pcbnew.ModuleEditor.saveFootprintToBoard", fpTab(() => fpLib.saveFootprintToBoard(fpc, ctx.boardParts)));

  // Add Pad -- PAD_TOOL::PlacePad (`doInteractiveItemPlacement`, `IPO_REPEAT | IPO_SINGLE_CLICK`): arms the pad tool; every click places a pad made from the default pad.
  m.set(
    "pcbnew.PadTool.placePad",
    fpTab(() => {
      const st = ctx.fpApi.getState();
      if (!st.name) return fpToast("Open a footprint first."); // `if( !board()->GetFirstFootprint() ) return 0`
      void ctx.fpApi.setTool(st.activeTool === "pad" ? "select" : "pad");
    })
  );
  // Default Pad Properties... -- FOOTPRINT_EDITOR_CONTROL::DefaultPadProperties: `ShowPadPropertiesDialog( nullptr )`, the dialog on `m_Pad_Master`.
  m.set("pcbnew.PadTool.defaultPadProperties", fpTab(() => ctx.fpDispatch({ type: "SET_DEFAULT_PAD_OPEN", open: true })));
  // Copy Pad Properties to Default -- PAD_TOOL::copyPadSettings: "can only copy from a single pad".
  m.set(
    "pcbnew.PadTool.CopyPadSettings",
    fpTab(() => {
      const pads = selectedPads();
      if (pads.length === 1 && ctx.fpApi.getState().selection.size === 1) {
        ctx.fpApi.copyPadProperties(pads[0]!);
        fpToast("Pad properties copied to the default pad");
      }
    })
  );
  // Paste Default Pad Properties to Selected -- PAD_TOOL::pastePadProperties: every selected pad.
  m.set(
    "pcbnew.PadTool.ApplyPadSettings",
    fpTab(() => {
      const pads = selectedPads();
      if (pads.length > 0) void ctx.fpApi.pastePadProperties(pads);
    })
  );
  // Push Pad Properties to Other Pads... -- PAD_TOOL::pushPadSettings: a single selected pad, then DIALOG_PUSH_PAD_PROPERTIES.
  m.set(
    "pcbnew.PadTool.PushPadSettings",
    fpTab(() => {
      if (selectedPads().length === 1 && ctx.fpApi.getState().selection.size === 1) ctx.fpDispatch({ type: "SET_PUSH_PAD_OPEN", open: true });
    })
  );
  // Renumber Pads... -- PAD_TOOL::EnumeratePads: DIALOG_ENUM_PADS, then the click-to-number tool. `if( !board()->GetFirstFootprint()->Pads().empty() )` only.
  m.set(
    "pcbnew.PadTool.enumeratePads",
    fpTab(() => {
      const fp = ctx.fpApi.getState().footprint;
      if (!fp || fp.pads.length === 0) return;
      ctx.fpDispatch({ type: "SET_RENUMBER_DIALOG_OPEN", open: true });
    })
  );

  // ===================================================================================================
  // Symbol editor: SYMBOL_EDITOR_CONTROL (eeschema/tools/symbol_editor_control.cpp), the drawing tools and the pin tool
  // ===================================================================================================

  // Edit Symbol -- SYMBOL_EDITOR_CONTROL::EditSymbol.
  m.set("eeschema.SymbolLibraryControl.editSymbol", symTab(() => symLib.editSymbol(sc)));
  // Cut / Copy / Paste / Duplicate / Delete Symbol -- CutCopyDelete, DuplicateSymbol (Paste is `DuplicateSymbol( true )`).
  m.set("eeschema.SymbolLibraryControl.cutSymbol", symTab(() => symLib.copySymbols(sc, true)));
  m.set("eeschema.SymbolLibraryControl.copySymbol", symTab(() => symLib.copySymbols(sc, false)));
  m.set("eeschema.SymbolLibraryControl.pasteSymbol", symTab(() => symLib.pasteSymbols(sc)));
  m.set("eeschema.SymbolLibraryControl.duplicateSymbol", symTab(() => symLib.duplicateSymbol(sc)));
  m.set("eeschema.SymbolLibraryControl.deleteSymbol", symTab(() => symLib.deleteSymbols(sc)));
  // Rename Symbol... -- RenameSymbol (the action is registered as `renameFootprint` in KiCad's own table, a copy-paste slip in its source).
  m.set("eeschema.SymbolLibraryControl.renameFootprint", symTab(() => symLib.renameSymbol(sc)));
  // Import Symbol... / Export... / Export View as PNG... -- SYMBOL_EDIT_FRAME::ImportSymbol / ExportSymbol, SYMBOL_EDITOR_CONTROL::ExportView.
  m.set("eeschema.SymbolLibraryControl.importSymbol", symTab(() => symLib.importSymbol(sc)));
  m.set("eeschema.SymbolLibraryControl.exportSymbol", symTab(() => symLib.exportSymbol(sc)));
  m.set("eeschema.SymbolLibraryControl.exportSymbolView", symTab(() => symLib.exportSymbolView(sc)));
  // Save As... / Save Copy As... -- SYMBOL_EDIT_FRAME::saveSymbolCopyAs( aOpenCopy ).
  m.set("eeschema.SymbolLibraryControl.saveSymbolAs", symTab(() => symLib.saveSymbolAs(sc, true)));
  m.set("eeschema.SymbolLibraryControl.saveSymbolCopyAs", symTab(() => symLib.saveSymbolAs(sc, false)));
  // Add Symbol to Schematic -- SYMBOL_EDITOR_CONTROL::AddSymbolToSchematic: the open symbol, at the unit being edited, becomes the schematic's symbol being placed.
  m.set(
    "eeschema.SymbolLibraryControl.addSymbolToSchematic",
    symTab(async () => {
      const placed = await symLib.symbolForSchematic(sc);
      if (!placed) return;
      ctx.studioDispatch({ type: "SET_TAB", tab: "schematic" });
      ctx.studioDispatch({ type: "SET_ARMED_SYMBOL", symbol: placed });
      ctx.studioDispatch({ type: "SET_ACTIVE_TOOL", tool: "sch_place_symbol" });
    })
  );
  // Show Pin Electrical Types / Show Pin Numbers / Show Hidden Pins -- ShowElectricalTypes, ShowPinNumbers, ToggleHiddenPins: render-setting toggles.
  m.set("eeschema.SymbolLibraryControl.showElectricalTypes", symTab(() => ctx.symDispatch({ type: "TOGGLE_ELECTRICAL_TYPES" })));
  m.set("eeschema.SymbolLibraryControl.showPinNumbers", symTab(() => ctx.symDispatch({ type: "TOGGLE_PIN_NUMBERS" })));
  m.set("eeschema.SymbolLibraryControl.showHiddenPins", symTab(() => ctx.symDispatch({ type: "TOGGLE_HIDDEN_PINS" })));
  // Synchronized Pins Mode -- ToggleSyncedPinsMode: `m_SyncPinEdit = !m_SyncPinEdit`; enabled for a multi-unit symbol only (`multiUnitModeCond`).
  m.set(
    "eeschema.SymbolLibraryControl.toggleSyncedPinsMode",
    symTab(() => {
      const st = ctx.symApi.getState();
      if (!st.symbol || st.symbol.unit_count <= 1) return symToast("Synchronized Pins Mode needs a symbol with more than one unit.");
      ctx.symDispatch({ type: "SET_SYNC_PINS", on: !st.syncPins });
      symToast(`Synchronized Pins Mode ${st.syncPins ? "off" : "on"}`);
    })
  );
  // Bulk Edit Symbol Fields... -- SYMBOL_EDITOR_CONTROL::ShowLibraryTable (DIALOG_LIB_FIELDS_TABLE, scope library).
  m.set("eeschema.SymbolLibraryControl.showLibraryFieldsTable", symTab(() => ctx.symDispatch({ type: "SET_FIELDS_TABLE_OPEN", open: true })));
  // Symbol Properties... (File menu and tree context menu of the symbol editor) -- `symbolProperties`: the schematic tab's own handler (a placed
  // symbol's dialog) stays what it is there; on the Symbol tab it is the open symbol's library properties.
  const placedSymbolProperties = m.get("eeschema.InteractiveEdit.symbolProperties");
  m.set("eeschema.InteractiveEdit.symbolProperties", () => (ctx.tab === "symbol" ? void symLib.symbolProperties(sc) : placedSymbolProperties?.()));

  // Push Pin Length / Name Size / Number Size -- SYMBOL_EDITOR_PIN_TOOL::PushPinProperties: from the one selected pin to every other pin.
  const pushPin = (field: "length" | "name_size" | "number_size") =>
    symTab(() => {
      const st = ctx.symApi.getState();
      const pins = [...st.selection].filter((id) => ctx.symApi.pinById(id));
      if (pins.length !== 1) return; // `singlePinCondition`
      void ctx.symApi.pushPinProperty(pins[0]!, field);
    });
  m.set("eeschema.PinEditing.pushPinLength", pushPin("length"));
  m.set("eeschema.PinEditing.pushPinNameSize", pushPin("name_size"));
  m.set("eeschema.PinEditing.pushPinNumSize", pushPin("number_size"));

  // Draw Lines / Draw Polygons / Move Symbol Anchor / Draw Text -- SYMBOL_EDITOR_DRAWING_TOOLS::DrawShape (SEGMENT / POLY), PlaceAnchor, TwoClickPlace: arm the tool.
  const armSymTool = (tool: "draw_lines" | "draw_polygon" | "anchor" | "text") =>
    symTab(() => {
      const st = ctx.symApi.getState();
      if (!st.symbol) return symToast("Open a symbol first.");
      ctx.symDispatch({ type: "SET_ACTIVE_TOOL", tool: st.activeTool === tool ? "select" : tool });
    });
  m.set("eeschema.SymbolDrawing.drawSymbolLines", armSymTool("draw_lines"));
  m.set("eeschema.SymbolDrawing.drawSymbolPolygon", armSymTool("draw_polygon"));
  m.set("eeschema.SymbolDrawing.placeSymbolAnchor", armSymTool("anchor"));
  m.set("eeschema.SymbolDrawing.placeSymbolText", armSymTool("text"));
}
