// dialog_gendrill.cpp ("Generate Drill Files", File > Fabrication Outputs
// > Drill Files...). The real dialog's map/report/units/zeros-format
// options aren't all wired here yet -- eda_fab::drill (KiCad's own
// EXCELLON_WRITER, ported) always writes millimetres in KiCad's "decimal"
// zeros format, its own default and the one `eda fab drill`/kicad-cli
// comparator both run against; the one option that changes the *file
// set* -- merged vs. separate PTH/NPTH files -- is exposed.
import { useState } from "react";
import { postFabDrill } from "../api/client";
import { useStudioDispatch, useStudioState } from "../state/store";

export function GenerateDrillDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const [separateTh, setSeparateTh] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; files: string[] } | null>(null);

  if (!state.generateDrillDialogOpen) return null;
  const close = () => dispatch({ type: "SET_GENERATE_DRILL_DIALOG_OPEN", open: false });

  const generate = async () => {
    setBusy(true);
    setResult(null);
    try {
      const reply = await postFabDrill(separateTh);
      setResult({ ok: reply.ok, message: reply.message ?? (reply.ok ? "Drill file written." : "Failed."), files: reply.files ?? [] });
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
            <label className="toggle">
              <input type="checkbox" checked={separateTh} onChange={(e) => setSeparateTh(e.target.checked)} />
              Generate independent files for NPTH and PTH holes
            </label>
          </div>
          <div style={{ fontSize: 11, color: "var(--chrome-text-dim)", marginBottom: 10 }}>
            Output directory: <code>export/</code>. Plated and non-plated holes merge into one <code>.drl</code> file unless separated above -- same default as
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
