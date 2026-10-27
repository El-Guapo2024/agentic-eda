// dialog_plot.cpp ("Plot", File > Fabrication Outputs > Gerbers...): the
// layer checklist, "Check zone fills before plotting", "Use drill/place file
// origin" and a Plot button. The real dialog's other controls (format, drill
// marks, scale, the Gerber options) are not ported; the Gerber options are
// plotted as the board's plot settings say.
//
// POSTs straight to /api/fab/gerbers (crates/cli/src/fab_api.rs), which
// runs `kicad-cli pcb export gerbers --layers ...` on the exported board with
// the board's own plot settings (`(setup (pcbplotparams ..))`: Protel names or
// not, precision, X2, subtract mask from silk, the DNP fab options -- what
// KiCad's dialog plots with) and writes into this board's own
// export/kicad/gerbers/ folder. The checklist and the origin box start from
// the board's plot settings each time the dialog opens (`DIALOG_PLOT::init_Dialog`,
// run by each construction), and Plot stores them back in the board first
// (`applyPlotSettings`: `SetPlotSettings( tempOptions ); OnModify()` when they
// changed), as `set_plot_settings`, which is no undo step (KiCad has no commit
// there; round 4 X4-3); the plot then takes the
// origin from the board.
import { useEffect, useRef, useState } from "react";
import { postFabGerbers } from "../api/client";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
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
  const api = useStudioApi();
  // `DIALOG_PLOT::init_Dialog`: the checklist is the board's plot settings' layer selection (`GetLayerSelection()`) of the layers it lists
  // (`m_layerList`, the board's enabled layers), KiCad's default for a board that has none.
  const fromBoard = (): Set<string> => {
    const listed = [...copperLayers, ...optional];
    return new Set((state.board?.plot_params?.layers ?? defaultLayers(copperLayers, optional)).filter((l) => listed.includes(l)));
  };
  const [checked, setChecked] = useState<Set<string>>(fromBoard);
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
  // "Use drill/place file origin" (`m_useAuxOriginCheckBox`) starts as the board's `useauxorigin`.
  const [useAuxOrigin, setUseAuxOrigin] = useState(() => state.board?.plot_params?.use_aux_origin ?? false);
  // KiCad builds the dialog each time it is opened, so both start from the board as it is then (the board loads after this component
  // is mounted, and its settings change between two plots).
  useEffect(() => {
    if (!state.plotDialogOpen) return;
    setChecked(fromBoard());
    setUseAuxOrigin(state.board?.plot_params?.use_aux_origin ?? false);
    setResult(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.plotDialogOpen]);
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
      // `applyPlotSettings` before the plot: the board keeps the choices (one undo step, only when they changed), and the plot takes the
      // origin from the board's settings.
      const before = fromBoard();
      const changed = before.size !== checked.size || [...checked].some((l) => !before.has(l)) || useAuxOrigin !== (state.board?.plot_params?.use_aux_origin ?? false);
      if (changed && !(await api.cmd({ op: "set_plot_settings", layers: [...checked], use_aux_origin: useAuxOrigin }))) return;
      const reply = await postFabGerbers([...checked], undefined, checkZones);
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
            with the board's plot settings, and a .gbrjob job file.
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
