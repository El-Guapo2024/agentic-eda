// `L`/Ctrl+`L`/`H` (sch_drawing_tools.cpp TwoClickPlace/createNewLabel):
// SchematicView.tsx's onPointerDown captures the click position into
// state.schLabelPending (scope already decided by which hotkey armed the
// tool), and this dialog is what turns that into a real add_label --
// same "click/draw first, small dialog last" shape ZoneDialog/TextDialog
// already use, not source's own "dialog pops up immediately, item then
// follows the cursor" order (see PARITY-sch.md for that documented
// adaptation).
//
// Auto-increment (dialog_label_properties doesn't have this itself --
// it's `createNewLabel`'s *caller*, via `SCH_LABEL_BASE::IncrementLabel`,
// that seeds a chained placement's next suggestion): state.lastLabelText
// is incremented once per successful placement, so stamping down a bus of
// labels (DATA0, DATA1, DATA2...) only means confirming the suggestion,
// not re-typing it.
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { incrementLabelText } from "../kicad-port/incrementLabelText";
import type { LabelShape } from "../api/types";

const SHAPES: LabelShape[] = ["input", "output", "bidirectional", "tri_state", "passive"];

const TITLE: Record<"local" | "global" | "hierarchical", string> = {
  local: "Label Properties",
  global: "Global Label Properties",
  hierarchical: "Hierarchical Label Properties",
};

export function LabelDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const pending = state.schLabelPending;

  const [text, setText] = useState("");
  const [shape, setShape] = useState<LabelShape>("input");

  // Re-seed on every fresh click (not on every keystroke) -- same
  // dependency shape TextDialog.tsx's own re-seed effect uses.
  useEffect(() => {
    if (!pending) return;
    setText(incrementLabelText(state.lastLabelText));
    setShape("input");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending?.at[0], pending?.at[1], pending?.scope]);

  if (!pending) return null;
  const close = () => dispatch({ type: "SET_SCH_LABEL_PENDING", pending: null });

  const submit = () => {
    if (!text.trim()) return;
    const kind = pending.scope === "local" ? ({ scope: "local" } as const) : ({ scope: pending.scope, shape } as const);
    api.cmd({ op: "add_label", net: text, at: { x: pending.at[0], y: pending.at[1] }, kind });
    dispatch({ type: "SET_LAST_LABEL_TEXT", text });
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 340 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{TITLE[pending.scope]}</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "80px 1fr" }}>
            <span>Net name</span>
            <input
              autoFocus
              value={text}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            {pending.scope !== "local" && (
              <>
                <span>Shape</span>
                <select value={shape} onChange={(e) => setShape(e.target.value as LabelShape)}>
                  {SHAPES.map((s) => (
                    <option key={s} value={s}>
                      {s}
                    </option>
                  ))}
                </select>
              </>
            )}
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!text.trim()} onClick={submit}>
            Add
          </button>
        </div>
      </div>
    </div>
  );
}
