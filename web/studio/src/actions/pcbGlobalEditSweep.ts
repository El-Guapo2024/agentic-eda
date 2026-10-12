// The Global Edit tool's rows of the UI-actions sweep (docs/parity/UI-ACTIONS.md): Global Deletions,
// Cleanup Graphics, Change Footprint(s) and Update Footprint(s) from Library. Each cites GLOBAL_EDIT_TOOL
// (pcbnew/tools/global_edit_tool.cpp at 8303b2ad); the logic is kicad-port/pcbGlobalEdit.ts and the
// dialogs components/PcbGlobalEditDialogs.tsx. Also the Inspect menu's Compare Footprint with Library
// (BOARD_INSPECTION_TOOL::DiffFootprint), which ends in the update dialog. Called from `registerPcbEditSweep`.

import { createElement } from "react";
import type { Cmd } from "../api/types";
import { DEFAULT_CLEANUP_GRAPHICS, DEFAULT_GLOBAL_DELETION, planCleanupGraphics, planGlobalDeletions, type CleanupGraphicsOptions, type GlobalDeletionOptions } from "../kicad-port/pcbGlobalEdit";
import { CHANGE_OPTIONS, UPDATE_OPTIONS, exchangeCmd, exchangeReport, isLibraryId, matchingFootprints, type ReportLine } from "../kicad-port/pcbExchange";
import { postExchangePlan } from "../api/fpAssign";
import { CleanupGraphicsDialog, ExchangeFootprintsDialog, GlobalDeletionDialog, type ExchangeState } from "../components/PcbGlobalEditDialogs";
import { FootprintDiffDialog } from "../components/FootprintDiffDialog";
import { closeSweepDialog, openSweepDialog } from "./pcbSweepDialogs";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";

/** Remembered between invocations, like the dialogs' C++ statics (`s_defaultTolerance`, `g_matchModeForUpdate`). */
const last: { cleanup: CleanupGraphicsOptions; change: ExchangeState | null; update: ExchangeState | null } = {
  cleanup: { ...DEFAULT_CLEANUP_GRAPHICS },
  change: null,
  update: null,
};

