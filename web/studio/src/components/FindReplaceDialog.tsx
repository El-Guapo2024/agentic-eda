// Ctrl+F "Find" / Ctrl+Alt+F "Find and Replace" for the schematic
// (eeschema/dialogs/dialog_sch_find.cpp over eeschema/tools/
// sch_find_replace_tool.cpp). One dialog, two modes like the source's
// DIALOG_SCH_FIND (`SetReplaceMode`): "find" hides the replace controls.
//
// Options ported from the dialog: Match case, Whole word (+ Wildcards, the
// third EDA_SEARCH_MATCH_MODE the dialog does not expose but the data
// struct has), Search all fields (hidden ones too: `searchAllFields`),
// Search pins (`searchAllPins`), Search net names (`searchNetNames`),
// Replace in references (`replaceReferences`) and "Search only selected
// objects" (`searchSelectedOnly`). Not ported: regex mode and "current
// sheet only" (this app edits one sheet's schematic at a time -- see
// PARITY-sch.md).
//
// Find Next/Previous cycle the match list GET /api/sch/find returns (the
// `nextMatch` order) with a cursor shared with F3 / Shift+F3
// (findNavigation.ts); every hit is selected and panned to. Replace /
// Replace All are the undoable `replace_text` verb: one match key for
// "Replace" (then Find Next, like ReplaceAndFindNext), all keys (or
// none, i.e. everything) for "Replace All".
import { useEffect } from "react";
import type { SchSearchData } from "../api/types";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { useActionRunner } from "../actions/useActionRunner";
import { setFindHighlights } from "../state/commonTool";
import { findNextMatch } from "./schematic/findNavigation";
import { updateFind } from "./schematic/findReplaceOps";

export function FindReplaceDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const { run } = useActionRunner();
  const open = state.schDialog === "find" || state.schDialog === "replace";
  const { selectedOnly, backward } = state.schFind;
  const search = state.schFind.search;

  // `DIALOG_SCH_FIND::OnSearchForText` / `SCH_BASE_FRAME::OnFindDialogClose` -> `ACTIONS::updateFind`: every match of the text is brightened while the dialog is
  // open and put back when the text changes or the dialog closes (common.Control.updateFind).
  useEffect(() => {
    if (!open) return;
    void updateFind(api);
    return () => setFindHighlights([]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, search, selectedOnly, state.schDialog]);

  if (!open) return null;
  const replaceMode = state.schDialog === "replace";
  const close = () => dispatch({ type: "SET_SCH_DIALOG", dialog: null });

  /** Any change to the search resets the wrap cursor (`m_afterItem`), like editing the find text does in the source. */
  const update = (patch: Partial<SchSearchData>) => dispatch({ type: "SET_SCH_FIND", find: { search: { ...search, ...patch }, cursor: null, status: "" } });

  const findNext = (reversed: boolean) => {
    if (search.find === "") return;
    void findNextMatch(state, dispatch, reversed);
  };

  const modeValue = search.mode;

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div
        className="dialog"
        style={{ width: 420 }}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
        }}
      >
        <div className="dialog-header">
          <span>{replaceMode ? "Find and Replace" : "Find"}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "84px 1fr", marginBottom: 10 }}>
            <span>Search for</span>
            <input
              autoFocus
              value={search.find}
              onChange={(e) => update({ find: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Enter") findNext(backward);
              }}
            />
            {replaceMode && (
              <>
                <span>Replace with</span>
                <input value={search.replace} onChange={(e) => update({ replace: e.target.value })} />
              </>
            )}
          </div>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 4, fontSize: 11 }}>
            <label className="toggle">
              <input type="checkbox" checked={search.match_case} onChange={(e) => update({ match_case: e.target.checked })} />
              Match case
            </label>
            <label className="toggle">
              <input type="checkbox" checked={modeValue === "whole_word"} onChange={(e) => update({ mode: e.target.checked ? "whole_word" : "plain" })} />
              Whole word
            </label>
            <label className="toggle">
              <input type="checkbox" checked={modeValue === "wildcard"} onChange={(e) => update({ mode: e.target.checked ? "wildcard" : "plain" })} />
              Wildcards (* and ?)
            </label>
            <label className="toggle">
              <input type="checkbox" checked={search.search_hidden_fields} onChange={(e) => update({ search_hidden_fields: e.target.checked })} />
              Search all fields (incl. hidden)
            </label>
            <label className="toggle">
              <input type="checkbox" checked={search.search_pins} onChange={(e) => update({ search_pins: e.target.checked })} />
              Search pins
            </label>
            <label className="toggle">
              <input type="checkbox" checked={search.search_net_names} onChange={(e) => update({ search_net_names: e.target.checked })} />
              Search net names
            </label>
            <label className="toggle">
              <input type="checkbox" checked={selectedOnly} onChange={(e) => dispatch({ type: "SET_SCH_FIND", find: { selectedOnly: e.target.checked } })} />
              Search only selected objects
            </label>
            {replaceMode && (
              <label className="toggle">
                <input type="checkbox" checked={search.replace_references} onChange={(e) => update({ replace_references: e.target.checked })} />
                Replace in reference designators
              </label>
            )}
          </div>
          <div style={{ display: "flex", gap: 14, marginTop: 10, fontSize: 11 }}>
            <span style={{ color: "var(--chrome-text-dim)" }}>Direction:</span>
            <label className="toggle">
              <input type="radio" checked={!backward} onChange={() => dispatch({ type: "SET_SCH_FIND", find: { backward: false } })} />
              Forward
            </label>
            <label className="toggle">
              <input type="radio" checked={backward} onChange={() => dispatch({ type: "SET_SCH_FIND", find: { backward: true } })} />
              Backward
            </label>
          </div>
          <div style={{ marginTop: 10, minHeight: 16, fontSize: 11, color: "var(--chrome-text-dim)" }}>{state.schFind.status}</div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Close</button>
          <button disabled={search.find === ""} onClick={() => findNext(true)}>
            Find Previous
          </button>
          <button className={replaceMode ? undefined : "primary"} disabled={search.find === ""} onClick={() => findNext(false)}>
            Find Next
          </button>
          {replaceMode && (
            <>
              <button disabled={search.find === ""} onClick={() => run("common.Interactive.replaceAndFindNext")}>
                Replace
              </button>
              <button className="primary" disabled={search.find === ""} onClick={() => run("common.Interactive.replaceAll")}>
                Replace All
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
