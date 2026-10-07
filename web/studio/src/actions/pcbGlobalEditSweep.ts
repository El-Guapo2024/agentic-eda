// The Global Edit tool's rows of the UI-actions sweep (docs/parity/UI-ACTIONS.md): Global Deletions,
// Cleanup Graphics, and Update Footprint(s) from Library. Each cites GLOBAL_EDIT_TOOL
// (pcbnew/tools/global_edit_tool.cpp at 8303b2ad); the logic is kicad-port/pcbGlobalEdit.ts and the
// dialogs components/PcbGlobalEditDialogs.tsx. Called from `registerPcbEditSweep`.

import { createElement } from "react";
import type { Cmd } from "../api/types";
import { fetchFootprint } from "../api/client";
import {
  DEFAULT_CLEANUP_GRAPHICS,
  DEFAULT_GLOBAL_DELETION,
  exchangeTargets,
  footprintNamesOf,
  planCleanupGraphics,
  planGlobalDeletions,
  type CleanupGraphicsOptions,
  type GlobalDeletionOptions,
} from "../kicad-port/pcbGlobalEdit";
import { CleanupGraphicsDialog, ExchangeFootprintsDialog, GlobalDeletionDialog, type ExchangeChoice } from "../components/PcbGlobalEditDialogs";
import { openSweepDialog } from "./pcbSweepDialogs";
import { createSweepHelpers, type SweepCtx } from "./pcbSweepKit";

/** Remembered between invocations, like the dialogs' C++ statics (`s_defaultTolerance`, `g_matchModeForUpdate`). */
const last: { cleanup: CleanupGraphicsOptions; exchange: ExchangeChoice | null } = {
  cleanup: { ...DEFAULT_CLEANUP_GRAPHICS },
  exchange: null,
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
            if (plan.clearMarkers) dispatch({ type: "SET_DRC_ENGINE", engine: state.drcEngine }); // `board->DeleteMARKERs()`: the DRC results go
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

  // ------------------------------------------------- Update Footprint(s) from Library
  // GLOBAL_EDIT_TOOL::ExchangeFootprints (update mode) -> DIALOG_EXCHANGE_FOOTPRINTS. A placed footprint takes its pads and courtyard
  // from its library entry once the entry is "updated on the board" (`Cmd::UpdateFootprintOnBoard`), so this publishes the entries of
  // the matching footprints. (Change Footprint(s) would give a footprint another library footprint; the board's footprints come from
  // the intent and design.json holds no per-footprint override, so that one is recorded as not wired.)
  const runUpdate = async (choice: ExchangeChoice, selected: string | null): Promise<void> => {
    if (!board) return;
    const refs = exchangeTargets(board, choice.scope, { selectedRef: selected, reference: choice.reference, value: choice.value, footprint: choice.footprint });
    if (refs.length === 0) {
      toast("No footprint matches.");
      return;
    }
    // Publish every library entry the matching footprints use that has not been yet.
    const names = footprintNamesOf(board, refs);
    const publish: string[] = [];
    let upToDate = 0;
    let noEntry = 0;
    for (const name of names) {
      try {
        const entry = await fetchFootprint(name);
        if (entry.published) upToDate++;
        else publish.push(name);
      } catch {
        noEntry++; // never opened in the Footprint Editor: the board already has the library's own geometry
      }
    }
    if (publish.length > 0) {
      const ok = await api.cmdBatch(publish.map((name): Cmd => ({ op: "update_footprint_on_board", name })));
      if (!ok) return;
    }
    const parts = [`Updated ${publish.length} footprint${publish.length === 1 ? "" : "s"} from the library`];
    if (upToDate > 0) parts.push(`${upToDate} already up to date`);
    if (noEntry > 0) parts.push(`${noEntry} with no edited library entry`);
    toast(`${parts.join("; ")}.`);
  };

  const updateFootprints = pcbOnly(() => {
    if (!board) return;
    const selected = [...state.selection].find((id) => board.parts.some((p) => p.ref === id && p.placed)) ?? null;
    const part = selected ? board.parts.find((p) => p.ref === selected) : undefined;
    const initial: ExchangeChoice = last.exchange ?? { scope: "all", reference: "*", value: "*", footprint: "*" };
    const start: ExchangeChoice = {
      ...initial,
      scope: selected ? "selected" : initial.scope === "selected" ? "all" : initial.scope,
      // `m_specifiedRef->ChangeValue( m_currentFootprint->GetReference() )` and the same for the value and library id
      reference: part?.ref ?? initial.reference,
      value: part?.value ?? initial.value,
      footprint: part?.footprint ?? initial.footprint,
    };
    openSweepDialog({
      kind: "element",
      element: createElement(ExchangeFootprintsDialog, {
        selectedRef: selected,
        initial: start,
        onOk: (choice: ExchangeChoice) => {
          last.exchange = choice;
          void runUpdate(choice, selected);
        },
      }),
    });
  });
  m.set("pcbnew.GlobalEdit.updateFootprint", updateFootprints);
  m.set("pcbnew.GlobalEdit.updateFootprints", updateFootprints);
}
