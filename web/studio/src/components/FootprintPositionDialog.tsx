// dialog_gen_footprint_position.cpp ("Generate Placement Files", File >
// Fabrication Outputs > Component Placement...), with the controls and
// defaults of `dialog_gen_footprint_position_file_base.cpp`: Format (Plain
// text, CSV, Gerber X3), Units (Inches, Millimeters), the footprint filters,
// "Use drill/place file origin", "Use negative X coordinates for footprints on
// bottom layer", "Generate single file with both front and back positions"
// and, for Gerber X3, "Include board edge layer"; the `onUpdateUI*` handlers'
// greying for Gerber X3. POSTs to /api/fab/pos (crates/cli/src/fab_api.rs),
// which runs `kicad-cli pcb export pos` -- KiCad's own PLACE_FILE_EXPORTER --
// with KiCad's file names (`PLACE_FILE_EXPORTER::DecorateFilename`,
// `PLACEFILE_GERBER_WRITER::GetPlaceFileName`) and reports as
// `CreateAsciiFiles` does ("No footprint for automated placement." when no
// footprint qualifies). Both sides always, as KiCad's dialog (it has no side
// choice). Not offered: "Exclude all footprints with the Exclude from BOM flag
// set" (kicad-cli has no flag for it; shown disabled) and the design variant.
import { useState } from "react";
import { postFabPos, type FabPosOptions } from "../api/client";
import { useStudioDispatch, useStudioState } from "../state/store";

export function FootprintPositionDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const [format, setFormat] = useState<FabPosOptions["format"]>("ascii");
  // m_singleFile starts checked in KiCad's dialog: both sides in one `<name>-all-pos` file.
  const [singleFile, setSingleFile] = useState(true);
  const [unitsMm, setUnitsMm] = useState(true);
  const [smdOnly, setSmdOnly] = useState(false);
  const [excludeFpTh, setExcludeFpTh] = useState(false);
  const [excludeDnp, setExcludeDnp] = useState(false);
  const [negateX, setNegateX] = useState(false);
  const [boardEdge, setBoardEdge] = useState(false);
  // m_useDrillPlaceOrigin starts checked in KiCad's dialog.
  const [useAuxOrigin, setUseAuxOrigin] = useState(true);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; files: string[] } | null>(null);

  if (!state.footprintPositionDialogOpen) return null;
  // Gerber X3 (`m_formatCtrl` choice 2): units, the single file and the filters do not apply (`onUpdateUIUnits`, `onUpdateUIFileOpt`, `onUpdateUIOnlySMD`, ...).
  const gerber = format === "gerber";
  const close = () => dispatch({ type: "SET_FOOTPRINT_POSITION_DIALOG_OPEN", open: false });

  const generate = async () => {
    setBusy(true);
    setResult(null);
    try {
      const reply = await postFabPos({
        format,
        side: "both",
        units_mm: unitsMm,
        smd_only: !gerber && smdOnly,
        exclude_fp_th: !gerber && excludeFpTh,
        exclude_dnp: !gerber && excludeDnp,
        use_aux_origin: useAuxOrigin,
        single_file: singleFile,
        negate_x: !gerber && negateX,
        board_edge: gerber && boardEdge,
      });
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
            <div style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginBottom: 6 }}>Format:</div>
            {(
              [
                ["ascii", "Plain text"],
                ["csv", "CSV"],
                ["gerber", "Gerber X3"],
              ] as const
            ).map(([f, label]) => (
              <label key={f} className="toggle">
                <input type="radio" name="pos-format" checked={format === f} onChange={() => setFormat(f)} />
                {label}
              </label>
            ))}
          </div>
          <div style={{ opacity: gerber ? 0.5 : 1 }}>
            <div style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginBottom: 6 }}>Units:</div>
            <label className="toggle">
              <input type="radio" name="pos-units" disabled={gerber} checked={!unitsMm} onChange={() => setUnitsMm(false)} />
              Inches
            </label>
            <label className="toggle">
              <input type="radio" name="pos-units" disabled={gerber} checked={unitsMm} onChange={() => setUnitsMm(true)} />
              Millimeters
            </label>
          </div>
          <div>
            <label className="toggle">
              <input type="checkbox" disabled={gerber} checked={!gerber && smdOnly} onChange={(e) => setSmdOnly(e.target.checked)} />
              Include only SMD footprints
            </label>
            <label className="toggle">
              <input type="checkbox" disabled={gerber} checked={!gerber && excludeFpTh} onChange={(e) => setExcludeFpTh(e.target.checked)} />
              Exclude all footprints with through hole pads
            </label>
            <label className="toggle">
              <input type="checkbox" disabled={gerber} checked={!gerber && excludeDnp} onChange={(e) => setExcludeDnp(e.target.checked)} data-testid="pos-exclude-dnp" />
              Exclude all footprints with the Do Not Populate flag set
            </label>
            <label className="toggle" title="Not available: kicad-cli has no option for it" style={{ opacity: 0.5 }}>
              <input type="checkbox" disabled checked={false} readOnly />
              Exclude all footprints with the Exclude from BOM flag set
            </label>
            <label className="toggle" style={{ opacity: gerber ? 1 : 0.5 }}>
              <input type="checkbox" disabled={!gerber} checked={gerber && boardEdge} onChange={(e) => setBoardEdge(e.target.checked)} />
              Include board edge layer
            </label>
            <label className="toggle">
              <input type="checkbox" checked={useAuxOrigin} onChange={(e) => setUseAuxOrigin(e.target.checked)} />
              Use drill/place file origin
            </label>
            <label className="toggle">
              <input type="checkbox" disabled={gerber} checked={!gerber && negateX} onChange={(e) => setNegateX(e.target.checked)} />
              Use negative X coordinates for footprints on bottom layer
            </label>
            <label className="toggle" title="Both sides in one -all-pos file; unchecked, KiCad's -top-pos and -bottom-pos files">
              <input type="checkbox" checked={singleFile} disabled={gerber} onChange={(e) => setSingleFile(e.target.checked)} data-testid="pos-single-file" />
              Generate single file with both front and back positions
            </label>
          </div>
          <div style={{ fontSize: 11, color: "var(--chrome-text-dim)" }}>
            Output directory: <code>export/kicad/pos/</code>, named as KiCad names it: <code>-all-pos</code> for one file, <code>-top-pos</code> and{" "}
            <code>-bottom-pos</code> for two, <code>-pnp_top.gbr</code> and <code>-pnp_bottom.gbr</code> for Gerber X3. Written by <code>kicad-cli pcb export pos</code>, so it is KiCad's own file.
          </div>
          {result && (
            <div className={result.ok ? "panel-empty" : "problem-row"} style={{ fontSize: 11 }}>
              <b style={{ whiteSpace: "pre-line" }}>{result.message}</b>
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
