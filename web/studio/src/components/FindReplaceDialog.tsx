// Ctrl+F "Find" / Ctrl+Alt+F "Find and Replace" for the schematic
// (eeschema/dialogs/dialog_sch_find.cpp over eeschema/tools/
// sch_find_replace_tool.cpp). One dialog, two modes like the source's
// DIALOG_SCH_FIND (`SetReplaceMode`): "find" hides the replace controls.
//
// Options ported from the dialog, with its labels (dialog_sch_find_base.cpp): Match case, Whole words only and Regular Expression (`updateFlags`:
// WHOLEWORD when "Whole words only" is ticked, else REGEX when "Regular Expression" is, else PLAIN; the dialog has no wildcard mode -- the
// expressions are crates/model/src/wx_regex.rs, the engine the net class patterns use too), Search pin names and numbers (`searchAllPins`), Include hidden fields (`searchAllFields`),
// "Search the current sheet only" (`searchCurrentSheetOnly`; off, the search goes over every sheet of the hierarchy in page order and
// each match takes you to its sheet), Search net names (`searchNetNames`), "Search the current selection only" (`searchSelectedOnly`, which implies
// the current sheet; shown in both modes, as the base shows it) and -- in the replace dialog only (`if( aStyle & wxFR_REPLACEDIALOG )`) -- "Replace
// matches in reference designators" (`replaceReferences`). The search reaches a sheet's name, file and fields, its pins, a power symbol's value, a
// text box and a directive label's fields too (UI-LEFT S12; `nextMatch`'s `addItem`); a Replace changes a sheet pin's name and a sheet's name, not
// its file (`SCH_FIELD::IsReplaceable`) nor a symbol's pin (`SCH_PIN::Replace` is a no-op in the schematic).
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
import { focusSchSearchPane, toggleSchSearchPane } from "../state/schSearchPane";

export function FindReplaceDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const { run } = useActionRunner();
  const open = state.schDialog === "find" || state.schDialog === "replace";
  const { selectedOnly, currentSheetOnly, backward } = state.schFind;
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
    void findNextMatch(api, dispatch, reversed);
  };

  // `updateFlags`: Whole words only wins over Regular Expression; the Regular Expression box keeps its tick under it (`regex_box`)
  const wholeChecked = search.mode === "whole_word";
  const regexChecked = search.mode === "regex" || !!search.regex_box;
  const setWhole = (on: boolean) => update({ mode: on ? "whole_word" : regexChecked ? "regex" : "plain" });
  const setRegex = (on: boolean) => update({ regex_box: on, mode: wholeChecked ? "whole_word" : on ? "regex" : "plain" });

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
              <input type="checkbox" aria-label="Whole words only" checked={wholeChecked} onChange={(e) => setWhole(e.target.checked)} />
              Whole words only
            </label>
            <label className="toggle">
              <input type="checkbox" aria-label="Regular Expression" checked={regexChecked} onChange={(e) => setRegex(e.target.checked)} />
              Regular Expression
            </label>
            <label className="toggle">
              <input type="checkbox" checked={search.search_pins} onChange={(e) => update({ search_pins: e.target.checked })} />
              Search pin names and numbers
            </label>
            <label className="toggle">
              <input type="checkbox" checked={search.search_hidden_fields} onChange={(e) => update({ search_hidden_fields: e.target.checked })} />
              Include hidden fields
            </label>
            <label className="toggle" title="Off: the search goes over every sheet of the hierarchy, in page order, and takes you to the sheet of each match">
              <input
                type="checkbox"
                checked={currentSheetOnly || selectedOnly}
                disabled={selectedOnly}
                onChange={(e) => dispatch({ type: "SET_SCH_FIND", find: { currentSheetOnly: e.target.checked, cursor: null, status: "" } })}
              />
              Search the current sheet only
            </label>
            <label className="toggle">
              <input type="checkbox" checked={search.search_net_names} onChange={(e) => update({ search_net_names: e.target.checked })} />
              Search net names
            </label>
            <label className="toggle">
              <input type="checkbox" checked={selectedOnly} onChange={(e) => dispatch({ type: "SET_SCH_FIND", find: { selectedOnly: e.target.checked, cursor: null, status: "" } })} />
              Search the current selection only
            </label>
            {replaceMode && (
              <label className="toggle">
                <input type="checkbox" checked={search.replace_references} onChange={(e) => update({ replace_references: e.target.checked })} />
                Replace matches in reference designators
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
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", gap: 12, marginTop: 10 }}>
            <div style={{ minHeight: 16, fontSize: 11, color: "var(--chrome-text-dim)" }}>{state.schFind.status}</div>
            {/* `DIALOG_SCH_FIND::onShowSearchPanel`: `ACTIONS::showSearch` (a toggle), the dialog closed, then `FocusSearch`; the label carries the hotkey (UI-LEFT-2 SS10) */}
            <a
              href="#"
              data-testid="sch-find-search-panel"
              style={{ fontSize: 11, whiteSpace: "nowrap" }}
              onClick={(e) => {
                e.preventDefault();
                toggleSchSearchPane();
                close();
                focusSchSearchPane();
              }}
            >
              Show search panel (Ctrl+G)
            </a>
          </div>
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
