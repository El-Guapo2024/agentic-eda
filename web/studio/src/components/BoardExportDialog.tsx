// pcbnew's Export / Fabrication Outputs dialogs that kicad-cli has a command for, in one component (a form per kind): Export
// STEP/GLB/BREP/XAO/PLY/STL (`DIALOG_EXPORT_STEP`, which itself runs `kicad-cli pcb export <format>`), Export VRML
// (`DIALOG_EXPORT_VRML`), Export to GenCAD (`DIALOG_GENCAD_EXPORT_OPTIONS`), Export IDFv3 (`DIALOG_EXPORT_IDF3`), IPC-D-356 (`GenD356File`, no options), IPC-2581
// (`DIALOG_EXPORT_2581`), ODB++ (`DIALOG_EXPORT_ODBPP`) and the board's Bill of Materials (`GenBOMFileFromBoard`) -- plus the
// footprint association (.cmp) file, the one here that is written by the studio itself (kicad-cli has no command for it).
// Each form's fields are the real dialog's, with its defaults; the options go to `/api/fab/<kind>` (crates/cli/src/
// board_output_api.rs), which builds the kicad-cli arguments the way the dialog does and runs it on the current design revision.
// Output lands in the board directory's `export/kicad/<kind>/` folder, as every other output of the studio does.
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { distributorAfter, ipc2581Chosen, ipc2581DialogStart, ipc2581FieldOptions, manufacturerAfter } from "../kicad-port/ipc2581Bom";
import { scaledModelsWarning, scaledModelsWarningSuppressed, suppressScaledModelsWarning, type ScaledModelsWarning } from "../kicad-port/stepScaleWarning";
import type { BoardExportKind } from "../kicad-port/boardControlState";
import {
  DEFAULT_3D_OPTIONS,
  DEFAULT_GENCAD_OPTIONS,
  DEFAULT_IDF_OPTIONS,
  DEFAULT_IPC2581_OPTIONS,
  DEFAULT_ODB_OPTIONS,
  DEFAULT_VRML_OPTIONS,
  THREE_D_FORMATS,
  postFab3d,
  postFabCmp,
  postFabGencad,
  postFabIdf,
  postFabIpc2581,
  postFabIpcD356,
  postFabOdb,
  postFabPcbBom,
  postFabVrml,
  type GencadOptions,
  type IdfOptions,
  type Ipc2581Options,
  type OdbOptions,
  type ThreeDOptions,
  type VrmlOptions,
} from "../api/boardControl";
import type { FabReply } from "../api/client";

const TITLES: Record<BoardExportKind, string> = {
  "3d": "Export STEP/GLB/BREP/XAO/PLY/STL",
  vrml: "Export VRML Board",
  // `ExportGenCAD`'s title for the dialog; `DIALOG_EXPORT_IDF3_BASE`'s.
  gencad: "Export to GenCAD",
  idf: "Export IDFv3",
  ipcd356: "IPC-D-356 Netlist File",
  ipc2581: "Export IPC-2581",
  odb: "Export ODB++",
  pcb_bom: "Bill of Materials",
  cmp: "Footprint Association File",
};

/** Where each kind's files land, for the note under the form. */
const WHERE: Record<BoardExportKind, string> = {
  "3d": "export/kicad/<format>/",
  vrml: "export/kicad/vrml/",
  gencad: "export/kicad/gencad/",
  idf: "export/kicad/idf/",
  ipcd356: "export/kicad/ipcd356/",
  ipc2581: "export/kicad/ipc2581/",
  odb: "export/kicad/odb/",
  pcb_bom: "export/kicad/sch-bom/",
  cmp: "export/kicad/cmp/",
};

