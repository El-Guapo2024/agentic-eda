// `S` (SCH_DRAWING_TOOLS::DrawSheet): the second click of the sheet rectangle lands in
// state.schSheetPending (the two corners, sized by kicad-port/schSheet.ts's `sizeSheet`), and this is
// `EditSheetProperties` -- the name and the file of the new sheet, "Untitled Sheet" / "untitled.kicad_sch"
// by default. Confirming commits `add_sheet` (a new empty screen for a new file, a shared one for a file
// another sheet already shows); cancelling leaves nothing behind, like `cleanup()` in the C++.
//
// Same "draw first, small dialog last" shape LabelDialog/TextDialog use (PARITY-sch.md).
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { DEFAULT_SHEET_FILE, defaultSheetFile, uniqueSheetName } from "../kicad-port/schSheet";

export function SheetDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const pending = state.schSheetPending;

  const [name, setName] = useState("");
  const [file, setFile] = useState(DEFAULT_SHEET_FILE);
  /** Whether the user typed their own file name: until then the file follows the name. */
  const [fileEdited, setFileEdited] = useState(false);

  useEffect(() => {
    if (!pending) return;
    const taken = (state.schematic?.sheets ?? []).map((s) => s.name);
    const fresh = uniqueSheetName(taken);
    setName(fresh);
    setFile(fresh === "Untitled Sheet" ? DEFAULT_SHEET_FILE : defaultSheetFile(fresh));
    setFileEdited(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending?.at[0], pending?.at[1], pending?.size[0], pending?.size[1]]);

  if (!pending) return null;
  const close = () => dispatch({ type: "SET_SCH_SHEET_PENDING", pending: null });
  const taken = (state.schematic?.sheets ?? []).some((s) => s.name === name.trim());
  const valid = name.trim() !== "" && file.trim() !== "" && !/[\\/]/.test(file) && !taken;

  const submit = async () => {
    if (!valid) return;
    close();
    await api.cmd({ op: "add_sheet", name: name.trim(), file: file.trim(), at: { x: pending.at[0], y: pending.at[1] }, size: pending.size });
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 380 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Hierarchical Sheet Properties</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "90px 1fr" }}>
            <span>Sheet name</span>
            <input
              autoFocus
              value={name}
              onChange={(e) => {
                setName(e.target.value);
                if (!fileEdited) setFile(defaultSheetFile(e.target.value));
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") void submit();
              }}
            />
            <span>File name</span>
            <input
              value={file}
              onChange={(e) => {
                setFile(e.target.value);
                setFileEdited(true);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") void submit();
              }}
            />
          </div>
          <div style={{ fontSize: 11, marginTop: 8, minHeight: 16, color: taken || /[\\/]/.test(file) ? "#ff6666" : undefined, opacity: taken || /[\\/]/.test(file) ? 1 : 0.7 }}>
            {taken ? `A sheet named '${name.trim()}' already exists on this sheet.` : /[\\/]/.test(file) ? "A sheet file is a bare file name (no folders)." : "A file no sheet uses yet gets a new, empty page; one that exists is shared."}
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!valid} onClick={() => void submit()}>
            Add
          </button>
        </div>
      </div>
    </div>
  );
}
