// "Exclusion Comment": the text prompt the marker menus' "Exclude with comment..." and "Edit exclusion comment..." ask with
// (`WX_TEXT_ENTRY_DIALOG( this, wxEmptyString, _( "Exclusion Comment" ), comment, true )` in dialog_drc.cpp's OnDRCItemRClick: a multi-line text box).
// One question at a time, kept in state/checkerView.ts so the DRC dialog and the canvas marker menu can both ask it; this component is mounted once, in App.tsx.
import { useEffect, useState } from "react";
import { answerExclusionComment, useCommentRequest } from "../state/checkerView";

export function ExclusionCommentDialog() {
  const request = useCommentRequest();
  const [text, setText] = useState("");
  useEffect(() => {
    if (request) setText(request.initial);
  }, [request]);
  if (!request) return null;
  const cancel = () => answerExclusionComment(null);
  const ok = () => answerExclusionComment(text);
  return (
    <div className="dialog-backdrop" style={{ zIndex: 5000 }} onClick={cancel}>
      <div
        className="dialog"
        role="dialog"
        aria-label="Exclusion Comment"
        style={{ width: 420 }}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") cancel();
          // Enter is a new line in a multi-line box; Ctrl/Cmd+Enter is OK.
          else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) ok();
        }}
      >
        <div className="dialog-header">
          <span>Exclusion Comment</span>
        </div>
        <div className="dialog-body">
          <textarea autoFocus aria-label="Exclusion comment" rows={4} style={{ width: "100%", boxSizing: "border-box", resize: "vertical" }} value={text} onChange={(e) => setText(e.target.value)} />
        </div>
        <div className="dialog-footer">
          <button onClick={cancel}>Cancel</button>
          <button className="primary" onClick={ok}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