const NOTE: Record<BoardExportKind, string> = {
  "3d": "Written by `kicad-cli pcb export <format>` with the options below -- the same command line KiCad's own dialog builds.",
  vrml: "Written by `kicad-cli pcb export vrml`.",
  gencad: "Written by `kicad-cli pcb export gencad`.",
  idf: "Written by `kicad-cli pcb export idf`: the board (.emn) and its library (.emp).",
  ipcd356: "Written by `kicad-cli pcb export ipcd356`; it has no options.",
  ipc2581: "Written by `kicad-cli pcb export ipc2581`.",
  odb: "Written by `kicad-cli pcb export odb`.",
  pcb_bom: "Written by `kicad-cli sch export bom`, with the columns of pcbnew's board BOM: Id, Designator, Footprint, Quantity, Designation, Supplier and ref -- one row per value and footprint, designators joined with commas.",
  cmp: "The footprint association (.cmp) file for schematic back annotation: one entry per footprint with its timestamp, schematic path, reference, value and footprint. kicad-cli has no command for it, so the studio writes it.",
};

function Check({ label, checked, onChange, disabled, title }: { label: string; checked: boolean; onChange: (v: boolean) => void; disabled?: boolean; title?: string }) {
  return (
    <label className="toggle" style={{ display: "block", marginBottom: 4 }} title={title}>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} /> {label}
    </label>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <span>{label}</span>
      <span>{children}</span>
    </>
  );
}

const grid = { gridTemplateColumns: "150px 1fr", marginBottom: 8 } as const;

