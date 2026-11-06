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
// choice). "Output directory:" is the dialog's own (`m_outputDirectoryName`,
// kept between openings as `DIALOG_SHIM` keeps it), relative to the board's
// folder; empty is `export/kicad/pos/`. Shown disabled with the reason:
// "Exclude all footprints with the Exclude from BOM flag set" (kicad-cli
// 10.99's `pcb export pos` still has no flag for it; only the api-server's
// position job has `exclude_from_bom`) and the folder browser. "Design variant:"
// (KiCad 25ec2335 `m_variantChoiceCtrl`) starts at the board's current variant
// (`GetBoard()->GetCurrentVariant()`) and goes to kicad-cli as `--variant`
// (`exporter.SetVariant( getSelectedVariant() )`).
import { useEffect, useState } from "react";
import { postFabPos, type FabPosOptions } from "../api/client";
import { withWarnings } from "../kicad-port/schOutputs";
import { useStudioDispatch, useStudioState } from "../state/store";
import { VariantChoice } from "./VariantChoice";

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
  const [outputDirectory, setOutputDirectory] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; message: string; files: string[] } | null>(null);
  const [variant, setVariant] = useState("");
  const variants = state.board?.variants ?? state.schematic?.variants ?? [];
  // the board's current variant at each opening (`m_variantChoiceCtrl->SetSelection( FindString( currentVariant ) )`)
  useEffect(() => {
    if (state.footprintPositionDialogOpen) setVariant(state.pcbVariant);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.footprintPositionDialogOpen]);

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
        output_directory: outputDirectory,
        ...(variant ? { variant } : {}),
      });
      setResult({ ok: reply.ok, message: reply.ok ? withWarnings(reply, reply.message ?? "Position file written.") : (reply.message ?? "Failed."), files: reply.files ?? [] });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 440 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">Generate Placement Files</div>
        <div className="dialog-body" style={{ paddingTop: 10, display: "flex", flexDirection: "column", gap: 12 }}>
          <VariantChoice variants={variants} value={variant} onChange={setVariant} testId="pos-variant" />
          <label style={{ display: "flex", alignItems: "center", gap: 6 }} title="Target directory for plot files. Can be absolute or relative to the board file location.">
            Output directory:
            <input type="text" value={outputDirectory} placeholder="export/kicad/pos/" onChange={(e) => setOutputDirectory(e.target.value)} style={{ flex: 1 }} data-testid="pos-output-dir" />
            <button disabled title="Not available: the studio runs in a browser: type the folder, relative to the board's folder">…</button>
          </label>
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
            <label className="toggle" title="Not available: kicad-cli pcb export pos has no flag for it" style={{ opacity: 0.5 }}>
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
            Files are named as KiCad names them: <code>-all-pos</code> for one file, <code>-top-pos</code> and{" "}
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
