// The Global Edit dialogs (actions/pcbGlobalEditSweep.ts):
//
//   GlobalDeletionDialog         DIALOG_GLOBAL_DELETION ("Delete Items"): what to delete, the locked / unlocked filters and the layer filter.
//   CleanupGraphicsDialog        DIALOG_CLEANUP_GRAPHICS: the cleanups, a live "Changes to be applied" list, and "Update PCB".
//   ExchangeFootprintsDialog     DIALOG_EXCHANGE_FOOTPRINTS, both modes (Change Footprints, Update Footprints from Library): which footprints (all / selected /
//                                reference / value / library id), the footprint to change them to, the update options and the message window.
import { useMemo, useState } from "react";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import type { ExchangeOptions, Shape } from "../api/types";
import { DEFAULT_CLEANUP_GRAPHICS, planCleanupGraphics, type CleanupGraphicsOptions, type CleanupGraphicsPlan, type GlobalDeletionOptions } from "../kicad-port/pcbGlobalEdit";
import { setAllOptions, type MatchChoice, type MatchMode, type ReportLine } from "../kicad-port/pcbExchange";
import { DialogShell, LengthRow, useEscape, useLengthField } from "./pcbDialogKit";
import { FootprintChooserDialog } from "./FootprintChooserDialog";
import { ReportWindow } from "./ReportWindow";

const check = (label: string, checked: boolean, onChange: (v: boolean) => void, opts: { disabled?: boolean; indent?: boolean; title?: string } = {}) => (
  <label className="filter-row" title={opts.title} style={{ paddingLeft: opts.indent ? 18 : 0, opacity: opts.disabled ? 0.5 : 1 }} key={label}>
    <input type="checkbox" checked={checked} disabled={opts.disabled} onChange={(e) => onChange(e.target.checked)} />
    {label}
  </label>
);

export function GlobalDeletionDialog({ options, currentLayer, onOk }: { options: GlobalDeletionOptions; currentLayer: string | null; onOk: (o: GlobalDeletionOptions) => void }) {
  const [o, setO] = useState<GlobalDeletionOptions>(options);
  const set = <K extends keyof GlobalDeletionOptions>(key: K, value: GlobalDeletionOptions[K]) => setO((cur) => ({ ...cur, [key]: value }));
  // The filters only apply while their kind of item is being deleted (`onCheckDelete*`).
  const drawingFilters = o.drawings || o.boardEdges;
  return (
    <DialogShell
      title="Delete Items"
      width={520}
      onCancel={closeSweepDialog}
      onOk={() => {
        closeSweepDialog();
        onOk(o);
      }}
    >
      <div style={{ display: "flex", gap: 14 }}>
        <fieldset style={{ flex: 1, border: "1px solid var(--border, #444)", padding: "4px 8px" }}>
          <legend style={{ fontSize: 12 }}>Items to Delete</legend>
          {check("Zones", o.zones, (v) => set("zones", v))}
          {check("Text", o.texts, (v) => set("texts", v))}
          {check("Board outlines", o.boardEdges, (v) => set("boardEdges", v))}
          {check("Graphics", o.drawings, (v) => set("drawings", v))}
          {check("Footprints", o.footprints, (v) => set("footprints", v))}
          {check("Tracks & vias", o.tracks, (v) => set("tracks", v))}
          {check("Teardrops", o.teardrops, (v) => set("teardrops", v))}
          {check("Markers", o.markers, (v) => set("markers", v))}
          {check("Clear board", o.all, (v) => set("all", v), { title: "Delete everything, locked items and every layer included" })}
        </fieldset>
        <fieldset style={{ flex: 1, border: "1px solid var(--border, #444)", padding: "4px 8px" }}>
          <legend style={{ fontSize: 12 }}>Filter Settings</legend>
          {check("Locked graphics", o.drawingFilterLocked, (v) => set("drawingFilterLocked", v), { disabled: !drawingFilters })}
          {check("Unlocked graphics", o.drawingFilterUnlocked, (v) => set("drawingFilterUnlocked", v), { disabled: !drawingFilters })}
          {check("Locked footprints", o.footprintFilterLocked, (v) => set("footprintFilterLocked", v), { disabled: !o.footprints })}
          {check("Unlocked footprints", o.footprintFilterUnlocked, (v) => set("footprintFilterUnlocked", v), { disabled: !o.footprints })}
          {check("Locked tracks", o.trackFilterLocked, (v) => set("trackFilterLocked", v), { disabled: !o.tracks })}
          {check("Unlocked tracks", o.trackFilterUnlocked, (v) => set("trackFilterUnlocked", v), { disabled: !o.tracks })}
          {check("Locked vias", o.viaFilterLocked, (v) => set("viaFilterLocked", v), { disabled: !o.tracks })}
          {check("Unlocked vias", o.viaFilterUnlocked, (v) => set("viaFilterUnlocked", v), { disabled: !o.tracks })}
        </fieldset>
      </div>
      <fieldset style={{ marginTop: 10, border: "1px solid var(--border, #444)", padding: "4px 8px" }}>
        <legend style={{ fontSize: 12 }}>Layer Filter</legend>
        <label className="filter-row">
          <input type="radio" name="gd-layers" checked={!o.currentLayerOnly} onChange={() => set("currentLayerOnly", false)} />
          All layers
        </label>
        <label className="filter-row">
          <input type="radio" name="gd-layers" checked={o.currentLayerOnly} onChange={() => set("currentLayerOnly", true)} />
          Current layer ({currentLayer ?? "none"}) only
        </label>
      </fieldset>
    </DialogShell>
  );
}