function ThreeDForm({ o, set, selectedRefs }: { o: ThreeDOptions; set: (patch: Partial<ThreeDOptions>) => void; selectedRefs: string[] }) {
  // The dialog's component radio buttons: all / only the selected ones / those matching a filter.
  const [components, setComponents] = useState<"all" | "selected" | "filter">("all");
  const [filter, setFilter] = useState("");
  const pick = (mode: "all" | "selected" | "filter", text = filter) => {
    setComponents(mode);
    set({ component_filter: mode === "all" ? "" : mode === "selected" ? selectedRefs.join(",") : text });
  };
  return (
    <>
      <div className="kv-grid" style={grid}>
        <Row label="Format">
          <select value={o.format} onChange={(e) => set({ format: e.target.value as ThreeDOptions["format"] })}>
            {THREE_D_FORMATS.map((f) => (
              <option key={f.value} value={f.value}>
                {f.label}
              </option>
            ))}
          </select>
        </Row>
        <Row label="Net filter">
          <input value={o.net_filter} placeholder="all nets (wildcards: GND*)" onChange={(e) => set({ net_filter: e.target.value })} style={{ width: 200 }} />
        </Row>
        <Row label="Board outline tolerance">
          <select value={o.tolerance} onChange={(e) => set({ tolerance: Number(e.target.value) })}>
            <option value={0.001}>0.001 mm (fine)</option>
            <option value={0.01}>0.01 mm</option>
            <option value={0.1}>0.1 mm (coarse)</option>
          </select>
        </Row>
      </div>
      <div style={{ display: "flex", gap: 24, alignItems: "flex-start" }}>
        <fieldset style={{ flex: 1, margin: 0 }}>
          <legend>Board options</legend>
          <Check label="Export board body" checked={o.board_body} onChange={(v) => set({ board_body: v })} />
          <Check label="Cut vias in board body" checked={o.cut_vias_in_body} onChange={(v) => set({ cut_vias_in_body: v })} title="Cut via holes in board body even if conductor layers are not exported." />
          <Check label="Export silkscreen" checked={o.silkscreen} onChange={(v) => set({ silkscreen: v })} title="Export silkscreen graphics as a set of flat faces." />
          <Check label="Export solder mask" checked={o.soldermask} onChange={(v) => set({ soldermask: v })} title="Export solder mask layers as a set of flat faces." />
          <Check label="Export components" checked={o.components} onChange={(v) => set({ components: v })} />
          <div style={{ marginLeft: 18, opacity: o.components ? 1 : 0.5 }}>
            <label style={{ display: "block" }}>
              <input type="radio" checked={components === "all"} disabled={!o.components} onChange={() => pick("all")} /> All components
            </label>
            <label style={{ display: "block" }} title="Export only the component models that are selected in the PCB editor">
              <input type="radio" checked={components === "selected"} disabled={!o.components || selectedRefs.length === 0} onChange={() => pick("selected")} /> Only selected{selectedRefs.length === 0 ? " (select footprints first)" : ` (${selectedRefs.length})`}
            </label>
            <label style={{ display: "block" }} title="A list of comma-separated reference designators to export (wildcards are supported)">
              <input type="radio" checked={components === "filter"} disabled={!o.components} onChange={() => pick("filter")} /> Components matching filter:
              <input
                value={filter}
                disabled={!o.components || components !== "filter"}
                placeholder="R1,C*"
                onChange={(e) => {
                  setFilter(e.target.value);
                  pick("filter", e.target.value);
                }}
                style={{ width: 110, marginLeft: 6 }}
              />
            </label>
          </div>
          <Check label="Remove 'Unspecified' components" checked={o.no_unspecified} onChange={(v) => set({ no_unspecified: v })} />
          <Check label="Remove 'Do not populate' components" checked={o.no_dnp} onChange={(v) => set({ no_dnp: v })} />
          <Check label="Substitute STEP or IGS models with the same name in place of VRML models" checked={o.subst_models} onChange={(v) => set({ subst_models: v })} />
        </fieldset>
        <fieldset style={{ flex: 1, margin: 0 }}>
          <legend>Conductor options</legend>
          <Check label="Export tracks and vias" checked={o.tracks} onChange={(v) => set({ tracks: v })} />
          <Check label="Export pads" checked={o.pads} onChange={(v) => set({ pads: v })} />
          <Check label="Export zones" checked={o.zones} onChange={(v) => set({ zones: v })} />
          <Check label="Export inner copper" checked={o.inner_copper} onChange={(v) => set({ inner_copper: v })} />
          <Check label="Fuse shapes" checked={o.fuse_shapes} onChange={(v) => set({ fuse_shapes: v })} title="Fuse overlapping geometry together." />
          <Check label="Fill all vias" checked={o.fill_all_vias} onChange={(v) => set({ fill_all_vias: v })} title="Don't cut via holes in conductor layers." />
          <Check label="Optimize STEP" checked={o.optimize} onChange={(v) => set({ optimize: v })} />
        </fieldset>
      </div>
      <fieldset style={{ marginTop: 8 }}>
        <legend>Output origin</legend>
        {(
          [
            ["drill", "Drill/place file origin"],
            ["grid", "Grid origin"],
            ["board_center", "Board center"],
            ["user", "User defined origin"],
          ] as const
        ).map(([value, label]) => (
          <label key={value} style={{ display: "block" }}>
            <input type="radio" checked={o.origin === value} onChange={() => set({ origin: value })} /> {label}
            {value === "user" && (
              <span style={{ marginLeft: 8, opacity: o.origin === "user" ? 1 : 0.5 }}>
                X <input type="number" step="any" disabled={o.origin !== "user"} value={o.origin_x} onChange={(e) => set({ origin_x: Number(e.target.value) })} style={{ width: 70 }} /> Y{" "}
                <input type="number" step="any" disabled={o.origin !== "user"} value={o.origin_y} onChange={(e) => set({ origin_y: Number(e.target.value) })} style={{ width: 70 }} /> mm
              </span>
            )}
          </label>
        ))}
      </fieldset>
    </>
  );
}

