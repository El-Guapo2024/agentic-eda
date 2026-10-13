// `T` (sch_drawing_tools.cpp TwoClickPlace/createNewText): click/dialog
// shape identical to LabelDialog's own (see its header comment) -- a
// click captures the position into state.schTextPending, this dialog
// confirms the content/size/orientation and how the text is justified about its
// anchor (`DIALOG_TEXT_PROPERTIES`), then `place_text` commits it as one step.
// The justification the tool starts with is KiCad's: centre and centre
// (`m_lastTextHJustify`, `m_lastTextVJustify`), and what was last chosen is the next
// dialog's start (kicad-port/schLastPlaced.ts).
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { lastPlaced, rememberPlaced } from "../kicad-port/schLastPlaced";
import type { SchHAlign, SchVAlign } from "../api/schEditTypes";

const DEFAULT_SIZE_UM = 1270; // 1.27mm -- eeschema's own SCHEMATIC_SETTINGS::m_DefaultTextSize default (50 mil)

export function SchTextDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const pending = state.schTextPending;

  const [content, setContent] = useState("");
  const [size, setSize] = useState(DEFAULT_SIZE_UM);
  const [vertical, setVertical] = useState(false);
  const [h, setH] = useState<SchHAlign>("center");
  const [v, setV] = useState<SchVAlign>("center");

  useEffect(() => {
    if (!pending) return;
    const last = lastPlaced();
    setContent("");
    setSize(DEFAULT_SIZE_UM);
    setVertical(last.textVertical);
    setH(last.textH);
    setV(last.textV);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending?.at[0], pending?.at[1]]);

  if (!pending) return null;
  const close = () => dispatch({ type: "SET_SCH_TEXT_PENDING", pending: null });

  const submit = () => {
    if (!content.trim()) return;
    void api.cmd({ op: "sch_edit", verb: "place_text", content, at: { x: pending.at[0], y: pending.at[1] }, angle: vertical ? 90_000 : 0, size_um: size, h, v });
    // `m_lastTextHJustify`, `m_lastTextVJustify`, `m_lastTextAngle`
    rememberPlaced({ textH: h, textV: v, textVertical: vertical });
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 340 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Text</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "100px 1fr" }}>
            <span>Text</span>
            <input
              autoFocus
              value={content}
              onChange={(e) => setContent(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            <span>Size</span>
            <input type="number" min={100} step={50} value={size} onChange={(e) => setSize(Number(e.target.value))} />
            <span>Orientation</span>
            <select aria-label="Orientation" value={vertical ? "v" : "h"} onChange={(e) => setVertical(e.target.value === "v")}>
              <option value="h">Horizontal</option>
              <option value="v">Vertical</option>
            </select>
            <span>Horizontal justification</span>
            <select aria-label="Horizontal justification" value={h} onChange={(e) => setH(e.target.value as SchHAlign)}>
              <option value="left">Left</option>
              <option value="center">Center</option>
              <option value="right">Right</option>
            </select>
            <span>Vertical justification</span>
            <select aria-label="Vertical justification" value={v} onChange={(e) => setV(e.target.value as SchVAlign)}>
              <option value="top">Top</option>
              <option value="center">Center</option>
              <option value="bottom">Bottom</option>
            </select>
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!content.trim()} onClick={submit}>
            Add
          </button>
        </div>
      </div>
    </div>
  );
}
