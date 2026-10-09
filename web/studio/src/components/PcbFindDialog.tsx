// The board editor's Find dialog (pcbnew/dialogs/dialog_find.cpp over dialog_find_base.cpp): the search text with its history, Match case, Whole words only,
// Wildcards, Wrap, the six "Search ..." checkboxes, Find Next, Find Previous, Restart Search and Close, and a status line ("Hit(s): 2 / 5", "No hits").
// Like KiCad's it is modeless -- a floating pane, so the board stays in reach and the hit it selects is in view -- and closing it only hides it: F3 and
// Shift+F3 go on through the same list (state/pcbFind.ts). What a press does is actions/pcbFindActions.ts; the matching is kicad-port/pcbFind.ts.
import { useEffect, useRef } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { setFindOptions, setPcbFind, usePcbFind } from "../state/pcbFind";
import { pcbFindStep } from "../actions/pcbFindActions";
import type { PcbFindOptions } from "../kicad-port/pcbFind";

export function PcbFindDialog() {
  const find = usePcbFind();
  const state = useStudioState();
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const input = useRef<HTMLInputElement>(null);

  // `DIALOG_FIND::Show`: the search text takes the focus, selected, each time the dialog is shown.
  useEffect(() => {
    if (!find.open) return;
    input.current?.focus();
    input.current?.select();
  }, [find.open, find.shown]);

  if (!find.open || state.tab !== "pcb") return null;
  const o = find.options;
  const close = () => setPcbFind({ open: false });
  const press = (forward: boolean, restart = false) => pcbFindStep(api, dispatch, forward, restart);
  const box = (key: keyof PcbFindOptions, label: string) => (
    <label className="toggle" style={{ whiteSpace: "nowrap" }}>
      <input type="checkbox" checked={o[key] as boolean} onChange={(e) => setFindOptions({ [key]: e.target.checked })} />
      {label}
    </label>
  );

  return (
    <div
      className="dialog"
      role="dialog"
      aria-label="Find"
      style={{ position: "fixed", top: 110, left: "50%", transform: "translateX(-50%)", width: "min(660px, calc(100vw - 24px))", zIndex: 1900 }}
      onKeyDown={(e) => {
        if (e.key === "Escape") close();
      }}
    >
      <div className="dialog-header">
        <span>Find</span>
        <button aria-label="Close" onClick={close} style={{ border: "none", background: "transparent", cursor: "pointer", color: "inherit" }}>
          &times;
        </button>
      </div>
      <div className="dialog-body" style={{ padding: "10px 12px" }}>
        <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) 108px", gap: 12 }}>
          <div>
            <div style={{ display: "grid", gridTemplateColumns: "76px 1fr", alignItems: "center", gap: 6, marginBottom: 8 }}>
              <span>Search for:</span>
              <input
                ref={input}
                list="pcb-find-history"
                title="Text with optional wildcards"
                value={o.text}
                onChange={(e) => setFindOptions({ text: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === "Enter") press(true);
                }}
              />
              <datalist id="pcb-find-history">
                {find.history.map((h) => (
                  <option key={h} value={h} />
                ))}
              </datalist>
            </div>
            <div style={{ display: "flex", flexWrap: "wrap", gap: "4px 14px", fontSize: 11, marginBottom: 8 }}>
              {box("matchCase", "Match case")}
              {box("wholeWord", "Whole words only")}
              {box("wildcards", "Wildcards")}
              {box("wrap", "Wrap")}
            </div>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(246px, 1fr))", gap: "4px 14px", fontSize: 11 }}>
              {box("references", "Search footprint reference designators")}
              {box("markers", "Search DRC markers")}
              {box("values", "Search footprint values")}
              {box("nets", "Search net names")}
              {box("hidden", "Include hidden fields")}
              {box("texts", "Search other text items")}
            </div>
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            <button className="primary" disabled={o.text === ""} onClick={() => press(true)}>
              Find Next
            </button>
            <button disabled={o.text === ""} onClick={() => press(false)}>
              Find Previous
            </button>
            <button disabled={o.text === ""} onClick={() => press(true, true)}>
              Restart Search
            </button>
            <button onClick={close}>Close</button>
          </div>
        </div>
        <div style={{ marginTop: 10, minHeight: 16, fontSize: 11, color: "var(--chrome-text-dim)" }}>{find.status}</div>
      </div>
    </div>
  );
}