export function registerPcbGlobalEditSweep(m: Map<string, () => void>, ctx: SweepCtx): void {
  const { state, dispatch, api } = ctx;
  const board = state.board;
  const { toast, pcbOnly, applyEdit } = createSweepHelpers(ctx);

  // ------------------------------------------------------------------ Global Deletions
  // GLOBAL_EDIT_TOOL::GlobalDeletions -> DIALOG_GLOBAL_DELETION::DoGlobalDeletions (always comes up in a benign state: `OptOut`).
  m.set(
    "pcbnew.GlobalEdit.globalDeletions",
    pcbOnly(() => {
      if (!board) return;
      const layer = state.activeLayer ?? board.layers[0] ?? null;
      openSweepDialog({
        kind: "element",
        element: createElement(GlobalDeletionDialog, {
          options: { ...DEFAULT_GLOBAL_DELETION },
          currentLayer: layer,
          onOk: (o: GlobalDeletionOptions) => {
            const plan = planGlobalDeletions(board, o, layer);
            dispatch({ type: "CLEAR_SELECTION" }); // "Clear selection before removing any items"
            if (plan.clearMarkers) dispatch({ type: "DRC_CLEAR" }); // `board->DeleteMARKERs()`: the DRC results go
            if (plan.cmds.length === 0) {
              if (!plan.clearMarkers) toast("Nothing matched the deletion options.");
              return;
            }
            void applyEdit(plan.cmds, plan.removed, `Deleted ${plan.removed.length} item${plan.removed.length === 1 ? "" : "s"}.`);
          },
        }),
      });
    })
  );

  // ----------------------------------------------------------------- Cleanup Graphics
  // GLOBAL_EDIT_TOOL::CleanupGraphics -> DIALOG_CLEANUP_GRAPHICS: a live list of what the options would change, then "Update PCB".
  m.set(
    "pcbnew.GlobalEdit.cleanupGraphics",
    pcbOnly(() => {
      if (!board) return;
      const shapes = board.drawings?.shapes ?? [];
      openSweepDialog({
        kind: "element",
        element: createElement(CleanupGraphicsDialog, {
          shapes,
          options: last.cleanup,
          onOk: (o: CleanupGraphicsOptions) => {
            last.cleanup = o;
            const plan = planCleanupGraphics(shapes, o, false);
            const cmds: Cmd[] = [...plan.remove.map((id): Cmd => ({ op: "delete_shape", id })), ...plan.add.map((shape): Cmd => ({ op: "add_shape", shape }))];
            dispatch({ type: "CLEAR_SELECTION" }); // "Clear current selection list to avoid selection of deleted items"
            if (cmds.length === 0) {
              toast("Nothing to clean up.");
              return;
            }
            void applyEdit(cmds, plan.remove, "Graphics cleaned up.");
          },
        }),
      });
    })
  );

  // ------------------------------------------------ Change / Update Footprint(s) (from Library)
  // GLOBAL_EDIT_TOOL::ExchangeFootprints -> DIALOG_EXCHANGE_FOOTPRINTS. The four actions differ in whether the dialog opens on a footprint of the selection (`updateFootprint`,
  // `changeFootprint`: always, `m_currentFootprint` the first selected one; the plural ones: when the selection holds a footprint) and in the mode (update / change).
  // The exchange is `exchange_footprints` (crates/ops/src/fp_exchange.rs), one undo step ("Change Footprint" / "Update Footprint"); the server brings the library copy.
  // `only` is a footprint the dialog opens on whatever the selection (Compare Footprint with Library's "Update Footprint from Library...": the footprint is selected, then the dialog opens).
  const exchangeDialog = (mode: "change" | "update", alwaysCurrent: boolean, only: string | null = null) =>
    pcbOnly(() => {
      if (!board) return;
      const placed = (id: string) => board.parts.some((p) => p.ref === id && p.placed);
      const selected = new Set(only ? [only] : [...state.selection].filter(placed));
      const current = only ?? [...state.selection].find(placed) ?? null;
      // `currentMode`: the singular actions are "of the current footprint" whatever the selection; the plural ones when the selection has a footprint
      const currentMode = alwaysCurrent ? true : selected.size > 0;
      const part = current ? board.parts.find((p) => p.ref === current) : undefined;
      const saved = mode === "update" ? last.update : last.change;
      const base: ExchangeState = saved ?? { choice: { mode: mode === "update" ? "all" : "reference", reference: "", value: "", footprint: "" }, newId: "", options: mode === "update" ? UPDATE_OPTIONS : CHANGE_OPTIONS };
      // `TransferDataToWindow`: the text rows start from the current footprint's own reference, value and id (an update's new id is its own)
      const start: ExchangeState = {
        choice: {
          // g_matchModeForExchangeSelected / g_matchModeForUpdateSelected when opened on a footprint, the other pair when not
          mode: currentMode && current ? "selected" : base.choice.mode === "selected" ? (mode === "update" ? "all" : "reference") : base.choice.mode,
          reference: part?.ref ?? base.choice.reference,
          value: part?.value ?? base.choice.value,
          footprint: part?.footprint ?? base.choice.footprint,
        },
        newId: mode === "update" ? (part?.footprint ?? base.newId) : base.newId,
        options: base.options,
      };
      openSweepDialog({
        kind: "element",
        element: createElement(ExchangeFootprintsDialog, {
          mode,
          hasCurrent: currentMode && current !== null,
          initial: start,
          onRun: async (st: ExchangeState): Promise<ReportLine[]> => {
            if (mode === "update") last.update = st;
            else last.change = st;
            const refs = matchingFootprints(board, st.choice, selected, current);
            if (mode === "change" && !isLibraryId(st.newId.trim())) return [{ severity: "error", text: `'${st.newId.trim()}' is not a library id: a footprint is Library:Name.` }];
            if (refs.length === 0) return [{ severity: "info", text: "No footprint matches." }];
            const target = mode === "change" ? st.newId.trim() : null;
            let entries;
            try {
              entries = await postExchangePlan(refs, target);
            } catch (e) {
              return [{ severity: "error", text: e instanceof Error ? e.message : String(e) }];
            }
            // The footprints whose library copy is found are exchanged in one step; the others are told, as KiCad does
            const found = entries.filter((e) => e.found).map((e) => e.reference);
            let applied = true;
            if (found.length > 0) {
              applied = await api.cmdBatch([exchangeCmd(found, target, st.options)]);
              if (applied) dispatch({ type: "SET_SELECTION", refs: found });
            }
            // the server says why in a toast (the Strict switch refuses a change that adds a gate failure)
            if (!applied) return [{ severity: "error", text: api.getState().toast?.message ?? "The footprints could not be changed." }];
            return exchangeReport(mode, entries);
          },
        }),
      });
    });
  m.set("pcbnew.GlobalEdit.updateFootprint", exchangeDialog("update", true));
  m.set("pcbnew.GlobalEdit.updateFootprints", exchangeDialog("update", false));
  m.set("pcbnew.GlobalEdit.changeFootprint", exchangeDialog("change", true));
  m.set("pcbnew.GlobalEdit.changeFootprints", exchangeDialog("change", false));

  // ------------------------------------------------------------ Compare Footprint with Library
  // BOARD_INSPECTION_TOOL::DiffFootprint( const TOOL_EVENT& ): exactly one footprint (the selection, or the one under the cursor); otherwise "Select a footprint to diff with
  // its library equivalent." The dialog is components/FootprintDiffDialog.tsx; its "Update Footprint from Library..." selects the footprint and opens the update dialog on it.
  m.set(
    "pcbnew.InspectionTool.DiffFootprint",
    pcbOnly(() => {
      const footprints = ctx.requestSelection().filter((id) => board?.parts.some((p) => p.ref === id && p.placed));
      if (footprints.length !== 1) {
        toast("Select a footprint to diff with its library equivalent.", "error");
        return;
      }
      const reference = footprints[0]!;
      openSweepDialog({
        kind: "element",
        element: createElement(FootprintDiffDialog, {
          reference,
          onUpdate: () => {
            dispatch({ type: "SET_SELECTION", refs: [reference] });
            closeSweepDialog();
            exchangeDialog("update", true, reference)();
          },
        }),
      });
    })
  );
}
