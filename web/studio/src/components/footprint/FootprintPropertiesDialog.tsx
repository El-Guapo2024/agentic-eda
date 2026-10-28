// Port of dialog_footprint_properties_fp_editor.cpp's General tab (GAPS.md
// #8 step 5) -- name/description/keywords, attributes, reference/value
// visibility -- and its 3D Models tab (`PANEL_FP_PROPERTIES_3D_MODEL`,
// components/footprint/FootprintModelsPanel.tsx): the footprint's list of
// models, each with its offset, scale, rotation, opacity and Show switch, a
// chooser for the files and a preview of the footprint with the models on it.
// OK sends the whole panel in one `edit_footprint_properties`, the list
// included. Named `FootprintLibraryPropertiesDialog` (not
// `FootprintPropertiesDialog`, already taken by the board-instance-level,
// read-only-ish dialog "E" opens on a placed part -- a different document
// entirely, see that file's own doc comment on the model split).
//
// The Fields grid (`PCB_FIELDS_GRID_TABLE`, components/footprint/FootprintFieldsGrid.tsx) is the General tab's first part: the Reference and the Value with their texts and places, and the
// user fields -- named, shown or hidden, with their layer, size, orientation and offset. It replaces the two "visible" check boxes this tab had: they are the grid's Show column.
//
// The Embedded Files tab is the shared `PANEL_EMBEDDED_FILES` (components/EmbeddedFilesPanel.tsx) on the library footprint's list; each of its edits is a verb of its own,
// sent at once (KiCad applies it with the dialog's OK).
//
// Not ported: the Layers tab (private/custom user layers -- this app's
// board has a fixed layer set, nothing to assign a private layer from),
// the Clearances tab beyond its "Zone connection" (the solder mask and paste
// margins and the clearance are the pad dialog's own override fields at the
// footprint level -- deferred, see PARITY-fpedit.md), and
// net-tie/jumper pad groups (no net-tie concept in this model).
import { useEffect, useMemo, useState } from "react";
import type { FootprintPropertiesFields } from "../../api/types";
import type { PadConnection } from "../../api/types";
import { boardLayerList } from "../../kicad-port/boardLayers";
import { COMPONENT_TYPES, componentTypeOf, gridProblem, panelOfRows, rowsOfFootprint, withComponentType, type ComponentType, type LibFieldRow } from "../../kicad-port/fpLibFields";
import { formatLength } from "../../state/units";
import { FootprintFieldsGrid } from "./FootprintFieldsGrid";
import { EmbeddedFilesPanel } from "../EmbeddedFilesPanel";
import { useFpApi, useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { PAD_CONNECTION_OPTIONS } from "../../kicad-port/padSettings";
import { libraryModels, rowsOfLibrary, type ModelRow } from "../../kicad-port/fpModels";
import { useStudioState } from "../../state/store";
import { FootprintModelsPanel } from "./FootprintModelsPanel";

const DEFAULT_FIELDS: FootprintPropertiesFields = {
  description: "",
  keywords: "",
  attributes: { smd: false, through_hole: false, exclude_from_bom: false, exclude_from_position_files: false, board_only: false, dnp: false, allow_missing_courtyard: false, allow_soldermask_bridges: false },
  reference_visible: true,
  value_visible: true,
  model: null,
  zone_connection: null,
};

export function FootprintLibraryPropertiesDialog() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const open = state.footprintPropertiesOpen;
  const close = () => dispatch({ type: "SET_FOOTPRINT_PROPERTIES_OPEN", open: false });

  const [form, setForm] = useState<FootprintPropertiesFields>(DEFAULT_FIELDS);
  // The 3D Models tab's working copy of the list; OK sends it.
  const [tab, setTab] = useState<"general" | "models" | "embedded">("general");
  const [models, setModels] = useState<ModelRow[]>([]);
  // The fields grid's working copy, and what is wrong with it.
  const [rows, setRows] = useState<LibFieldRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const studio = useStudioState();
  const units = studio.units;
  // The layers the board has (Board Setup > Board Editor Layers): a field can be put on any of them.
  const layerSetup = studio.board?.board_rules?.layer_setup;
  const layers = useMemo(() => boardLayerList(studio.board?.layers ?? [], layerSetup), [studio.board?.layers, layerSetup]);

  useEffect(() => {
    if (open && state.footprint) {
      const { description, keywords, attributes, reference_visible, value_visible, model } = state.footprint;
      // The backend leaves an empty description or keywords (and an unset model or zone connection) out of the JSON: read them as empty, or the
      // command that sends them back is refused for a missing field.
      setForm({ description: description ?? "", keywords: keywords ?? "", attributes, reference_visible, value_visible, model: model ?? null, zone_connection: state.footprint.zone_connection ?? null });
      setModels(rowsOfLibrary(state.footprint));
      setRows(rowsOfFootprint(state.footprint));
      setError(null);
      setTab("general");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open || !state.footprint) return null;
  const fp = state.footprint;

  const setAttr = (key: keyof FootprintPropertiesFields["attributes"], value: boolean) => setForm((f) => ({ ...f, attributes: { ...f.attributes, [key]: value } }));

  const submit = async () => {
    // `Validate`: the grid's rows first (a size, a pen, a name), then the whole panel in one step -- the list of models with it: `model` is the first path, as the footprint file's
    // first (model ...) is.
    const problem = gridProblem(rows, (um) => formatLength(um, units));
    if (problem) return setError(problem);
    setError(null);
    if (await api.editProperties({ ...form, ...panelOfRows(rows, fp.name), ...libraryModels(models) })) close();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 800, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Footprint Properties</span>
          <span>{fp.name}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "72vh", overflowY: "auto" }} data-testid="fp-library-properties">
          <div style={{ display: "flex", gap: 2, borderBottom: "1px solid var(--chrome-border, #444)", marginBottom: 8 }} role="tablist">
            {(
              [
                ["general", "General"],
                ["models", "3D Models"],
                ["embedded", "Embedded Files"],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                role="tab"
                aria-selected={tab === id}
                onClick={() => setTab(id)}
                style={{ borderBottom: tab === id ? "2px solid var(--chrome-accent, #4a90e2)" : "2px solid transparent", fontWeight: tab === id ? 600 : 400, borderRadius: 0 }}
                data-testid={`fp-library-properties-tab-${id}`}
              >
                {label}
              </button>
            ))}
          </div>
          {tab === "models" && <FootprintModelsPanel rows={models} onRows={setModels} previewOf={{ footprint: fp.name }} units={units} />}
          {tab === "embedded" && <EmbeddedFilesPanel owner={{ kind: "library_footprint", name: fp.name }} run={api.cmd} version={studio.version} />}
          <div style={{ display: tab === "general" ? undefined : "none" }}>
          <FootprintFieldsGrid rows={rows} onChange={setRows} units={units} layers={layers} layerSetup={layerSetup} onError={setError} />
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr", marginTop: 10 }}>
            <span>Description</span>
            <input value={form.description} onChange={(e) => setForm((f) => ({ ...f, description: e.target.value }))} style={{ width: "100%" }} />
            <span>Keywords</span>
            <input value={form.keywords} onChange={(e) => setForm((f) => ({ ...f, keywords: e.target.value }))} style={{ width: "100%" }} />
          </div>

          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Attributes</p>
          <label style={{ display: "block", marginBottom: 4 }}>
            Component type:{" "}
            <select value={componentTypeOf(form.attributes)} onChange={(e) => setForm((f) => ({ ...f, attributes: withComponentType(f.attributes, e.target.value as ComponentType) }))} data-testid="fp-lib-component-type">
              {COMPONENT_TYPES.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
          </label>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "4px 12px" }}>
            {(
              [
                ["board_only", "Not in schematic"],
                ["exclude_from_position_files", "Exclude from position files"],
                ["exclude_from_bom", "Exclude from bill of materials"],
                ["dnp", "Do not populate"],
                ["allow_missing_courtyard", "Exempt from courtyard requirement"],
                ["allow_soldermask_bridges", "Allow bridged solder mask apertures between pads"],
              ] as const
            ).map(([key, label]) => (
              <label key={key}>
                <input type="checkbox" checked={form.attributes[key]} onChange={(e) => setAttr(key, e.target.checked)} /> {label}
              </label>
            ))}
          </div>

          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>Clearance overrides and settings</p>
          <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
            <span>Zone connection</span>
            <select
              value={form.zone_connection ?? ""}
              onChange={(e) => setForm((f) => ({ ...f, zone_connection: e.target.value === "" ? null : (e.target.value as PadConnection) }))}
              title="FOOTPRINT::GetLocalZoneConnection: how a copper zone connects to every pad of this footprint that does not set its own"
            >
              {PAD_CONNECTION_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.value === "" ? "Use zone setting" : o.label}
                </option>
              ))}
            </select>
          </div>

          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 12 }}>
            {fp.pads.length} pad(s), {fp.graphics.length} graphic(s), {fp.texts.length} text item(s), {models.length} 3D model(s).{" "}
            {fp.published ? "Board instances currently follow this definition." : "Not yet pushed to the board (Update Footprint on Board)."}
          </p>
          </div>
        </div>
        {error && (
          <p style={{ color: "var(--chrome-danger)", margin: "0 16px 8px" }} data-testid="fp-lib-properties-error">
            {error}
          </p>
        )}
        <div className="dialog-footer">
          <button
            onClick={() => {
              void api.deleteFootprint();
              close();
            }}
          >
            Delete Footprint
          </button>
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={submit} data-testid="fp-lib-properties-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