export function CleanupGraphicsDialog({ shapes, options, onOk }: { shapes: readonly Shape[]; options: CleanupGraphicsOptions; onOk: (o: CleanupGraphicsOptions) => void }) {
  const [o, setO] = useState<CleanupGraphicsOptions>({ ...DEFAULT_CLEANUP_GRAPHICS, ...options });
  const tol = useLengthField(options.toleranceUm);
  const tolerance = tol.parse();
  const opts: CleanupGraphicsOptions = { ...o, toleranceUm: tolerance ?? o.toleranceUm };
  // `doCleanup( true )` after every change: the dry run that lists what Update PCB will do.
  const plan: CleanupGraphicsPlan = useMemo(() => planCleanupGraphics(shapes, { ...o, toleranceUm: tolerance ?? o.toleranceUm }, true), [shapes, o, tolerance]);
  const any = o.mergeRects || o.deleteRedundant || o.fixBoardOutlines;
  return (
    <DialogShell
      title="Cleanup Graphics"
      width={460}
      onCancel={closeSweepDialog}
      okLabel="Update PCB"
      okDisabled={!any || (o.fixBoardOutlines && tolerance === null)}
      onOk={() => {
        closeSweepDialog();
        onOk(opts);
      }}
    >
      {check("Merge lines into rectangles", o.mergeRects, (v) => setO((c) => ({ ...c, mergeRects: v })))}
      {check("Delete redundant graphics", o.deleteRedundant, (v) => setO((c) => ({ ...c, deleteRedundant: v })))}
      {check("Fix discontinuities in board outlines", o.fixBoardOutlines, (v) => setO((c) => ({ ...c, fixBoardOutlines: v })))}
      <div style={{ marginLeft: 22 }}>
        <LengthRow label="Tolerance:" field={tol} disabled={!o.fixBoardOutlines} />
      </div>
      <div style={{ fontSize: 12, marginTop: 8 }}>Changes to be applied:</div>
      <div style={{ fontSize: 11, marginTop: 4, minHeight: 80, maxHeight: 200, overflowY: "auto", border: "1px solid var(--chrome-border, #333)", borderRadius: 4, padding: 6 }}>
        {plan.items.length === 0 ? (
          <div style={{ opacity: 0.7 }}>{any ? "Nothing to clean up with the current options." : "Pick the cleanups to run."}</div>
        ) : (
          <ul style={{ margin: 0, paddingLeft: 16 }}>
            {plan.items.map((item, i) => (
              <li key={i}>
                {item.label} ({item.ids.length} item{item.ids.length === 1 ? "" : "s"})
              </li>
            ))}
          </ul>
        )}
        {o.fixBoardOutlines && <div style={{ opacity: 0.7, marginTop: 4 }}>Board outline gaps are fixed when the PCB is updated.</div>}
      </div>
    </DialogShell>
  );
}

/** The dialog's state, kept between invocations like the C++ statics (`g_matchModeForExchange`, the saved control values). */
export interface ExchangeState {
  choice: MatchChoice;
  /** `m_newID`: the footprint the matching footprints are changed to (change mode). */
  newId: string;
  options: ExchangeOptions;
}

/** The options as the checkboxes list them, with the labels of each mode (`DIALOG_EXCHANGE_FOOTPRINTS` relabels them in change mode). */
const OPTION_ROWS: { key: keyof ExchangeOptions; update: string; change: string }[] = [
  { key: "remove_extra_texts", update: "Remove text items if not in library footprint", change: "Remove text items if not in library footprint" },
  { key: "reset_text_layers", update: "Update/reset text layers and visibilities", change: "Update text layers and visibilities" },
  { key: "reset_text_effects", update: "Update/reset text sizes and styles", change: "Update text sizes and styles" },
  { key: "reset_text_positions", update: "Update/reset text position and rotation", change: "Update text positions" },
  { key: "reset_text_content", update: "Update/reset text content", change: "Update text content" },
  { key: "reset_fabrication_attrs", update: "Update/reset fabrication attributes", change: "Update fabrication attributes" },
  { key: "reset_clearance_overrides", update: "Update/reset clearance overrides", change: "Update clearance overrides" },
  { key: "reset_3d_models", update: "Update/reset 3D models", change: "Update 3D models" },
  { key: "match_pad_positions", update: "Match pad positions", change: "Match pad positions" },
];

