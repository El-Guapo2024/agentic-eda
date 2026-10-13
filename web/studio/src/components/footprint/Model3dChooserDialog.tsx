// The model chooser (`DIALOG_SELECT_3DMODEL`, 3d-viewer/dialogs/dialog_select_3d_model.cpp), opened by the 3D Models tab's folder button (`OnAdd3DModel`): the folders KiCad's
// resolver searches -- the board's own, the ones its path variables name, KiCad's 3D model library -- as a list to walk through, the model files in each (STEP, VRML), a viewer for the
// file selected and "Configure Paths...". The answer is the path to write in the footprint: the file shortened to the first folder that holds it (`ShortenPath`), so
// `${KICAD10_3DMODEL_DIR}/Resistor_SMD.3dshapes/R_0603_1608Metric.step`, `${KIPRJMOD}/parts/chip.step`. It comes from the server (`GET /api/3dmodel/browse`, which lists only
// what `GET /api/3dmodel` may read), so the page never holds a path of the machine it cannot use. A chosen model starts with KiCad's defaults: no offset, scale 1, no rotation.
//
// `mode: "folder"` is the same window as KiCad's folder picker (`wxDirDialog`, "Add 3D Model Search Directory" of Migrate 3D Models): only the folders are listed, a click picks one (a double click
// opens it) and OK answers the one picked, or the one being listed; there is no viewer.
import { useEffect, useState } from "react";
import { browseModels, type ModelEntry, type ModelFilter, type ModelListing } from "../../api/model3dClient";
import { plainRow } from "../../kicad-port/fpModels";
import { ModelPreview3D } from "./ModelPreview3D";

const LAST_DIR_KEY = "eda-studio.model3d-dir";
const LAST_FILTER_KEY = "eda-studio.model3d-filter";

function remembered(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function remember(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* it is a convenience */
  }
}

export interface Model3dChooserProps {
  onChoose: (path: string) => void;
  onCancel: () => void;
  /** "Configure Paths...": opens the paths dialog; the chooser asks for its folders again when it closes. */
  onConfigurePaths?: () => void;
  /** What is chosen: a model file (the default) or a folder (`wxDirDialog`). */
  mode?: "file" | "folder";
  title?: string;
  /** Only the files of one kind are listed and the filter is not offered ("Open External File..." of Migrate 3D Models picks a STEP file). */
  only?: ModelFilter;
}

