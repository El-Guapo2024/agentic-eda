// Port of pcbnew/dialogs/dialog_tuning_pattern_properties.cpp ("Tuning Pattern Properties"): the settings of a length-tuning pattern -- its target, the amplitude and
// spacing of its meanders, their corners, and which side of the track they start on.
//
// Two things open it, as in KiCad (`PCB_TUNING_PATTERN::ShowPropertiesDialog` and the `lengthTunerSettings` handler of `DRAWING_TOOL::PlaceTuningPattern`):
//
//   a pattern of the board   (E / double-click on it): OK tunes it again with the new settings (crates/cli/src/tune_api.rs `update`), one undo step;
//   the tool (Ctrl+L, armed)  the settings the next patterns start from -- and the pattern being placed, if one is -- over the board's Length-tuning Patterns page.
//
// A design rule that gives the net a length (or skew) target sets the target field and disables it, with "(from <rule>)", until "Override custom rules" is on. The
// time-domain delay KiCad also offers (a target in picoseconds, from a tuning profile) is not modelled: its radio button is not shown.
import { useEffect, useState } from "react";
import { tunePattern, tuneSettings, tuneUpdate } from "../api/tuneClient";
import { LENGTH_UNCONSTRAINED_UM, type PatternSide, type TuneKind, type TuneSettings, type TuningPattern } from "../api/tuneTypes";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { DialogShell, lengthText, parseLength } from "./pcbDialogKit";
import { kindOfMode, pageSettings, settingsOfPattern, tuneStateFromReply } from "../kicad-port/tuneTool";

const TITLES: Record<TuneKind, string> = {
  single: "Tuning Pattern Properties: single track",
  diffpair: "Tuning Pattern Properties: differential pair",
  skew: "Tuning Pattern Properties: differential pair skew",
};

interface Draft {
  target: string;
  override: boolean;
  min: string;
  max: string;
  spacing: string;
  rounded: boolean;
  radius: string;
  side: PatternSide;
  singleSided: boolean;
}

function draftOf(s: Required<Pick<TuneSettings, "min_amplitude" | "max_amplitude" | "spacing" | "corner_radius_percent" | "rounded" | "single_sided">> & TuneSettings, kind: TuneKind, units: "mm" | "mil" | "in"): Draft {
  const t = kind === "skew" ? s.target_skew : s.target_length;
  const opt = typeof t === "number" ? t : t && typeof t === "object" ? t.opt : undefined;
  return {
    target: opt == null || (kind !== "skew" && opt >= LENGTH_UNCONSTRAINED_UM) ? "" : lengthText(opt, units),
    override: !!s.override_custom_rules,
    min: lengthText(s.min_amplitude, units),
    max: lengthText(s.max_amplitude, units),
    spacing: lengthText(s.spacing, units),
    rounded: s.rounded,
    radius: String(s.corner_radius_percent),
    side: s.initial_side === "right" ? "right" : "left",
    singleSided: s.single_sided,
  };
}

