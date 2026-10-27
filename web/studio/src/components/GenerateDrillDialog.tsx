// dialog_gendrill.cpp ("Generate Drill Files", File > Fabrication Outputs
// > Drill Files...). The real dialog's map/report/units/zeros-format
// options aren't all wired here yet -- the dialog POSTs to /api/fab/drill
// (crates/cli/src/fab_api.rs), which runs `kicad-cli pcb export drill`
// (Excellon, millimetres, "decimal" zeros). Wired, with KiCad's labels and
// defaults (`dialog_gendrill_base.cpp`): "PTH and NPTH in single file"
// (`m_Check_Merge_PTH_NPTH`, unchecked: two files), "Use alternate drill mode
// for oval holes" (`m_altDrillMode`, unchecked: slots are routed,
// `SetRouteModeForOvalHoles( !m_altDrillMode )`), and the Origin choice, which
// starts as the board's plot settings say (`m_plotOpts.GetUseAuxOrigin()`).
import { useState } from "react";
import { postFabDrill } from "../api/client";
import { useStudioDispatch, useStudioState } from "../state/store";

export function GenerateDrillDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const [mergePthNpth, setMergePthNpth] = useState(false);
  const [altDrillMode, setAltDrillMode] = useState(false);
  const [useAuxOrigin, setUseAuxOrigin] = useState(() => state.board?.plot_params?.use_aux_origin ?? false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; files: string[] } | null>(null);

  if (!state.generateDrillDialogOpen) return null;
  const close = () => dispatch({ type: "SET_GENERATE_DRILL_DIALOG_OPEN", open: false });

  const generate = async () => {
    setBusy(true);
    setResult(null);
    try {
      const reply = await postFabDrill({ merge_pth_npth: mergePthNpth, alt_drill_mode: altDrillMode, use_aux_origin: useAuxOrigin });
      setResult({ ok: reply.ok, message: reply.message ?? (reply.ok ? "Done." : "Failed."), files: reply.files ?? [] });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">Generate Drill Files</div>
        <div className="dialog-body" style={{ paddingTop: 10 }}>
          <div style={{ marginBottom: 10 }}>
            <div style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginBottom: 6 }}>Drill File Format: Excellon, metric, decimal format</div>
            <label className="toggle" title={"Not recommended.\nOnly use for board houses which ask for merged PTH and NPTH into a single file."}>
              <input type="checkbox" checked={mergePthNpth} onChange={(e) => setMergePthNpth(e.target.checked)} />
              PTH and NPTH in single file
            </label>
            <label className="toggle" style={{ display: "block", marginTop: 4 }}>
              <input type="checkbox" checked={altDrillMode} onChange={(e) => setAltDrillMode(e.target.checked)} />
              Use alternate drill mode for oval holes
            </label>
            {/* m_origin: "Absolute" or "Drill/place file origin" (pcbnew.EditorControl.drillOrigin). */}
            <label style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 8 }}>
              Origin:
              <select value={useAuxOrigin ? "aux" : "abs"} onChange={(e) => setUseAuxOrigin(e.target.value === "aux")}>
                <option value="abs">Absolute</option>
                <option value="aux">Drill/place file origin</option>
              </select>
            </label>
          </div>
          <div style={{ fontSize: 11, color: "var(--chrome-text-dim)", marginBottom: 10 }}>
            Output directory: <code>export/kicad/drill/</code>. Plated and non-plated holes go into two files (<code>-PTH.drl</code>, <code>-NPTH.drl</code>) unless merged above -- written by
            `kicad-cli pcb export drill`.
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
          <button className="primary" onClick={generate} disabled={busy}>
            {busy ? "Generating…" : "Generate Drill File"}
          </button>
          <button onClick={close}>Close</button>
        </div>
      </div>
    </div>
  );
}
