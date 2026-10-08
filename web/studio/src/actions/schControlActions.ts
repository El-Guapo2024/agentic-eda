// The schematic editor's control actions, registered into the action runner's registry (`useActionRunner.ts` calls `registerSchControlActions` once):
// `eeschema.EditorControl.*` (SCH_EDITOR_CONTROL and the attribute commands of SCH_EDIT_TOOL), `eeschema.NavigateTool.*` (SCH_NAVIGATE_TOOL),
// `eeschema.InspectionTool.*` (SCH_INSPECTION_TOOL) and `eeschema.Interactive.increment*` (SCH_TOOL_BASE::Increment). Each handler cites the KiCad
// function it ports at 8303b2ad; the heavy parts live in `kicad-port/` (tested) and `components/schControl/` (the dialogs and panels).
//
// Not registered, with the reason recorded in `tools/ui-parity-missing.json`: the simulator rows (deferred), Import Graphics and the three
// drag-and-drop handlers, Import Non-KiCad Schematic, the legacy-library rescue and remap, the remote symbol panel, the design variants, Show Hidden
// Fields, Show Pin Alternate Icons, Annotate Automatically, the internal restartMove and updateNetHighlighting events.
import type { Dispatch } from "react";
import type { Action as StudioAction, StudioApi, StudioState } from "../state/store";
import type { SchControlAction, SchControlState } from "../state/schControlStore";
import type { SymAction, SymbolEditorApi } from "../state/symbolEditorStore";
import type { Cmd } from "../api/types";
import { fetchHierarchy } from "../api/schControlClient";
import { neighbourSheet, samePath } from "../kicad-port/sheetPages";
import { ATTRIBUTE_ACTIONS, attributeChecked, hitSheet, INCREMENT_PARAMS, nearestTextItem, nextAttributeState, planIncrement, type IncrementTarget, type SymbolAttrKey } from "../kicad-port/schControl";
import { netAtPoint } from "../kicad-port/schNetAtPoint";
import { copySheetImage, exportSymbolSvg, saveSheetCopy } from "./schControlExports";
import { importFootprintAssignments } from "./schControlImports";

export interface SchControlContext {
  tab: string;
  state: StudioState;
  api: StudioApi;
  dispatch: Dispatch<StudioAction>;
  /** The selection, or -- with nothing selected -- the symbol or wire under the cursor (`RequestSelection`). */
  requestSelection: () => string[];
  symApi: SymbolEditorApi;
  symDispatch: Dispatch<SymAction>;
  control: SchControlState;
  controlDispatch: Dispatch<SchControlAction>;
}

type Registry = Map<string, (param?: unknown) => void>;

/** The state a check mark shows for a View-menu toggle, the Net Navigator panel or an attribute of the selected symbols; `undefined` for an action that has no check. */
export function schControlChecked(name: string, ctx: Pick<SchControlContext, "control" | "state" | "requestSelection">): boolean | undefined {
  const d = ctx.control.display;
  switch (name) {
    case "eeschema.EditorControl.showHiddenPins":
      return d.showHiddenPins;
    case "eeschema.EditorControl.showDirectiveLabels":
      return d.showDirectiveLabels;
    case "eeschema.EditorControl.showERCErrors":
      return d.showErcErrors;
    case "eeschema.EditorControl.showERCWarnings":
      return d.showErcWarnings;
    case "eeschema.EditorControl.showERCExclusions":
      return d.showErcExclusions;
    case "eeschema.EditorControl.markSimExclusions":
      return d.markSimExclusions;
    case "eeschema.EditorControl.showNetNavigator":
      return ctx.control.netNavigatorOpen;
    default: {
      const key = ATTRIBUTE_ACTIONS[name];
      if (!key) return undefined;
      const sch = ctx.state.schematic;
      if (!sch) return false;
      const ids = new Set(ctx.requestSelection());
      return attributeChecked(sch.symbols.filter((s) => ids.has(s.id)), key);
    }
  }
}

