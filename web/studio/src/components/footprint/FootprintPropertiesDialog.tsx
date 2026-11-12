// Port of dialog_footprint_properties_fp_editor.cpp (`DIALOG_FOOTPRINT_PROPERTIES_FP_EDITOR`, GAPS.md #8 step 5, UI-LEFT F4) -- its six pages in KiCad's order:
// General (the fields grid, the Metadata box with the editable "Footprint name:", the Fabrication Attributes), Layers (the Private Layers grid), Clearance Overrides (the
// pad clearance, the solder mask expansion, the bridged-apertures box, the one "Solder paste clearance:" field of `MARGIN_OFFSET_BINDER`, and the Courtyards box), Pad
// Connections (the zone connection, the Net Ties grid, the jumpers), 3D Models (`PANEL_FP_PROPERTIES_3D_MODEL`, components/footprint/FootprintModelsPanel.tsx) and Embedded
// Files. OK runs `Validate` (the grid's rows, `checkFootprintName` with its "Overwrite" question, the clearance, the jumper groups: kicad-port/fpPropertiesTabs.ts) and sends
// the whole dialog as ONE batch -- `edit_footprint_properties`, `fp_editor set_settings` and, for a new name, `rename_library_footprint` -- the editor's one undo step
// (`commit.Push( "Edit Footprint Properties" )`). Named `FootprintLibraryPropertiesDialog` (not `FootprintPropertiesDialog`, already taken by the board-instance-level
// dialog "E" opens on a placed part -- a different document entirely, see that file's own doc comment on the model split).
//
// The Fields grid (`PCB_FIELDS_GRID_TABLE`, components/footprint/FootprintFieldsGrid.tsx) is the General tab's first part: the Reference and the Value with their texts and places, and the
// user fields -- named, shown or hidden, with their layer, size, orientation and offset.
//
// The Embedded Files tab is the shared `PANEL_EMBEDDED_FILES` (components/EmbeddedFilesPanel.tsx) on the library footprint's list: the dialog holds the panel's copy, and OK
// sends it as one `embedded apply` in the same batch, before a rename (`TransferDataFromWindow`, kicad-port/embeddedFiles.ts `applyCmd`).
//
// Not ported: the Layers tab's Custom Layers ("Use custom stackup", the copper layer count and the user layers of `FOOTPRINT_STACKUP::CUSTOM_LAYERS`), which a footprint's
// file keeps as written (`(stackup ..)`) and the studio does not edit.
import { useEffect, useMemo, useState } from "react";
import { fetchFootprintLibraryNames } from "../../api/client";
import { askConfirm } from "../library/libraryDialogs";
import { checkFootprintName, footprintExistsMessage, fpidParts, nextPrivateLayer, privateLayerChoices, settingsForm, settingsOfForm, type SettingsForm } from "../../kicad-port/fpPropertiesTabs";
import { joinLibName } from "../../kicad-port/libraryNames";
import type { Cmd } from "../../api/types";
import type { FootprintPropertiesFields } from "../../api/types";
import type { PadConnection } from "../../api/types";
import { boardLayerList } from "../../kicad-port/boardLayers";
import { COMPONENT_TYPES, componentTypeOf, gridProblem, panelOfRows, rowsOfFootprint, withComponentType, type ComponentType, type LibFieldRow } from "../../kicad-port/fpLibFields";
import { formatLength } from "../../state/units";
import { FootprintFieldsGrid } from "./FootprintFieldsGrid";
import { EmbeddedFilesPanel } from "../EmbeddedFilesPanel";
import { applyCmd, EMPTY_DRAFT, type EmbeddedDraft } from "../../kicad-port/embeddedFiles";
import type { EmbeddedListing } from "../../api/embeddedTypes";
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
  const [tab, setTab] = useState<TabId>("general");
  const [models, setModels] = useState<ModelRow[]>([]);
  // The fields grid's working copy, and what is wrong with it.
  const [rows, setRows] = useState<LibFieldRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  // "Footprint name:" (the item name of `GetFPID()`), and the Layers, Clearance Overrides and Pad Connections tabs
  const [itemName, setItemName] = useState("");
  const [tabs, setTabs] = useState<SettingsForm | null>(null);
  // The Embedded Files tab's copy of the footprint's list (`m_localFiles`), which OK sends
  const [embedded, setEmbedded] = useState<{ draft: EmbeddedDraft; listing: EmbeddedListing | null }>({ draft: EMPTY_DRAFT, listing: null });
  const studio = useStudioState();
  const units = studio.units;
  // The layers the board has (Board Setup > Board Editor Layers): a field can be put on any of them.
  const layerSetup = studio.board?.board_rules?.layer_setup;
  const layers = useMemo(() => boardLayerList(studio.board?.layers ?? [], layerSetup), [studio.board?.layers, layerSetup]);
  const privateChoices = useMemo(() => privateLayerChoices(studio.board?.layers ?? []), [studio.board?.layers]);

  useEffect(() => {
    if (open && state.footprint) {
      const { description, keywords, attributes, reference_visible, value_visible, model } = state.footprint;
      // The backend leaves an empty description or keywords (and an unset model or zone connection) out of the JSON: read them as empty, or the
      // command that sends them back is refused for a missing field.
      setForm({ description: description ?? "", keywords: keywords ?? "", attributes, reference_visible, value_visible, model: model ?? null, zone_connection: state.footprint.zone_connection ?? null });
      setModels(rowsOfLibrary(state.footprint));
      setRows(rowsOfFootprint(state.footprint));
      setItemName(fpidParts(fpidOf(state.footprint)).item);
      setTabs(settingsForm(state.footprint.settings, units));
      setError(null);
      setTab("general");
      setEmbedded({ draft: EMPTY_DRAFT, listing: null });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  if (!open || !state.footprint || !tabs) return null;
  const fp = state.footprint;
  const fpid = fpidOf(fp);

  const setAttr = (key: keyof FootprintPropertiesFields["attributes"], value: boolean) => setForm((f) => ({ ...f, attributes: { ...f.attributes, [key]: value } }));
  const setTabsField = <K extends keyof SettingsForm>(key: K, value: SettingsForm[K]) => setTabs((t) => (t ? { ...t, [key]: value } : t));

  const submit = async () => {
    // `Validate`: the grid's rows first (a size, a pen, a name), then the footprint's name (`checkFootprintName`), then the clearance and the jumper groups
    // (`TransferDataFromWindow`); then the whole dialog in one step -- the list of models with it: `model` is the first path, as the footprint file's first (model ...) is.
    const problem = gridProblem(rows, (um) => formatLength(um, units));
    if (problem) return setError(problem);
    const nameProblem = checkFootprintName(itemName);
    if (nameProblem) {
      setTab("general");
      return setError(nameProblem);
    }
    const padNumbers = new Set(fp.pads.map((p) => p.number));
    const settings = settingsOfForm(tabs, padNumbers, units, fp.settings);
    if ("error" in settings) return setError(settings.error);
    const { lib, item } = fpidParts(fpid);
    let overwrite = false;
    if (itemName !== item && !fp.board_link) {
      // `adapter->FootprintExists( libraryName, aFootprintName )`: "Footprint '%s' already exists in library '%s'." with Overwrite
      let taken = false;
      try {
        taken = (await fetchFootprintLibraryNames()).names.includes(joinLibName(lib, itemName));
      } catch {
        /* the rename verb refuses a taken name itself */
      }
      if (taken) {
        overwrite = await askConfirm({ title: "Confirmation", message: footprintExistsMessage(itemName, lib), okLabel: "Overwrite" });
        if (!overwrite) return;
      }
    }
    setError(null);
    const cmds: Cmd[] = [
      { op: "edit_footprint_properties", name: fp.name, ...form, ...panelOfRows(rows, fp.name), ...libraryModels(models) },
      { op: "fp_editor", verb: "set_settings", footprint: fp.name, settings: settings.settings },
    ];
    // The Embedded Files tab's copy (`PANEL_EMBEDDED_FILES::TransferDataFromWindow`), on the footprint's list under the name it has now
    const embeddedCmd = applyCmd({ kind: "library_footprint", name: fp.name }, embedded.listing, embedded.draft);
    if (embeddedCmd) cmds.push(embeddedCmd);
    // `fpID.SetLibItemName( m_FootprintNameCtrl->GetValue() )`: the library entry takes the new name (a board copy its own name, its key stays)
    const newName = joinLibName(lib, itemName);
    if (itemName !== item) cmds.push({ op: "rename_library_footprint", name: fp.name, new_name: newName, overwrite });
    if (!(await api.cmd({ op: "batch", cmds }))) return;
    close();
    // the editor follows the footprint to its new name (no step of its own: the open is the batch's rename)
    if (itemName !== item && !fp.board_link) {
      dispatch({ type: "SET_NAME", name: newName });
      // the tree's selection goes with the renamed footprint: left on the old name, `GetTargetFPID` would name a footprint that is no longer the one shown
      if (api.getState().treeSelection === fp.name) dispatch({ type: "SET_TREE_SELECTION", name: newName });
    }
  };

  const tabList: [TabId, string][] = [
    ["general", "General"],
    ["layers", "Layers"],
    ["clearances", "Clearance Overrides"],
    ["connections", "Pad Connections"],
    ["models", "3D Models"],
    ["embedded", "Embedded Files"],
  ];
  const box = (title: string) => <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "12px 0 4px", fontWeight: 600 }}>{title}</p>;

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 800, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Footprint Properties</span>
          <span>{fp.board_link ? `${fp.board_link.reference} [from board]` : fp.name}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "72vh", overflowY: "auto" }} data-testid="fp-library-properties">
          <div style={{ display: "flex", gap: 2, borderBottom: "1px solid var(--chrome-border, #444)", marginBottom: 8 }} role="tablist">
            {tabList.map(([id, label]) => (
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
          {tab === "embedded" && (
            <EmbeddedFilesPanel owner={{ kind: "library_footprint", name: fp.name }} run={api.cmd} version={studio.version} draft={embedded.draft} onDraft={(draft, listing) => setEmbedded({ draft, listing })} />
          )}
          <div style={{ display: tab === "general" ? undefined : "none" }}>
            <FootprintFieldsGrid rows={rows} onChange={setRows} units={units} layers={layers} layerSetup={layerSetup} onError={setError} datasheetStack={[{ kind: "library_footprint", name: fp.name }]} />
            {box("Metadata")}
            <div className="kv-grid" style={{ gridTemplateColumns: "140px 1fr" }}>
              <span>Footprint name:</span>
              <input value={itemName} onChange={(e) => setItemName(e.target.value)} style={{ width: "100%" }} data-testid="fp-lib-name" />
              <span>Description:</span>
              <input value={form.description} onChange={(e) => setForm((f) => ({ ...f, description: e.target.value }))} style={{ width: "100%" }} />
              <span>Keywords:</span>
              <input value={form.keywords} onChange={(e) => setForm((f) => ({ ...f, keywords: e.target.value }))} style={{ width: "100%" }} />
            </div>

            {box("Fabrication Attributes")}
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
                ] as const
              ).map(([key, label]) => (
                <label key={key}>
                  <input type="checkbox" checked={form.attributes[key]} onChange={(e) => setAttr(key, e.target.checked)} /> {label}
                </label>
              ))}
            </div>
            <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 12 }}>
              {fp.pads.length} pad(s), {fp.graphics.length} graphic(s), {fp.texts.length} text item(s), {models.length} 3D model(s).{" "}
              {fp.board_link ? "" : fp.published ? "Board instances currently follow this definition." : "Not yet pushed to the board (Update Footprint on Board)."}
            </p>
          </div>

          {tab === "layers" && (
            <div data-testid="fp-lib-layers">
              {box("Custom Layers")}
              <label title="A footprint's own stackup (`FOOTPRINT_STACKUP::CUSTOM_LAYERS`) is kept as its file has it; the studio does not edit it.">
                <input type="checkbox" checked={false} disabled /> Use custom stackup
              </label>
              {box("Private Layers")}
              <GroupGrid
                rows={tabs.privateLayers}
                onRows={(r) => setTabsField("privateLayers", r)}
                testid="fp-lib-private-layers"
                // `onLayerGridRowAddUserLayer`: the first user layer that is not in the grid
                newRow={() => nextPrivateLayer(tabs.privateLayers, privateChoices)}
                render={(value, onChange) => (
                  <select value={value} onChange={(e) => onChange(e.target.value)}>
                    {privateChoices.map((l) => (
                      <option key={l} value={l} disabled={l !== value && tabs.privateLayers.includes(l)}>
                        {l}
                      </option>
                    ))}
                  </select>
                )}
              />
            </div>
          )}

          {tab === "clearances" && (
            <div data-testid="fp-lib-clearances">
              {box("Clearances")}
              <p style={{ margin: "0 0 6px" }}>Leave values blank to use netclass values.</p>
              <div className="kv-grid" style={{ gridTemplateColumns: "180px 1fr" }}>
                <span title={"This is the local net clearance for all pads of this footprint.\nIf 0, the Netclass values are used.\nThis value can be overridden on a pad-by-pad basis in the Local\nClearance and Settings tab of Pad Properties."}>Pad clearance:</span>
                <span>
                  <input value={tabs.clearance} onChange={(e) => setTabsField("clearance", e.target.value)} style={{ width: 120 }} data-testid="fp-lib-clearance" /> {units}
                </span>
                <span title={"This is the local clearance between pads and the solder mask for \nthis footprint.\nIf 0, the global value is used.\nThis value can be overridden on a pad-by-pad basis in the Local\nClearance and Settings tab of Pad Properties."}>Solder mask expansion:</span>
                <span>
                  <input value={tabs.solderMask} onChange={(e) => setTabsField("solderMask", e.target.value)} style={{ width: 120 }} data-testid="fp-lib-solder-mask" /> {units}
                </span>
                <span />
                <label>
                  <input type="checkbox" checked={form.attributes.allow_soldermask_bridges} onChange={(e) => setAttr("allow_soldermask_bridges", e.target.checked)} /> Allow bridged solder mask apertures between pads
                </label>
                <span title={"Local solder paste clearance for this footprint.\nEnter an absolute value (e.g., -0.1mm), a percentage (e.g., -5%), or both (e.g., -0.1mm - 5%).\nIf blank, the global value is used."}>Solder paste clearance:</span>
                <input
                  value={tabs.solderPaste}
                  onChange={(e) => setTabs((t) => (t ? { ...t, solderPaste: e.target.value, solderPasteTouched: true } : t))}
                  style={{ width: 200 }}
                  title={"Local solder paste clearance for this footprint.\nEnter an absolute value (e.g., -0.1mm), a percentage (e.g., -5%), or both (e.g., -0.1mm - 5%).\nIf blank, the global value is used."}
                  data-testid="fp-lib-solder-paste"
                />
              </div>
              <p style={{ fontStyle: "italic", fontSize: 11, color: "var(--chrome-text-dim)" }}>Note: solder mask and paste values are used only for pads on copper layers.</p>
              {box("Courtyards")}
              <label title={'Will not generate "missing courtyard" DRC violations'}>
                <input type="checkbox" checked={form.attributes.allow_missing_courtyard} onChange={(e) => setAttr("allow_missing_courtyard", e.target.checked)} /> Exempt from courtyard requirement
              </label>
            </div>
          )}

          {tab === "connections" && (
            <div data-testid="fp-lib-connections">
              {box("Connection to Copper Zones")}
              <label>
                Pad connection to zones:{" "}
                <select
                  value={form.zone_connection ?? ""}
                  onChange={(e) => setForm((f) => ({ ...f, zone_connection: e.target.value === "" ? null : (e.target.value as PadConnection) }))}
                  data-testid="fp-lib-zone-connection"
                >
                  {PAD_CONNECTION_OPTIONS.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.value === "" ? "Use zone setting" : o.label}
                    </option>
                  ))}
                </select>
              </label>
              {box("Net Ties")}
              <p style={{ margin: "0 0 4px" }}>Pad groups allowed to short different nets:</p>
              <GroupGrid rows={tabs.netTieGroups} onRows={(r) => setTabsField("netTieGroups", r)} testid="fp-lib-net-ties" newRow={() => ""} render={(value, onChange) => <input value={value} onChange={(e) => onChange(e.target.value)} style={{ width: "100%" }} />} />
              {box("Jumpers")}
              <label title="When enabled, this footprint can have more than one pad with the same number, and pads with the same number will be considered to be jumpered together internally.">
                <input type="checkbox" checked={tabs.duplicatePadsAreJumpers} onChange={(e) => setTabsField("duplicatePadsAreJumpers", e.target.checked)} data-testid="fp-lib-duplicate-jumpers" /> All pads with duplicate numbers are jumpers
              </label>
              <p style={{ margin: "6px 0 4px" }}>Explicit jumper pad groups:</p>
              <GroupGrid rows={tabs.jumperGroups} onRows={(r) => setTabsField("jumperGroups", r)} testid="fp-lib-jumper-groups" newRow={() => ""} render={(value, onChange) => <input value={value} onChange={(e) => onChange(e.target.value)} style={{ width: "100%" }} />} />
            </div>
          )}
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

