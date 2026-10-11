// Port of pcbnew/dialogs/dialog_cleanup_tracks_and_vias{,_base}.cpp --
// "Cleanup Tracks & Vias..." (pcbnew.GlobalEdit.cleanupTracksAndVias,
// task item 1). Two-step flow, matching source exactly: the primary
// button reads "Build Changes" the first time (a dry run -- computes and
// lists what would change, touches nothing) and "Update PCB" once a
// build has run with the current checkbox state (commits it); ticking
// any checkbox or changing a filter after a build resets back to "Build
// Changes" (`DIALOG_CLEANUP_TRACKS_AND_VIAS::OnCheckBox`,
// `OnNetFilterSelect`, `OnNetclassFilterSelect`, `OnLayerFilterSelect`).
//
// The "Filter Items" box (net, net class, layer, "Selected items only";
// kicad-port/cleanupFilters.ts, crates/connectivity/src/cleanup_filter.rs)
// narrows which tracks and vias the cleanup may remove or merge; the ones it
// skips still count as copper to the rest. "Refill zones before and after
// cleanup" (`m_cbRefillZones`): the studio computes a zone's fill on demand,
// so the checks always see the real one; ticked, the dialog refreshes the
// displayed fill before the build (`CheckAllZones`) and after Update PCB
// (`FillAllZones`), as B does.
//
// Not as in source: `doCleanup` clears the selection before the real run's
// filter reads it, which would make "Selected items only" skip everything on
// Update PCB; here Update PCB applies the selection the build used, and
// clears it afterwards ("Clear current selection list to avoid selection of
// deleted items"). The dialog starts, as `TransferDataToWindow` does, on the
// highlighted net, the Default class and the active layer. See PARITY-pcb.md
// for the adaptations in the Rust port this dialog drives
// (`crates/connectivity/src/cleanup.rs`).
import { useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { cleanupTracksApply, cleanupTracksPreview } from "../api/client";
import type { CleanupActions, CleanupChange, CleanupOptions } from "../api/types";
import { anyFilterOn, filterBoxDefaults, filterChoices, filterFields, type CleanupFilterBox } from "../kicad-port/cleanupFilters";

const DEFAULT_OPTIONS: CleanupActions = {
  delete_shorting: false,
  delete_redundant_vias: false,
  delete_dangling_vias: false,
  merge_segments: false,
  delete_dangling_tracks: false,
  delete_tracks_in_pads: false,
};

const CHECKBOXES: { key: keyof CleanupActions; label: string; tooltip: string }[] = [
  { key: "delete_shorting", label: "Delete tracks connecting different nets", tooltip: "remove track/via segments touching a pad, track or via of a different net (short circuit)" },
  { key: "delete_redundant_vias", label: "Delete redundant vias", tooltip: "remove vias on through-hole pads and superimposed vias" },
  { key: "delete_dangling_vias", label: "Delete vias connected on only one layer", tooltip: "a via that never actually bridges two copper layers" },
  { key: "merge_segments", label: "Merge co-linear tracks", tooltip: "merge aligned track segments, and remove null segments" },
  { key: "delete_dangling_tracks", label: "Delete tracks unconnected at one end", tooltip: "delete tracks having at least one dangling end" },
  { key: "delete_tracks_in_pads", label: "Delete tracks fully inside pads", tooltip: "delete tracks that have both start and end positions inside of a pad" },
];

const GROUP_TITLE = { fontWeight: 600, fontSize: 11, marginBottom: 4 } as const;
const FILTER_ROW = { display: "flex", alignItems: "center", gap: 6, marginBottom: 4 } as const;

export function CleanupTracksDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.cleanupTracksDialogOpen;

  const [opts, setOpts] = useState<CleanupActions>(DEFAULT_OPTIONS);
  const [refill, setRefill] = useState(false);
  // null until the user touches a filter: the defaults follow the highlighted net and the active layer of the moment the dialog opens.
  const [box, setBox] = useState<CleanupFilterBox | null>(null);
  const [built, setBuilt] = useState(false);
  const [busy, setBusy] = useState(false);
  const [changes, setChanges] = useState<CleanupChange[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  const filters = box ?? filterBoxDefaults(state.board, state.netHighlight, state.activeLayer);
  const choices = filterChoices(state.board);

  const reset = () => {
    setOpts(DEFAULT_OPTIONS);
    setRefill(false);
    setBox(null);
    setBuilt(false);
    setChanges(null);
    setError(null);
  };

  const close = () => {
    reset();
    dispatch({ type: "SET_CLEANUP_TRACKS_DIALOG_OPEN", open: false });
  };

  const toggle = (key: keyof CleanupActions) => {
    setOpts((o) => ({ ...o, [key]: !o[key] }));
    setBuilt(false); // OnCheckBox: any option change forces a fresh "Build Changes"
  };

  const setFilters = (patch: Partial<CleanupFilterBox>) => {
    setBox({ ...filters, ...patch });
    setBuilt(false); // OnCheckBox / OnNetFilterSelect / OnNetclassFilterSelect / OnLayerFilterSelect
  };

  const anyOptionOn = Object.values(opts).some(Boolean);

  const request = (): CleanupOptions => ({ ...opts, ...filterFields(filters, state.selection) });

  const build = async () => {
    setBusy(true);
    setError(null);
    try {
      if (refill) await api.fillZones(); // "Checking zones..."
      const reply = await cleanupTracksPreview(request());
      if (!reply.ok) {
        setError(reply.message ?? "could not compute the cleanup");
        return;
      }
      setChanges(reply.changes ?? []);
      setBuilt(true);
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      const reply = await cleanupTracksApply(request());
      if (!reply.ok) {
        setError(reply.message ?? "could not apply the cleanup");
        return;
      }
      if (refill) await api.fillZones(); // "Refilling all zones..."
      dispatch({ type: "SET_SELECTION", refs: [] }); // selectionClear: nothing selected is left pointing at a deleted item
      close();
    } finally {
      setBusy(false);
    }
  };

  const primary = built ? apply : build;
  const primaryLabel = built ? "Update PCB" : "Build Changes";

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 460 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Cleanup Tracks & Vias</span>
        </div>
        <div className="dialog-body">
          <div style={GROUP_TITLE}>Actions</div>
          <label className="filter-row" style={{ display: "block", marginBottom: 8 }} title="Refill the zones before the changes are built and again after they are applied">
            <input type="checkbox" checked={refill} onChange={(e) => setRefill(e.target.checked)} aria-label="Refill zones before and after cleanup" />
            Refill zones before and after cleanup
          </label>
          {CHECKBOXES.map(({ key, label, tooltip }) => (
            <label key={key} className="filter-row" title={tooltip} style={{ display: "block", marginBottom: 4 }}>
              <input type="checkbox" checked={opts[key]} onChange={() => toggle(key)} />
              {label}
            </label>
          ))}

          <div style={{ ...GROUP_TITLE, marginTop: 10 }}>Filter Items</div>
          <label className="filter-row" style={FILTER_ROW}>
            <input type="checkbox" checked={filters.netOn} onChange={(e) => setFilters({ netOn: e.target.checked })} aria-label="Filter items by net" />
            Filter items by net:
            <select value={filters.net} onChange={(e) => setFilters({ net: e.target.value })} style={{ flex: 1 }} aria-label="Net filter">
              {choices.nets.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
          <label className="filter-row" style={FILTER_ROW}>
            <input type="checkbox" checked={filters.netclassOn} onChange={(e) => setFilters({ netclassOn: e.target.checked })} aria-label="Filter items by net class" />
            Filter items by net class:
            <select value={filters.netclass} onChange={(e) => setFilters({ netclass: e.target.value })} style={{ flex: 1 }} aria-label="Net class filter">
              {choices.netclasses.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </label>
          <label className="filter-row" style={FILTER_ROW}>
            <input type="checkbox" checked={filters.layerOn} onChange={(e) => setFilters({ layerOn: e.target.checked })} aria-label="Filter items by layer" />
            Filter items by layer:
            <select value={filters.layer} onChange={(e) => setFilters({ layer: e.target.value })} style={{ flex: 1 }} aria-label="Layer filter">
              {choices.layers.map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
          </label>
          <label className="filter-row" style={{ display: "block", marginBottom: 4 }}>
            <input type="checkbox" checked={filters.selectedOnly} onChange={(e) => setFilters({ selectedOnly: e.target.checked })} aria-label="Selected items only" />
            Selected items only
          </label>

          <div style={{ fontSize: 11, marginTop: 10, minHeight: 90, maxHeight: 220, overflowY: "auto", border: "1px solid var(--chrome-border, #333)", borderRadius: 4, padding: 6 }}>
            {error && <div style={{ color: "#ff6666" }}>{error}</div>}
            {!error && !built && <div style={{ opacity: 0.7 }}>Pick the cleanups to run, then Build Changes to preview them -- nothing is touched until Update PCB.</div>}
            {!error && built && changes && changes.length === 0 && (
              <div style={{ opacity: 0.7 }}>{anyFilterOn(filters) ? "Nothing to clean up with the current options and filters." : "Nothing to clean up with the current options."}</div>
            )}
            {!error && built && changes && changes.length > 0 && (
              <ul style={{ margin: 0, paddingLeft: 16 }}>
                {changes.map((c, i) => (
                  <li key={i}>
                    {c.label} ({c.net || "no net"})
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={primary} disabled={busy || !anyOptionOn}>
            {primaryLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
