// The board editor's Find by Properties dialog (pcbnew/dialogs/dialog_find_by_properties.cpp over dialog_find_by_properties_base.cpp): two tabs, "From Selection" (a grid of the
// properties the selected items share, each with a Match of Ignored, Matching or Different) and "Query" (an expression: Recently Used, the editor, a status line, Check Syntax and
// Create from Selection Tab), the options Deselect non-matching items and Zoom to fit selected items, and Close and Select Matching. Like KiCad's it is modeless -- a floating
// pane, so the board stays in reach and the items it selects are in view -- and closing it only hides it. What the buttons do is actions/findByPropertiesActions.ts, what the rows
// and the query mean kicad-port/findByProperties.ts, and its state state/findByProperties.ts.
import { useEffect, useMemo, useRef, type KeyboardEvent } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { setFindByProperties, useFindByProperties } from "../state/findByProperties";
import { checkSyntax, createFromSelectionTab, rebuildRows, selectMatching, selectionKey, setMatchMode, setTab } from "../actions/findByPropertiesActions";
import { anyActive, completionNames, MATCH_LABELS, type MatchMode } from "../kicad-port/findByProperties";

const MODES: MatchMode[] = ["ignored", "matching", "different"];

export function FindByPropertiesDialog() {
  const f = useFindByProperties();
  const state = useStudioState();
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const editor = useRef<HTMLTextAreaElement>(null);

  // `OnSelectionChanged` / `Show`: the grid is built for the selection whenever the dialog is up on the first tab and the selection (or the board) is not the one it was built for.
  const key = selectionKey(state.selection, state.version);
  useEffect(() => {
    if (f.open && f.tab === "selection" && f.builtFor !== key) rebuildRows(api.getState());
  }, [f.open, f.tab, f.builtFor, key, api]);

  // `DIALOG_FIND_BY_PROPERTIES::Show`: the query takes the focus when the dialog is shown on its tab.
  useEffect(() => {
    if (f.open && f.tab === "query") editor.current?.focus();
  }, [f.open, f.shown, f.tab]);

  // `onScintillaCharAdded` after "A.": the names of the properties and fields.
  const names = useMemo(() => completionNames(f.rows), [f.rows]);
  const partial = /(?:^|[^A-Za-z0-9_])A\.([A-Za-z0-9_]*)$/.exec(f.query);
  const suggestions = partial ? names.filter((n) => n.toLowerCase().startsWith((partial[1] as string).toLowerCase())).slice(0, 14) : [];

  if (!f.open || state.tab !== "pcb") return null;
  const close = () => setFindByProperties({ open: false });
  const press = () => selectMatching(api, dispatch);
  const onEditorKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // `SCINTILLA_TRICKS( ..., singleLine = true )`: Enter selects what matches; there is no second line.
    if (e.key === "Enter") {
      e.preventDefault();
      press();
    }
  };
  const tabButton = (tab: "selection" | "query", label: string) => (
    <button
      key={tab}
      role="tab"
      aria-selected={f.tab === tab}
      onClick={() => setTab(api, tab)}
      style={{ borderBottomLeftRadius: 0, borderBottomRightRadius: 0, fontWeight: f.tab === tab ? 700 : 400, background: f.tab === tab ? "var(--chrome-active)" : undefined }}
    >
      {label}
    </button>
  );

  return (
    <div
      className="dialog"
      role="dialog"
      aria-label="Find by Properties"
      data-testid="find-by-properties"
      style={{ position: "fixed", top: 90, left: "50%", transform: "translateX(-50%)", width: "min(560px, calc(100vw - 24px))", maxHeight: "78vh", zIndex: 1900 }}
      onKeyDown={(e) => {
        if (e.key === "Escape") close();
      }}
    >
      <div className="dialog-header">
        <span>Find by Properties</span>
        <button aria-label="Close" onClick={close} style={{ border: "none", background: "transparent", cursor: "pointer", color: "inherit" }}>
          &times;
        </button>
      </div>
      <div className="dialog-body" style={{ padding: "8px 12px", display: "flex", flexDirection: "column", gap: 8 }}>
        <div role="tablist" style={{ display: "flex", gap: 2, borderBottom: "1px solid var(--chrome-border)" }}>
          {tabButton("selection", "From Selection")}
          {tabButton("query", "Query")}
        </div>

        {f.tab === "selection" ? (
          <div>
            <div data-testid="fbp-status" style={{ fontSize: 11, marginBottom: 6 }}>
              {f.status}
            </div>
            <div style={{ maxHeight: 280, overflow: "auto", border: "1px solid var(--chrome-border)" }}>
              <table className="setup-table" style={{ margin: 0 }} data-testid="fbp-grid">
                <thead>
                  <tr>
                    <th style={{ textAlign: "left" }}>Property</th>
                    <th style={{ textAlign: "left" }}>Value</th>
                    <th style={{ textAlign: "left", width: 96 }}>Match</th>
                  </tr>
                </thead>
                <tbody>
                  {(state.selection.size === 0 ? [] : f.rows).map((r, i) => (
                    <tr key={r.name} data-testid={`fbp-row-${r.name}`}>
                      <td>{r.name}</td>
                      <td style={r.mixed ? { color: "var(--chrome-text-dim)" } : undefined}>{r.display}</td>
                      <td>
                        {r.mixed ? (
                          <span style={{ color: "var(--chrome-text-dim)" }}>&nbsp;</span>
                        ) : (
                          <select value={r.mode} aria-label={`Match ${r.name}`} onChange={(e) => setMatchMode(i, e.target.value as MatchMode)} style={{ fontSize: 11 }}>
                            {MODES.map((m) => (
                              <option key={m} value={m}>
                                {MATCH_LABELS[m]}
                              </option>
                            ))}
                          </select>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            <label style={{ fontSize: 11 }}>
              Recently Used
              <select
                value=""
                aria-label="Recently Used"
                style={{ width: "100%", fontSize: 11 }}
                onChange={(e) => {
                  if (e.target.value !== "") setFindByProperties({ query: e.target.value });
                }}
              >
                <option value="" />
                {f.recent.map((q) => (
                  <option key={q} value={q}>
                    {q}
                  </option>
                ))}
              </select>
            </label>
            <label style={{ fontSize: 11 }}>
              Expression
              <textarea
                ref={editor}
                value={f.query}
                rows={5}
                spellCheck={false}
                aria-label="Expression"
                data-testid="fbp-query"
                onKeyDown={onEditorKey}
                onChange={(e) => setFindByProperties({ query: e.target.value.replace(/\n/g, "") })}
                style={{ width: "100%", fontFamily: "var(--mono, monospace)", fontSize: 12, boxSizing: "border-box", resize: "vertical" }}
              />
            </label>
            {suggestions.length > 0 && (
              <div data-testid="fbp-suggestions" style={{ display: "flex", flexWrap: "wrap", gap: 4, fontSize: 11 }}>
                {suggestions.map((n) => (
                  <button
                    key={n}
                    style={{ padding: "1px 6px", fontSize: 11 }}
                    onClick={() => {
                      setFindByProperties({ query: f.query.replace(/A\.([A-Za-z0-9_]*)$/, `A.${n}`) });
                      editor.current?.focus();
                    }}
                  >
                    {n}
                  </button>
                ))}
              </div>
            )}
            <div data-testid="fbp-query-status" style={{ fontSize: 11, minHeight: 15 }}>
              {f.queryStatus}
            </div>
            <div style={{ display: "flex", gap: 8 }}>
              <button onClick={() => checkSyntax(api)}>Check Syntax</button>
              <button disabled={!anyActive(f.rows) || state.selection.size === 0} onClick={() => createFromSelectionTab(api)}>
                Create from Selection Tab
              </button>
            </div>
          </div>
        )}

        <div style={{ display: "flex", flexDirection: "column", gap: 2, fontSize: 11 }}>
          <label className="toggle">
            <input type="checkbox" checked={f.deselect} onChange={(e) => setFindByProperties({ deselect: e.target.checked })} />
            Deselect non-matching items
          </label>
          <label className="toggle">
            <input type="checkbox" checked={f.zoom} onChange={(e) => setFindByProperties({ zoom: e.target.checked })} />
            Zoom to fit selected items
          </label>
        </div>

        {f.errorBox !== null && (
          <div role="alertdialog" aria-label="Expression Error" data-testid="fbp-error" style={{ border: "1px solid var(--chrome-error, #e55)", borderRadius: 6, padding: 8, fontSize: 11 }}>
            <div style={{ fontWeight: 600, marginBottom: 4 }}>Expression Error</div>
            <pre style={{ margin: 0, whiteSpace: "pre-wrap" }}>{f.errorBox}</pre>
            <div style={{ textAlign: "right", marginTop: 4 }}>
              <button onClick={() => setFindByProperties({ errorBox: null })}>OK</button>
            </div>
          </div>
        )}
      </div>
      <div className="dialog-footer">
        <button onClick={close}>Close</button>
        <button className="primary" data-testid="fbp-select" onClick={press}>
          Select Matching
        </button>
      </div>
    </div>
  );
}
