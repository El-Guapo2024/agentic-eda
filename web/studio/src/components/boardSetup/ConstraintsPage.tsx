// Board Setup > Design Rules > Constraints -- port of pcbnew/dialogs/panel_setup_constraints.cpp (labels and tips from
// panel_setup_constraints_base.cpp). The minimums DRC holds every item to: the page's numbers are the `rules.*` keys of the derived
// `.kicad_pro`, so kicad-cli judges the board by them. Apply sends them whole (`set_constraints`); the backend checks the ranges again
// (BOARD_DESIGN_SETTINGS::ValidateDesignRules) and Undo takes the edit back.
import { useEffect } from "react";
import { useStudioState } from "../../state/store";
import { validateConstraints } from "../../kicad-port/boardSetupRules";
import type { Constraints } from "../../api/types";
import { Check, FieldRow, LengthField, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

interface Item {
  key: keyof Constraints;
  label: string;
  title: string;
}

const ONLY_REDUCED_BY_RULES = "If set, this can only be reduced by custom rules.";

const SECTIONS: ReadonlyArray<{ title: string; items: Item[] }> = [
  {
    title: "Copper",
    items: [
      { key: "min_clearance_um", label: "Minimum clearance:", title: `The minimum clearance between copper items which do not belong to the same net. ${ONLY_REDUCED_BY_RULES}` },
      { key: "min_track_width_um", label: "Minimum track width:", title: `The minimum track width. ${ONLY_REDUCED_BY_RULES}` },
      { key: "min_connection_um", label: "Minimum connection width:", title: "The minimum copper width of connected copper items." },
      { key: "min_annular_width_um", label: "Minimum annular width:", title: `The minimum annular ring width. ${ONLY_REDUCED_BY_RULES}` },
      { key: "min_via_diameter_um", label: "Minimum via diameter:", title: `The minimum via diameter. ${ONLY_REDUCED_BY_RULES}` },
      { key: "min_hole_clearance_um", label: "Copper to hole clearance:", title: `The minimum clearance between a hole and an unassociated copper item. ${ONLY_REDUCED_BY_RULES}` },
      { key: "min_copper_edge_clearance_um", label: "Copper to edge clearance:", title: `The minimum clearance between the board edge and any copper item. ${ONLY_REDUCED_BY_RULES}` },
      // "Minimum groove for creepage" (min_groove_width_um) is hidden unless KiCad's advanced setting m_EnableCreepageSlot is on; the value is kept as it is.
    ],
  },
  {
    title: "Holes",
    items: [
      { key: "min_through_hole_um", label: "Minimum drill size:", title: `The minimum drill size. ${ONLY_REDUCED_BY_RULES}` },
      { key: "min_hole_to_hole_um", label: "Hole to hole clearance:", title: `The minimum clearance between two drilled holes. ${ONLY_REDUCED_BY_RULES} (Note: does not apply to milled holes.)` },
    ],
  },
  {
    title: "uVias",
    items: [
      { key: "min_microvia_diameter_um", label: "Minimum uVia diameter:", title: `The minimum diameter for micro-vias. ${ONLY_REDUCED_BY_RULES}` },
      { key: "min_microvia_drill_um", label: "Minimum uVia hole:", title: `The minimum micro-via hole size. ${ONLY_REDUCED_BY_RULES}` },
    ],
  },
  {
    title: "Silk",
    items: [
      {
        key: "min_silk_clearance_um",
        label: "Minimum item clearance:",
        title: "Minimum clearance between two items on the same silkscreen layer. If set this can improve legibility.  (Note: does not apply to multiple shapes within a single footprint.)",
      },
      { key: "min_silk_text_height_um", label: "Minimum text height:", title: "" },
      { key: "min_silk_text_thickness_um", label: "Minimum text thickness:", title: "" },
    ],
  },
];

export function ConstraintsPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const rules = state.board?.board_rules;
  const units = state.units;
  const page = usePageDraft<Constraints>(rules?.constraints);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;
  const errors = draft ? validateConstraints(draft) : {};

  const onApply = () => {
    if (!draft) return;
    if (Object.keys(errors).length > 0) {
      apply.setMessage({ text: "Fix the values marked in red first.", bad: true });
      return;
    }
    void apply.run({ op: "set_constraints", constraints: draft }, page.markClean);
  };
  const set = <K extends keyof Constraints>(key: K, value: Constraints[K]) => page.edit((d) => ({ ...d, [key]: value }));
  const length = (item: Item) => (
    <FieldRow key={item.key} label={item.label} title={item.title || undefined} error={errors[item.key]} unit={units}>
      <LengthField ariaLabel={item.label.replace(/:$/, "")} units={units} value={draft?.[item.key] as number} invalid={!!errors[item.key]} onChange={(v) => set(item.key, (v ?? Number.NaN) as never)} />
    </FieldRow>
  );

  return (
    <PageFrame title="Constraints" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!draft ? (
        <p className="bs-note">This backend does not report the board's constraints; update the eda binary.</p>
      ) : (
        <>
          {rules?.constraints_explicit === false && (
            <p className="bs-note">
              The board's description states no minimums, so DRC uses the ones shown (the smallest sizes its net classes ask for). Apply makes them the board's own.
            </p>
          )}
          {SECTIONS.map((s) => (
            <div key={s.title}>
              <div className="bs-section">{s.title}</div>
              {s.items.map(length)}
            </div>
          ))}

          <div className="bs-section">Arc/Circle Approximations</div>
          <FieldRow
            label="Maximum allowed deviation:"
            title="The maximum allowed deviation between a true arc or circle and segments used to approximate it.  Smaller values produce smoother graphics at the expense of performance."
            error={errors.max_error_um}
            unit={units}
          >
            <LengthField ariaLabel="Maximum allowed deviation" units={units} value={draft.max_error_um} invalid={!!errors.max_error_um} onChange={(v) => set("max_error_um", v ?? Number.NaN)} />
          </FieldRow>
          <p className="bs-note">Note: zone filling can be slow when &lt; 0.005 mm.</p>

          <div className="bs-section">Zone Fill Strategy</div>
          <Check checked={draft.zones_allow_external_fillets} onChange={(v) => set("zones_allow_external_fillets", v)}>
            Allow fillets/chamfers outside zone outline
          </Check>
          <FieldRow label="Minimum thermal relief spoke count:" error={errors.min_resolved_spokes}>
            <NumberField ariaLabel="Minimum thermal relief spoke count" width={50} value={draft.min_resolved_spokes} invalid={!!errors.min_resolved_spokes} onChange={(v) => set("min_resolved_spokes", v ?? Number.NaN)} />
          </FieldRow>

          <div className="bs-section">Length Tuning</div>
          <Check
            checked={draft.use_height_for_length_calcs}
            onChange={(v) => set("use_height_for_length_calcs", v)}
            title="When enabled, the distance between copper layers will be included in track length calculations for tracks with vias.  When disabled, via stackup height is ignored."
          >
            Include stackup height in track length calculations
          </Check>
        </>
      )}
    </PageFrame>
  );
}