function VrmlForm({ o, set }: { o: VrmlOptions; set: (patch: Partial<VrmlOptions>) => void }) {
  return (
    <>
      <div className="kv-grid" style={grid}>
        <Row label="Units">
          <select value={o.units} onChange={(e) => set({ units: e.target.value as VrmlOptions["units"] })}>
            <option value="mm">mm</option>
            <option value="m">meter</option>
            <option value="tenths">0.1 inch</option>
            <option value="in">inch</option>
          </select>
        </Row>
        <Row label="Footprint 3D model path">
          <input value={o.models_dir} disabled={!o.copy_models} onChange={(e) => set({ models_dir: e.target.value })} style={{ width: 160 }} />
        </Row>
      </div>
      <Check label="User defined origin" checked={o.user_origin} onChange={(v) => set({ user_origin: v })} />
      {o.user_origin && (
        <div style={{ marginLeft: 18, marginBottom: 6 }}>
          X <input type="number" step="any" value={o.origin_x} onChange={(e) => set({ origin_x: Number(e.target.value) })} style={{ width: 80 }} /> Y{" "}
          <input type="number" step="any" value={o.origin_y} onChange={(e) => set({ origin_y: Number(e.target.value) })} style={{ width: 80 }} /> mm
        </div>
      )}
      <Check label="Ignore 'Do not populate' components" checked={o.no_dnp} onChange={(v) => set({ no_dnp: v })} />
      <Check label="Ignore 'Unspecified' components" checked={o.no_unspecified} onChange={(v) => set({ no_unspecified: v })} />
      <Check label="Copy 3D model files to 3D model path" checked={o.copy_models} onChange={(v) => set({ copy_models: v })} title="If checked: copy 3D models to the destination folder. If not checked: embed 3D models in the VRML board file." />
      <Check label="Use relative paths to model files in board VRML file" checked={o.relative_paths} disabled={!o.copy_models} onChange={(v) => set({ relative_paths: v })} />
    </>
  );
}

function GencadForm({ o, set }: { o: GencadOptions; set: (patch: Partial<GencadOptions>) => void }) {
  return (
    <>
      <Check label="Flip bottom footprint padstacks" checked={o.flip_bottom_pads} onChange={(v) => set({ flip_bottom_pads: v })} />
      <Check label="Generate unique pin names" checked={o.unique_pins} onChange={(v) => set({ unique_pins: v })} />
      <Check label="Generate a new shape for each footprint instance (do not reuse shapes)" checked={o.unique_footprints} onChange={(v) => set({ unique_footprints: v })} />
      <Check label="Use drill/place file origin as origin" checked={o.use_drill_origin} onChange={(v) => set({ use_drill_origin: v })} />
      <Check label="Save the origin coordinates in the file" checked={o.store_origin} onChange={(v) => set({ store_origin: v })} />
    </>
  );
}

/** `DIALOG_EXPORT_IDF3` (`dialog_export_idf_base.cpp`): its file name is the studio's (the output directory below). */
function IdfForm({ o, set }: { o: IdfOptions; set: (patch: Partial<IdfOptions>) => void }) {
  // `OnBoardReferencePointChecked`: the positions are enabled while the box is checked.
  const on = o.set_reference_point;
  return (
    <>
      <Check label="Set board reference point:" checked={on} onChange={(v) => set({ set_reference_point: v })} />
      <div className="kv-grid" style={{ ...grid, marginLeft: 18, opacity: on ? 1 : 0.5 }}>
        <Row label="X position:">
          <input type="number" step="any" disabled={!on} value={o.x_ref} onChange={(e) => set({ x_ref: Number(e.target.value) || 0 })} style={{ width: 90 }} /> mm
        </Row>
        <Row label="Y position:">
          <input type="number" step="any" disabled={!on} value={o.y_ref} onChange={(e) => set({ y_ref: Number(e.target.value) || 0 })} style={{ width: 90 }} /> mm
        </Row>
      </div>
      <div className="kv-grid" style={grid}>
        <Row label="Output units:">
          <select value={o.units} onChange={(e) => set({ units: e.target.value as IdfOptions["units"] })}>
            <option value="mm">Millimeters</option>
            <option value="mils">Mils</option>
          </select>
        </Row>
      </div>
      <Check label="Ignore 'Do not populate' components" checked={o.no_dnp} onChange={(v) => set({ no_dnp: v })} />
      <Check label="Ignore 'Unspecified' components" checked={o.no_unspecified} onChange={(v) => set({ no_unspecified: v })} />
    </>
  );
}

