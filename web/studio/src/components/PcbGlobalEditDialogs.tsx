// The Global Edit dialogs (actions/pcbGlobalEditSweep.ts):
//
//   GlobalDeletionDialog         DIALOG_GLOBAL_DELETION ("Delete Items"): what to delete, the locked / unlocked filters and the layer filter.
//   CleanupGraphicsDialog        DIALOG_CLEANUP_GRAPHICS: the cleanups, a live "Changes to be applied" list, and "Update PCB".
//   ExchangeFootprintsDialog     DIALOG_EXCHANGE_FOOTPRINTS (update mode): which footprints (selected / reference / value / library id / all) to
//                                update from the library.
import { useMemo, useState } from "react";
import { closeSweepDialog } from "../actions/pcbSweepDialogs";
import type { Shape } from "../api/types";
import { DEFAULT_CLEANUP_GRAPHICS, planCleanupGraphics, type CleanupGraphicsOptions, type CleanupGraphicsPlan, type ExchangeScope, type GlobalDeletionOptions } from "../kicad-port/pcbGlobalEdit";
import { DialogShell, LengthRow, useLengthField } from "./pcbDialogKit";

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

export interface ExchangeChoice {
  scope: ExchangeScope;
  reference: string;
  value: string;
  footprint: string;
}

export function ExchangeFootprintsDialog({ selectedRef, initial, onOk }: { selectedRef: string | null; initial: ExchangeChoice; onOk: (c: ExchangeChoice) => void }) {
  const [c, setC] = useState<ExchangeChoice>(initial);
  const set = <K extends keyof ExchangeChoice>(key: K, value: ExchangeChoice[K]) => setC((cur) => ({ ...cur, [key]: value }));
  const row = (scope: ExchangeScope, label: string, field?: "reference" | "value" | "footprint") => (
    <div key={scope} style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 4 }}>
      <label className="filter-row" style={{ flex: field ? "0 0 auto" : 1 }}>
        <input type="radio" name="exchange-scope" checked={c.scope === scope} onChange={() => set("scope", scope)} />
        {label}
      </label>
      {field && <input style={{ flex: 1 }} value={c[field]} aria-label={label} onFocus={() => set("scope", scope)} onChange={(e) => set(field, e.target.value)} />}
    </div>
  );
  return (
    <DialogShell
      title="Update Footprints from Library"
      width={520}
      onCancel={closeSweepDialog}
      okLabel="Update"
      onOk={() => {
        closeSweepDialog();
        onOk(c);
      }}
    >
      {row("all", "Update all footprints on board")}
      {selectedRef && row("selected", "Update selected footprint(s)")}
      {row("reference", "Update footprints matching reference designator:", "reference")}
      {row("value", "Update footprints matching value:", "value")}
      {row("footprint", "Update footprints with library id:", "footprint")}
      <div style={{ fontSize: 11, opacity: 0.7, marginTop: 8 }}>A footprint takes its library entry's pads and courtyard once the entry has been updated on the board; footprints that were never edited in the Footprint Editor have nothing to update.</div>
    </DialogShell>
  );
}
