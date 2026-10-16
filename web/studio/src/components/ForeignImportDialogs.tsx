// The two dialogs of File > Import > Non-KiCad Board File...:
//
//   "Edit Mapping of Imported Layers" -- DIALOG_MAP_LAYERS (pcbnew/dialogs/dialog_map_layers.cpp, dialog_imported_layers_base.cpp): the Unmatched Layers (the Imported Layers and
//     the KiCad Layers to choose from), the `>` `<` `<<` buttons, the Matched Layers, "Auto-Match Layers" and "Keep KiCad layer names". It is the layer callback of the importers
//     (`LAYER_MAPPABLE_PLUGIN`) put to the person. kicad-cli answers that callback with the importer's best guess and takes a map of the layers that should go elsewhere, so the
//     dialog opens with the guesses matched (the state of KiCad's after "Auto-Match Layers") and OK sends the layers that were moved. Cancel abandons the import: KiCad's dialog
//     has no way out but OK (it asks again while a required layer is unmatched).
//
//   "Import Issues" -- the report of what the importers said (the errors and warnings; the DIALOG_HTML_REPORTER KiCad opens when "Show import issues" is on).
//
// The state is in state/foreignImport.ts; the pure logic (the lists, the buttons, what is sent) in kicad-port/foreignImport.ts, unit-tested there.
import { useMemo, useState } from "react";
import { DialogShell } from "./pcbDialogKit";
import {
  REQUIRED_LAYERS_MESSAGE,
  addMappings,
  autoMatch,
  initialMapping,
  isRequiredLayer,
  keptGuesses,
  kicadLayerLabel,
  kicadLayerList,
  layerMapRequest,
  removeMappings,
  unmatchedRequired,
  wrapRequired,
  type SourceLayer,
} from "../kicad-port/foreignImport";
import {
  answerLayerMapping,
  closeImportReport,
  importShowIssues,
  setImportKeepKiCadLayerNames,
  setImportShowIssues,
  useForeignImportDialogs,
  type ImportReport,
  type LayerMapDialog,
} from "../state/foreignImport";

export function ForeignImportDialogs() {
  const { layers, report } = useForeignImportDialogs();
  return (
    <>
      {layers && <LayerMapping dialog={layers} />}
      {report && <IssuesReport report={report} />}
    </>
  );
}

const listBox = { border: "1px solid var(--chrome-border)", background: "var(--chrome-input-bg)", height: 240, overflowY: "auto", fontSize: 12 } as const;

/** One row of a list: a click selects it (a click on a selected row lets it go), a double click is the list's button. */
function Row({ label, selected, onClick, onDoubleClick, testId, dim }: { label: string; selected: boolean; onClick: () => void; onDoubleClick?: () => void; testId?: string; dim?: boolean }) {
  return (
    <div
      role="option"
      aria-selected={selected}
      data-testid={testId}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      style={{ padding: "2px 6px", cursor: "default", whiteSpace: "nowrap", background: selected ? "var(--chrome-selected-bg)" : "transparent", color: selected ? "var(--chrome-selected-text)" : dim ? "var(--chrome-text-dim)" : "inherit" }}
    >
      {label}
    </div>
  );
}

