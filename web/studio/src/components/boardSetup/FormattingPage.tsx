// Board Setup > Text & Graphics > Formatting -- port of pcbnew/dialogs/panel_setup_formatting.cpp: the length of the dash and of the gap of a dashed line (ratios of the line width,
// `dashed_line_dash_ratio` and `dashed_line_gap_ratio` of the board's plot parameters, 12 and 3 from ISO 128-2) and the five "Apply board defaults to footprint .." boxes
// (`defaults.apply_defaults_to_fp_*` of the project: a footprint put on the board takes the Text & Graphics defaults for its fields, text, shapes, dimensions and barcodes,
// `FOOTPRINT::ApplyDefaultSettings`). Apply sends the whole page as one undoable command (`set_formatting`).
import { useEffect } from "react";
import { useStudioState } from "../../state/store";
import { DEFAULT_FORMATTING, formattingOf, formattingProblems, sameFormatting } from "../../kicad-port/setupPages";
import type { FormattingSettings } from "../../api/types";
import { Check, FieldRow, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

const BOXES: ReadonlyArray<{ key: keyof FormattingSettings; label: string }> = [
  { key: "apply_to_fp_fields", label: "Apply board defaults to footprint fields" },
  { key: "apply_to_fp_text", label: "Apply board defaults to footprint text" },
  { key: "apply_to_fp_shapes", label: "Apply board defaults to non-copper footprint shapes" },
  { key: "apply_to_fp_dimensions", label: "Apply board defaults to footprint dimensions" },
  { key: "apply_to_fp_barcodes", label: "Apply board defaults to footprint barcodes" },
];

export function FormattingPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const stored = state.board?.board_rules?.formatting;
  const page = usePageDraft<FormattingSettings>(state.board?.board_rules && stored ? formattingOf(stored) : undefined);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;
  const problems = draft ? formattingProblems(draft) : {};
  const set = <K extends keyof FormattingSettings>(key: K, value: FormattingSettings[K]) => page.edit((d) => ({ ...d, [key]: value }));

  const onApply = () => {
    if (!draft) return;
    if (Object.keys(problems).length > 0) {
      apply.setMessage({ text: "Fix the values marked in red first.", bad: true });
      return;
    }
    if (sameFormatting(draft, stored)) {
      apply.setMessage({ text: "Nothing to apply.", bad: false });
      page.markClean();
      return;
    }
    void apply.run({ op: "set_formatting", settings: draft }, page.markClean);
  };

  return (
    <PageFrame title="Formatting" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!draft ? (
        <p className="bs-note">This backend does not report the board's formatting settings; update the eda binary.</p>
      ) : (
        <>
          <div className="bs-section">Dashed Lines</div>
          <FieldRow label="Dash length:" error={problems.dash_length_ratio} title="The length of a dash, in line widths">
            <NumberField ariaLabel="Dash length" value={draft.dash_length_ratio} invalid={!!problems.dash_length_ratio} onChange={(v) => set("dash_length_ratio", v ?? Number.NaN)} />
          </FieldRow>
          <FieldRow label="Gap length:" error={problems.gap_length_ratio} title="The length of the gap between two dashes, in line widths">
            <NumberField ariaLabel="Gap length" value={draft.gap_length_ratio} invalid={!!problems.gap_length_ratio} onChange={(v) => set("gap_length_ratio", v ?? Number.NaN)} />
          </FieldRow>
          <p className="bs-note">
            Dash and dot lengths are ratios of the line width (KiCad's start is {DEFAULT_FORMATTING.dash_length_ratio} and {DEFAULT_FORMATTING.gap_length_ratio}, from ISO 128-2). They are the board's plot parameters, so kicad-cli's plots draw dashed lines with them.
          </p>
          <div className="bs-section">Footprint Defaults</div>
          {BOXES.map((b) => (
            <Check key={b.key} checked={draft[b.key] as boolean} onChange={(v) => set(b.key, v as never)}>
              {b.label}
            </Check>
          ))}
          <p className="bs-note">
            A footprint put on the board from a library takes the Text &amp; Graphics defaults of its layers' classes for what is ticked: a footprint's Reference and Value, the text and the non-copper shapes of its own copy. (Library footprints here hold no dimensions or barcodes.)
          </p>
        </>
      )}
    </PageFrame>
  );
}