export function TuningPatternDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.lengthTuningDialogOpen;
  const patternId = state.pcbx.tuneDialogPattern;
  const units = state.units;
  const [pattern, setPattern] = useState<TuningPattern | null>(null);
  const [rule, setRule] = useState<{ name: string; opt: number } | null>(null);
  const [kind, setKind] = useState<TuneKind>(state.lengthTuningMode);
  const [d, setD] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setError(null);
    if (patternId) {
      void tunePattern(patternId).then((r) => {
        if (cancelled) return;
        if (!r.ok || !r.pattern) {
          setError(r.message ?? "No such tuning pattern.");
          setD(null);
          return;
        }
        const k = kindOfMode(r.pattern.mode);
        setPattern(r.pattern);
        setKind(k);
        setRule(r.constraint ? { name: r.constraint.name, opt: r.constraint.opt } : null);
        setD(draftOf({ ...settingsOfPattern(r.pattern) }, k, units));
      });
    } else {
      const k = state.lengthTuningMode;
      const draw = state.drawState?.kind === "tune" ? state.drawState : null;
      setPattern(null);
      setKind(k);
      setRule(draw?.readout?.constraint ? { name: draw.readout.constraint, opt: 0 } : null);
      const base = draw?.pattern ? settingsOfPattern(draw.pattern) : { ...pageSettings(state.board?.board_rules?.tuning_patterns, k), initial_side: "left" as PatternSide, override_custom_rules: false, ...(state.pcbx.tuneSettings[k] ?? {}) };
      setD(draftOf(base as Required<TuneSettings>, k, units));
    }
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, patternId]);

  if (!open) return null;
  const close = () => {
    dispatch({ type: "SET_LENGTH_TUNING_DIALOG_OPEN", open: false });
    dispatch({ type: "PCBX", patch: { tuneDialogPattern: null } });
  };
  if (!d) {
    return (
      <DialogShell title={TITLES[kind]} onCancel={close} onOk={close} okDisabled>
        <div style={{ fontSize: 12, opacity: 0.8 }}>{error ?? "Loading..."}</div>
      </DialogShell>
    );
  }

  const set = (patch: Partial<Draft>) => setD((cur) => (cur ? { ...cur, ...patch } : cur));
  const fromRule = rule != null && !d.override;
  const targetDisabled = fromRule;

  const submit = async () => {
    const min = parseLength(d.min, units);
    const max = parseLength(d.max, units);
    const spacing = parseLength(d.spacing, units);
    const radius = Number(d.radius);
    if (min == null || min <= 0) return setError("The minimum amplitude must be above 0.");
    if (max == null || max <= 0) return setError("The maximum amplitude must be above 0.");
    if (min > max) return setError("The minimum amplitude cannot be above the maximum amplitude.");
    if (spacing == null || spacing <= 0) return setError("The spacing must be above 0.");
    if (!Number.isFinite(radius) || radius < 0 || radius > 100) return setError("The corner radius is a percentage from 0 to 100.");
    let target: number | null = null;
    if (d.target.trim() !== "") {
      const t = parseLength(d.target, units);
      if (t == null) return setError("The target must be a length.");
      target = t;
    }
    const settings: TuneSettings = {
      min_amplitude: min,
      max_amplitude: max,
      spacing,
      corner_radius_percent: Math.round(radius),
      rounded: d.rounded,
      single_sided: d.singleSided,
      initial_side: d.side,
      override_custom_rules: d.override,
    };
    // a target the dialog shows as a rule's is the rule's again unless the pattern overrides the rules (`TransferDataFromWindow`)
    if (!fromRule) {
      if (kind === "skew") settings.target_skew = target ?? 0;
      else settings.target_length = target;
    }
    if (patternId) {
      const r = await tuneUpdate(patternId, settings);
      if (!r.ok) return setError(r.message ?? "The tuning pattern could not be tuned again.");
      close();
      await api.refresh();
      return;
    }
    // the tool's settings (the next patterns start from them, `meanderSettings`) and the pattern being placed, if there is one
    const carried = { ...state.pcbx.tuneSettings[kind], ...settings };
    dispatch({ type: "PCBX", patch: { tuneSettings: { ...state.pcbx.tuneSettings, [kind]: carried } } });
    const draw = state.drawState?.kind === "tune" ? state.drawState : null;
    if (draw) {
      const r = await tuneSettings(settings);
      if (r.ok) dispatch({ type: "SET_DRAW_STATE", draw: tuneStateFromReply(r, draw, { mode: draw.mode, layer: draw.layer }) });
    }
    close();
  };

  const label = (text: string) => <span>{text}</span>;
  const unitLabel = units;
  return (
    <DialogShell title={TITLES[kind]} width={400} onCancel={close} onOk={() => void submit()}>
      <div className="kv-grid" style={{ gridTemplateColumns: "150px 1fr 40px", alignItems: "center", gap: "6px 8px" }}>
        {label(kind === "skew" ? "Target skew:" : "Target length:")}
        <input autoFocus type="text" value={d.target} disabled={targetDisabled} placeholder={kind === "skew" ? "0" : "none"} onChange={(e) => set({ target: e.target.value })} />
        <span>{unitLabel}</span>
        <span />
        <label className="filter-row" style={{ gridColumn: "2 / span 2" }}>
          <input type="checkbox" checked={d.override} onChange={(e) => set({ override: e.target.checked })} />
          Override custom rules
        </label>
        {fromRule && (
          <>
            <span />
            <span style={{ gridColumn: "2 / span 2", fontSize: 11, opacity: 0.75 }}>(from {rule?.name})</span>
          </>
        )}
        {label("Minimum amplitude:")}
        <input type="text" value={d.min} onChange={(e) => set({ min: e.target.value })} />
        <span>{unitLabel}</span>
        {label("Maximum amplitude:")}
        <input type="text" value={d.max} onChange={(e) => set({ max: e.target.value })} />
        <span>{unitLabel}</span>
        {label("Minimum spacing:")}
        <input type="text" value={d.spacing} onChange={(e) => set({ spacing: e.target.value })} />
        <span>{unitLabel}</span>
        {label("Corner style:")}
        <select value={d.rounded ? "round" : "chamfer"} onChange={(e) => set({ rounded: e.target.value === "round" })}>
          <option value="chamfer">Chamfered</option>
          <option value="round">Rounded</option>
        </select>
        <span />
        {label("Corner radius:")}
        <input type="text" value={d.radius} onChange={(e) => set({ radius: e.target.value })} />
        <span>%</span>
        {label("Initial side:")}
        <select value={d.side} onChange={(e) => set({ side: e.target.value as PatternSide })}>
          <option value="left">Left</option>
          <option value="right">Right</option>
        </select>
        <span />
        <span />
        <label className="filter-row" style={{ gridColumn: "2 / span 2" }}>
          <input type="checkbox" checked={d.singleSided} onChange={(e) => set({ singleSided: e.target.checked })} />
          Single-sided
        </label>
      </div>
      {pattern && <div style={{ fontSize: 11, opacity: 0.7, marginTop: 8 }}>{pattern.info ?? ""}</div>}
      <div style={{ fontSize: 11, marginTop: 8, minHeight: 16, color: "#ff6666" }}>{error ?? ""}</div>
    </DialogShell>
  );
}