export function registerSchControlActions(m: Registry, ctx: SchControlContext): void {
  const { state, api, dispatch, control, controlDispatch } = ctx;
  const sch = state.schematic;
  const onSchematic = (fn: (param?: unknown) => void) => (param?: unknown) => {
    if (ctx.tab === "schematic") fn(param);
  };
  const onSymbolEditor = (fn: () => void) => () => {
    if (ctx.tab === "symbol") fn();
  };
  const toast = (message: string, kind: "info" | "error" = "info") => dispatch({ type: "TOAST", message, kind });
  // Every schematic edit these actions send is addressed to the sheet in view (`api.cmd`, kicad-port/schSheetCmd.ts), so they act on that sheet's items.
  const open = (dialog: SchControlState["dialog"]) => dialog && controlDispatch({ type: "OPEN_DIALOG", dialog });

  // ============================================================================================ SCH_NAVIGATE_TOOL (hierarchy navigation)

  /** `SCH_NAVIGATE_TOOL::changeSheet`: cancel what is being drawn, clear the selection, push the history and show the sheet. */
  const changeSheet = (path: string[]) => {
    dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
    dispatch({ type: "SET_DRAW_STATE", draw: null });
    void api.navigateToSheet(path);
  };
  // Change Sheet -- `ChangeSheet`: the sheet path comes with the event (the Hierarchy panel's clicks); without one there is nothing to do (`wxCHECK( path, 0 )`).
  m.set(
    "eeschema.NavigateTool.changeSheet",
    onSchematic((param) => {
      if (Array.isArray(param) && param.every((id) => typeof id === "string")) changeSheet(param as string[]);
    })
  );
  // Enter Sheet -- `EnterSheet`: `RequestSelection( { SCH_SHEET_T } )`, and with exactly one sheet its contents are shown. The selection first, else the sheet under the cursor.
  m.set(
    "eeschema.NavigateTool.enterSheet",
    onSchematic(() => {
      if (!sch) return;
      const selected = [...state.selection].filter((id) => sch.sheets.some((s) => s.id === id));
      const hovered = selected.length === 0 && state.cursorUm ? hitSheet(sch.sheets, state.cursorUm.x, state.cursorUm.y) : null;
      const ids = selected.length > 0 ? selected : hovered ? [hovered] : [];
      if (ids.length === 1) changeSheet([...state.currentSheetPath, ids[0]!]);
    })
  );
  // Next Sheet / Previous Sheet (Page Down / Page Up) -- `Next` / `Previous`: the sheet one step along the page order; at either end KiCad rings the bell (nothing happens).
  const step = (delta: 1 | -1) =>
    onSchematic(() => {
      void fetchHierarchy().then((sheets) => {
        const target = neighbourSheet(sheets, state.currentSheetPath, delta);
        if (target && !samePath(target.path, state.currentSheetPath)) changeSheet(target.path);
      });
    });
  m.set("eeschema.NavigateTool.next", step(1));
  m.set("eeschema.NavigateTool.previous", step(-1));

  // ============================================================================================ SCH_EDITOR_CONTROL (the View menu, panels, cross-probing)

  // Switch to PCB Editor -- `ShowPcbNew` -> `OnOpenPcbnew`: the board editor; here its tab.
  m.set("eeschema.EditorControl.showPcbNew", onSchematic(() => dispatch({ type: "SET_TAB", tab: "pcb" })));
  // Show Hidden Pins / Show Directive Labels / Show ERC Errors / Show ERC Warnings / Show ERC Exclusions / Mark items excluded from simulation -- `ToggleHiddenPins`,
  // `ToggleDirectiveLabels`, `ToggleERCErrors`, `ToggleERCWarnings`, `ToggleERCExclusions`, `MarkSimExclusions`: `cfg->m_Appearance.<flag> = !cfg->m_Appearance.<flag>`, then a repaint.
  const toggleDisplay = (key: keyof SchControlState["display"]) => onSchematic(() => controlDispatch({ type: "TOGGLE_DISPLAY", key }));
  m.set("eeschema.EditorControl.showHiddenPins", toggleDisplay("showHiddenPins"));
  m.set("eeschema.EditorControl.showDirectiveLabels", toggleDisplay("showDirectiveLabels"));
  m.set("eeschema.EditorControl.showERCErrors", toggleDisplay("showErcErrors"));
  m.set("eeschema.EditorControl.showERCWarnings", toggleDisplay("showErcWarnings"));
  m.set("eeschema.EditorControl.showERCExclusions", toggleDisplay("showErcExclusions"));
  m.set("eeschema.EditorControl.markSimExclusions", toggleDisplay("markSimExclusions"));
  // Net Navigator -- `ShowNetNavigator` -> `ToggleNetNavigator`.
  m.set("eeschema.EditorControl.showNetNavigator", onSchematic(() => controlDispatch({ type: "SET_NET_NAVIGATOR", open: !control.netNavigatorOpen })));
  // Highlight Nets -- `HighlightNetCursor`: the picker tool; each click is `highlightNet( toolMgr, position )` (SchematicView.tsx owns the click), Esc leaves it.
  m.set("eeschema.EditorControl.highlightNetTool", onSchematic(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "sch_highlight_net" ? "select" : "sch_highlight_net" })));
  // Find Net in Inspector -- `SCH_EDITOR_CONTROL::FindNetInInspector` -> `SCH_EDIT_FRAME::FindNetInInspector`: the net of the first selected item that has one (a wire, label or
  // power symbol; a symbol itself has no connection), else the highlighted net; the Net Navigator opens with that name in its filter and the highlight is cleared.
  m.set(
    "eeschema.InteractiveEdit.findNetInInspector",
    onSchematic(() => {
      if (!sch) return;
      const netOfItem = new Map<string, string>([...sch.wires, ...sch.labels, ...sch.power_symbols].map((i) => [i.id, i.net]));
      let net = "";
      for (const id of state.selection) {
        const n = netOfItem.get(id);
        if (n) {
          net = n;
          break;
        }
      }
      if (net === "") net = state.netHighlight ?? "";
      if (net === "") return toast("No connected net selected.", "error");
      dispatch({ type: "SET_NET_HIGHLIGHT", net: null });
      controlDispatch({ type: "SET_NET_FILTER", text: net });
      controlDispatch({ type: "SET_NET_NAVIGATOR", open: true });
    })
  );
  // Select on PCB -- `ExplicitCrossProbeToPcb` -> `SendSelectItemsToPcb( items, true )`: the selected symbols' footprints are selected in the board editor.
  // (`RequestSelection`: the symbol under the cursor when nothing is selected.) Sheets and pins are the other two kinds KiCad sends; this schematic selects neither.
  m.set(
    "eeschema.EditorControl.selectOnPCB",
    onSchematic(() => {
      const refs = ctx.requestSelection().filter((id) => api.symbolById(id));
      const footprints = [...new Set(refs)].filter((ref) => api.partByRef(ref)?.placed);
      if (footprints.length === 0) return;
      dispatch({ type: "SET_SELECTION", refs: footprints });
      dispatch({ type: "SET_TAB", tab: "pcb" });
    })
  );

  // Do not Populate / Exclude from Bill of Materials / Exclude from Board / Exclude from Simulation -- `SCH_EDIT_TOOL::SetAttribute`: the whole selection (every unit of a
  // multi-unit symbol) goes to one state -- set when any of it lacks the attribute, cleared when all of it has it.
  const setAttribute = (name: string) => {
    const key: SymbolAttrKey = ATTRIBUTE_ACTIONS[name]!;
    return onSchematic(() => {
      if (!sch) return;
      const ids = [...new Set(ctx.requestSelection().filter((id) => api.symbolById(id)))];
      if (ids.length === 0) return;
      const items = sch.symbols.filter((s) => ids.includes(s.id));
      void api.cmd({ op: "set_symbol_attrs", ids, [key]: nextAttributeState(items, key) });
    });
  };
  m.set("eeschema.EditorControl.setDNP", setAttribute("eeschema.EditorControl.setDNP"));
  m.set("eeschema.EditorControl.setExcludeFromBOM", setAttribute("eeschema.EditorControl.setExcludeFromBOM"));
  m.set("eeschema.EditorControl.setExcludeFromBoard", setAttribute("eeschema.EditorControl.setExcludeFromBoard"));
  m.set("eeschema.EditorControl.setExcludeFromSimulation", setAttribute("eeschema.EditorControl.setExcludeFromSimulation"));

  // Increment Annotations From... -- `IncrementAnnotations`: asks for the first reference and the step, then moves every reference with those letters from that number up.
  m.set("eeschema.EditorControl.incrementAnnotations", onSchematic(() => open({ kind: "increment_annotations" })));
  // Edit Sheet Page Number... -- `SCH_EDIT_TOOL::EditPageNumber`: the selected sheet's page, else the page of the sheet being shown.
  m.set(
    "eeschema.EditorControl.editPageNumber",
    onSchematic(() => {
      const picked = sch ? [...state.selection].filter((id) => sch.sheets.some((s) => s.id === id)) : [];
      const hovered = sch && picked.length === 0 && state.cursorUm ? hitSheet(sch.sheets, state.cursorUm.x, state.cursorUm.y) : null;
      const ids = picked.length > 0 ? picked : hovered ? [hovered] : [];
      if (ids.length > 1) return; // `if( selection.GetSize() > 1 ) return 0`
      if (ids.length === 1) return open({ kind: "page_number", path: [...state.currentSheetPath, ids[0]!] });
      if (state.currentSheetPath.length === 0) return toast("The root sheet is page 1: its page number is not stored, only the sub-sheets' are.");
      open({ kind: "page_number", path: state.currentSheetPath });
    })
  );
  // Bulk Edit Symbol Library Links... -- `EditSymbolLibraryLinks` -> `InvokeDialogEditSymbolsLibId`.
  m.set("eeschema.EditorControl.editSymbolLibraryLinks", onSchematic(() => open({ kind: "library_links" })));
  // Assign Footprints... -- `ShowCvpcb` -> `OnOpenCvpcb`: the footprint assignment tool (CvPcb); here a dialog of the same three lists.
  m.set("eeschema.EditorControl.assignFootprints", onSchematic(() => open({ kind: "assign_footprints" })));
  // Import Footprint Assignments... -- `ImportFPAssignments` -> `processCmpToFootprintLinkFile`: a `.cmp` file's footprints go to the symbols it names.
  m.set("eeschema.EditorControl.importFPAssignments", onSchematic(() => void importFootprintAssignments(ctx)));
  // Generate Bill of Materials... -- `GenerateBOM`: the Symbol Fields Table on its Export tab (`ShowExportTab`).
  m.set(
    "eeschema.EditorControl.generateBOM",
    onSchematic(() => {
      controlDispatch({ type: "SET_FIELDS_TABLE_ON_EXPORT", on: true });
      dispatch({ type: "SET_SCH_DIALOG", dialog: "fields_table" });
    })
  );
  // Generate Legacy Bill of Materials... -- `GenerateBOMLegacy` -> `InvokeDialogCreateBOM`: KiCad's BOM generator scripts over the intermediate XML netlist.
  m.set("eeschema.EditorControl.generateBOMLegacy", onSchematic(() => open({ kind: "legacy_bom" })));
  // Save Current Sheet Copy As... -- `SaveCurrSheetCopyAs`: the sheet being shown, as a `.kicad_sch` file.
  m.set("eeschema.EditorControl.saveCurrSheetCopyAs", onSchematic(() => void saveSheetCopy(ctx)));
  // Export Drawing to Clipboard -- `DrawSheetOnClipboard`: the whole page of the sheet being shown, as a picture.
  m.set("eeschema.EditorControl.drawSheetOnClipboard", onSchematic(() => void copySheetImage(ctx)));
  // Export > Symbols... -- `ExportSymbolsToLibrary`: the library symbols the schematic uses, into a library file.
  m.set("eeschema.EditorControl.exportSymbolsToLibrary", onSchematic(() => open({ kind: "export_symbols" })));

  // ============================================================================================ SCH_INSPECTION_TOOL

  // Show Bus Syntax Help -- `ShowBusSyntaxHelp` -> `SCH_TEXT::ShowSyntaxHelp`.
  m.set("eeschema.InspectionTool.showBusSyntaxHelp", () => open({ kind: "bus_syntax" }));
  // Compare Symbol with Library -- `DiffSymbol`: `RequestSelection( { SCH_SYMBOL_T } )`, empty -> "Select a symbol to diff against its library equivalent."
  m.set(
    "eeschema.InspectionTool.diffSymbol",
    onSchematic(() => {
      const ref = ctx.requestSelection().find((id) => api.symbolById(id));
      if (!ref) return toast("Select a symbol to diff against its library equivalent.", "error");
      open({ kind: "symbol_diff", ref });
    })
  );
  // Symbol Checker -- `CheckSymbol`: the warnings for the symbol open in the Symbol Editor (`if( !symbol ) return 0`).
  m.set(
    "eeschema.InspectionTool.checkSymbol",
    onSymbolEditor(() => {
      if (ctx.symApi.getState().symbol) open({ kind: "symbol_check" });
    })
  );

  // Export Symbol as SVG... -- `SYMBOL_EDITOR_CONTROL::ExportSymbolAsSVG`: the symbol being edited, in its unit and body style, plotted by kicad-cli.
  m.set("eeschema.SymbolLibraryControl.exportSymbolAsSVG", onSymbolEditor(() => void exportSymbolSvg(ctx)));

  // ============================================================================================ SYMBOL_EDITOR_CONTROL::ChangeUnit

  // Next / Previous Symbol Unit -- `ChangeUnit`: `newUnit = ( ( unit - 1 + delta + nUnits ) % nUnits ) + 1`.
  const changeUnit = (delta: number) =>
    onSymbolEditor(() => {
      const st = ctx.symApi.getState();
      const n = st.symbol?.unit_count ?? 0;
      if (n < 1) return;
      const unit = ((Math.max(st.activeUnit, 1) - 1 + delta + n) % n) + 1;
      ctx.symDispatch({ type: "SET_ACTIVE_UNIT", unit });
    });
  m.set("eeschema.EditorControl.nextUnit", changeUnit(1));
  m.set("eeschema.EditorControl.previousUnit", changeUnit(-1));

  // ============================================================================================ SCH_TOOL_BASE::Increment

  // Increment / Increment Primary / Decrement Primary / Increment Secondary / Decrement Secondary -- `Increment`: the selected labels (or texts) change by
  // `delta` in their `index`-th incrementable part; a selection of mixed kinds does nothing. `RequestSelection( incrementable )` is the selection, else the item under the cursor.
  const increment = (name: string) => {
    const { delta, index } = INCREMENT_PARAMS[name]!;
    return onSchematic(() => {
      if (!sch) return;
      const targets = incrementTargets(ctx);
      const plan = planIncrement(targets, delta, index, { skipIOSQXZ: false });
      if (!plan || plan.length === 0) return;
      const cmds: Cmd[] = plan.map((p) => ({ op: "set_sch_item_text", id: p.id, text: p.text }));
      void api.cmdBatch(cmds);
    });
  };
  m.set("eeschema.Interactive.increment", increment("eeschema.Interactive.increment"));
  m.set("eeschema.Interactive.incrementPrimary", increment("eeschema.Interactive.incrementPrimary"));
  m.set("eeschema.Interactive.decrementPrimary", increment("eeschema.Interactive.decrementPrimary"));
  m.set("eeschema.Interactive.incrementSecondary", increment("eeschema.Interactive.incrementSecondary"));
  m.set("eeschema.Interactive.decrementSecondary", increment("eeschema.Interactive.decrementSecondary"));
}

