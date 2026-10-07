// pcbnew's Export / Fabrication Outputs dialogs that kicad-cli has a command for, in one component (a form per kind): Export
// STEP/GLB/BREP/XAO/PLY/STL (`DIALOG_EXPORT_STEP`, which itself runs `kicad-cli pcb export <format>`), Export VRML
// (`DIALOG_EXPORT_VRML`), Export GenCAD (`DIALOG_GENCAD_EXPORT_OPTIONS`), IPC-D-356 (`GenD356File`, no options), IPC-2581
// (`DIALOG_EXPORT_2581`), ODB++ (`DIALOG_EXPORT_ODBPP`) and the board's Bill of Materials (`GenBOMFileFromBoard`) -- plus the
// footprint association (.cmp) file, the one here that is written by the studio itself (kicad-cli has no command for it).
// Each form's fields are the real dialog's, with its defaults; the options go to `/api/fab/<kind>` (crates/cli/src/
// board_output_api.rs), which builds the kicad-cli arguments the way the dialog does and runs it on the current design revision.
// Output lands in the board directory's `export/kicad/<kind>/` folder, as every other output of the studio does.
import { useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import type { BoardExportKind } from "../kicad-port/boardControlState";
import {
  DEFAULT_3D_OPTIONS,
  DEFAULT_GENCAD_OPTIONS,
  DEFAULT_IPC2581_OPTIONS,
  DEFAULT_ODB_OPTIONS,
  DEFAULT_VRML_OPTIONS,
  THREE_D_FORMATS,
  postFab3d,
  postFabCmp,
  postFabGencad,
  postFabIpc2581,
  postFabIpcD356,
  postFabOdb,
  postFabPcbBom,
  postFabVrml,
  type GencadOptions,
  type Ipc2581Options,
  type OdbOptions,
  type ThreeDOptions,
  type VrmlOptions,
} from "../api/boardControl";
import type { FabReply } from "../api/client";

const TITLES: Record<BoardExportKind, string> = {
  "3d": "Export STEP/GLB/BREP/XAO/PLY/STL",
  vrml: "Export VRML Board",
  gencad: "Export GenCAD",
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
      <Check label="User defined origin (the board center otherwise)" checked={o.user_origin} onChange={(v) => set({ user_origin: v })} />
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

function Ipc2581Form({ o, set }: { o: Ipc2581Options; set: (patch: Partial<Ipc2581Options>) => void }) {
  const text = (label: string, key: "col_id" | "col_mpn" | "col_mfg" | "col_dist_pn" | "col_dist" | "bom_rev", placeholder: string) => (
    <Row label={label}>
      <input value={o[key]} placeholder={placeholder} onChange={(e) => set({ [key]: e.target.value })} style={{ width: 170 }} />
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
        <legend>BOM columns (part field names)</legend>
        <div className="kv-grid" style={grid}>
          {text("BOM revision", "bom_rev", "")}
          {text("Internal ID", "col_id", "Generate unique")}
          {text("Manufacturer P/N", "col_mpn", "Omit")}
          {text("Manufacturer", "col_mfg", "N/A")}
          {text("Distributor P/N", "col_dist_pn", "Omit")}
          {text("Distributor", "col_dist", "N/A")}
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
  const kind = state.bcx.boardExport;
  const [three, setThree] = useState<ThreeDOptions>(DEFAULT_3D_OPTIONS);
  const [vrml, setVrml] = useState<VrmlOptions>(DEFAULT_VRML_OPTIONS);
  const [gencad, setGencad] = useState<GencadOptions>(DEFAULT_GENCAD_OPTIONS);
  const [ipc, setIpc] = useState<Ipc2581Options>(DEFAULT_IPC2581_OPTIONS);
  const [odb, setOdb] = useState<OdbOptions>(DEFAULT_ODB_OPTIONS);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; files: string[] } | null>(null);

  if (!kind) return null;
  const close = () => {
    setResult(null);
    dispatch({ type: "BCX", patch: { boardExport: null } });
  };
  const board = state.board;
  const selectedRefs = [...state.selection].filter((id) => board?.parts.some((p) => p.ref === id && p.placed));

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
              : kind === "ipc2581"
                ? await postFabIpc2581(ipc)
                : kind === "odb"
                  ? await postFabOdb(odb)
                  : kind === "pcb_bom"
                    ? await postFabPcbBom()
                    : kind === "cmp"
                      ? await postFabCmp()
                      : await postFabIpcD356();
      setResult({ ok: reply.ok, message: reply.message ?? (reply.ok ? "Done." : "Failed."), files: reply.files ?? [] });
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
          {kind === "3d" && <ThreeDForm o={three} set={(p) => setThree((o) => ({ ...o, ...p }))} selectedRefs={selectedRefs} />}
          {kind === "vrml" && <VrmlForm o={vrml} set={(p) => setVrml((o) => ({ ...o, ...p }))} />}
          {kind === "gencad" && <GencadForm o={gencad} set={(p) => setGencad((o) => ({ ...o, ...p }))} />}
          {kind === "ipc2581" && <Ipc2581Form o={ipc} set={(p) => setIpc((o) => ({ ...o, ...p }))} />}
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
