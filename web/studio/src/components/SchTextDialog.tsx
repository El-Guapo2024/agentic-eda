// `T` (sch_drawing_tools.cpp TwoClickPlace/createNewText): click/dialog
// shape identical to LabelDialog's own (see its header comment) -- a
// click captures the position into state.schTextPending, this dialog
// confirms the content/size/orientation, then add_sch_text commits it.
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";

const DEFAULT_SIZE_UM = 1270; // 1.27mm -- eeschema's own SCHEMATIC_SETTINGS::m_DefaultTextSize default (50 mil)

export function SchTextDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const pending = state.schTextPending;

  const [content, setContent] = useState("");
  const [size, setSize] = useState(DEFAULT_SIZE_UM);
  const [angle, setAngle] = useState(0);

  useEffect(() => {
    if (!pending) return;
    setContent("");
    setSize(DEFAULT_SIZE_UM);
    setAngle(0);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending?.at[0], pending?.at[1]]);

  if (!pending) return null;
  const close = () => dispatch({ type: "SET_SCH_TEXT_PENDING", pending: null });

  const submit = () => {
    if (!content.trim()) return;
    api.cmd({ op: "add_sch_text", content, at: { x: pending.at[0], y: pending.at[1] }, angle_millideg: angle, size_um: size });
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 340 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Text</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "80px 1fr" }}>
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
            <input type="number" step={90} value={angle / 1000} onChange={(e) => setAngle(Number(e.target.value) * 1000)} />
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
