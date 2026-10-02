// Port of pcbnew/router/pns_meander_placer.cpp's "tune a track's length"
// operation (`7`, pcbnew.LengthTuner.TuneSingleTrack) and of its two
// differential-pair siblings, `8` (pcbnew.LengthTuner.TuneDiffPair,
// pns_dp_meander_placer.cpp) and `9` (pcbnew.LengthTuner.TuneDiffPairSkew,
// pns_meander_skew_placer.cpp), as one dialog rather than upstream's own
// live mouse-driven interactive session -- see crates/pns/src/meander.rs's
// header comment for why (a third interactive session type, after route
// and drag, was out of proportion to this task's remaining time). Pick a
// single straight track, type a target, and the backend computes a
// meandered replacement; the length readout here (current/achieved) is
// this app's stand-in for upstream's own status-bar tuning readout.
//
//   7 single   one track lengthened to the target length
//   8 diffpair the track's whole pair meandered together (one baseline,
//              both lines offset by half the pitch) until the longer net
//              reaches the target length
//   9 skew     the selected line lengthened until its net is `target skew`
//              longer than the complementary net (0 = equal length)
//
// The pair/skew maths lives in crates/pns/src/dp_tune.rs; the dialog only
// asks `POST /api/tune_length/{preview,apply}` with the mode.
import { useEffect, useState } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { tuneLengthApply, tuneLengthPreview } from "../api/client";
import type { TuneLengthReply, TuneMode } from "../api/types";
import { formatLength, umFrom, umTo } from "../state/units";

const TITLES: Record<TuneMode, string> = {
  single: "Tune Length of a Single Track...",
  diffpair: "Tune Length of a Differential Pair...",
  skew: "Tune Skew of a Differential Pair...",
};

interface PreviewState {
  achieved?: number;
  colliding?: boolean;
  message?: string;
  reply?: TuneLengthReply;
}