/** The labels and texts `Increment` may act on: the selected ones, else the one nearest the cursor. */
function incrementTargets(ctx: SchControlContext): IncrementTarget[] {
  const sch = ctx.state.schematic;
  if (!sch) return [];
  const all: Array<IncrementTarget & { at: readonly [number, number] }> = [
    ...sch.labels.map((l) => ({ id: l.id, kind: (l.scope === "global" ? "label:global" : l.scope === "hierarchical" ? "label:hierarchical" : "label:local") as IncrementTarget["kind"], text: l.net, at: l.at })),
    ...sch.texts.map((t) => ({ id: t.id, kind: "text" as const, text: t.content, at: t.at })),
  ];
  const selected = all.filter((t) => ctx.state.selection.has(t.id));
  if (selected.length > 0) return selected;
  const c = ctx.state.cursorUm;
  if (!c) return [];
  const id = nearestTextItem(all, c.x, c.y, Math.max(1500, 12 / (ctx.state.schematicView.scale || 1)));
  return all.filter((t) => t.id === id);
}

/** The net a Highlight Nets click lands on: the nearest wire, or label / power symbol anchor, within the pick radius; none clears the highlight (`highlightNet` with no connection). */
export function netAtClick(sch: NonNullable<StudioState["schematic"]>, x: number, y: number, scale: number): string | null {
  return netAtPoint(
    sch.wires,
    [...sch.labels.map((l) => ({ net: l.net, at: l.at })), ...sch.power_symbols.map((p) => ({ net: p.net, at: p.at }))],
    x,
    y,
    400 / (scale || 1)
  );
}
