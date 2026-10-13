// Migrate 3D Models (`pcbnew.GlobalEdit.migrate3DModels`, Tools menu): port of DIALOG_MIGRATE_3D_MODELS (pcbnew/dialogs/dialog_migrate_3d_models.cpp at 8303b2ad,
// GLOBAL_EDIT_TOOL::Migrate3DModels -- the dialog always comes up, with empty lists when nothing is missing).
//
// Three panes. On the left the `.wrl` and `.wrz` models the board's footprints name that no search finds (`collectMissingModels`; bold while the row has no replacement chosen). In the
// middle the STEP files that could stand for the one selected (`rankCandidatesFor`: the best first, "(keep existing reference)" last), with "Add Search Directory..." (a folder is
// scanned too and every row ranked again against the bigger catalog, a row keeping its choice when that file is still there) and "Open External File..." (a file picked by hand
// becomes the row's first candidate, "user-selected"). On the right the footprint with the candidate in the place the missing model had (`showPreview`). "Replace Models" sends
// one `migrate_board_models` -- the old name for the new in every footprint that has it, one undo step -- and with nothing chosen it is Keep; "Keep Existing" and Escape close the dialog.
// The server finds and ranks (`GET /api/3dmodel/migrate`, crates/model/src/model_subst.rs); kicad-port/migrate3d.ts is the state of the choices.
import { useCallback, useEffect, useRef, useState } from "react";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import { fetchMigration, fetchPreviewPart } from "../api/model3dClient";
import type { Part } from "../api/types";
import { defaultChoices, migrateCmds, previewRow, rechoose, saveSearchDirs, savedSearchDirs, userSelectedDisplay, withPickedFile, withSearchDir, type MissingModel, type Migration } from "../kicad-port/migrate3d";
import { useStudioApi } from "../state/store";
import { Model3dChooserDialog } from "./footprint/Model3dChooserDialog";
import { ModelPreview3D } from "./footprint/ModelPreview3D";
import { useEscape } from "./pcbDialogKit";

const KEEP_EXISTING = "(keep existing reference)";
const LIST_STYLE = { border: "1px solid var(--chrome-border, #444)", height: 300, overflowY: "auto" } as const;

