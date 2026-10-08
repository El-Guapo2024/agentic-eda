// Board Setup > Board Stackup > Solder Mask/Paste -- port of pcbnew/dialogs/panel_setup_mask_and_paste.cpp (labels and tips from
// panel_setup_mask_and_paste_base.cpp). The mask half is what the solder-mask checks read and what `(setup (pad_to_mask_clearance ..))`
// of the derived `.kicad_pcb` says; the paste half goes there too (`pad_to_paste_clearance`, `_ratio`).
import { useEffect } from "react";
import { useStudioState } from "../../state/store";
import { validateMaskPaste } from "../../kicad-port/boardSetupRules";
import type { MaskPaste } from "../../api/types";
import { Check, FieldRow, LengthField, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

const MASK_EXPANSION_TIP =
  "Global clearance between pads and the solder mask.\nThis value can be superseded by local values for a footprint or a pad.\nPositive clearance means a solder mask opening bigger than the pad (typical for solder mask clearance).";
const WEB_TIP = "Minimum distance between openings in the solder mask.\nPad openings closer than this distance will be plotted as a single opening.";
const MASK_COPPER_TIP =
  "Minimum distance between a solder mask opening and a copper item with a different net than the solder mask opening's parent.\nDistances smaller than this minimum will create a DRC error.";
const TENT_TIP = "Tented: vias are covered with solder mask.\nNot tented: vias are not covered with solder mask.";
const PASTE_TIP = "Solder paste clearance relative to pad size.\nThis value can be superseded by local values for a footprint or a pad.";
const PASTE_RATIO_TIP =
  "Global clearance ratio between pads and the solder paste as a percentage of the pad size.\nA value of 10 means the clearance value is 10 percent of the pad size.\nThis value can be superseded by local values for a footprint or a pad.\nFinal clearance value is the sum of this value and the absolute clearance value.";

/** The ratio is stored as a fraction (-0.05) and shown as a percentage (-5); rounding keeps 7 from showing as 7.000000000000001. */
const toPercent = (ratio: number) => Math.round(ratio * 100 * 1e6) / 1e6;

export function MaskPastePage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const units = state.units;
  const page = usePageDraft<MaskPaste>(state.board?.board_rules?.mask_paste);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;
  const errors = draft ? validateMaskPaste(draft) : {};
  const set = <K extends keyof MaskPaste>(key: K, value: MaskPaste[K]) => page.edit((d) => ({ ...d, [key]: value }));

  const onApply = () => {
    if (!draft) return;
    if (Object.keys(errors).length > 0) {
      apply.setMessage({ text: "Fix the values marked in red first.", bad: true });
      return;
    }
    void apply.run({ op: "set_mask_paste", settings: draft }, page.markClean);
  };
  const length = (key: "expansion_um" | "min_width_um" | "to_copper_clearance_um" | "paste_margin_um", label: string, title: string) => (
    <FieldRow label={label} title={title} error={errors[key]} unit={units}>
      <LengthField ariaLabel={label.replace(/:$/, "")} units={units} value={draft?.[key]} invalid={!!errors[key]} onChange={(v) => set(key, v ?? Number.NaN)} />
    </FieldRow>
  );

  return (
    <PageFrame title="Solder Mask/Paste" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!draft ? (
        <p className="bs-note">This backend does not report the board's solder mask and paste settings; update the eda binary.</p>
      ) : (
        <>
          <p className="bs-note">
            Consult your PCB manufacturer's specifications for solder mask expansion, web width, and clearance settings. If no specifications are provided, setting these values to zero is recommended.
          </p>
          <div className="bs-section">Solder Mask Settings</div>
          {length("expansion_um", "Solder mask expansion:", MASK_EXPANSION_TIP)}
          {length("min_width_um", "Solder mask minimum web width:", WEB_TIP)}
          {length("to_copper_clearance_um", "Solder mask to copper clearance:", MASK_COPPER_TIP)}
          <Check checked={draft.allow_bridges_in_footprints} onChange={(v) => set("allow_bridges_in_footprints", v)} title="Disable DRC error checking for solder mask aperture bridging between pads in the same footprint.">
            Allow bridged solder mask apertures between pads within footprints
          </Check>
          <FieldRow label="Tent vias:" title={TENT_TIP}>
            <Check checked={draft.tent_vias_front} onChange={(v) => set("tent_vias_front", v)} title={TENT_TIP}>
              Front
            </Check>
            <Check checked={draft.tent_vias_back} onChange={(v) => set("tent_vias_back", v)} title={TENT_TIP}>
              Back
            </Check>
          </FieldRow>

          <div className="bs-section">Solder Paste Settings</div>
          {length("paste_margin_um", "Solder paste clearance:", PASTE_TIP)}
          <FieldRow label="Solder paste relative clearance:" title={PASTE_RATIO_TIP} error={errors.paste_margin_ratio} unit="%">
            <NumberField
              ariaLabel="Solder paste relative clearance"
              value={toPercent(draft.paste_margin_ratio)}
              invalid={!!errors.paste_margin_ratio}
              onChange={(v) => set("paste_margin_ratio", v === undefined ? Number.NaN : v / 100)}
            />
          </FieldRow>
        </>
      )}
    </PageFrame>
  );
}
