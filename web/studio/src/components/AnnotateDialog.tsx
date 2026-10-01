// `Ctrl+A` (dialog_annotate.cpp): scope ("Schematic"/"Sheet"/"Selection"
// in source -- this app's IR has no sheet hierarchy, so "whole sheet" is
// the only non-selection scope, not source's own three), order options
// (sort by Y then X, KiCad's own default, or X then Y), and "Clear and
// re-annotate" vs "Keep existing annotations". Numbering-scheme options
// (First Free / Sheet x100 / Sheet x1000) are a multi-sheet concept this
// app has nothing to offer for, so they're left out entirely rather than
// shown as dead controls.
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";

export function AnnotateDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.annotateDialogOpen;

  const selectedSymbolIds = [...state.selection].filter((id) => api.symbolById(id));
  const [scope, setScope] = useState<"sheet" | "selection">("sheet");
  const [order, setOrder] = useState<"y_then_x" | "x_then_y">("y_then_x");
  const [resetExisting, setResetExisting] = useState(false);

  useEffect(() => {
    if (!open) return;
    setScope(selectedSymbolIds.length > 0 ? "selection" : "sheet");
    setOrder("y_then_x");
    setResetExisting(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_ANNOTATE_DIALOG_OPEN", open: false });

  const submit = async () => {
    const ids = scope === "selection" ? selectedSymbolIds : undefined;
    const ok = await api.cmd({ op: "annotate", reset_existing: resetExisting, order, ids });
    dispatch({ type: "TOAST", message: ok ? "Annotated." : "Nothing to annotate.", kind: "info" });
    close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 360 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Annotate Schematic</span>
        </div>
        <div className="dialog-body">
          <fieldset style={{ marginBottom: 10 }}>
            <legend>Scope</legend>
            <label style={{ display: "block" }}>
              <input type="radio" checked={scope === "sheet"} onChange={() => setScope("sheet")} /> Whole sheet
            </label>
            <label style={{ display: "block", opacity: selectedSymbolIds.length > 0 ? 1 : 0.5 }}>
              <input type="radio" checked={scope === "selection"} disabled={selectedSymbolIds.length === 0} onChange={() => setScope("selection")} /> Selection only (
              {selectedSymbolIds.length} symbol{selectedSymbolIds.length === 1 ? "" : "s"})
            </label>
          </fieldset>
          <fieldset style={{ marginBottom: 10 }}>
            <legend>Order</legend>
            <label style={{ display: "block" }}>
              <input type="radio" checked={order === "y_then_x"} onChange={() => setOrder("y_then_x")} /> Sort by Y position (top to bottom)
            </label>
            <label style={{ display: "block" }}>
              <input type="radio" checked={order === "x_then_y"} onChange={() => setOrder("x_then_y")} /> Sort by X position (left to right)
            </label>
          </fieldset>
          <label style={{ display: "block" }}>
            <input type="checkbox" checked={resetExisting} onChange={(e) => setResetExisting(e.target.checked)} /> Clear and re-annotate (reset existing references first)
          </label>
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>
            Assigns the next free number, per reference-letter prefix, to every unannotated symbol in scope. With "Clear and re-annotate", every symbol in scope is first reset to its own "&lt;prefix&gt;?" placeholder, so the whole scope renumbers from scratch instead of only filling gaps.
          </p>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={submit}>
            Annotate
          </button>
        </div>
      </div>
    </div>
  );
}
