// Port of pcbnew/dialogs/dialog_cleanup_tracks_and_vias{,_base}.cpp --
// "Cleanup Tracks & Vias..." (pcbnew.GlobalEdit.cleanupTracksAndVias,
// task item 1). Two-step flow, matching source exactly: the primary
// button reads "Build Changes" the first time (a dry run -- computes and
// lists what would change, touches nothing) and "Update PCB" once a
// build has run with the current checkbox state (commits it); ticking
// any checkbox after a build resets back to "Build Changes"
// (`DIALOG_CLEANUP_TRACKS_AND_VIAS::OnCheckBox`).
//
// Not ported from the real dialog: the net/netclass/layer/"selected
// items only" filters (`m_netFilterOpt`/`m_netclassFilterOpt`/
// `m_layerFilterOpt`/`m_selectedItemsFilter`) and "Refill zones before
// and after cleanup" (`m_cbRefillZones`) -- this app's cleanup always
// scans the whole board and never touches zone fills itself (the
// existing Fill All Zones action already covers that separately). See
// PARITY-pcb.md for the full list of adaptations in the Rust port this
// dialog drives (`crates/connectivity/src/cleanup.rs`).
import { useState } from "react";
import { useStudioDispatch, useStudioState } from "../state/store";
import { cleanupTracksApply, cleanupTracksPreview } from "../api/client";
import type { CleanupChange, CleanupOptions } from "../api/types";

const DEFAULT_OPTIONS: CleanupOptions = {
  delete_shorting: false,
  delete_redundant_vias: false,
  delete_dangling_vias: false,
  merge_segments: false,
  delete_dangling_tracks: false,
  delete_tracks_in_pads: false,
};

const CHECKBOXES: { key: keyof CleanupOptions; label: string; tooltip: string }[] = [
  { key: "delete_shorting", label: "Delete tracks connecting different nets", tooltip: "remove track/via segments touching a pad, track or via of a different net (short circuit)" },
  { key: "delete_redundant_vias", label: "Delete redundant vias", tooltip: "remove vias on through-hole pads and superimposed vias" },
  { key: "delete_dangling_vias", label: "Delete vias connected on only one layer", tooltip: "a via that never actually bridges two copper layers" },
  { key: "merge_segments", label: "Merge co-linear tracks", tooltip: "merge aligned track segments, and remove null segments" },
  { key: "delete_dangling_tracks", label: "Delete tracks unconnected at one end", tooltip: "delete tracks having at least one dangling end" },
  { key: "delete_tracks_in_pads", label: "Delete tracks fully inside pads", tooltip: "delete tracks that have both start and end positions inside of a pad" },
];

export function CleanupTracksDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const open = state.cleanupTracksDialogOpen;

  const [opts, setOpts] = useState<CleanupOptions>(DEFAULT_OPTIONS);
  const [built, setBuilt] = useState(false);
  const [busy, setBusy] = useState(false);
  const [changes, setChanges] = useState<CleanupChange[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  const reset = () => {
    setOpts(DEFAULT_OPTIONS);
    setBuilt(false);
    setChanges(null);
    setError(null);
  };

  const close = () => {
    reset();
    dispatch({ type: "SET_CLEANUP_TRACKS_DIALOG_OPEN", open: false });
  };

  const toggle = (key: keyof CleanupOptions) => {
    setOpts((o) => ({ ...o, [key]: !o[key] }));
    setBuilt(false); // OnCheckBox: any option change forces a fresh "Build Changes"
  };

  const anyOptionOn = Object.values(opts).some(Boolean);

  const build = async () => {
    setBusy(true);
    setError(null);
    try {
      const reply = await cleanupTracksPreview(opts);
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
      const reply = await cleanupTracksApply(opts);
      if (!reply.ok) {
        setError(reply.message ?? "could not apply the cleanup");
        return;
      }
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
          {CHECKBOXES.map(({ key, label, tooltip }) => (
            <label key={key} className="filter-row" title={tooltip} style={{ display: "block", marginBottom: 4 }}>
              <input type="checkbox" checked={opts[key]} onChange={() => toggle(key)} />
              {label}
            </label>
          ))}
          <div style={{ fontSize: 11, marginTop: 10, minHeight: 90, maxHeight: 220, overflowY: "auto", border: "1px solid var(--chrome-border, #333)", borderRadius: 4, padding: 6 }}>
            {error && <div style={{ color: "#ff6666" }}>{error}</div>}
            {!error && !built && <div style={{ opacity: 0.7 }}>Pick the cleanups to run, then Build Changes to preview them -- nothing is touched until Update PCB.</div>}
            {!error && built && changes && changes.length === 0 && <div style={{ opacity: 0.7 }}>Nothing to clean up with the current options.</div>}
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
