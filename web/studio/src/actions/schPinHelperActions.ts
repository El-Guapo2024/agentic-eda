// Pin Helpers -- the submenu `PIN_TRICKS_MENU` (eeschema/tools/sch_edit_tool.cpp at 8303b2ad) of a selection of pins and sheet pins: Wire, No Connect, Net Label, Hierarchical Label
// and Global Label, run by `SCH_SELECTION_TOOL::Main` on `ID_POPUP_SCH_PIN_TRICKS_*` (sch_selection_tool.cpp). The work is the `sch_edit` verb `pin_helper`
// (crates/ops/src/sch_pin_helpers.rs), one undo step; this file is what the studio does around it (kicad-port/schPinHelpers.ts): the items of the selection it takes, the wire grid a
// stub is one step of, and what is selected afterwards -- the new labels, nothing for a no-connect, and for a wire the stubs with their far ends picked and held by the Drag tool, so
// the next click drops them (the drop joins the step: one undo step) and Escape takes them away again.
//
// These are the studio's own actions (`studio.PinHelpers.*`): in KiCad they are menu ids, with no action of their own and no hotkey.
import { fetchSchematic } from "../api/client";
import { gridSizeFor } from "../kicad-port/gridOverrides";
import { madeByPinHelper, PIN_HELPERS, pinHelperCmd, pinHelperTargets, type PinHelperKind } from "../kicad-port/schPinHelpers";
import { pinItems } from "../kicad-port/schPins";
import { getGridOverrides } from "../state/gridOverrides";
import { getGridSettings } from "../state/gridSettings";
import { schematicActions, type ActionMap } from "./schActionRegistry";
import type { SchEditContext } from "./schEditActions";

export function registerSchPinHelperActions(registry: ActionMap, ctx: SchEditContext): void {
  const { state, api, dispatch } = ctx;
  const m = schematicActions(registry, state.tab);

  const run = (kind: PinHelperKind) => () => {
    const sch = state.schematic;
    if (!sch || state.tab !== "schematic") return;
    // the selection itself (`for( EDA_ITEM* item : m_selection )`): `requestSelection` leaves the pins out, as it does for every tool that moves, turns or edits
    const targets = pinHelperTargets(sch, [...state.selection]);
    if (targets.length === 0) return;
    // `grid.GetGridSize( GRID_HELPER_GRIDS::GRID_WIRES )`: a stub is one step of the wire grid
    const grid = gridSizeFor("wires", state.schGridNm, getGridSettings("schematic").grids, getGridOverrides("schematic"));
    const cmd = pinHelperCmd(kind, targets, grid);
    // where the pins are, to tell a stub's far end from the end at its pin
    const wanted = new Set(targets);
    const pinPoints: Array<[number, number]> = [
      ...pinItems(sch).filter((p) => wanted.has(p.id)).map((p) => p.tip),
      ...sch.sheets.flatMap((s) => s.pins.filter((p) => p.id && wanted.has(p.id)).map((p) => [p.at[0], p.at[1]] as [number, number])),
    ];
    if (kind === "wire") {
      // `RunSynchronousAction( SCH_ACTIONS::drag, &commit )`: the stubs are held by their ends; a click drops them, Escape reverts the commit
      void api.dragNewSchItems(cmd, (before, after) => {
        const made = madeByPinHelper("wire", before, after, pinPoints);
        return made.at ? { ids: made.select, ends: made.ends, at: { x: made.at[0], y: made.at[1] } } : null;
      });
      return;
    }
    void api.cmd(cmd).then(async (ok) => {
      if (!ok) return;
      // the sheet as the backend has it now (the state only catches up on the next render)
      const after = await fetchSchematic(state.currentSheetPath).catch(() => null);
      if (!after) return;
      // "Label Pins": the new labels are the selection ("Many users will want to drag these items to wire off of the pins"); "No Connect Pins": `ClearSelection()`
      dispatch({ type: "SET_SELECTION", refs: madeByPinHelper(kind, sch, after, pinPoints).select, raw: true });
    });
  };
  for (const h of PIN_HELPERS) m.set(h.action, run(h.kind));
}
