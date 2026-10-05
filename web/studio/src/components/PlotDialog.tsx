// dialog_plot.cpp ("Plot", File > Fabrication Outputs > Gerbers...): a
// layer checklist, a few format options, and a Plot button -- the real
// dialog's drill/vias/refdes/colour options aren't ported (nothing here
// yet reads them back), so this keeps the two that actually change the
// output: X2 format and whether a board-edge check should block the plot.
//
// POSTs straight to /api/fab/gerbers (crates/cli/src/fab_api.rs), which
// runs `kicad-cli pcb export gerbers --layers ...` on the exported board and
// writes into this board's own export/kicad/gerbers/ folder.
import { useState } from "react";
import { postFabGerbers } from "../api/client";
import { useStudioDispatch, useStudioState } from "../state/store";

/** The default checklist: every copper layer (board.layers), both mask/
 * paste/silk layers and the board outline -- the layers a JLCPCB order
 * actually needs (crates/cli/src/fab_api.rs `default_gerber_layers`). */
function defaultLayers(copperLayers: string[]): string[] {
  return [...copperLayers, "F.Mask", "B.Mask", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "Edge.Cuts"];
}

export function PlotDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const copperLayers = state.board?.layers ?? ["F.Cu", "B.Cu"];
  const [checked, setChecked] = useState<Set<string>>(() => new Set(defaultLayers(copperLayers)));
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; files: string[] } | null>(null);

  if (!state.plotDialogOpen) return null;
  const close = () => dispatch({ type: "SET_PLOT_DIALOG_OPEN", open: false });

  const toggle = (layer: string) =>
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(layer)) next.delete(layer);
      else next.add(layer);
      return next;
    });

  const plot = async () => {
    setBusy(true);
    setResult(null);
    try {
      const reply = await postFabGerbers([...checked]);
      setResult({ ok: reply.ok, message: reply.message ?? (reply.ok ? "Plotted." : "Plot failed."), files: reply.files ?? [] });
    } finally {
      setBusy(false);
    }
  };

  const allLayers = [...defaultLayers(copperLayers)];

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 460 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">Plot</div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          <div style={{ marginBottom: 8, color: "var(--chrome-text-dim)", fontSize: 11 }}>Include Layers</div>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 4, marginBottom: 12 }}>
            {allLayers.map((layer) => (
              <label key={layer} className="toggle">
                <input type="checkbox" checked={checked.has(layer)} onChange={() => toggle(layer)} />
                {layer}
              </label>
            ))}
          </div>
          <div style={{ fontSize: 11, color: "var(--chrome-text-dim)", marginBottom: 10 }}>
            Output directory: <code>export/kicad/gerbers/</code> (inside this board's own directory). Written by `kicad-cli pcb export gerbers`
            (Gerber X2, with its .gbrjob job file).
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
          <button className="primary" onClick={plot} disabled={busy || checked.size === 0}>
            {busy ? "Plotting…" : "Plot"}
          </button>
          <button onClick={close}>Close</button>
        </div>
      </div>
    </div>
  );
}