function LayerMapping({ dialog }: { dialog: LayerMapDialog }) {
  const layers: SourceLayer[] = dialog.layers;
  const required = useMemo(() => new Set(layers.filter(isRequiredLayer).map((l) => l.source)), [layers]);
  const kicadLayers = useMemo(() => kicadLayerList(layers), [layers]);
  const [state, setState] = useState(() => initialMapping(layers));
  const [pickedUnmatched, setPickedUnmatched] = useState<string[]>([]);
  const [pickedMatched, setPickedMatched] = useState<string[]>([]);
  const [pickedKicad, setPickedKicad] = useState<string>(kicadLayers[0] ?? "F.Cu");
  const [keepNames, setKeepNames] = useState(dialog.keepNames);
  const [message, setMessage] = useState<string | null>(null);
  const toggle = (list: string[], set: (v: string[]) => void, name: string) => set(list.includes(name) ? list.filter((n) => n !== name) : [...list, name]);

  // `AddMappings`: the selected unmatched layers go to the selected KiCad layer.
  const add = (sources = pickedUnmatched) => {
    setState((s) => addMappings(s, sources, pickedKicad));
    setPickedUnmatched((p) => p.filter((n) => !sources.includes(n)));
    setMessage(null);
  };
  // `RemoveMappings`
  const remove = (sources: string[] | "all" = pickedMatched) => {
    setState((s) => removeMappings(s, sources));
    setPickedMatched((p) => (sources === "all" ? [] : p.filter((n) => !sources.includes(n))));
    setMessage(null);
  };

  const kept = keptGuesses(state);
  const ok = () => {
    // `RunModal`: while a required layer is unmatched the dialog is asked again.
    if (unmatchedRequired(state, layers).length > 0) {
      setMessage(REQUIRED_LAYERS_MESSAGE);
      return;
    }
    setImportKeepKiCadLayerNames(keepNames);
    answerLayerMapping({ map: layerMapRequest(state), keepNames });
  };
  const cancel = () => answerLayerMapping(null);

  const button = { height: 36, minWidth: 36 } as const;
  return (
    <DialogShell title="Edit Mapping of Imported Layers" width={700} onCancel={cancel} onOk={ok}>
      <div data-testid="map-layers" style={{ fontSize: 12 }}>
        <div style={{ marginBottom: 6, color: "var(--chrome-text-dim)" }}>
          {dialog.name} ({dialog.format}). A layer marked * is copper: it must be matched.
        </div>
        <div style={{ display: "flex", gap: 8, alignItems: "stretch" }}>
          <fieldset style={{ flex: 3, margin: 0, padding: "4px 8px 8px" }}>
            <legend>Unmatched Layers</legend>
            <div style={{ display: "flex", gap: 8 }}>
              <div style={{ flex: 1 }}>
                <div style={{ marginBottom: 2 }}>Imported Layers</div>
                <div style={listBox} role="listbox" aria-label="Imported Layers" aria-multiselectable="true" data-testid="unmatched-layers">
                  {state.unmatched.map((name) => (
                    <Row
                      key={name}
                      label={wrapRequired(name, required.has(name))}
                      selected={pickedUnmatched.includes(name)}
                      onClick={() => toggle(pickedUnmatched, setPickedUnmatched, name)}
                      onDoubleClick={() => add([name])}
                      testId={`unmatched:${name}`}
                    />
                  ))}
                </div>
              </div>
              <div style={{ flex: 1 }}>
                <div style={{ marginBottom: 2 }}>KiCad Layers</div>
                <div style={listBox} role="listbox" aria-label="KiCad Layers" data-testid="kicad-layers">
                  {kicadLayers.map((name) => (
                    <Row key={name} label={kicadLayerLabel(name)} selected={pickedKicad === name} onClick={() => setPickedKicad(name)} testId={`kicad:${name}`} />
                  ))}
                </div>
              </div>
            </div>
          </fieldset>
          <div style={{ display: "flex", flexDirection: "column", justifyContent: "center", gap: 6 }}>
            <button style={button} title="Add selected layers to matched layers list." data-testid="map-add" onClick={() => add()}>
              &gt;
            </button>
            <button style={button} title="Remove selected layers from matched layers list." data-testid="map-remove" onClick={() => remove()}>
              &lt;
            </button>
            <button style={button} title="Remove all matched layers." data-testid="map-remove-all" onClick={() => remove("all")}>
              &lt;&lt;
            </button>
          </div>
          <fieldset style={{ flex: 2, margin: 0, padding: "4px 8px 8px" }}>
            <legend>Matched Layers</legend>
            <div style={{ display: "flex", fontWeight: 600, marginBottom: 2 }}>
              <span style={{ flex: 1 }}>Imported Layer</span>
              <span style={{ flex: 1 }}>KiCad Layer</span>
            </div>
            <div style={listBox} role="listbox" aria-label="Matched Layers" aria-multiselectable="true" data-testid="matched-layers">
              {state.matched.map((m) => (
                <div key={m.source} style={{ display: "flex" }}>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <Row label={wrapRequired(m.source, required.has(m.source))} selected={pickedMatched.includes(m.source)} onClick={() => toggle(pickedMatched, setPickedMatched, m.source)} onDoubleClick={() => remove([m.source])} testId={`matched:${m.source}`} />
                  </div>
                  <div style={{ flex: 1, padding: "2px 6px", whiteSpace: "nowrap", color: state.guess[m.source] === m.kicad ? "var(--chrome-text-dim)" : "inherit" }} title={state.guess[m.source] === m.kicad ? "kicad-cli's own guess" : "your choice"} data-testid={`matched-to:${m.source}`}>
                    {kicadLayerLabel(m.kicad)}
                  </div>
                </div>
              ))}
            </div>
          </fieldset>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 12, marginTop: 8 }}>
          <button
            title="Automatically match any unmatched layers to their KiCad equivalent."
            data-testid="map-auto"
            onClick={() => {
              setState((s) => autoMatch(s));
              setMessage(null);
            }}
          >
            Auto-Match Layers
          </button>
          <label title="If checked, layers will keep their standard KiCad names instead of being renamed to the imported layer names." style={{ display: "flex", alignItems: "center", gap: 4 }}>
            <input type="checkbox" checked={keepNames} onChange={(e) => setKeepNames(e.target.checked)} data-testid="map-keep-names" />
            Keep KiCad layer names
          </label>
        </div>
        {kept.length > 0 && (
          <div style={{ marginTop: 6, color: "var(--chrome-warn)" }} data-testid="map-kept">
            kicad-cli maps {kept.length === 1 ? "a layer" : `${kept.length} layers`} left unmatched ({kept.slice(0, 4).join(", ")}
            {kept.length > 4 ? ", ..." : ""}) by its own guess: a layer map can say where a layer goes, not leave it out.
          </div>
        )}
        {message && (
          <div style={{ marginTop: 6, color: "var(--chrome-danger)" }} role="alert" data-testid="map-error">
            {message}
          </div>
        )}
      </div>
    </DialogShell>
  );
}

