// What Find by Properties' buttons do (pcbnew/dialogs/dialog_find_by_properties.cpp at 8303b2ad): `rebuildPropertyGrid` when the dialog is shown, the tab changes or the
// selection does; `onSelectMatchingClick` (`selectMatchingFromProperties`, `selectMatchingFromQuery`, `applyMatchResults`); `onCheckSyntaxClick`; `onCreateQueryClick`. The rows,
// the matching and the query are kicad-port/findByProperties.ts; the dialog is components/FindByPropertiesDialog.tsx and its state state/findByProperties.ts.
import type { Dispatch } from "react";
import type { Action, StudioApi, StudioState } from "../state/store";
import { postPcbQuery } from "../api/client";
import { buildRows, checkQuery, generateExpression, matchedText, savedModes, selectMatchingFromQuery, selectMatchingFromRows, withRecentQuery, type MatchMode } from "../kicad-port/findByProperties";
import { itemBounds, unionBounds } from "../kicad-port/pcbItems";
import { zoomFitBox } from "../kicad-port/zoomFit";
import { getFindByProperties, setFindByProperties } from "../state/findByProperties";

/** The selection and the board's revision a build of the rows is for. */
export const selectionKey = (ids: Iterable<string>, version: string | null): string => `${version}|${[...ids].join("\u0001")}`;

function canvasSize(): { width: number; height: number } | null {
  const r = document.querySelector(".pcb-canvas-container")?.getBoundingClientRect();
  return r && r.width >= 50 && r.height >= 50 ? { width: r.width, height: r.height } : null;
}

/**
 * `DIALOG_FIND_BY_PROPERTIES::rebuildPropertyGrid` for the selection of `state`: with nothing selected the label says so and the Create button is off, but the rows stay (KiCad
 * returns before it clears them); otherwise the rows are built again, keeping the Match modes of the rows of the same name.
 */
export function rebuildRows(state: StudioState): void {
  const f = getFindByProperties();
  const key = selectionKey(state.selection, state.version);
  if (!state.board || state.selection.size === 0) {
    setFindByProperties({ status: "No items selected", builtFor: key });
    return;
  }
  const built = buildRows(state.board, state.selection, state.units, savedModes(f.rows));
  setFindByProperties({ rows: built.rows, types: built.types, status: built.caption, builtFor: key });
}

/** `PCB_EDIT_FRAME::ShowFindByPropertiesDialog`: created once, shown and raised; showing it builds the rows. */
export function showFindByProperties(api: StudioApi): void {
  const f = getFindByProperties();
  setFindByProperties({ open: true, shown: f.shown + 1 });
  rebuildRows(api.getState());
}

/** A Match mode set in the grid (`onGridCellChanged`). */
export function setMatchMode(index: number, mode: MatchMode): void {
  const f = getFindByProperties();
  const rows = f.rows.map((r, i) => (i === index && !r.mixed ? { ...r, mode } : r));
  setFindByProperties({ rows });
}

/** `applyMatchResults`: clear the selection when asked, select the matches, frame them when asked, and say how many there were. */
function applyMatchResults(api: StudioApi, dispatch: Dispatch<Action>, ids: string[], after: (status: string) => void): void {
  const f = getFindByProperties();
  const state = api.getState();
  const base = f.deselect ? [] : [...state.selection];
  const refs = [...new Set([...base, ...ids])];
  dispatch({ type: "SET_SELECTION", refs, raw: true });
  if (f.zoom && ids.length > 0 && state.board) {
    const box = unionBounds(ids.map((id) => itemBounds(state.board!, id)));
    const size = canvasSize();
    if (box && size) {
      const view = zoomFitBox(box, [0, 0, 100_000, 100_000], size.width, size.height);
      if (view) dispatch({ type: "SET_VIEW", view });
    }
  }
  // The grid is built for the new selection right here, so that it is not rebuilt (and the label not replaced) when the selection reaches the dialog.
  if (state.board && refs.length > 0) {
    const built = buildRows(state.board, refs, state.units, savedModes(f.rows));
    setFindByProperties({ rows: built.rows, types: built.types, builtFor: selectionKey(refs, state.version) });
  } else {
    setFindByProperties({ builtFor: selectionKey(refs, state.version) });
  }
  after(matchedText(ids.length));
}

/** `onSelectMatchingClick`. */
export function selectMatching(api: StudioApi, dispatch: Dispatch<Action>): void {
  const f = getFindByProperties();
  const state = api.getState();
  const board = state.board;
  if (!board) return;
  if (f.tab === "selection") {
    const ids = selectMatchingFromRows(board, f.rows, f.types, state.units);
    applyMatchResults(api, dispatch, ids, (status) => setFindByProperties({ status }));
    return;
  }
  const expression = f.query.trim();
  if (expression === "") return;
  void selectMatchingFromQuery(expression, f.rows, board, state.units, postPcbQuery).then(({ check, ids }) => {
    if (!ids) {
      // `m_queryStatusLabel` and the `Expression Error` message box
      const status = check.status === "B. expressions are not supported." ? check.status : "Syntax error in expression";
      setFindByProperties({ queryStatus: status, errorBox: check.errors.map((e) => `${e}\n`).join("") || check.status });
      return;
    }
    setFindByProperties({ recent: withRecentQuery(getFindByProperties().recent, expression), errorBox: null });
    applyMatchResults(api, dispatch, ids, (status) => setFindByProperties({ queryStatus: status }));
  });
}

/** `onCheckSyntaxClick`. */
export function checkSyntax(api: StudioApi): void {
  const f = getFindByProperties();
  const state = api.getState();
  if (!state.board) return;
  void checkQuery(f.query, f.rows, state.board, state.units, postPcbQuery).then((check) => setFindByProperties({ queryStatus: check.status }));
}

/** `onCreateQueryClick`: the rows written as an expression, which goes to the Query tab; nothing when no row is active. */
export function createFromSelectionTab(api: StudioApi): void {
  const f = getFindByProperties();
  const state = api.getState();
  if (!state.board) return;
  const expression = generateExpression(f.rows, state.board, state.selection, state.units);
  if (expression !== "") setFindByProperties({ query: expression, tab: "query" });
}

/** The tab changed (`onNotebookPageChanged`): the grid is built again when it is the first. */
export function setTab(api: StudioApi, tab: "selection" | "query"): void {
  setFindByProperties({ tab });
  if (tab === "selection") rebuildRows(api.getState());
}

