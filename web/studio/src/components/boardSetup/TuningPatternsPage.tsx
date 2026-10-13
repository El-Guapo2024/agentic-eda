// Board Setup > Design Rules > Length-tuning Patterns -- port of pcbnew/dialogs/panel_setup_tuning_patterns.cpp: what a tuning pattern of each kind starts from (`PNS::MEANDER_SETTINGS`):
// the minimum and the maximum amplitude, the spacing, the corner style and its radius, and whether the meanders are all on one side. Three groups -- a single track, a differential pair
// and the skew of a pair -- kept in the project (`board.design_settings.tuning_pattern_settings`). The length tuner starts from the group of the tuning it is opened for (its amplitude
// is the maximum amplitude, which the amplitude hot keys step but never below the minimum). Apply sends the whole page as one undoable command (`set_tuning_patterns`).
import { useEffect } from "react";
import { useStudioState } from "../../state/store";
import { TUNING_GROUPS, sameTuningPatterns, tuningPatternsOf, tuningPatternsProblems, type TuningGroup } from "../../kicad-port/setupPages";
import type { MeanderCornerStyle, MeanderSettings, TuningPatterns } from "../../api/types";
import { Check, FieldRow, LengthField, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

const GROUP_TITLE: Record<TuningGroup, string> = {
  single_track: "Single Track Tuning Pattern",
  diff_pair: "Differential Pair Tuning Pattern",
  diff_pair_skew: "Differential Pair Skew Tuning Pattern",
};

export function TuningPatternsPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const units = state.units;
  const stored = state.board?.board_rules?.tuning_patterns;
  const page = usePageDraft<TuningPatterns>(state.board?.board_rules && stored ? tuningPatternsOf(stored) : undefined);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;
  const problems = draft ? tuningPatternsProblems(draft) : {};
  const set = (group: TuningGroup, patch: Partial<MeanderSettings>) => page.edit((d) => ({ ...d, [group]: { ...d[group], ...patch } }));

  const onApply = () => {
    if (!draft) return;
    if (Object.keys(problems).length > 0) {
      apply.setMessage({ text: "Fix the values marked in red first.", bad: true });
      return;
    }
    if (sameTuningPatterns(draft, stored)) {
      apply.setMessage({ text: "Nothing to apply.", bad: false });
      page.markClean();
      return;
    }
    void apply.run({ op: "set_tuning_patterns", settings: draft }, page.markClean);
  };

  return (
    <PageFrame title="Length-tuning Patterns" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!draft ? (
        <p className="bs-note">This backend does not report the board's tuning patterns; update the eda binary.</p>
      ) : (
        <>
          <p className="bs-note">The settings a new tuning pattern of each kind starts from. The length-tuning dialog opens with the maximum amplitude and the spacing of its kind.</p>
          {TUNING_GROUPS.map((group) => {
            const s = draft[group];
            const err = (field: string) => problems[`${group}.${field}`];
            const length = (field: "min_amplitude" | "max_amplitude" | "spacing", label: string) => {
              const key = `${field}_um` as "min_amplitude_um" | "max_amplitude_um" | "spacing_um";
              return (
                <FieldRow label={label} error={err(field)} unit={units}>
                  <LengthField ariaLabel={`${GROUP_TITLE[group]} ${label.replace(/:$/, "")}`} units={units} value={s[key]} invalid={!!err(field)} onChange={(v) => set(group, { [key]: v ?? Number.NaN })} />
                </FieldRow>
              );
            };
            return (
              <div key={group} data-group={group}>
                <div className="bs-section">{GROUP_TITLE[group]}</div>
                {length("min_amplitude", "Minimum amplitude:")}
                {length("max_amplitude", "Maximum amplitude:")}
                {length("spacing", "Spacing:")}
                <FieldRow label="Corner style:">
                  <select aria-label={`${GROUP_TITLE[group]} corner style`} value={s.corner_style} onChange={(e) => set(group, { corner_style: e.target.value as MeanderCornerStyle })}>
                    <option value="chamfer">Chamfer</option>
                    <option value="round">Round</option>
                  </select>
                </FieldRow>
                <FieldRow label="Corner radius:" error={err("corner_radius_percentage")} unit="%">
                  <NumberField ariaLabel={`${GROUP_TITLE[group]} corner radius`} value={s.corner_radius_percentage} invalid={!!err("corner_radius_percentage")} onChange={(v) => set(group, { corner_radius_percentage: v ?? Number.NaN })} />
                </FieldRow>
                <Check checked={s.single_sided} onChange={(v) => set(group, { single_sided: v })} title="Place the meanders on one side of the track only">
                  Single-sided
                </Check>
              </div>
            );
          })}
        </>
      )}
    </PageFrame>
  );
}