type TabId = "general" | "layers" | "clearances" | "connections" | "models" | "embedded";

/** The footprint's own `Lib:Name` (`GetFPID()`): a footprint loaded from the board has it beside the key the editor holds it by. */
function fpidOf(fp: { name: string; board_link?: { fpid: string } }): string {
  return fp.board_link?.fpid ?? fp.name;
}

/**
 * The dialog's one-column grids (`m_privateLayersGrid`, `m_nettieGroupsGrid`, `m_jumperGroupsGrid`) with their add and delete buttons (`onAddGroup` / `onRemoveGroup`,
 * `OnAddPrivateLayer` / `OnDeletePrivateLayer`): a row per entry, the selected row deleted. `newRow` answers the new row's value, or null when there is none to add.
 */
function GroupGrid({ rows, onRows, newRow, render, testid }: { rows: string[]; onRows: (r: string[]) => void; newRow: () => string | null; render: (value: string, onChange: (v: string) => void) => React.ReactNode; testid: string }) {
  const [selected, setSelected] = useState<number | null>(null);
  return (
    <div data-testid={testid}>
      <table className="setup-table" style={{ margin: 0, width: "100%" }}>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i} onClick={() => setSelected(i)} style={{ background: i === selected ? "var(--chrome-hover, rgba(128,128,128,.18))" : undefined }} data-testid={`${testid}-row-${i}`}>
              <td>{render(r, (v) => onRows(rows.map((x, j) => (j === i ? v : x))))}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <div style={{ display: "flex", gap: 6, marginTop: 4 }}>
        <button
          title="Add"
          data-testid={`${testid}-add`}
          onClick={() => {
            const v = newRow();
            if (v === null) return;
            onRows([...rows, v]);
            setSelected(rows.length);
          }}
        >
          Add
        </button>
        <button
          title="Delete"
          data-testid={`${testid}-delete`}
          disabled={selected === null || selected >= rows.length}
          onClick={() => {
            if (selected === null) return;
            onRows(rows.filter((_, j) => j !== selected));
            setSelected(rows.length > 1 ? Math.min(selected, rows.length - 2) : null);
          }}
        >
          Delete
        </button>
      </div>
    </div>
  );
}
