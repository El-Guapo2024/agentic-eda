// Port of pcbnew/router/pns_meander_placer.cpp's "tune a track's length"
// operation (`7`, pcbnew.LengthTuner.TuneSingleTrack), as a dialog rather
// than upstream's own live mouse-driven interactive session -- see
// crates/pns/src/meander.rs's own header comment for why (a third
// interactive session type, after route and drag, was out of proportion
// to this task's remaining time). Pick a single straight track, type a
// target length, and the backend computes a meandered replacement; the
// length readout here (current/achieved) is this app's stand-in for
// upstream's own status-bar tuning readout.
//
// Not yet ported: diff-pair length tuning (`8`) and skew tuning (`9`) --
// see crates/pns/PARITY.md's own tracking note for gap #7 task item 4.
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { tuneLengthApply, tuneLengthPreview } from "../api/client";
import { formatLength, umFrom, umTo } from "../state/units";

export function LengthTuningDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.lengthTuningDialogOpen;

  const trackId = [...state.selection][0];
  const track = trackId ? api.trackById(trackId) : undefined;

  const [targetLength, setTargetLength] = useState(0); // display units
  // Amplitude/spacing are the persistent MEANDER_SETTINGS (state.pcbx.lengthTuner) -- the same
  // values `pcbnew.lengthTuner.AmplIncrease/Decrease/SpacingIncrease/Decrease` ("4"/"3"/"2"/"1")
  // step while this dialog is open. Typed values are kept locally (so "0." survives a keystroke)
  // and mirrored into the store; an outside change (a hotkey step) flows back into the field.
  const tuner = state.pcbx.lengthTuner;
  const [amplitude, setAmplitudeLocal] = useState(umTo(tuner.amplitudeUm, state.units)); // display units
  const [spacing, setSpacingLocal] = useState(umTo(tuner.spacingUm, state.units)); // display units
  const setAmplitude = (v: number) => {
    setAmplitudeLocal(v);
    dispatch({ type: "PCBX", patch: { lengthTuner: { ...tuner, amplitudeUm: Math.max(1, Math.round(umFrom(v, state.units))) } } });
  };
  const setSpacing = (v: number) => {
    setSpacingLocal(v);
    dispatch({ type: "PCBX", patch: { lengthTuner: { ...tuner, spacingUm: Math.max(1, Math.round(umFrom(v, state.units))) } } });
  };
  useEffect(() => {
    if (Math.max(1, Math.round(umFrom(amplitude, state.units))) !== tuner.amplitudeUm) setAmplitudeLocal(umTo(tuner.amplitudeUm, state.units));
    if (Math.max(1, Math.round(umFrom(spacing, state.units))) !== tuner.spacingUm) setSpacingLocal(umTo(tuner.spacingUm, state.units));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tuner.amplitudeUm, tuner.spacingUm, state.units]);
  const [flip, setFlip] = useState(false);
  const [preview, setPreview] = useState<{ achieved?: number; colliding?: boolean; message?: string } | null>(null);

  const originalLengthUm = track ? Math.hypot(track.pts[1]![0] - track.pts[0]![0], track.pts[1]![1] - track.pts[0]![1]) : 0;

  useEffect(() => {
    if (!open || !track) return;
    setTargetLength(Math.round((originalLengthUm * 1.2) / umFrom(1, state.units)) || 0);
    setFlip(false);
    setPreview(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, trackId]);

  useEffect(() => {
    if (!open || !track) return;
    const targetUm = Math.round(umFrom(targetLength, state.units));
    const amplitudeUm = Math.max(1, Math.round(umFrom(amplitude, state.units)));
    const spacingUm = Math.max(1, Math.round(umFrom(spacing, state.units)));
    let cancelled = false;
    const timer = setTimeout(() => {
      tuneLengthPreview({ trackId: track.id, amplitude: amplitudeUm, spacing: spacingUm, targetLength: targetUm, flip }).then((reply) => {
        if (cancelled) return;
        if (!reply.ok) {
          setPreview({ message: reply.message });
          return;
        }
        setPreview({ achieved: reply.achieved_length, colliding: reply.colliding });
      });
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [open, track, targetLength, amplitude, spacing, flip, state.units]);

  if (!open) return null;

  const close = () => dispatch({ type: "SET_LENGTH_TUNING_DIALOG_OPEN", open: false });

  const apply = async () => {
    if (!track) return;
    const targetUm = Math.round(umFrom(targetLength, state.units));
    const amplitudeUm = Math.max(1, Math.round(umFrom(amplitude, state.units)));
    const spacingUm = Math.max(1, Math.round(umFrom(spacing, state.units)));
    const reply = await tuneLengthApply({ trackId: track.id, amplitude: amplitudeUm, spacing: spacingUm, targetLength: targetUm, flip });
    if (!reply.ok) {
      dispatch({ type: "TOAST", message: reply.message ?? "Could not tune this track's length.", kind: "error" });
      return;
    }
    close();
    await api.refresh();
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 380 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Tune Length of a Single Track...</span>
        </div>
        <div className="dialog-body">
          {!track ? (
            <div style={{ fontSize: 12, opacity: 0.8 }}>Select a single straight track segment first.</div>
          ) : (
            <>
              <div style={{ fontSize: 11, opacity: 0.7, marginBottom: 8 }}>
                Current length: {formatLength(originalLengthUm, state.units)} · net {track.net}
              </div>
              <div className="kv-grid" style={{ gridTemplateColumns: "110px 1fr" }}>
                <span>Target length:</span>
                <input autoFocus type="number" value={targetLength} onChange={(e) => setTargetLength(Number(e.target.value))} />
                <span>Amplitude:</span>
                <input type="number" value={amplitude} onChange={(e) => setAmplitude(Number(e.target.value))} />
                <span>Spacing:</span>
                <input type="number" value={spacing} onChange={(e) => setSpacing(Number(e.target.value))} />
              </div>
              <label className="filter-row" style={{ marginTop: 6 }}>
                <input type="checkbox" checked={flip} onChange={(e) => setFlip(e.target.checked)} />
                Flip side
              </label>
              <div style={{ fontSize: 11, marginTop: 8, minHeight: 16 }}>
                {preview?.message && <span style={{ color: "#ff6666" }}>{preview.message}</span>}
                {preview?.achieved != null && (
                  <span style={{ color: preview.colliding ? "#ff6666" : undefined }}>
                    Achieved length: {formatLength(preview.achieved, state.units)}
                    {preview.colliding ? " -- collides with the board, adjust and retry" : ""}
                  </span>
                )}
              </div>
            </>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={apply} disabled={!track || !preview?.achieved || preview.colliding}>
            Apply
          </button>
        </div>
      </div>
    </div>
  );
}