export function Model3dChooserDialog({ onChoose, onCancel, onConfigurePaths, mode = "file", title, only }: Model3dChooserProps) {
  const folderMode = mode === "folder";
  const [listing, setListing] = useState<ModelListing | null>(null);
  const [chosenFilter, setFilter] = useState<ModelFilter>(() => (["all", "step", "wrl"].includes(remembered(LAST_FILTER_KEY) ?? "") ? (remembered(LAST_FILTER_KEY) as ModelFilter) : "all"));
  const filter = only ?? chosenFilter;
  const [selected, setSelected] = useState<ModelEntry | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  /** The folder being listed, as a footprint writes it; "" is the list of roots. */
  const [dir, setDir] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        let want = dir;
        if (want === null) {
          // first the list of roots (and what is kept of the last visit), then the folder to start in
          const roots = await browseModels("", filter);
          const last = remembered(LAST_DIR_KEY);
          want = last && roots.roots.some((r) => last === r.form || last.startsWith(`${r.form}/`)) ? last : (roots.roots.find((r) => r.name.toUpperCase().includes("3DMODEL"))?.form ?? roots.roots[0]?.form ?? "");
        }
        const next = await browseModels(want, filter).catch(async (e: unknown) => {
          // a folder that is gone (kept from the last visit): the roots
          if (dir === null) return browseModels("", filter);
          throw e;
        });
        if (cancelled) return;
        setListing(next);
        setError(null);
        if (next.dir) remember(LAST_DIR_KEY, next.dir);
        // the folder the visit started in is the one being listed from here on: a change of filter keeps it
        if (dir === null) setDir(next.dir ?? "");
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [dir, filter, reload]);

  const open = (to: string) => {
    setSelected(null);
    setDir(to);
  };
  const shown = (listing?.entries ?? []).filter((e) => !folderMode || e.kind === "dir");
  const files = shown.filter((e) => e.kind === "file");
  const choose = (path: string) => onChoose(path);
  /** What OK answers: the file selected, or in folder mode the folder selected, else the one being listed. */
  const answer = folderMode ? (selected?.path ?? listing?.dir ?? null) : (selected?.path ?? null);

  return (
    <div className="dialog-backdrop" data-model-chooser onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
      <div className="dialog" style={{ width: folderMode ? 480 : 860, maxWidth: "96vw" }} role="dialog" aria-label={title ?? "Select 3D Model"} data-testid="model3d-chooser">
        <div className="dialog-header">
          <span>{title ?? "Select 3D Model"}</span>
        </div>
        <div className="dialog-body" style={{ display: "grid", gridTemplateColumns: folderMode ? "minmax(0, 1fr)" : "minmax(0, 1fr) minmax(0, 1fr)", gap: 10, maxHeight: "72vh" }}>
          <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
            <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
              <select
                value=""
                onChange={(e) => e.target.value && open(e.target.value)}
                title="The folders model names are searched in"
                data-testid="model3d-chooser-roots"
                style={{ flex: 1, minWidth: 0 }}
              >
                <option value="">Go to a folder...</option>
                {(listing?.roots ?? []).map((r) => (
                  <option key={r.name} value={r.form} title={r.path}>
                    {r.form}
                  </option>
                ))}
              </select>
              {!folderMode && !only && (
                <select
                  value={filter}
                  onChange={(e) => {
                    const f = e.target.value as ModelFilter;
                    setFilter(f);
                    remember(LAST_FILTER_KEY, f);
                  }}
                  data-testid="model3d-chooser-filter"
                >
                  <option value="all">All supported files</option>
                  <option value="step">STEP (*.step, *.stp)</option>
                  <option value="wrl">VRML (*.wrl)</option>
                </select>
              )}
            </div>
            <div style={{ display: "flex", gap: 6, alignItems: "center", fontSize: 12 }}>
              <button disabled={!listing?.parent} onClick={() => listing?.parent && open(listing.parent)} data-testid="model3d-chooser-up">
                Up
              </button>
              <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: "var(--chrome-text-dim)" }} title={listing?.dir ?? ""} data-testid="model3d-chooser-dir">
                {listing?.dir ?? "..."}
              </span>
            </div>
            <div style={{ border: "1px solid var(--chrome-border, #444)", height: 300, overflowY: "auto" }} data-testid="model3d-chooser-list">
              {shown.map((e) => (
                <div
                  key={e.path}
                  onClick={() => (e.kind === "dir" && !folderMode ? open(e.path) : setSelected(e))}
                  onDoubleClick={() => (e.kind === "file" ? choose(e.path) : folderMode && open(e.path))}
                  style={{ padding: "1px 6px", cursor: "pointer", fontSize: 12, background: selected?.path === e.path ? "var(--chrome-hover, rgba(128,128,128,.25))" : undefined }}
                  data-testid={`model3d-chooser-${e.kind}`}
                  data-path={e.path}
                >
                  {e.kind === "dir" ? `${e.name}/` : e.name}
                </div>
              ))}
              {listing && shown.length === 0 && <div style={{ padding: 6, fontSize: 12, color: "var(--chrome-text-dim)" }}>{listing.dir ? (folderMode ? "No folder here." : "No model here.") : "Choose a folder above."}</div>}
              {listing?.truncated && <div style={{ padding: 6, fontSize: 11, color: "var(--chrome-text-dim)" }}>Only the first part of this folder is listed.</div>}
            </div>
            <div style={{ fontSize: 11, color: "var(--chrome-text-dim)", minHeight: 14 }} data-testid="model3d-chooser-selected">
              {selected ? selected.path : folderMode ? (listing?.dir ?? "") : files.length > 0 ? `${files.length} model${files.length === 1 ? "" : "s"}` : ""}
            </div>
            {error && (
              <p style={{ color: "var(--chrome-danger, #e5534b)", margin: 0, fontSize: 12 }} data-testid="model3d-chooser-error">
                {error}
              </p>
            )}
          </div>
          {!folderMode && <ModelPreview3D part={null} rows={selected ? [plainRow(selected.path)] : []} height={300} testId="model3d-chooser-preview" />}
        </div>
        <div className="dialog-footer">
          {onConfigurePaths && (
            <button
              style={{ marginRight: "auto" }}
              onClick={() => {
                onConfigurePaths();
                setReload((n) => n + 1);
              }}
              data-testid="model3d-chooser-paths"
            >
              Configure Paths...
            </button>
          )}
          <button onClick={onCancel} data-testid="model3d-chooser-cancel">
            Cancel
          </button>
          <button className="primary" disabled={!answer} onClick={() => answer && choose(answer)} data-testid="model3d-chooser-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
