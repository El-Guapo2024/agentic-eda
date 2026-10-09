// Find, Find Next and Find Previous on the board (Ctrl+F, F3, Shift+F3): what `DIALOG_FIND::search` does with a press, and the three actions that run it.
//
//   pcbnew/dialogs/dialog_find.cpp     DIALOG_FIND::search                 clear the selection, build the list when it is out of date, step, show the hit
//   pcbnew/pcb_edit_frame.cpp          ShowFindDialog, FindNext            the dialog opens with the selection's text; F3 opens it first when it was never shown
//   pcbnew/tools/board_editor_control.cpp  Find / FindNext                 `ACTIONS::find`, `findNext`, `findPrevious` run on the board editor
//   pcbnew/tools/pcb_selection_tool.cpp    FindItem                         select the hit and bring the view to it (kicad-port/pcbFind.ts `focusView`)
//
// Find by Properties (`PCB_ACTIONS::findByProperties`) stays unwired.
import type { Dispatch } from "react";
import type { Action, StudioApi, StudioState } from "../state/store";
import type { SweepCtx } from "./pcbSweepKit";
import { findHits, findStatus, focusView, preloadText, stepFind, type PcbHit } from "../kicad-port/pcbFind";
import { itemBounds, unionBounds } from "../kicad-port/pcbItems";
import { netItems } from "../kicad-port/pcbSelectionOps";
import { getPcbFind, setFindOptions, setPcbFind, withHistory } from "../state/pcbFind";

type Box = [number, number, number, number];

/** The canvas's pixel size, as the other board actions read it. */
function canvasSize(): { width: number; height: number } | null {
  const r = document.querySelector(".pcb-canvas-container")?.getBoundingClientRect();
  return r && r.width >= 50 && r.height >= 50 ? { width: r.width, height: r.height } : null;
}

/** `itemPassesFilter( item, true )` for the items of a net: the tracks and vias the selection filter lets through, locked ones only when it allows them. */
function netRefs(state: StudioState, net: string): string[] {
  const board = state.board;
  if (!board) return [];
  const locked = new Set(board.locked ?? []);
  const tracks = new Set((board.routing?.tracks ?? []).map((t) => t.id));
  return netItems(board, new Set([net])).filter((id) => (state.selectionFilter.lockedItems || !locked.has(id)) && (tracks.has(id) ? state.selectionFilter.tracks : state.selectionFilter.vias));
}

/**
 * `PCB_SELECTION_TOOL::FindItem`: a footprint or a text is selected; a net selects its tracks and vias (`SelectAllItemsOnNet`); a marker is the DRC dialog's selected
 * violation. The view is then brought to it. (The C++ clears the selection first.)
 */
function showHit(api: StudioApi, dispatch: Dispatch<Action>, hit: PcbHit): void {
  const state = api.getState();
  const board = state.board;
  if (!board) return;
  let refs: string[] = [];
  let box: Box | null = null;
  if (hit.kind === "footprint" || hit.kind === "text") {
    refs = [hit.id];
    box = itemBounds(board, hit.id);
  } else if (hit.kind === "net") {
    refs = netRefs(state, hit.id);
    box = unionBounds(refs.map((id) => itemBounds(board, id)));
  } else {
    const at = state.drc?.violations[hit.index]?.items[0]?.pos ?? null;
    dispatch({ type: "SET_DRC_SELECTED", index: hit.index });
    box = at ? [at[0], at[1], at[0], at[1]] : null;
  }
  dispatch({ type: "SET_SELECTION", refs, raw: true });
  const size = canvasSize();
  if (box && size) dispatch({ type: "SET_VIEW", view: focusView(state.view, size.width, size.height, box) });
}

/**
 * One press of Find Next (`forward`) or Find Previous: `DIALOG_FIND::search`. With nothing to search for the dialog is shown instead. The selection is cleared first, every
 * time, the list is built again when the options, the board or the DRC run changed (or `restart`: "Restart Search"), and the hit it lands on is shown; past the end (Wrap off)
 * or with nothing found it says so.
 */
export function pcbFindStep(api: StudioApi, dispatch: Dispatch<Action>, forward: boolean, restart = false): void {
  const state = api.getState();
  const board = state.board;
  if (!board) return;
  const find = getPcbFind();
  const o = find.options;
  if (o.text === "") {
    setPcbFind({ open: true, shown: find.shown + 1 });
    return;
  }
  dispatch({ type: "SET_SELECTION", refs: [], raw: true }); // `RunAction( ACTIONS::selectionClear )`
  const key = JSON.stringify([o, state.version, state.drcVersion]);
  const stale = restart || find.builtFor !== key;
  const hits = stale ? findHits(board, state.drc?.violations ?? [], o) : find.hits;
  const step = stepFind(hits, stale ? 0 : find.cursor, forward, o.wrap, stale);
  setPcbFind({ hits, cursor: step.cursor, builtFor: key, status: findStatus(o.text, hits, step), history: withHistory(find.history, o.text) });
  if (hits.length === 0) dispatch({ type: "TOAST", message: `'${o.text}' not found`, kind: "info" });
  else if (step.hit == null) dispatch({ type: "TOAST", message: "No more items to show", kind: "info" });
  else showHit(api, dispatch, step.hit);
}

/** `PCB_EDIT_FRAME::ShowFindDialog`: the dialog opens, with the value of the selected footprint or the first line of the selected text when there is one. */
export function showPcbFindDialog(api: StudioApi): void {
  const state = api.getState();
  const find = getPcbFind();
  const text = preloadText(state.board, state.selection);
  setPcbFind({ open: true, shown: find.shown + 1, created: true });
  if (text !== "") setFindOptions({ text });
}

/** `PCB_EDIT_FRAME::FindNext( reverse )`: the dialog is shown first if it never was, then the press. */
export function pcbFindNext(api: StudioApi, dispatch: Dispatch<Action>, reverse: boolean): void {
  if (!getPcbFind().created) showPcbFindDialog(api);
  pcbFindStep(api, dispatch, !reverse);
}

export function registerPcbFindActions(m: Map<string, (param?: unknown) => void>, ctx: SweepCtx): void {
  if (ctx.state.tab !== "pcb") return;
  const { api, dispatch } = ctx;
  m.set("common.Interactive.find", () => showPcbFindDialog(api));
  m.set("common.Interactive.findNext", () => pcbFindNext(api, dispatch, false));
  m.set("common.Interactive.findPrevious", () => pcbFindNext(api, dispatch, true));
}