export function Migrate3dModelsDialog() {
  const api = useStudioApi();
  const [dirs, setDirs] = useState<string[]>(() => savedSearchDirs());
  const [loaded, setLoaded] = useState<Migration | null>(null);
  const [missing, setMissing] = useState<MissingModel[]>([]);
  const [choices, setChoices] = useState<number[]>([]);
  /** The row of the left list that is selected. */
  const [row, setRow] = useState(0);
  /** The row of the middle list that is highlighted (and previewed): the choice, else the top one. */
  const [highlight, setHighlight] = useState(0);
  const [pick, setPick] = useState<"folder" | "file" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [parts, setParts] = useState<Record<string, Part | null>>({});
  const asked = useRef(new Set<string>());
  // Escape is "Keep Existing" -- or, with the folder or file chooser up, closes that first
  const closeChooser = useCallback(() => setPick(null), []);
  useEscape(pick ? closeChooser : closeSweepDialog);

  // `collectMissingModels`, `buildCatalog` and `rankAllCandidates` on opening: the saved search folders go with the request; one that is gone (or no longer readable) is dropped, not an error to stay in.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        let m: Migration;
        try {
          m = await fetchMigration(dirs);
        } catch (e) {
          if (dirs.length === 0) throw e;
          m = await fetchMigration([]);
          saveSearchDirs([]);
          if (!cancelled) {
            setDirs([]);
            setNote(`The folders searched before were dropped: ${e instanceof Error ? e.message : String(e)}`);
          }
        }
        if (cancelled) return;
        setLoaded(m);
        setMissing(m.missing);
        setChoices(defaultChoices(m.missing));
        setHighlight(0);
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
    // the folders are read once, on opening: a folder added later goes through addDirectory
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const current: MissingModel | undefined = missing[row];
  const choice = choices[row] ?? -1;

  // the footprint shown beside the candidate: the first footprint that has the missing model, at the origin, face up
  const rep = current?.rep ?? null;
  useEffect(() => {
    if (!rep || asked.current.has(rep)) return;
    asked.current.add(rep);
    void fetchPreviewPart({ part: rep }).then((p) => setParts((cur) => ({ ...cur, [rep]: p })));
  }, [rep]);

  /** `populateCandidatesList`: what is highlighted when a row comes up -- the choice, else the top row (also when that is only "keep existing"). */
  const showRow = useCallback((index: number, picks: readonly number[]) => {
    setRow(index);
    const c = picks[index] ?? -1;
    setHighlight(c >= 0 ? c : 0);
  }, []);

  const candidates = current?.candidates ?? [];
  /** The middle list: the candidates, then "(keep existing reference)" (the row with no path, score 0). */
  const shown = [...candidates.map((c) => ({ path: c.path, display: c.display })), { path: "", display: KEEP_EXISTING }];
  const highlighted = candidates[highlight];

  const chooseCandidate = (i: number) => {
    setHighlight(i);
    // `OnCandidateSelected`: the "keep existing" row stores -1
    setChoices((cur) => cur.map((c, r) => (r === row ? (i < candidates.length ? i : -1) : c)));
  };

  const addDirectory = async (dir: string) => {
    setPick(null);
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const next = withSearchDir(dirs, dir);
      const m = await fetchMigration(next);
      const picks = rechoose(missing, choices, m.missing);
      setDirs(next);
      saveSearchDirs(next);
      setLoaded(m);
      setMissing(m.missing);
      setChoices(picks);
      const keep = Math.min(row, Math.max(0, m.missing.length - 1));
      showRow(keep, picks);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const openFile = (path: string) => {
    setPick(null);
    if (!current) return;
    const picked = withPickedFile(current, path, userSelectedDisplay(path));
    setMissing((cur) => cur.map((m, r) => (r === row ? picked.model : m)));
    setChoices((cur) => cur.map((c, r) => (r === row ? picked.choice : c)));
    setHighlight(picked.choice);
  };

  const replace = async () => {
    const cmds = migrateCmds(missing, choices);
    // "User clicked Replace without choosing anything; treat as Keep."
    if (cmds.length === 0) {
      closeSweepDialog();
      return;
    }
    setBusy(true);
    try {
      const ok = await api.cmdBatch(cmds);
      // the server says why in a toast (a name it will not write)
      if (ok) closeSweepDialog();
      else setError(api.getState().toast?.message ?? "The models could not be replaced.");
    } finally {
      setBusy(false);
    }
  };

  const chosen = choices.filter((c) => c >= 0).length;
  const row3d = current ? previewRow(current, highlighted) : null;

  return (
    <div className="dialog-backdrop" onClick={closeSweepDialog}>
      <div className="dialog" style={{ width: 1040, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Migrate 3D Models" data-testid="migrate3d-dialog">
        <div className="dialog-header">
          <span>Migrate 3D Models</span>
        </div>
        <div className="dialog-body">
          <div style={{ whiteSpace: "pre-line", marginBottom: 8 }} data-testid="migrate3d-header">
            {"This board references 3D models that are no longer present on your system.\nSelect replacements below, or keep the existing references unchanged."}
          </div>
          <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1.2fr) minmax(0, 1fr)", gap: 10 }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 0 }}>
              <span style={{ fontSize: 12 }}>Missing 3D model references:</span>
              <div style={LIST_STYLE} data-testid="migrate3d-missing">
                {missing.map((m, i) => (
                  <div
                    key={m.name}
                    onClick={() => showRow(i, choices)}
                    title={`${m.name}\nUsed by ${m.footprints.join(", ")}`}
                    style={{
                      padding: "1px 6px",
                      cursor: "pointer",
                      fontSize: 12,
                      // `updateMissingItemStyle`: bold while no replacement is chosen
                      fontWeight: (choices[i] ?? -1) < 0 ? 700 : 400,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                      background: i === row ? "var(--chrome-hover, rgba(128,128,128,.25))" : undefined,
                    }}
                    data-testid={`migrate3d-missing-${i}`}
                    data-name={m.name}
                    data-chosen={(choices[i] ?? -1) >= 0 ? "1" : "0"}
                  >
                    {m.name}
                  </div>
                ))}
                {loaded && missing.length === 0 && <div style={{ padding: 6, fontSize: 12, color: "var(--chrome-text-dim)" }}>No 3D model of this board is missing.</div>}
              </div>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 0 }}>
              <span style={{ fontSize: 12 }}>Potential replacements:</span>
              <div style={LIST_STYLE} data-testid="migrate3d-candidates">
                {current &&
                  shown.map((c, j) => (
                    <div
                      key={`${j}:${c.path}`}
                      onClick={() => chooseCandidate(j)}
                      title={c.path}
                      style={{
                        padding: "1px 6px",
                        cursor: "pointer",
                        fontSize: 12,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        background: j === highlight ? "var(--chrome-hover, rgba(128,128,128,.25))" : undefined,
                      }}
                      data-testid={`migrate3d-candidate-${j}`}
                      data-path={c.path}
                      data-picked={j === choice ? "1" : "0"}
                    >
                      {c.display}
                    </div>
                  ))}
              </div>
              <div style={{ display: "flex", gap: 6 }}>
                <button disabled={busy} onClick={() => setPick("folder")} title="Scan one more folder for STEP models" data-testid="migrate3d-add-dir">
                  Add Search Directory...
                </button>
                <button disabled={!current} onClick={() => setPick("file")} title="Use a model file of your own for the selected reference" data-testid="migrate3d-open-file">
                  Open External File...
                </button>
              </div>
              <span style={{ fontSize: 11, color: "var(--chrome-text-dim)", minHeight: 14 }} data-testid="migrate3d-status">
                {loaded ? `${loaded.scanned} STEP file${loaded.scanned === 1 ? "" : "s"} searched${dirs.length > 0 ? `, with ${dirs.length} folder${dirs.length === 1 ? "" : "s"} added` : ""}` : "Searching..."}
              </span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 0 }}>
              <span style={{ fontSize: 12 }}>Preview:</span>
              <ModelPreview3D part={rep ? (parts[rep] ?? null) : null} rows={row3d ? [row3d] : []} height={300} testId="migrate3d-preview" />
            </div>
          </div>
          {note && (
            <p style={{ margin: "6px 0 0", fontSize: 12, color: "var(--chrome-text-dim)" }} data-testid="migrate3d-note">
              {note}
            </p>
          )}
          {error && (
            <p style={{ color: "var(--chrome-danger, #e5534b)", margin: "6px 0 0", fontSize: 12 }} data-testid="migrate3d-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={closeSweepDialog} data-testid="migrate3d-keep">
            Keep Existing
          </button>
          <button className="primary" disabled={busy} onClick={() => void replace()} title={chosen === 0 ? "Nothing is chosen: the references stay" : undefined} data-testid="migrate3d-replace">
            Replace Models
          </button>
        </div>
      </div>
      {pick === "folder" && <Model3dChooserDialog mode="folder" title="Add 3D Model Search Directory" onChoose={(dir) => void addDirectory(dir)} onCancel={() => setPick(null)} />}
      {pick === "file" && <Model3dChooserDialog only="step" title="Select 3D Model File" onChoose={openFile} onCancel={() => setPick(null)} />}
    </div>
  );
}
