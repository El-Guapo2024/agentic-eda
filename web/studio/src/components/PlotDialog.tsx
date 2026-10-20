// dialog_plot.cpp ("Plot", File > Fabrication Outputs > Gerbers...): a
// layer checklist, a few format options, and a Plot button -- the real
// dialog's drill/vias/refdes/colour options aren't ported (nothing here
// yet reads them back), so this keeps the two that actually change the
// output: X2 format and whether a board-edge check should block the plot.
//
// POSTs straight to /api/fab/gerbers (crates/cli/src/fab_api.rs), which
// runs `kicad-cli pcb export gerbers --layers ...` on the exported board and
// writes into this board's own export/kicad/gerbers/ folder.
import { useEffect, useRef, useState } from "react";
import { postFabGerbers } from "../api/client";
import { useStudioDispatch, useStudioState } from "../state/store";
import { enabledOptionalLayers, layerDisplayName } from "../kicad-port/boardLayers";

/** The default checklist: every copper layer (board.layers), both mask/
 * paste/silk layers and the board outline -- the layers a JLCPCB order
 * actually needs (crates/cli/src/fab_api.rs `default_gerber_layers`) -- of
 * those the board has (Board Setup > Board Editor Layers). */
function defaultLayers(copperLayers: string[], optional: readonly string[]): string[] {
  return [...copperLayers, ...["F.Mask", "B.Mask", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "Edge.Cuts"].filter((l) => optional.includes(l))];
}

export function PlotDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const copperLayers = state.board?.layers ?? ["F.Cu", "B.Cu"];
  const layerSetup = state.board?.board_rules?.layer_setup;
  const optional = enabledOptionalLayers(layerSetup);
  const [checked, setChecked] = useState<Set<string>>(() => new Set(defaultLayers(copperLayers, optional)));
  // Board Setup > Physical Stackup changes the number of copper layers while this dialog's state lives on: a copper layer the board gained
  // starts checked (the default is every layer the board has) and one it lost is dropped, whatever else was ticked or cleared stays.
  const knownCopper = useRef(copperLayers);
  const copperKey = copperLayers.join(",");
  useEffect(() => {
    const before = knownCopper.current;
    knownCopper.current = copperLayers;
    setChecked((prev) => {
      const next = new Set(prev);
      for (const l of copperLayers) if (!before.includes(l)) next.add(l);
      for (const l of before) if (!copperLayers.includes(l)) next.delete(l);
      return next;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [copperKey]);
  // The same for the optional layers: one the board turned off is dropped, one it turned on that a plot starts with is checked.
  const knownOptional = useRef(optional);
  const optionalKey = optional.join(",");
  useEffect(() => {
    const before = knownOptional.current;
    knownOptional.current = optional;
    const starts = defaultLayers([], optional);
    setChecked((prev) => {
      const next = new Set(prev);
      for (const l of starts) if (!before.includes(l)) next.add(l);
      for (const l of before) if (!optional.includes(l)) next.delete(l);
      return next;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [optionalKey]);
  const [useAuxOrigin, setUseAuxOrigin] = useState(false);
  // `DIALOG_PLOT::m_zoneFillCheck`: on by default in KiCad.
  const [checkZones, setCheckZones] = useState(true);
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
      const reply = await postFabGerbers([...checked], useAuxOrigin, checkZones);
      setResult({ ok: reply.ok, message: reply.message ?? (reply.ok ? "Plotted." : "Plot failed."), files: reply.files ?? [] });
    } finally {
      setBusy(false);
    }
  };

  const allLayers = [...copperLayers, ...optional];

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
                {layerDisplayName(layerSetup, layer)}
              </label>
            ))}
          </div>
          <label className="toggle" style={{ marginBottom: 10, display: "block" }} title="Run KiCad's zone filler first if some zone has no current fill (never filled, unfilled, or edited since)">
            <input type="checkbox" checked={checkZones} onChange={(e) => setCheckZones(e.target.checked)} data-testid="plot-check-zones" />
            Check zone fills before plotting
          </label>
          <label className="toggle" style={{ marginBottom: 10 }} title="Use the drill/place file origin as the coordinate origin for plotted files">
            <input type="checkbox" checked={useAuxOrigin} onChange={(e) => setUseAuxOrigin(e.target.checked)} />
            Use drill/place file origin
          </label>
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
