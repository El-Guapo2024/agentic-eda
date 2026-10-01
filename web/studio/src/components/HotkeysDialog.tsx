// common.SuiteControl.listHotKeys ("List Hotkeys...", Ctrl+F1). KiCad's
// own dialog is searchable and grouped by section; this is a flatter
// version (sorted by label) over the same extracted data -- every
// action with a real default hotkey, not just the ones this app has
// implemented, so it doubles as a reference for what's still to come.
import { useMemo, useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { useActionRunner } from "../actions/useActionRunner";
import { displayHotkey, effectiveHotkey } from "../actions/hotkeys";
import actionsData from "../kicad/actions.json";
import type { ActionsFile } from "../kicad/types";

const actionsFile = actionsData as ActionsFile;

export function HotkeysDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const { isEnabled } = useActionRunner();
  const [filter, setFilter] = useState("");

  const rows = useMemo(
    () =>
      actionsFile.actions
        .map((a) => ({ a, ...effectiveHotkey(a) }))
        .filter(({ hotkey, altHotkey }) => hotkey || altHotkey)
        .filter(({ a }) => !filter || a.label.toLowerCase().includes(filter.toLowerCase()))
        .sort((x, y) => x.a.label.localeCompare(y.a.label)),
    [filter]
  );

  if (!state.hotkeysDialogOpen) return null;
  const close = () => dispatch({ type: "SET_HOTKEYS_DIALOG_OPEN", open: false });

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 480, maxHeight: "80vh" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>List Hotkeys</span>
          <span>{rows.length} of {actionsFile.actions.length} actions</span>
        </div>
        <div className="dialog-body" style={{ paddingTop: 8 }}>
          <input
            autoFocus
            placeholder="Filter…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            style={{ width: "100%", padding: "5px 8px", marginBottom: 8 }}
          />
          <div className="kv-grid" style={{ gridTemplateColumns: "1fr auto", rowGap: 4 }}>
            {rows.map(({ a, hotkey, altHotkey }) => (
              <span key={a.name} style={{ display: "contents" }}>
                <span style={{ opacity: isEnabled(a.name) ? 1 : 0.45 }}>{a.label}</span>
                <span style={{ color: "var(--chrome-text-dim)", fontVariantNumeric: "tabular-nums", textAlign: "right" }}>
                  {displayHotkey(hotkey)}
                  {altHotkey ? ` / ${displayHotkey(altHotkey)}` : ""}
                </span>
              </span>
            ))}
            {rows.length === 0 && <span className="panel-empty">No matches.</span>}
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Close</button>
        </div>
      </div>
    </div>
  );
}