/** `DIALOG_EXPORT_2581`: the BOM columns are choices of the board's footprint field names (`init`), the first item of each meaning none;
 *  Manufacturer is enabled with a Manufacturer P/N column and Distributor with a Distributor P/N column, each guessed from it
 *  (`onMfgPNChange`, `onDistPNChange`; `kicad-port/ipc2581Bom.ts`). */
function Ipc2581Form({ o, set, fields }: { o: Ipc2581Options; set: (patch: Partial<Ipc2581Options>) => void; fields: string[] }) {
  const choice = (label: string, key: "col_id" | "col_mpn" | "col_mfg" | "col_dist_pn", first: string, onPick: (v: string) => Partial<Ipc2581Options>, title: string, disabled = false) => (
    <Row label={label}>
      <select value={o[key]} disabled={disabled} title={title} onChange={(e) => set(onPick(e.target.value))} style={{ width: 170 }}>
        <option value="">{first}</option>
        {fields.map((f) => (
          <option key={f} value={f}>
            {f}
          </option>
        ))}
      </select>
    </Row>
  );
  return (
    <>
      <div className="kv-grid" style={grid}>
        <Row label="Units">
          <select value={o.units} onChange={(e) => set({ units: e.target.value as Ipc2581Options["units"] })}>
            <option value="mm">Millimeters</option>
            <option value="in">Inches</option>
          </select>
        </Row>
        <Row label="Precision">
          <input type="number" min={4} max={10} value={o.precision} onChange={(e) => set({ precision: Math.max(4, Math.min(10, Number(e.target.value) || 6)) })} style={{ width: 60 }} />
        </Row>
        <Row label="Version">
          <select value={o.version} onChange={(e) => set({ version: e.target.value as "B" | "C" })}>
            <option value="B">B</option>
            <option value="C">C</option>
          </select>
        </Row>
      </div>
      <Check label="Compress output" checked={o.compress} onChange={(v) => set({ compress: v })} title="Compress output into 'zip' file" />
      <fieldset style={{ marginTop: 6 }}>
        <legend>BOM Columns</legend>
        <div className="kv-grid" style={grid}>
          <Row label="BOM revision:">
            <input value={o.bom_rev} title="Revision string for the BOM section. Auto-populated from schematic title block revision" onChange={(e) => set({ bom_rev: e.target.value })} style={{ width: 170 }} />
          </Row>
          {choice("Internal ID:", "col_id", "Generate unique", (v) => ({ col_id: v }), "Part ID number used internally during design.\nThis number must be unique to each part.")}
          {choice("Manufacturer P/N:", "col_mpn", "Omit", (v) => ({ col_mpn: v, col_mfg: manufacturerAfter(v, o.col_mfg, fields) }), "Column containing the manufacturer part number")}
          {choice("Manufacturer:", "col_mfg", "N/A", (v) => ({ col_mfg: v }), "", !o.col_mpn)}
          {choice("Distributor P/N:", "col_dist_pn", "Omit", (v) => ({ col_dist_pn: v, col_dist: distributorAfter(v, o.col_dist) }), "Column containing the distributor part number")}
          <Row label="Distributor:">
            <input value={o.col_dist} disabled={!o.col_dist_pn} onChange={(e) => set({ col_dist: e.target.value })} style={{ width: 170 }} />
          </Row>
        </div>
      </fieldset>
    </>
  );
}

