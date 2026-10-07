// The dialogs of the pcbnew edit-tool actions (actions/pcbEditSweep.ts), one
// small component each, driven by actions/pcbSweepDialogs.ts:
//
//   unit_entry        WX_UNIT_ENTRY_DIALOG -- a single length: Fillet Lines'
//                     radius, Chamfer Lines' setback, the Simplify / Heal
//                     tolerance, Fillet Tracks' radius.
//   dogbone           GetDogboneParams (edit_tool.cpp): "Arc radius" and
//                     "Add slots in acute corners".
//   filter_selection  DIALOG_FILTER_SELECTION (dialog_filter_selection.cpp).
import { useState } from "react";
import { closeSweepDialog, useSweepDialog, type DogboneDialog, type FilterSelectionDialog, type UnitEntryDialog } from "../actions/pcbSweepDialogs";
import type { FilterOptions } from "../kicad-port/pcbSelectionOps";
import { useEscape, useLengthField } from "./pcbDialogKit";

export function PcbSweepDialogs() {
  const dlg = useSweepDialog();
  if (!dlg) return null;
  // Remount per dialog so the local field state starts from the dialog's own values.
  switch (dlg.kind) {
    case "unit_entry":
      return <UnitEntry dlg={dlg} />;
    case "dogbone":
      return <Dogbone dlg={dlg} />;
    case "filter_selection":
      return <FilterSelection dlg={dlg} />;
    case "element":
      return <>{dlg.element}</>;
  }
}

function UnitEntry({ dlg }: { dlg: UnitEntryDialog }) {
  useEscape();
  const f = useLengthField(dlg.valueUm);
  const value = f.parse();
  const ok = value !== null && (dlg.allowZero || value !== 0);
  const submit = () => {
    if (!ok || value === null) return;
    closeSweepDialog();
    dlg.onOk(value);
  };
  return (
    <div className="dialog-backdrop" onClick={closeSweepDialog}>
      <div className="dialog" style={{ width: 320 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{dlg.title}</span>
        </div>
        <div className="dialog-body">
          <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12 }}>
            <span style={{ minWidth: 90 }}>{dlg.label}</span>
            <input
              autoFocus
              style={{ flex: 1 }}
              value={f.text}
              aria-label={dlg.label}
              onChange={(e) => f.setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
            <span>{f.units}</span>
          </label>
          {!ok && <div style={{ color: "#ef5b5b", fontSize: 11, marginTop: 6 }}>{value === null ? "Enter a length." : "The value must not be zero."}</div>}
        </div>
        <div className="dialog-footer">
          <button onClick={closeSweepDialog}>Cancel</button>
          <button className="primary" disabled={!ok} onClick={submit}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

function Dogbone({ dlg }: { dlg: DogboneDialog }) {
  useEscape();
  const f = useLengthField(dlg.radiusUm);
  const [addSlots, setAddSlots] = useState(dlg.addSlots);
  const radius = f.parse();
  const ok = radius !== null && radius > 0;
  const submit = () => {
    if (!ok || radius === null) return;
    closeSweepDialog();
    dlg.onOk({ radiusUm: radius, addSlots });
  };
  return (
    <div className="dialog-backdrop" onClick={closeSweepDialog}>
      <div className="dialog" style={{ width: 360 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Dogbone Corner Settings</span>
        </div>
        <div className="dialog-body">
          <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12 }}>
            <span style={{ minWidth: 90 }}>Arc radius:</span>
            <input autoFocus style={{ flex: 1 }} value={f.text} aria-label="Arc radius" onChange={(e) => f.setText(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} />
            <span>{f.units}</span>
          </label>
          <label className="filter-row" title="Add slots in acute corners to allow access to a cutter of the given radius" style={{ marginTop: 8 }}>
            <input type="checkbox" checked={addSlots} onChange={(e) => setAddSlots(e.target.checked)} />
            Add slots in acute corners
          </label>
        </div>
        <div className="dialog-footer">
          <button onClick={closeSweepDialog}>Cancel</button>
          <button className="primary" disabled={!ok} onClick={submit}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

const FILTER_ROWS: { key: keyof FilterOptions; label: string; indent?: boolean }[] = [
  { key: "includeFootprints", label: "Include footprints" },
  { key: "includeLockedFootprints", label: "Include locked footprints", indent: true },
  { key: "includeTracks", label: "Include tracks" },
  { key: "includeVias", label: "Include vias" },
  { key: "includeZones", label: "Include zones" },
  { key: "includeItemsOnTechLayers", label: "Include drawings" },
  { key: "includeBoardOutlineLayer", label: "Include board outline layer" },
  { key: "includePcbTexts", label: "Include text items" },
];

/** `DIALOG_FILTER_SELECTION`: "All items" is three-state (checked / unchecked / some), and "locked footprints" only counts while footprints are included. */
function FilterSelection({ dlg }: { dlg: FilterSelectionDialog }) {
  useEscape();
  const [opts, setOpts] = useState<FilterOptions>(dlg.options);
  const set = (key: keyof FilterOptions, value: boolean) => setOpts((o) => ({ ...o, [key]: value }));
  // GetSuggestedAllItemsState
  const boxes = FILTER_ROWS.filter((r) => r.key !== "includeLockedFootprints" || opts.includeFootprints);
  const checked = boxes.filter((r) => opts[r.key]).length;
  const all = checked === 0 ? "none" : checked === boxes.length ? "all" : "some";
  const forceAll = (value: boolean) => setOpts(Object.fromEntries(FILTER_ROWS.map((r) => [r.key, value])) as unknown as FilterOptions);
  const submit = () => {
    closeSweepDialog();
    dlg.onOk(opts);
  };
  return (
    <div className="dialog-backdrop" onClick={closeSweepDialog}>
      <div className="dialog" style={{ width: 320 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Filter Selected Items</span>
        </div>
        <div className="dialog-body">
          <label className="filter-row">
            <input
              type="checkbox"
              checked={all === "all"}
              ref={(el) => {
                if (el) el.indeterminate = all === "some";
              }}
              onChange={(e) => forceAll(e.target.checked)}
            />
            All items
          </label>
          <div style={{ borderTop: "1px solid var(--border, #444)", margin: "6px 0" }} />
          {FILTER_ROWS.map((r) => (
            <label key={r.key} className="filter-row" style={{ paddingLeft: r.indent ? 18 : 0, opacity: r.key === "includeLockedFootprints" && !opts.includeFootprints ? 0.5 : 1 }}>
              <input type="checkbox" disabled={r.key === "includeLockedFootprints" && !opts.includeFootprints} checked={opts[r.key]} onChange={(e) => set(r.key, e.target.checked)} />
              {r.label}
            </label>
          ))}
        </div>
        <div className="dialog-footer">
          <button onClick={closeSweepDialog}>Cancel</button>
          <button className="primary" onClick={submit}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
