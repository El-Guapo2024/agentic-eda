// The sheet-pin actions: Place Pins from Sheet, Autoplace All Sheet Pins, Cleanup Sheet Pins and Sync Selected / All Sheet Pins -- ported from
// `SCH_DRAWING_TOOLS::TwoClickPlace / AutoPlaceAllSheetPins / SyncSheetsPins / SyncAllSheetsPins` and `SCH_EDIT_TOOL::CleanupSheetPins`
// (eeschema/tools at 8303b2ad); the rules are kicad-port/schSheetPins.ts, the pin verbs `sch_edit` `add_sheet_pin` / `delete_sheet_pin` / `edit_sheet_pin`.
//
// A sheet's pins are matched by name against the hierarchical labels of the sheet's own file, which the studio serves as the sheet's page.
import type { Cmd, Sheet } from "../api/types";
import { loadHierLabels, pinExtentOf, sheetAt } from "../components/schematic/schPinTool";
import { autoplacePins, hasUndefinedPins, pinsToCleanUp, unplacedLabels } from "../kicad-port/schSheetPins";
import { schematicActions, type ActionMap } from "./schActionRegistry";
import type { SchEditContext } from "./schEditActions";

const NO_NEW_LABELS = "No new hierarchical labels found.";

export function registerSchSheetPinActions(registry: ActionMap, ctx: SchEditContext): void {
  const { state, api, requestSelection, dispatch } = ctx;
  const m = schematicActions(registry, state.tab);
  const sch = state.schematic;
  const info = (message: string) => dispatch({ type: "TOAST", message, kind: "info" });

  /** A sheet pin is part of the sheet it sits on -- whichever sheet is in view: the pin verbs are addressed to it (kicad-port/schSheetCmd.ts). */
  const rootSheetOnly = (fn: () => void) => () => {
    if (state.tab !== "schematic" || !sch) return;
    fn();
  };

  /** "If we have a selected sheet use it, otherwise try to get one under the cursor." */
  const targetSheet = (): Sheet | undefined => {
    if (!sch) return undefined;
    const selected = requestSelection().find((id) => sch.sheets.some((s) => s.id === id));
    if (selected) return sch.sheets.find((s) => s.id === selected);
    return state.cursorUm ? sheetAt(sch, [state.cursorUm.x, state.cursorUm.y], 6 / (state.schematicView.scale || 1)) : undefined;
  };

  // Place Pins from Sheet: arms the tool; with a sheet at hand its labels are read straight away, otherwise the first click picks a sheet.
  m.set(
    "eeschema.InteractiveDrawing.placeSheetPin",
    rootSheetOnly(() => {
      dispatch({ type: "SET_DRAW_STATE", draw: null });
      if (state.activeTool === "sch_sheet_pin") return void dispatch({ type: "SET_ACTIVE_TOOL", tool: "select" });
      const sheet = targetSheet();
      if (!sheet) {
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "sch_sheet_pin" });
        dispatch({ type: "SET_DRAW_STATE", draw: { kind: "sch_shape", pin: { sheetId: null, queue: [] } } });
        return;
      }
      void loadHierLabels(state.currentSheetPath, sheet.id).then((labels) => {
        const queue = unplacedLabels(sheet.pins, labels);
        if (queue.length === 0) return info(NO_NEW_LABELS);
        dispatch({ type: "SET_ACTIVE_TOOL", tool: "sch_sheet_pin" });
        dispatch({ type: "SET_DRAW_STATE", draw: { kind: "sch_shape", pin: { sheetId: sheet.id, queue } } });
      });
    })
  );

  // Autoplace All Sheet Pins: a pin for every label of the sheet's file that has none, laid out from the sheet's corner (kicad-port/schSheetPins.ts `autoplacePins`), one undo step.
  m.set(
    "eeschema.InteractiveDrawing.autoplaceAllSheetPins",
    rootSheetOnly(() => {
      const sheet = targetSheet();
      if (!sheet) return;
      void loadHierLabels(state.currentSheetPath, sheet.id).then((labels) => {
        const placed = autoplacePins({ at: sheet.at, size: sheet.size }, sheet.pins, labels, pinExtentOf);
        if (placed.length === 0) return info(NO_NEW_LABELS);
        const cmds: Cmd[] = placed.map((p) => ({ op: "sch_edit", verb: "add_sheet_pin", sheet: sheet.id, name: p.label.name, shape: p.label.shape ?? "passive", at: { x: p.at[0], y: p.at[1] } }));
        void api.cmdBatch(cmds);
      });
    })
  );

  // Cleanup Sheet Pins: after asking ("Do you wish to delete the unreferenced pins from this sheet?"), the pins with no hierarchical label of their name go.
  m.set(
    "eeschema.InteractiveEdit.cleanupSheetPins",
    rootSheetOnly(() => {
      const sheet = targetSheet();
      if (!sheet) return;
      void loadHierLabels(state.currentSheetPath, sheet.id).then((labels) => {
        const pins = hasUndefinedPins(sheet.pins, labels) ? pinsToCleanUp(sheet.pins, labels).map((p) => ({ id: p.id, name: p.name })) : [];
        if (pins.length === 0) return info("Every pin of this sheet has a matching hierarchical label.");
        dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "cleanup_pins", sheetId: sheet.id, sheetName: sheet.name, pins } });
      });
    })
  );

  // Sync Selected Sheet Pins / Sync All Sheet Pins: the dialog that lines a sheet's pins up with its file's hierarchical labels (components/SchToolDialogs.tsx).
  m.set(
    "eeschema.InteractiveDrawing.syncSheetPins",
    rootSheetOnly(() => {
      const sheet = targetSheet();
      if (sheet) dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "sync_pins", sheetIds: [sheet.id], first: sheet.id } });
    })
  );
  m.set(
    "eeschema.InteractiveDrawing.syncAllSheetsPins",
    rootSheetOnly(() => {
      if (!sch || sch.sheets.length === 0) return info("No sub schematic found in the current project");
      const selected = targetSheet();
      dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "sync_pins", sheetIds: sch.sheets.map((s) => s.id), first: selected?.id } });
    })
  );
}