function OdbForm({ o, set }: { o: OdbOptions; set: (patch: Partial<OdbOptions>) => void }) {
  return (
    <div className="kv-grid" style={grid}>
      <Row label="Units">
        <select value={o.units} onChange={(e) => set({ units: e.target.value as OdbOptions["units"] })}>
          <option value="mm">Millimeters</option>
          <option value="in">Inches</option>
        </select>
      </Row>
      <Row label="Precision">
        <input type="number" min={2} max={16} value={o.precision} onChange={(e) => set({ precision: Math.max(2, Math.min(16, Number(e.target.value) || 6)) })} style={{ width: 60 }} />
      </Row>
      <Row label="Compression format">
        <select value={o.compression} onChange={(e) => set({ compression: e.target.value as OdbOptions["compression"] })}>
          <option value="none">None</option>
          <option value="zip">ZIP</option>
          <option value="tgz">TGZ</option>
        </select>
      </Row>
    </div>
  );
}

export function BoardExportDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const kind = state.bcx.boardExport;
  const [three, setThree] = useState<ThreeDOptions>(DEFAULT_3D_OPTIONS);
  const [vrml, setVrml] = useState<VrmlOptions>(DEFAULT_VRML_OPTIONS);
  const [gencad, setGencad] = useState<GencadOptions>(DEFAULT_GENCAD_OPTIONS);
  const [idf, setIdf] = useState<IdfOptions>(DEFAULT_IDF_OPTIONS);
  const [ipc, setIpc] = useState<Ipc2581Options>(DEFAULT_IPC2581_OPTIONS);
  const [odb, setOdb] = useState<OdbOptions>(DEFAULT_ODB_OPTIONS);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; files: string[]; log?: string } | null>(null);
  // `DIALOG_EXPORT_STEP::DIALOG_EXPORT_STEP`: the "Model Scale Warning" when the STEP dialog opens on a board with scaled models.
  const [scaleWarning, setScaleWarning] = useState<ScaledModelsWarning | null>(null);
  const [doNotShow, setDoNotShow] = useState(false);
  // The board's footprints' field names, the BOM columns' choices (`DIALOG_EXPORT_2581::init`).
  const ipcFields = ipc2581FieldOptions((state.board?.parts ?? []).filter((p) => p.placed));
  useEffect(() => {
    // `TransferDataToWindow`: the BOM columns the board kept, the BOM revision else the schematic's.
    if (kind === "ipc2581") setIpc((o) => ({ ...o, ...ipc2581DialogStart(state.board?.plot_params?.ipc2581_bom, state.board?.plot_params?.sch_revision ?? "", ipcFields) }));
    setScaleWarning(kind === "3d" && !scaledModelsWarningSuppressed() ? scaledModelsWarning(state.board?.parts ?? []) : null);
    setDoNotShow(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind]);

  if (!kind) return null;
  const close = () => {
    setResult(null);
    dispatch({ type: "BCX", patch: { boardExport: null } });
  };
  const board = state.board;
  const selectedRefs = [...state.selection].filter((id) => board?.parts.some((p) => p.ref === id && p.placed));

  // `TransferDataFromWindow` then the export: the columns kept in the board when they changed (no undo step), and given to kicad-cli.
  const exportIpc2581 = async (): Promise<FabReply> => {
    const chosen = ipc2581Chosen(ipc);
    const kept = { id: "", mpn: "", mfg: "", dist_pn: "", dist: "", bom_rev: "", ...(state.board?.plot_params?.ipc2581_bom ?? {}) };
    if (JSON.stringify(chosen) !== JSON.stringify({ id: kept.id, mpn: kept.mpn, mfg: kept.mfg, dist_pn: kept.dist_pn, dist: kept.dist, bom_rev: kept.bom_rev })) {
      await api.cmd({ op: "set_ipc2581_bom", bom: chosen });
    }
    return postFabIpc2581({ ...ipc, col_id: chosen.id, col_mpn: chosen.mpn, col_mfg: chosen.mfg, col_dist_pn: chosen.dist_pn, col_dist: chosen.dist });
  };

  const run = async () => {
    setBusy(true);
    setResult(null);
    try {
      const reply: FabReply =
        kind === "3d"
          ? await postFab3d(three)
          : kind === "vrml"
            ? await postFabVrml(vrml)
            : kind === "gencad"
              ? await postFabGencad(gencad)
              : kind === "idf"
                ? await postFabIdf(idf)
                : kind === "ipc2581"
                  ? await exportIpc2581()
                  : kind === "odb"
                    ? await postFabOdb(odb)
                    : kind === "pcb_bom"
                      ? await postFabPcbBom()
                      : kind === "cmp"
                        ? await postFabCmp()
                        : await postFabIpcD356();
      setResult({ ok: reply.ok, message: reply.message ?? (reply.ok ? "Done." : "Failed."), files: reply.files ?? [], log: reply.log });
    } catch (e) {
      setResult({ ok: false, message: e instanceof Error ? e.message : String(e), files: [] });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: kind === "3d" ? 640 : 480 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">{TITLES[kind]}</div>
        <div className="dialog-body" style={{ paddingTop: 10, maxHeight: "70vh", overflowY: "auto" }}>
          {kind === "3d" && scaleWarning && (
            <div className="problem-row" role="alertdialog" aria-label={scaleWarning.title} data-testid="step-scale-warning" style={{ fontSize: 11, marginBottom: 10 }}>
              <b>{scaleWarning.title}</b>
              <div style={{ marginTop: 4 }}>{scaleWarning.message}</div>
              <pre style={{ margin: "6px 0", whiteSpace: "pre-wrap" }}>{scaleWarning.extended}</pre>
              <label className="toggle">
                <input type="checkbox" checked={doNotShow} onChange={(e) => setDoNotShow(e.target.checked)} />
                Do not show again
              </label>{" "}
              <button
                onClick={() => {
                  if (doNotShow) suppressScaledModelsWarning();
                  setScaleWarning(null);
                }}
              >
                OK
              </button>
            </div>
          )}
          {kind === "3d" && <ThreeDForm o={three} set={(p) => setThree((o) => ({ ...o, ...p }))} selectedRefs={selectedRefs} />}
          {kind === "vrml" && <VrmlForm o={vrml} set={(p) => setVrml((o) => ({ ...o, ...p }))} />}
          {kind === "gencad" && <GencadForm o={gencad} set={(p) => setGencad((o) => ({ ...o, ...p }))} />}
          {kind === "idf" && <IdfForm o={idf} set={(p) => setIdf((o) => ({ ...o, ...p }))} />}
          {kind === "ipc2581" && <Ipc2581Form o={ipc} set={(p) => setIpc((o) => ({ ...o, ...p }))} fields={ipcFields} />}
          {kind === "odb" && <OdbForm o={odb} set={(p) => setOdb((o) => ({ ...o, ...p }))} />}
          <div style={{ fontSize: 11, color: "var(--chrome-text-dim)", margin: "10px 0" }}>
            {NOTE[kind]} Output directory: <code>{WHERE[kind]}</code>
          </div>
          {result && (
            <div className={result.ok ? "panel-empty" : "problem-row"} style={{ fontSize: 11 }}>
              <b>{result.message}</b>
              {result.files.length > 0 && (
                <ul style={{ margin: "6px 0 0 16px", padding: 0 }}>
                  {result.files.map((f) => (
                    <li key={f}>{f}</li>
                  ))}
                </ul>
              )}
              {/* kicad-cli's own output, its warnings included (KiCad's STEP export shows it in `DIALOG_EXPORT_STEP_LOG`). */}
              {result.log && (
                <pre data-testid="board-export-log" style={{ margin: "6px 0 0", whiteSpace: "pre-wrap", maxHeight: 160, overflowY: "auto" }}>
                  {result.log}
                </pre>
              )}
            </div>
          )}
        </div>
        <div className="dialog-footer">
          <button className="primary" onClick={run} disabled={busy}>
            {busy ? "Exporting…" : "Export"}
          </button>
          <button onClick={close}>Close</button>
        </div>
      </div>
    </div>
  );
}
