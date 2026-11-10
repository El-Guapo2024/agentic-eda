// `S` (SCH_DRAWING_TOOLS::DrawSheet): the second click of the sheet rectangle lands in
// state.schSheetPending (the two corners, sized by kicad-port/schSheet.ts's `sizeSheet`), and this is
// `EditSheetProperties` -- the name and the file of the new sheet, "Untitled Sheet" / "untitled.kicad_sch"
// as every new sheet starts (`DrawSheet`: `SetText( wxT( "Untitled Sheet" ) )`, `wxT( "untitled" ) + ext`; the file does not follow the name, and a name another
// sheet has is allowed: ERC reports it, UI-LEFT S21). Confirming commits `add_sheet` (a new empty screen for a new file); a file a sheet of the hierarchy
// already shows, or a screen the project keeps that no sheet places (a deleted sheet's), is linked only after `ChangeSheetFile`'s question, "'%s' already exists.\n\nLink '%s' to this file?" (Cancel keeps the dialog open);
// cancelling leaves nothing behind, like `cleanup()` in the C++.
//
// Same "draw first, small dialog last" shape LabelDialog/TextDialog use (PARITY-sch.md).
//
// With a `source` (Import Sheet... / Draw Sheet from File, `DrawSheet( drawSheetFromFile )`) the sheet is named after the file (`fn.GetName()`) and shows a copy of it: the
// dialog asks the server what taking the file in brings (`LoadSheetFromFile`'s checks, POST /api/sch/sheet_file), lists the findings and refuses to add while one is an error.
import { useEffect, useState } from "react";
import { fetchSheetFileCheck } from "../api/hierarchyClient";
import type { SheetFileCheck } from "../api/hierarchyTypes";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { DEFAULT_SHEET_FILE, DEFAULT_SHEET_NAME, linkQuestion } from "../kicad-port/schSheet";
import { fetchHierarchyFiles } from "../api/schControlClient";
import { askConfirm } from "./library/libraryDialogs";
import { rememberedAnnotateSettings } from "../kicad-port/schAnnotate";
import { describeCheck, importNumbering, importSheetCmd, sheetFileOfFile, sheetNameOfFile } from "../kicad-port/schImportSheet";
import { sendAndSelectNew } from "./schematic/schImportTool";

export function SheetDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const pending = state.schSheetPending;

  const [name, setName] = useState(DEFAULT_SHEET_NAME);
  const [file, setFile] = useState(DEFAULT_SHEET_FILE);
  /** What taking the file in brings (an import only): null while the server is reading it. */
  const [check, setCheck] = useState<SheetFileCheck | null>(null);
  const source = pending?.source;

  useEffect(() => {
    if (!pending) return;
    // `DrawSheet`: "Untitled Sheet" / `untitled.kicad_sch`, or for a sheet from a file its name (`fn.GetName()`) and a copy of it
    setName(pending.source ? sheetNameOfFile(pending.source.path) : DEFAULT_SHEET_NAME);
    setFile(pending.source ? sheetFileOfFile(pending.source.path) : DEFAULT_SHEET_FILE);
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
  const described = check?.ok ? describeCheck(check) : null;
  const blocked = source ? !check || !check.ok || check.blocked : false;
  const valid = name.trim() !== "" && file.trim() !== "" && !/[\\/]/.test(file) && !blocked;

  const submit = async () => {
    if (!valid) return;
    if (!source) {
      // `ChangeSheetFile` for a new sheet: a file the hierarchy already shows (`SearchHierarchy`), or a screen the project keeps that no sheet places
      // (`wxFileExists`), is linked only if the user says so
      const known = await fetchHierarchyFiles().catch(() => ({ sheets: [], screens: [] as string[] }));
      const question = linkQuestion(file, known.sheets, known.screens);
      if (question && !(await askConfirm({ title: "Confirmation", message: question, okLabel: "OK", cancelLabel: "Cancel" }))) return;
    }
    close();
    if (source) {
      const ok = await sendAndSelectNew(api, dispatch, importSheetCmd(source, { name, file, at: pending.at, size: pending.size }, importNumbering(rememberedAnnotateSettings())));
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
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void submit();
              }}
            />
            <span>File name</span>
            <input
              value={file}
              onChange={(e) => setFile(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void submit();
              }}
            />
          </div>
          <div style={{ fontSize: 11, marginTop: 8, minHeight: 16, color: /[\\/]/.test(file) ? "#ff6666" : undefined, opacity: /[\\/]/.test(file) ? 1 : 0.7 }}>
            {/[\\/]/.test(file)
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
