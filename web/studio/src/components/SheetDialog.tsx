// `S` (SCH_DRAWING_TOOLS::DrawSheet): the second click of the sheet rectangle lands in
// state.schSheetPending (the two corners, sized by kicad-port/schSheet.ts's `sizeSheet`), and this is
// `EditSheetProperties` -- the name and the file of the new sheet, "Untitled Sheet" / "untitled.kicad_sch"
// by default. Confirming commits `add_sheet` (a new empty screen for a new file, a shared one for a file
// another sheet already shows); cancelling leaves nothing behind, like `cleanup()` in the C++.
//
// Same "draw first, small dialog last" shape LabelDialog/TextDialog use (PARITY-sch.md).
//
// With a `source` (Import Sheet... / Draw Sheet from File, `DrawSheet( drawSheetFromFile )`) the sheet is named after the file (`fn.GetName()`) and shows a copy of it: the
// dialog asks the server what taking the file in brings (`LoadSheetFromFile`'s checks, POST /api/sch/sheet_file), lists the findings and refuses to add while one is an error.
import { useEffect, useState } from "react";
import { fetchSheetFileCheck } from "../api/hierarchyClient";
import type { SheetFileCheck } from "../api/hierarchyTypes";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { DEFAULT_SHEET_FILE, defaultSheetFile, uniqueSheetName } from "../kicad-port/schSheet";
import { rememberedAnnotateSettings } from "../kicad-port/schAnnotate";
import { describeCheck, importNumbering, importSheetCmd, sheetFileOfFile, sheetNameOfFile } from "../kicad-port/schImportSheet";

export function SheetDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const pending = state.schSheetPending;

  const [name, setName] = useState("");
  const [file, setFile] = useState(DEFAULT_SHEET_FILE);
  /** Whether the user typed their own file name: until then the file follows the name. */
  const [fileEdited, setFileEdited] = useState(false);
  /** What taking the file in brings (an import only): null while the server is reading it. */
  const [check, setCheck] = useState<SheetFileCheck | null>(null);
  const source = pending?.source;

  useEffect(() => {
    if (!pending) return;
    const taken = (state.schematic?.sheets ?? []).map((s) => s.name);
    const fresh = pending.source ? uniqueSheetName(taken, sheetNameOfFile(pending.source.path)) : uniqueSheetName(taken);
    setName(fresh);
    setFile(pending.source ? sheetFileOfFile(pending.source.path) : fresh === "Untitled Sheet" ? DEFAULT_SHEET_FILE : defaultSheetFile(fresh));
    setFileEdited(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending?.at[0], pending?.at[1], pending?.size[0], pending?.size[1]]);

  useEffect(() => {
    setCheck(null);
    if (!source) return;
    let alive = true;
    fetchSheetFileCheck({ path: source.path, keep_annotations: source.keepAnnotations }).then(
      (c) => alive && setCheck(c),
      (e) => alive && setCheck({ ok: false, message: e instanceof Error ? e.message : String(e), path: source.path, stem: "", sheets: [], symbols: 0, references: [], libraries: [], findings: [], blocked: true, dest_file: "" })
    );
    return () => {
      alive = false;
    };
  }, [source?.path, source?.keepAnnotations]);

  if (!pending) return null;
  const close = () => dispatch({ type: "SET_SCH_SHEET_PENDING", pending: null });
  const taken = (state.schematic?.sheets ?? []).some((s) => s.name === name.trim());
  const described = check?.ok ? describeCheck(check) : null;
  const blocked = source ? !check || !check.ok || check.blocked : false;
  const valid = name.trim() !== "" && file.trim() !== "" && !/[\\/]/.test(file) && !taken && !blocked;

  const submit = async () => {
    if (!valid) return;
    close();
    if (source) {
      const ok = await api.cmd(importSheetCmd(source, { name, file, at: pending.at, size: pending.size }, importNumbering(rememberedAnnotateSettings())));
      // Without "Place repeated copies" the tool ends with the sheet (`PopTool`).
      if (ok && !source.repeated) dispatch({ type: "SET_SCH_SHEET_IMPORT", plan: null });
      return;
    }
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
            {taken
              ? `A sheet named '${name.trim()}' already exists on this sheet.`
              : /[\\/]/.test(file)
                ? "A sheet file is a bare file name (no folders)."
                : source
                  ? `A copy of ${source.path} goes in the project${check?.dest_file ? ` as ${check.dest_file}` : ""}${source.keepAnnotations ? "; the symbols keep their references" : "; its symbols are numbered anew"}.`
                  : "A file no sheet uses yet gets a new, empty page; one that exists is shared."}
          </div>
          {source && !check && <div style={{ fontSize: 11, marginTop: 6, opacity: 0.7 }}>Reading {source.path}...</div>}
          {source && check && !check.ok && <div style={{ fontSize: 11, marginTop: 6, color: "#ff6666" }}>{check.message ?? "The file could not be read."}</div>}
          {described && (
            <div style={{ fontSize: 11, marginTop: 6 }}>
              <div style={{ opacity: 0.8 }}>{described.summary}</div>
              {described.errors.map((t) => (
                <div key={t} style={{ color: "#ff6666", marginTop: 3 }}>
                  {t}
                </div>
              ))}
              {described.warnings.map((t) => (
                <div key={t} style={{ color: "#d4a017", marginTop: 3 }}>
                  {t}
                </div>
              ))}
            </div>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={!valid} onClick={() => void submit()}>
            {source ? "Import" : "Add"}
          </button>
        </div>
      </div>
    </div>
  );
}