export function LengthTuningDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.lengthTuningDialogOpen;
  const mode = state.lengthTuningMode;

  const trackId = [...state.selection][0];
  const track = trackId ? api.trackById(trackId) : undefined;

  const [targetLength, setTargetLength] = useState(0); // display units
  const [targetSkew, setTargetSkew] = useState(0); // display units (`9`)
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
  const [preview, setPreview] = useState<PreviewState | null>(null);

  const trackLengthUm = track ? Math.hypot(track.pts[1]![0] - track.pts[0]![0], track.pts[1]![1] - track.pts[0]![1]) : 0;

  // On open (or a different track / tuner): reset, and pick a default target -- 20% over the track for `7`; for `8` 10% over the pair's
  // current length, which only the backend can say (the pair spans both nets), so a throwaway preview with no target asks for it; `9`
  // starts at skew 0 ("match the partner").
  useEffect(() => {
    if (!open || !track) return;
    setFlip(false);
    setPreview(null);
    setTargetSkew(0);
    if (mode === "single") {
      setTargetLength(Math.round((trackLengthUm * 1.2) / umFrom(1, state.units)) || 0);
      return;
    }
    if (mode === "diffpair") {
      let cancelled = false;
      tuneLengthPreview({ trackId: track.id, amplitude: 1, spacing: 1, targetLength: 0, flip: false, mode }).then((reply) => {
        if (cancelled) return;
        if (reply.original_length) setTargetLength(Math.round((reply.original_length * 1.1) / umFrom(1, state.units)) || 0);
        setPreview({ message: reply.ok ? undefined : reply.message, reply });
      });
      return () => {
        cancelled = true;
      };
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, trackId, mode]);

  useEffect(() => {
    if (!open || !track) return;
    const targetUm = Math.round(umFrom(targetLength, state.units));
    const targetSkewUm = Math.round(umFrom(targetSkew, state.units));
    const amplitudeUm = Math.max(1, Math.round(umFrom(amplitude, state.units)));
    const spacingUm = Math.max(1, Math.round(umFrom(spacing, state.units)));
    let cancelled = false;
    const timer = setTimeout(() => {
      tuneLengthPreview({ trackId: track.id, amplitude: amplitudeUm, spacing: spacingUm, targetLength: targetUm, flip, mode, targetSkew: targetSkewUm }).then((reply) => {
        if (cancelled) return;
        if (!reply.ok) {
          setPreview({ message: reply.message, reply });
          return;
        }
        setPreview({ achieved: reply.achieved_length, colliding: reply.colliding, reply });
      });
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [open, track, targetLength, targetSkew, amplitude, spacing, flip, mode, state.units]);

  if (!open) return null;

  const close = () => dispatch({ type: "SET_LENGTH_TUNING_DIALOG_OPEN", open: false });

  const apply = async () => {
    if (!track) return;
    const targetUm = Math.round(umFrom(targetLength, state.units));
    const targetSkewUm = Math.round(umFrom(targetSkew, state.units));
    const amplitudeUm = Math.max(1, Math.round(umFrom(amplitude, state.units)));
    const spacingUm = Math.max(1, Math.round(umFrom(spacing, state.units)));
    const reply = await tuneLengthApply({ trackId: track.id, amplitude: amplitudeUm, spacing: spacingUm, targetLength: targetUm, flip, mode, targetSkew: targetSkewUm });
    if (!reply.ok) {
      dispatch({ type: "TOAST", message: reply.message ?? "Could not tune this track's length.", kind: "error" });
      return;
    }
    close();
    await api.refresh();
  };

  const info = preview?.reply;
  const fmt = (um: number | undefined) => (um == null ? "" : formatLength(um, state.units));
  const signed = (um: number | undefined) => (um == null ? "" : `${um > 0 ? "+" : ""}${fmt(um)}`);

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 380 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>{TITLES[mode]}</span>
        </div>
        <div className="dialog-body">
          {!track ? (
            <div style={{ fontSize: 12, opacity: 0.8 }}>{mode === "single" ? "Select a single straight track segment first." : "Select one straight track of the differential pair first."}</div>
          ) : (
            <>
              <div style={{ fontSize: 11, opacity: 0.7, marginBottom: 8 }}>
                {mode === "single" && (
                  <>
                    Current length: {formatLength(trackLengthUm, state.units)} · net {track.net}
                  </>
                )}
                {mode === "diffpair" && (
                  <>
                    {info?.partner_net ? `Pair ${track.net} / ${info.partner_net}` : `Net ${track.net}`}
                    {info?.original_length != null && <> · current length: {fmt(info.original_length)}</>}
                    {info?.pitch != null && <> · pitch {fmt(info.pitch)}</>}
                  </>
                )}
                {mode === "skew" && (
                  <>
                    {info?.partner_net ? `${track.net} against ${info.partner_net}` : `Net ${track.net}`}
                    {info?.original_length != null && <> · length {fmt(info.original_length)}</>}
                    {info?.partner_length != null && <> vs {fmt(info.partner_length)}</>}
                    {info?.skew_before != null && <> · skew {signed(info.skew_before)}</>}
                  </>
                )}
              </div>
              <div className="kv-grid" style={{ gridTemplateColumns: "110px 1fr" }}>
                {mode === "skew" ? (
                  <>
                    <span>Target skew:</span>
                    <input autoFocus type="number" value={targetSkew} onChange={(e) => setTargetSkew(Number(e.target.value))} />
                  </>
                ) : (
                  <>
                    <span>{mode === "diffpair" ? "Pair length:" : "Target length:"}</span>
                    <input autoFocus type="number" value={targetLength} onChange={(e) => setTargetLength(Number(e.target.value))} />
                  </>
                )}
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
                    {mode === "single" ? "Achieved length: " : mode === "diffpair" ? "Achieved pair length: " : "Achieved length: "}
                    {formatLength(preview.achieved, state.units)}
                    {mode !== "single" && info?.skew_after != null ? ` · skew ${signed(info.skew_after)}` : ""}
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