export function ExchangeFootprintsDialog({
  mode,
  hasCurrent,
  initial,
  onRun,
}: {
  mode: "change" | "update";
  /** `m_currentFootprint`: the dialog was opened on a footprint, so "selected" is a choice. */
  hasCurrent: boolean;
  initial: ExchangeState;
  /** Do the work (`OnOKClicked` -> `processMatchingFootprints`) and answer the message window's lines. */
  onRun: (state: ExchangeState) => Promise<ReportLine[]>;
}) {
  useEscape();
  const update = mode === "update";
  const [state, setState] = useState<ExchangeState>(initial);
  const [lines, setLines] = useState<ReportLine[]>([]);
  const [busy, setBusy] = useState(false);
  const [browsing, setBrowsing] = useState<"new" | "match" | null>(null);
  const verb = update ? "Update" : "Change";
  const setChoice = (patch: Partial<MatchChoice>) => setState((cur) => ({ ...cur, choice: { ...cur.choice, ...patch } }));
  const setOption = (key: keyof ExchangeOptions, on: boolean) => setState((cur) => ({ ...cur, options: { ...cur.options, [key]: on } }));
  const row = (m: MatchMode, label: string, field?: "reference" | "value" | "footprint") => (
    <div key={m} style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 4 }}>
      <label className="filter-row" style={{ flex: field ? "0 0 auto" : 1 }}>
        <input type="radio" name="exchange-match" checked={state.choice.mode === m} onChange={() => setChoice({ mode: m })} data-testid={`exchange-match-${m}`} />
        {label}
      </label>
      {field && (
        <input
          style={{ flex: 1, minWidth: 0 }}
          value={state.choice[field]}
          aria-label={label}
          data-testid={`exchange-${field}`}
          onFocus={() => setChoice({ mode: m })}
          onChange={(e) => setChoice({ [field]: e.target.value, mode: m })}
        />
      )}
      {field === "footprint" && (
        <button onClick={() => setBrowsing("match")} title="Choose a footprint from the libraries" data-testid="exchange-browse-match">
          Browse…
        </button>
      )}
    </div>
  );
  const run = async () => {
    setBusy(true);
    try {
      setLines(await onRun(state));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="dialog-backdrop" onClick={closeSweepDialog}>
      <div className="dialog" style={{ width: 600 }} onClick={(e) => e.stopPropagation()} data-testid="exchange-dialog">
        <div className="dialog-header">
          <span>{update ? "Update Footprints from Library" : "Change Footprints"}</span>
        </div>
        <div className="dialog-body">
          {update && row("all", "Update all footprints on board")}
          {hasCurrent && row("selected", `${verb} selected footprint(s)`)}
          {row("reference", `${verb} footprints matching reference designator:`, "reference")}
          {row("value", `${verb} footprints matching value:`, "value")}
          {row("footprint", `${verb} footprints with library id:`, "footprint")}
          {!update && (
            <div style={{ display: "flex", alignItems: "center", gap: 6, margin: "8px 0" }}>
              <span style={{ flex: "0 0 auto" }}>New footprint library id:</span>
              <input style={{ flex: 1, minWidth: 0 }} value={state.newId} aria-label="New footprint library id" data-testid="exchange-new-id" onChange={(e) => setState((cur) => ({ ...cur, newId: e.target.value }))} />
              <button onClick={() => setBrowsing("new")} title="Choose a footprint from the libraries" data-testid="exchange-browse-new">
                Browse…
              </button>
            </div>
          )}
          <fieldset style={{ border: "1px solid var(--border, #444)", padding: "4px 8px", marginTop: 8 }}>
            <legend style={{ fontSize: 12 }}>Update Options</legend>
            {OPTION_ROWS.map((o) => (
              <label className="filter-row" key={o.key} style={{ display: "block" }}>
                <input type="checkbox" checked={state.options[o.key]} data-testid={`exchange-opt-${o.key}`} onChange={(e) => setOption(o.key, e.target.checked)} /> {update ? o.update : o.change}
              </label>
            ))}
            <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
              <button onClick={() => setState((cur) => ({ ...cur, options: setAllOptions(true) }))}>Check All Update Options</button>
              <button onClick={() => setState((cur) => ({ ...cur, options: setAllOptions(false) }))}>Uncheck All Update Options</button>
            </div>
          </fieldset>
          <div style={{ marginTop: 8 }}>
            <ReportWindow lines={lines} testId="exchange-messages" height={110} />
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={closeSweepDialog}>Close</button>
          <button className="primary" disabled={busy} onClick={() => void run()} data-testid="exchange-run">
            {verb}
          </button>
        </div>
      </div>
      {browsing && (
        <FootprintChooserDialog
          preselect={(browsing === "new" ? state.newId : state.choice.footprint).includes(":") ? (browsing === "new" ? state.newId : state.choice.footprint) : null}
          onCancel={() => setBrowsing(null)}
          onChoose={(pick) => {
            const which = browsing;
            setBrowsing(null);
            if (pick.kind !== "footprint") return;
            if (which === "new") setState((cur) => ({ ...cur, newId: pick.name }));
            else setChoice({ footprint: pick.name, mode: "footprint" });
          }}
        />
      )}
    </div>
  );
}
