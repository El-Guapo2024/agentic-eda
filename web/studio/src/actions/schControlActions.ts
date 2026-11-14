// The schematic editor's control actions, registered into the action runner's registry (`useActionRunner.ts` calls `registerSchControlActions` once):
// `eeschema.EditorControl.*` (SCH_EDITOR_CONTROL and the attribute commands of SCH_EDIT_TOOL), `eeschema.NavigateTool.*` (SCH_NAVIGATE_TOOL),
// `eeschema.InspectionTool.*` (SCH_INSPECTION_TOOL) and `eeschema.Interactive.increment*` (SCH_TOOL_BASE::Increment). Each handler cites the KiCad
// function it ports at 8303b2ad; the heavy parts live in `kicad-port/` (tested) and `components/schControl/` (the dialogs and panels).
//
// Not registered, with the reason recorded in `tools/ui-parity-missing.json`: the simulator rows (deferred), Import Graphics and the three
// drag-and-drop handlers, Import Non-KiCad Schematic, the remote symbol panel, Show Hidden
// Fields, Show Pin Alternate Icons, Annotate Automatically, the internal restartMove and updateNetHighlighting events. The design variants
// (KiCad 25ec2335 `SCH_EDITOR_CONTROL::AddVariant` ...) open components/DesignVariantDialogs.tsx; the attribute toggles act on the current variant.
import { ARC_DRAW_MODES } from "../kicad-port/arcGeom";
import { ARC_MODE_ACTIONS, schArcDrawMode } from "../kicad-port/schArcDraw";
import type { Dispatch } from "react";
import type { Action as StudioAction, StudioApi, StudioState } from "../state/store";
import type { SchControlAction, SchControlState } from "../state/schControlStore";
import type { SymAction, SymbolEditorApi } from "../state/symbolEditorStore";
import type { Cmd } from "../api/types";
import { fetchHierarchy } from "../api/schControlClient";
import { neighbourSheet, samePath } from "../kicad-port/sheetPages";
import { ATTRIBUTE_ACTIONS, attributeChecked, hitSheet, INCREMENT_PARAMS, nearestTextItem, planIncrement, type IncrementTarget, type SymbolAttrKey } from "../kicad-port/schControl";
import { variantFlags } from "../kicad-port/designVariants";
import { labelNet, netAtPoint } from "../kicad-port/schNetAtPoint";
import { parsePinItemId, pinDistance, pinItems } from "../kicad-port/schPins";
import { padIds } from "../kicad-port/pcbItems";
import { collectSheetRefs } from "../kicad-port/crossProbe";
import { fetchSchematic } from "../api/client";
import { measureStrokeText } from "../components/text/strokeFont";
import { copySheetImage, exportSymbolSvg, saveSheetCopy } from "./schControlExports";
import { importFootprintAssignments } from "./schControlImports";
import { openAssignNetclass } from "./assignNetclass";
import { netNamePattern } from "../kicad-port/schNetclassPattern";
import { setFindHighlights } from "../state/commonTool";
import { schematicActions } from "./schActionRegistry";

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
    case "eeschema.EditorControl.showHiddenFields":
      return d.showHiddenFields;
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
    case "eeschema.EditorControl.togglePinAltIcons":
      return d.showPinAltIcons;
    case "eeschema.EditorControl.showNetNavigator":
      return ctx.control.netNavigatorOpen;
    default: {
      // KiCad 25ec2335's Draw Arc modes are `TOOLBAR_STATE::TOGGLE`: the mode in force is pressed while the tool runs (`SelectToolbarAction( DrawArcForMode( mode ) )`)
      const arcMode = ARC_DRAW_MODES.find((m) => ARC_MODE_ACTIONS[m].action === name);
      if (arcMode) return ctx.state.activeTool === "sch_arc" && schArcDrawMode() === arcMode;
      const key = ATTRIBUTE_ACTIONS[name];
      if (!key) return undefined;
      const sch = ctx.state.schematic;
      if (!sch) return false;
      // KiCad 25ec2335 `attribDNPCond` and kin: `GetDNP( sheet, variant )` of the current variant, over the symbols, sheets and rule areas selected
      return attributeChecked(attributeItems(sch, ctx.requestSelection(), ctx.state.schVariant).map((i) => i.flags), key);
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
      const hovered = selected.length === 0 && state.cursorNm ? hitSheet(sch.sheets, state.cursorNm.x, state.cursorNm.y) : null;
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
  // Show Hidden Fields -- `ToggleHiddenFields`, the same flag for the fields (the ones Delete or Field Properties hid).
  m.set("eeschema.EditorControl.showHiddenFields", toggleDisplay("showHiddenFields"));
  m.set("eeschema.EditorControl.showDirectiveLabels", toggleDisplay("showDirectiveLabels"));
  m.set("eeschema.EditorControl.showERCErrors", toggleDisplay("showErcErrors"));
  m.set("eeschema.EditorControl.showERCWarnings", toggleDisplay("showErcWarnings"));
  m.set("eeschema.EditorControl.showERCExclusions", toggleDisplay("showErcExclusions"));
  m.set("eeschema.EditorControl.markSimExclusions", toggleDisplay("markSimExclusions"));
  // Show Pin Alternate Icons -- `SCH_EDITOR_CONTROL::TogglePinAltIcons`: `cfg->m_Appearance.show_pin_alt_icons = !..`, then every item repainted. The Symbol Editor's
  // `SYMBOL_EDITOR_CONTROL::TogglePinAltIcons` is the same action there, with its own setting: the schematic tab's handler leaves the other tabs to it.
  schematicActions(m, ctx.tab).set("eeschema.EditorControl.togglePinAltIcons", () => controlDispatch({ type: "TOGGLE_DISPLAY", key: "showPinAltIcons" }));
  // Net Navigator -- `ShowNetNavigator` -> `ToggleNetNavigator`.
  m.set("eeschema.EditorControl.showNetNavigator", onSchematic(() => controlDispatch({ type: "SET_NET_NAVIGATOR", open: !control.netNavigatorOpen })));
  // Highlight Nets -- `HighlightNetCursor`: the picker tool; each click is `highlightNet( toolMgr, position )` (SchematicView.tsx owns the click), Esc leaves it.
  m.set("eeschema.EditorControl.highlightNetTool", onSchematic(() => dispatch({ type: "SET_ACTIVE_TOOL", tool: state.activeTool === "sch_highlight_net" ? "select" : "sch_highlight_net" })));
  // Find Net in Inspector -- `SCH_EDITOR_CONTROL::FindNetInInspector` -> `SCH_EDIT_FRAME::FindNetInInspector`: the net of the first selected item that has one (a wire, label or
  // power symbol or pin; a symbol itself has no connection), else the highlighted net; the Net Navigator opens with that name in its filter and the highlight is cleared.
  m.set(
    "eeschema.InteractiveEdit.findNetInInspector",
    onSchematic(() => {
      if (!sch) return;
      const netOfItem = new Map<string, string>([...sch.wires, ...sch.labels, ...sch.power_symbols].map((i) => [i.id, i.net]));
      // a selected pin's connection too (`SCH_PIN::Connection`)
      for (const pin of pinItems(sch)) if (pin.net) netOfItem.set(pin.id, pin.net);
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
  // Assign Netclass... -- `SCH_EDITOR_CONTROL::AssignNetclass`: the connections of the selected items (`GetSelection()`: no hover; a wire, bus, label, power symbol
  // or pin -- a symbol itself has no connection), "No nets selected." without one; the selection is cleared "in favor of highlighting"; each connection becomes its
  // net's name, a bus's `PREFIX*` (vector) or `PREFIX.*` (group), and a net no label names (`GetDriverPriority( driver ) < SHEET_PIN`: KiCad named it itself) stops
  // it with "All selected nets must be labeled to assign a netclass."; then the dialog (actions/assignNetclass.ts) on those patterns. The assignment is stored
  // with the board's rules, so it is undone from the board editor, as KiCad's project settings are not part of the schematic's undo either.
  m.set(
    "eeschema.InteractiveEdit.assignNetclass",
    onSchematic(() => {
      if (!sch) return;
      const connected = [...sch.wires, ...sch.labels, ...sch.power_symbols];
      const netOf = new Map<string, { net: string; bus: boolean }>(connected.map((i) => [i.id, { net: i.net, bus: "bus" in i && i.bus === true }]));
      for (const pin of pinItems(sch)) if (pin.net) netOf.set(pin.id, { net: pin.net, bus: false });
      const conns = [...state.selection].map((id) => netOf.get(id)).filter((c): c is { net: string; bus: boolean } => !!c && c.net !== "");
      if (conns.length === 0) return toast("No nets selected.", "error");
      dispatch({ type: "SET_SELECTION", refs: [] });
      const patterns = new Set<string>();
      for (const c of conns) {
        const pattern = netNamePattern(c.net, c.bus);
        if (pattern === null) return toast("All selected nets must be labeled to assign a netclass.", "error");
        patterns.add(pattern);
      }
      const candidates = [...new Set(connected.map((i) => i.net).filter((n) => n !== ""))].sort((a, b) => a.localeCompare(b));
      // the previewer: every item of the sheet on a matching net brightened, the others not
      const preview = (matching: readonly string[]) => {
        const want = new Set(matching);
        setFindHighlights(connected.filter((i) => i.net !== "" && want.has(i.net)).map((i) => i.id));
      };
      openAssignNetclass([...patterns], candidates, toast, preview);
    })
  );
  // Select on PCB -- `ExplicitCrossProbeToPcb` -> `doCrossProbeSchToPcb( aEvent, true )`: `RequestSelection()` (the whole selection, else the item under the
  // cursor) -> `SendSelectItemsToPcb( items, true )`, and the board's `FindItemsFromSyncSelection` selects: for a symbol (`F<ref>`) its footprint, for a sheet
  // (`S<sheet path>`) every footprint whose sheet path starts with the sheet's (its subsheets too), for a pin (`P<ref>/<number>`) the footprint's pads of that
  // number. Nothing sent (`parts.empty()`) when none of these is selected. The board editor is a tab here, which comes up with that selection.
  m.set(
    "eeschema.EditorControl.selectOnPCB",
    onSchematic(() => {
      if (!sch) return;
      const picked = state.selection.size > 0 ? [...state.selection] : ctx.requestSelection();
      const footprints: string[] = [];
      const sheetPaths: string[] = [];
      const add = (ids: Iterable<string>) => {
        for (const id of ids) if (!footprints.includes(id)) footprints.push(id);
      };
      for (const id of picked) {
        const pin = parsePinItemId(id);
        if (pin) {
          const part = api.partByRef(pin.key);
          if (part?.placed) add(padIds(part).filter((pad) => pad === `${part.ref}.${pin.number}` || pad.startsWith(`${part.ref}.${pin.number}#`)));
        } else if (sch.sheets.some((s) => s.id === id)) sheetPaths.push([...state.currentSheetPath, id].join("/"));
        else if (api.symbolById(id) && api.partByRef(id)?.placed) add([id]);
      }
      const select = () => {
        if (footprints.length === 0) return;
        dispatch({ type: "SET_SELECTION", refs: footprints });
        dispatch({ type: "SET_TAB", tab: "pcb" });
      };
      if (sheetPaths.length === 0) return select();
      void collectSheetRefs(async (path) => {
        const s = await fetchSchematic(path);
        return { symbolIds: s.symbols.map((x) => x.id), childIds: s.sheets.map((x) => x.id) };
      })
        .then((sheets) => {
          for (const [path, refs] of sheets) {
            if (sheetPaths.some((sp) => path === sp || path.startsWith(`${sp}/`))) add([...refs].filter((r) => api.partByRef(r)?.placed));
          }
          select();
        })
        .catch(() => select());
    })
  );

  // Do not Populate / Exclude from Bill of Materials / Exclude from Board / Exclude from Simulation -- `SCH_EDIT_TOOL::SetAttribute`:
  // `RequestSelection( { SCH_SYMBOL_T, SCH_SHEET_T, SCH_RULE_AREA_T } )`, and the whole collection (every unit of a multi-unit symbol, the sheets, the rule
  // areas) goes to one state -- set when any of it lacks the attribute, cleared when all of it has it -- in one commit ("Toggle Attribute"). KiCad 25ec2335:
  // in the current variant (`GetDNP( sheet, variant )`, `SetDNP( new_state, sheet, variant )`), the items' own flags when it is the default; a rule area
  // has no variant record (`SCH_RULE_AREA::SetDNP` ignores the variant). A power symbol is an `SCH_SYMBOL` taken as any (its flags are its Symbol Properties',
  // `SchExtras::power_props`); kept: the studio keeps no variant record for a power symbol, so in a variant its own flags are read and set.
  const setAttribute = (name: string) => {
    const key: SymbolAttrKey = ATTRIBUTE_ACTIONS[name]!;
    return onSchematic(() => {
      if (!sch) return;
      const variant = state.schVariant;
      const items = attributeItems(sch, ctx.requestSelection(), variant);
      if (items.length === 0) return;
      // the server collects the units of the reference this sheet path gives each symbol on every sheet and works out `new_state` over all of them and the
      // sheets and rule areas (`CollectOtherUnits` over `Hierarchy()`, r5 D5-M2, UI-LEFT S9), in the variant too: every unit's record of it (UI-LEFT-3 S3-18)
      void api.cmd({ op: "sch_edit", verb: "toggle_symbol_attribute", ids: items.map((i) => i.id), attribute: key, ...(variant !== "" ? { variant } : {}) });
    });
  };
  m.set("eeschema.EditorControl.setDNP", setAttribute("eeschema.EditorControl.setDNP"));
  m.set("eeschema.EditorControl.setExcludeFromBOM", setAttribute("eeschema.EditorControl.setExcludeFromBOM"));
  m.set("eeschema.EditorControl.setExcludeFromBoard", setAttribute("eeschema.EditorControl.setExcludeFromBoard"));
  m.set("eeschema.EditorControl.setExcludeFromSimulation", setAttribute("eeschema.EditorControl.setExcludeFromSimulation"));

  // ============================================================================================ design variants (KiCad 25ec2335)

  // Add Design Variant... -- `SCH_EDITOR_CONTROL::AddVariant` -> `SCH_EDIT_FRAME::AddVariant`: the variant choice's last entry, "Add New Design Variant...", which opens
  // `ShowAddVariantDialog`. Remove / Edit Description / Rename / Copy -- `RemoveVariant`, `EditVariantDescription`, `RenameVariant`, `CopyVariant`: a choice of the
  // variant, then the change (components/DesignVariantDialogs.tsx).
  const variantDialog = (action: "add" | "remove" | "describe" | "rename" | "copy") => onSchematic(() => controlDispatch({ type: "OPEN_DIALOG", dialog: { kind: "variant", action } }));
  m.set("eeschema.EditorControl.addVariant", variantDialog("add"));
  m.set("eeschema.EditorControl.removeVariant", variantDialog("remove"));
  m.set("eeschema.EditorControl.editVariantDescription", variantDialog("describe"));
  m.set("eeschema.EditorControl.renameVariant", variantDialog("rename"));
  m.set("eeschema.EditorControl.copyVariant", variantDialog("copy"));

  // Increment Annotations From... -- `IncrementAnnotations`: asks for the first reference and the step, then moves every reference with those letters from that number up.
  m.set("eeschema.EditorControl.incrementAnnotations", onSchematic(() => open({ kind: "increment_annotations" })));
  // Edit Sheet Page Number... -- `SCH_EDIT_TOOL::EditPageNumber`: the selected sheet's page, else the page of the sheet being shown.
  m.set(
    "eeschema.EditorControl.editPageNumber",
    onSchematic(() => {
      const picked = sch ? [...state.selection].filter((id) => sch.sheets.some((s) => s.id === id)) : [];
      const hovered = sch && picked.length === 0 && state.cursorNm ? hitSheet(sch.sheets, state.cursorNm.x, state.cursorNm.y) : null;
      const ids = picked.length > 0 ? picked : hovered ? [hovered] : [];
      if (ids.length > 1) return; // `if( selection.GetSize() > 1 ) return 0`
      if (ids.length === 1) return open({ kind: "page_number", path: [...state.currentSheetPath, ids[0]!] });
      // the sheet in view, the root included ("The root sheet and root screen are effectively the same thing"; UI-LEFT S21)
      open({ kind: "page_number", path: state.currentSheetPath });
    })
  );
  // Bulk Edit Symbol Library Links... -- `EditSymbolLibraryLinks` -> `InvokeDialogEditSymbolsLibId`.
  m.set("eeschema.EditorControl.editSymbolLibraryLinks", onSchematic(() => open({ kind: "library_links" })));
  // Rescue Symbols... -- `RescueSymbols` -> `RescueSymbolLibTableProject( true )` (components/schControl/RescueSymbolsDialog.tsx, crates/cli/src/rescue_api.rs).
  m.set("eeschema.EditorControl.rescueSymbols", onSchematic(() => open({ kind: "rescue_symbols" })));
  // Remap Legacy Library Symbols... -- `RemapSymbols` -> `DIALOG_SYMBOL_REMAP` (components/schControl/RemapSymbolsDialog.tsx).
  m.set("eeschema.EditorControl.remapSymbols", onSchematic(() => open({ kind: "remap_symbols" })));
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

  // Next / Previous Symbol Unit -- `ChangeUnit`: `newUnit = ( ( unit - 1 + delta + nUnits ) % nUnits ) + 1`, then `SYMBOL_EDIT_FRAME::SetUnit`: nothing when the
  // unit is the same (a one-unit symbol), else the tool in progress is cancelled (`cancelInteractive`) and the selection cleared (`selectionClear`) first.
  const changeUnit = (delta: number) =>
    onSymbolEditor(() => {
      const st = ctx.symApi.getState();
      const n = st.symbol?.unit_count ?? 0;
      if (n < 1) return;
      const unit = ((Math.max(st.activeUnit, 1) - 1 + delta + n) % n) + 1;
      if (unit === st.activeUnit) return;
      ctx.symDispatch({ type: "SET_DRAW_STATE", draw: null });
      ctx.symDispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      ctx.symDispatch({ type: "CLEAR_SELECTION" });
      ctx.symDispatch({ type: "SET_ACTIVE_UNIT", unit });
    });
  m.set("eeschema.EditorControl.nextUnit", changeUnit(1));
  m.set("eeschema.EditorControl.previousUnit", changeUnit(-1));

  // ============================================================================================ SCH_TOOL_BASE::Increment

  // Increment / Increment Primary / Decrement Primary / Increment Secondary / Decrement Secondary -- `Increment`: the selected labels (or texts) change by
  // `delta` in their `index`-th incrementable part; a selection of mixed kinds does nothing. `RequestSelection( incrementable )` is the selection, else the item under the cursor.
  const increment = (name: string) => {
    const { delta, index } = INCREMENT_PARAMS[name]!;
    return (param?: unknown) => {
      // the Symbol Editor runs the same `SCH_TOOL_BASE::Increment` on its pins and texts (`SYMBOL_EDITOR_EDIT_TOOL`, kicad-port/symIncrement.ts)
      if (ctx.tab === "symbol") return void ctx.symApi.incrementSelection(delta, index);
      onSchematic(() => schIncrement(delta, index))(param);
    };
  };
  const schIncrement = (delta: number, index: number) => {
    if (!sch) return;
    const targets = incrementTargets(ctx);
    const plan = planIncrement(targets, delta, index, { skipIOSQXZ: false });
    if (!plan || plan.length === 0) return;
    const cmds: Cmd[] = plan.map((p) => ({ op: "set_sch_item_text", id: p.id, text: p.text }));
    void api.cmdBatch(cmds);
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
  const c = ctx.state.cursorNm;
  if (!c) return [];
  const id = nearestTextItem(all, c.x, c.y, Math.max(1_500_000, 12 / (ctx.state.schematicView.scale || 1)));
  return all.filter((t) => t.id === id);
}

/** The net a Highlight Nets click lands on: the nearest wire, or label / power symbol anchor, within the pick radius; none clears the highlight (`highlightNet` with no connection). */
export function netAtClick(sch: NonNullable<StudioState["schematic"]>, x: number, y: number, scale: number, gridNm = 1_270_000): string | null {
  return netAtPoint(
    sch.wires,
    [...sch.labels.map((l) => ({ net: labelNet(l), at: l.at })), ...sch.power_symbols.map((p) => ({ net: p.net, at: p.at }))],
    x,
    y,
    // `GetNode`: the larger of five pixels and the length of the grid's diagonal (`GetGridSize().EuclideanNorm()`), tried from nothing up (kicad-port/schNetAtPoint.ts)
    Math.max(5 / (scale || 1), gridNm * Math.SQRT2),
    // a pin is hit through its box (the line, the name and the number), and its net is the one the sheet says
    pinItems(sch).map((pin) => ({ net: pin.net, distNm: pinDistance(pin, [x, y], measureStrokeText) }))
  );
}

type AttrFlags = Partial<Record<SymbolAttrKey, boolean>>;

/**
 * `SetAttribute`'s `RequestSelection( { SCH_SYMBOL_T, SCH_SHEET_T, SCH_RULE_AREA_T } )` over `ids`: each symbol, sheet and rule area, with its four flags in
 * `variant` (a symbol's and a sheet's record of it, else their own flags; a rule area's own, which no variant has; a power symbol's own, `power_props`, the
 * studio keeping no variant record for it).
 */
export function attributeItems(sch: NonNullable<StudioState["schematic"]>, ids: readonly string[], variant: string): Array<{ id: string; kind: "symbol" | "power" | "sheet" | "rule_area"; flags: AttrFlags; graphic?: NonNullable<typeof sch.graphics>[number] }> {
  const out: ReturnType<typeof attributeItems> = [];
  for (const id of new Set(ids)) {
    const sym = sch.symbols.find((s) => s.id === id);
    if (sym) {
      out.push({ id, kind: "symbol", flags: variantFlags(sym, variant) });
      continue;
    }
    const power = sch.power_symbols.find((p) => p.id === id);
    if (power) {
      const pp = power.props ?? {};
      out.push({ id, kind: "power", flags: { dnp: !!pp.dnp, exclude_from_bom: !!pp.exclude_from_bom, exclude_from_board: !!pp.exclude_from_board, exclude_from_sim: !!pp.exclude_from_sim } });
      continue;
    }
    const sheet = sch.sheets.find((s) => s.id === id);
    if (sheet) {
      const own = sheet.props ?? {};
      const rec = variant !== "" ? sheet.variants?.[variant] : undefined;
      // a record holds all four flags (`SCH_SHEET_VARIANT::InitializeAttributes` snapshots the sheet's own; a false one is not written)
      const flag = (k: SymbolAttrKey) => (rec ? !!rec[k as keyof typeof rec] : !!own[k as keyof typeof own]);
      out.push({ id, kind: "sheet", flags: { dnp: flag("dnp"), exclude_from_bom: flag("exclude_from_bom"), exclude_from_board: flag("exclude_from_board"), exclude_from_sim: flag("exclude_from_sim") } });
      continue;
    }
    const g = (sch.graphics ?? []).find((x) => x.id === id);
    if (g?.shape.type === "rule_area") {
      const r = g.shape;
      out.push({ id, kind: "rule_area", flags: { dnp: !!r.dnp, exclude_from_bom: !!r.exclude_from_bom, exclude_from_board: !!r.exclude_from_board, exclude_from_sim: !!r.exclude_from_sim }, graphic: g });
    }
  }
  return out;
}

