// Increment Annotations From... (`eeschema.EditorControl.incrementAnnotations`, `SCH_EDITOR_CONTROL::IncrementAnnotations`, DIALOG_INCREMENT_ANNOTATIONS):
// the first reference to move and the step; every symbol with those letters and a number from that one up is renumbered, together, in one undo step.
import { useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../../state/store";
import { SchDialogFrame } from "./SchDialogFrame";

export function IncrementAnnotationsDialog({ onClose }: { onClose: () => void }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const [start, setStart] = useState("");
  const [by, setBy] = useState(1);
  const hasSubSheets = (state.schematic?.sheets.length ?? 0) > 0;
  const [scope, setScope] = useState<"current" | "all">("current");

  const submit = async () => {
    // `dlg.m_FirstRefDes->SetValidator( wxTextValidator( wxFILTER_EMPTY ) )`: the start must be given.
    if (start.trim() === "") return;
    const ok = await api.cmd({ op: "increment_annotations", start: start.trim(), increment: by });
    if (!ok) return; // the verb's refusal (a clash, a number below 0) is already on screen as a toast
    dispatch({ type: "TOAST", message: "Incremented annotations.", kind: "info" });
    onClose();
  };

  return (
    <SchDialogFrame
      title="Increment Annotations"
      width={380}
      onClose={onClose}
      footer={
        <button className="primary" disabled={start.trim() === ""} onClick={() => void submit()}>
          OK
        </button>
      }
    >
      <div className="kv-grid" style={{ gridTemplateColumns: "170px 1fr" }}>
        <span title="The first reference designator to move: it and every later one with the same letters">Start reference designator:</span>
        <input autoFocus value={start} onChange={(e) => setStart(e.target.value)} onKeyDown={(e) => e.key === "Enter" && void submit()} placeholder="e.g. R5" />
        <span>Increment by:</span>
        <input type="number" min={1} max={64} value={by} onChange={(e) => setBy(Math.min(64, Math.max(1, Math.round(Number(e.target.value) || 1))))} />
      </div>
      <label style={{ display: "block", marginTop: 12 }}>
        <input type="radio" checked={scope === "current"} onChange={() => setScope("current")} /> Current sheet only
      </label>
      <label style={{ display: "block", opacity: hasSubSheets ? 0.5 : 1 }} title={hasSubSheets ? "Sub-sheets cannot be edited here yet" : undefined}>
        <input type="radio" checked={scope === "all"} disabled={hasSubSheets} onChange={() => setScope("all")} /> All sheets
      </label>
    </SchDialogFrame>
  );
}
