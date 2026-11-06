// The Symbol Editor's Find / Find and Replace dialog: `DIALOG_SCH_FIND` (eeschema/dialogs/dialog_sch_find.cpp, dialog_sch_find_base.cpp at 8303b2ad) as a
// `SYMBOL_EDIT_FRAME` opens it -- "Search for:", "Replace with:" (Find and Replace only), Match case, Whole words only, Regular Expression, Include hidden fields
// and "Search the current selection only"; the constructor hides "Search pin names and numbers" (and turns it on), "Search the current sheet only", "Search net
// names", "Replace matches in reference designators" and the search panel link there, and the base hides the direction. Find, Replace, Replace All and Close
// run the editor's actions (actions/symFindOps.ts); the walk and its rules are kicad-port/symFind.ts.
//
// `updateFlags`: Whole words only wins over Regular Expression (the Regular Expression box keeps its tick under it). Enter in either text is Find
// (`OnSearchForEnter`, `OnReplaceWithEnter`). Not ported: the find and replace histories of the combo boxes, and the brightening of every match while the dialog
// is open (`UpdateFind`: the Symbol Editor's canvas has no highlight of its own); the match found is selected and shown.
import { useSymApi, useSymDispatch } from "../../state/symbolEditorStore";
import { closeSymFind, setSymSelectedOnly, symFindNext, symReplaceAll, symReplaceAndFindNext, updateSymSearch, useSymFind } from "../../actions/symFindOps";

export function SymbolFindDialog() {
  const f = useSymFind();
  const api = useSymApi();
  const dispatch = useSymDispatch();
  if (!f.open) return null;
  const replaceMode = f.open === "replace";
  const s = f.search;
  const wholeChecked = s.mode === "whole_word";
  const regexChecked = s.mode === "regex" || !!s.regex_box;
  const setWhole = (on: boolean) => updateSymSearch({ mode: on ? "whole_word" : regexChecked ? "regex" : "plain" });
  const setRegex = (on: boolean) => updateSymSearch({ regex_box: on, mode: wholeChecked ? "whole_word" : on ? "regex" : "plain" });
  const find = () => void symFindNext(api, dispatch, false);

  return (
    <div className="dialog-backdrop" onClick={closeSymFind}>
      <div
        className="dialog"
        style={{ width: 420 }}
        role="dialog"
        aria-label={replaceMode ? "Find and Replace" : "Find"}
        data-testid="sym-find-dialog"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") closeSymFind();
        }}
      >
        <div className="dialog-header">
          <span>{replaceMode ? "Find and Replace" : "Find"}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "96px 1fr", marginBottom: 10 }}>
            <span>Search for:</span>
            <input
              autoFocus
              title="Text with optional wildcards"
              data-testid="sym-find-text"
              value={s.find}
              onChange={(e) => updateSymSearch({ find: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Enter") find();
              }}
            />
            {replaceMode && (
              <>
                <span>Replace with:</span>
                <input
                  data-testid="sym-replace-text"
                  value={s.replace}
                  onChange={(e) => updateSymSearch({ replace: e.target.value })}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") find();
                  }}
                />
              </>
            )}
          </div>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 4, fontSize: 11 }}>
            <label className="toggle">
              <input type="checkbox" checked={s.match_case} onChange={(e) => updateSymSearch({ match_case: e.target.checked })} />
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
              <input type="checkbox" data-testid="sym-find-hidden" checked={s.search_hidden_fields} onChange={(e) => updateSymSearch({ search_hidden_fields: e.target.checked })} />
              Include hidden fields
            </label>
            <label className="toggle">
              <input type="checkbox" checked={f.selectedOnly} onChange={(e) => setSymSelectedOnly(e.target.checked)} />
              Search the current selection only
            </label>
          </div>
          <div style={{ marginTop: 10, minHeight: 16, fontSize: 11, color: "var(--chrome-text-dim)" }} data-testid="sym-find-status">
            {f.status}
          </div>
        </div>
        <div className="dialog-footer">
          <button className={replaceMode ? undefined : "primary"} disabled={s.find === ""} onClick={find} data-testid="sym-find-next">
            Find
          </button>
          {replaceMode && (
            <>
              <button disabled={s.find === ""} onClick={() => void symReplaceAndFindNext(api, dispatch)} data-testid="sym-replace">
                Replace
              </button>
              <button className="primary" disabled={s.find === ""} onClick={() => void symReplaceAll(api)} data-testid="sym-replace-all">
                Replace All
              </button>
            </>
          )}
          <button onClick={closeSymFind}>Close</button>
        </div>
      </div>
    </div>
  );
}
