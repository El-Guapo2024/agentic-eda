// dialog_gen_footprint_position.cpp ("Generate Placement Files", File >
// Fabrication Outputs > Component Placement...): format, side and unit
// radios plus the SMD-only / exclude-through-hole filters. Backed by
// eda_fab::position -- a port of KiCad's own PLACE_FILE_EXPORTER, not the
// JLCPCB-template CPL this app already writes for `eda ... --fab`
// (crates/fab/src/lib.rs's cpl_csv) -- see position.rs's own doc comment
// for why those are two separate writers.
import { useState } from "react";
import { postFabPos, type FabPosOptions } from "../api/client";
import { useStudioDispatch, useStudioState } from "../state/store";

export function FootprintPositionDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const [format, setFormat] = useState<FabPosOptions["format"]>("ascii");
  const [side, setSide] = useState<FabPosOptions["side"]>("both");
  const [unitsMm, setUnitsMm] = useState(true);
  const [smdOnly, setSmdOnly] = useState(false);
  const [excludeFpTh, setExcludeFpTh] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; files: string[] } | null>(null);

  if (!state.footprintPositionDialogOpen) return null;
  const close = () => dispatch({ type: "SET_FOOTPRINT_POSITION_DIALOG_OPEN", open: false });

  const generate = async () => {
    setBusy(true);
    setResult(null);
    try {
      const reply = await postFabPos({ format, side, units_mm: unitsMm, smd_only: smdOnly, exclude_fp_th: excludeFpTh });
      setResult({ ok: reply.ok, message: reply.message ?? (reply.ok ? "Position file written." : "Failed."), files: reply.files ?? [] });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 440 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">Generate Placement Files</div>
        <div className="dialog-body" style={{ paddingTop: 10, display: "flex", flexDirection: "column", gap: 12 }}>
          <div>
            <div style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginBottom: 6 }}>Format</div>
            <label className="toggle">
              <input type="radio" name="pos-format" checked={format === "ascii"} onChange={() => setFormat("ascii")} />
              ASCII
            </label>
            <label className="toggle">
              <input type="radio" name="pos-format" checked={format === "csv"} onChange={() => setFormat("csv")} />
              CSV
            </label>
          </div>
          <div>
            <div style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginBottom: 6 }}>Side</div>
            {(["front", "back", "both"] as const).map((s) => (
              <label key={s} className="toggle">
                <input type="radio" name="pos-side" checked={side === s} onChange={() => setSide(s)} />
                {s === "front" ? "Front" : s === "back" ? "Back" : "Both sides"}
              </label>
            ))}
          </div>
          <div>
            <div style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginBottom: 6 }}>Units</div>
            <label className="toggle">
              <input type="radio" name="pos-units" checked={unitsMm} onChange={() => setUnitsMm(true)} />
              Millimeters
            </label>
            <label className="toggle">
              <input type="radio" name="pos-units" checked={!unitsMm} onChange={() => setUnitsMm(false)} />
              Inches
            </label>
          </div>
          <div>
            <label className="toggle">
              <input type="checkbox" checked={smdOnly} onChange={(e) => setSmdOnly(e.target.checked)} />
              Include only SMD footprints
            </label>
            <label className="toggle">
              <input type="checkbox" checked={excludeFpTh} onChange={(e) => setExcludeFpTh(e.target.checked)} />
              Exclude all footprints with through-hole pads
            </label>
          </div>
          <div style={{ fontSize: 11, color: "var(--chrome-text-dim)" }}>
            Output directory: <code>export/</code>. KiCad's own column layout (<code>Ref,Val,Package,PosX,PosY,Rot,Side</code>), checked byte-for-byte against
            `kicad-cli pcb export pos` (PARITY.md).
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
            {busy ? "Generating…" : "Generate Position File"}
          </button>
          <button onClick={close}>Close</button>
        </div>
      </div>
    </div>
  );
}
