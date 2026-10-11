// Import Sheet... (`eeschema.InteractiveDrawing.importSheet`, `SCH_DRAWING_TOOLS::ImportSheet`): the "Choose Schematic" file chooser of KiCad with its four option
// checkboxes (`FILEDLG_IMPORT_SHEET_CONTENTS`: Place repeated copies / Place as group / Place as sheet / Keep annotations -- all but the group, which the schematic does not have).
// The files are the project's own `.kicad_sch` files (GET /api/sch/sheet_files) or a path typed in; what a file would bring, and `LoadSheetFromFile`'s checks on it, come from
// the server (POST /api/sch/sheet_file, nothing written). Open arms the placement tool (components/schematic/schImportTool.ts).
import { useEffect, useRef, useState } from "react";
import { fetchSheetFileCheck, fetchSheetFiles } from "../../api/hierarchyClient";
import type { SheetFileCheck, SheetFileEntry } from "../../api/hierarchyTypes";
import { defaultImportOptions, describeCheck, looksLikeSchematic, type SheetImportPlan } from "../../kicad-port/schImportSheet";
import { useStudioDispatch } from "../../state/store";
import { SchDialogFrame } from "./SchDialogFrame";

/** The checkboxes keep their state between openings, as KiCad keeps them in its settings (`TransferDataFromCustomControls`). */
let remembered: Omit<SheetImportPlan, "path"> = defaultImportOptions();

export function ImportSheetDialog({ forceSheet, initialPath, onClose }: { forceSheet?: boolean; initialPath?: string; onClose: () => void }) {
  const dispatch = useStudioDispatch();
  const [files, setFiles] = useState<SheetFileEntry[] | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [path, setPath] = useState(initialPath ?? "");
  const [options, setOptions] = useState<Omit<SheetImportPlan, "path">>({ ...remembered, ...(forceSheet ? { asSheet: true } : {}) });
  const [check, setCheck] = useState<SheetFileCheck | null>(null);
  const [checking, setChecking] = useState(false);
  /** Answers arrive in any order; only the last request counts. */
  const latest = useRef(0);

  useEffect(() => {
    let alive = true;
    fetchSheetFiles().then(
      (r) => alive && (r.ok ? setFiles(r.files) : setListError(r.message ?? "The project's schematics could not be listed.")),
      (e) => alive && setListError(e instanceof Error ? e.message : String(e))
    );
    return () => {
      alive = false;
    };
  }, []);

  // What the file would bring: asked when the path names a schematic, after the typing has paused.
  useEffect(() => {
    setCheck(null);
    if (!looksLikeSchematic(path)) return;
    const mine = ++latest.current;
    setChecking(true);
    const timer = window.setTimeout(() => {
      fetchSheetFileCheck({ path: path.trim(), keep_annotations: options.keepAnnotations }).then(
        (c) => {
          if (latest.current !== mine) return;
          setCheck(c);
          setChecking(false);
        },
        (e) => {
          if (latest.current !== mine) return;
          setCheck({ ok: false, message: e instanceof Error ? e.message : String(e), path, stem: "", sheets: [], symbols: 0, references: [], libraries: [], findings: [], blocked: true, dest_file: "" });
          setChecking(false);
        }
      );
    }, 250);
    return () => window.clearTimeout(timer);
  }, [path, options.keepAnnotations]);

  const update = (patch: Partial<Omit<SheetImportPlan, "path">>) =>
    setOptions((o) => {
      const next = { ...o, ...patch };
      remembered = next;
      return next;
    });

  const described = check?.ok ? describeCheck(check) : null;
  const canOpen = looksLikeSchematic(path) && !checking && !!check?.ok && !check.blocked;

  /** `Open` of the file dialog: the tool is armed and waits for the click(s). */
  const open = () => {
    if (!canOpen) return;
    dispatch({ type: "SET_SCH_SHEET_IMPORT", plan: { path: path.trim(), ...options } });
    onClose();
  };

  return (
    <SchDialogFrame
      title="Choose Schematic"
      width={560}
      onClose={onClose}
      footer={
        <button className="primary" disabled={!canOpen} onClick={open}>
          Open
        </button>
      }
    >
      <div style={{ marginBottom: 6 }}>Schematic files of the project</div>
      <div style={{ border: "1px solid var(--chrome-border)", maxHeight: 150, overflow: "auto", marginBottom: 8 }} data-testid="import-sheet-files">
        {listError && <div className="problem-row">{listError}</div>}
        {!listError && files === null && <div style={{ padding: 6, opacity: 0.7 }}>Looking for schematics...</div>}
        {files?.length === 0 && <div style={{ padding: 6, opacity: 0.7 }}>The project folder has no .kicad_sch file; type a path below.</div>}
        {files?.map((f) => (
          <div
            key={f.path}
            role="option"
            aria-selected={f.path === path}
            onClick={() => setPath(f.path)}
            style={{ padding: "3px 8px", cursor: "pointer", display: "flex", justifyContent: "space-between", gap: 12, background: f.path === path ? "var(--chrome-selected, rgba(78,161,255,0.25))" : undefined }}
          >
            <span>{f.path}</span>
            <span style={{ opacity: 0.6 }}>{f.bytes >= 1024 ? `${Math.round(f.bytes / 1024)} kB` : `${f.bytes} B`}</span>
          </div>
        ))}
      </div>
      <div className="kv-grid" style={{ gridTemplateColumns: "60px 1fr" }}>
        <span>File</span>
        <input autoFocus placeholder="power.kicad_sch, or /path/to/other/project.kicad_sch" value={path} onChange={(e) => setPath(e.target.value)} onKeyDown={(e) => e.key === "Enter" && open()} />
      </div>
      <div style={{ display: "grid", gap: 4, marginTop: 10 }}>
        <label>
          <input type="checkbox" checked={options.repeated} onChange={(e) => update({ repeated: e.target.checked })} /> Place repeated copies
        </label>
        <label>
          <input type="checkbox" checked={options.asSheet} disabled={forceSheet} onChange={(e) => update({ asSheet: e.target.checked })} /> Place as sheet
        </label>
        <label>
          <input type="checkbox" checked={options.keepAnnotations} onChange={(e) => update({ keepAnnotations: e.target.checked })} /> Keep annotations
        </label>
      </div>
      <div style={{ fontSize: 11, marginTop: 10, minHeight: 32 }}>
        {checking && <div style={{ opacity: 0.7 }}>Reading {path.trim()}...</div>}
        {check && !check.ok && <div style={{ color: "#ff6666" }}>{check.message ?? "The file could not be read."}</div>}
        {described && (
          <>
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
          </>
        )}
        {!path.trim() && <div style={{ opacity: 0.7 }}>Pick a file above or type its path. A file outside the project is copied in; the original is not touched.</div>}
      </div>
    </SchDialogFrame>
  );
}