function IssuesReport({ report }: { report: ImportReport }) {
  const [show, setShow] = useState(importShowIssues());
  const close = () => {
    setImportShowIssues(show);
    closeImportReport();
  };
  const colour = (severity: string) => (severity === "error" ? "var(--chrome-danger)" : severity === "warning" ? "var(--chrome-warn)" : "var(--chrome-text-dim)");
  return (
    <DialogShell title="Import Issues" width={680} onCancel={close} onOk={close}>
      <div data-testid="import-issues" style={{ fontSize: 12 }}>
        <div style={{ marginBottom: 8 }}>{report.summary}</div>
        <div style={{ border: "1px solid var(--chrome-border)", background: "var(--chrome-input-bg)", maxHeight: 280, overflowY: "auto" }}>
          {report.findings.map((f, i) => (
            <div key={i} data-testid={`issue:${f.severity}`} style={{ display: "flex", gap: 8, padding: "3px 8px", borderBottom: "1px solid var(--chrome-border)" }}>
              <span style={{ width: 62, color: colour(f.severity), fontWeight: 600 }}>{f.severity === "error" ? "Error" : f.severity === "warning" ? "Warning" : "Info"}</span>
              <span style={{ flex: 1, wordBreak: "break-word" }}>{f.message}</span>
              {f.count > 1 && <span style={{ color: "var(--chrome-text-dim)" }}>x{f.count}</span>}
            </div>
          ))}
        </div>
        {report.notes.length > 0 && (
          <ul style={{ margin: "8px 0 0", paddingLeft: 18, color: "var(--chrome-text-dim)" }}>
            {report.notes.map((n, i) => (
              <li key={i}>{n}</li>
            ))}
          </ul>
        )}
        <label style={{ display: "flex", alignItems: "center", gap: 4, marginTop: 8 }} title="Open this report after an import that has errors or warnings.">
          <input type="checkbox" checked={show} onChange={(e) => setShow(e.target.checked)} data-testid="import-show-issues" />
          Show import issues
        </label>
      </div>
    </DialogShell>
  );
}
