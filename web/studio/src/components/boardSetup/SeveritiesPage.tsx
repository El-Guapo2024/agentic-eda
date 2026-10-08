// Board Setup > Design Rules > Violation Severity -- port of common/dialogs/panel_setup_severities.cpp: one row per DRC check (grouped under
// the headings DRC_ITEM::GetItemsWithSeverities lists them by) with Error / Warning / Ignore. The rows come from KiCad's own list
// (src/kicad/drc_checks.json, generated from drc_item.cpp by tools/extract-drc-checks.js) and the table is the derived project's
// `rule_severities`: kicad-cli reports each check at the severity chosen here, and an ignored check is not reported at all.
import { useEffect, useMemo } from "react";
import checks from "../../kicad/drc_checks.json";
import { useStudioState } from "../../state/store";
import { severitiesToSend, severityOf, type SeverityItem } from "../../kicad-port/boardSetupRules";
import type { RuleSeverity } from "../../api/types";
import { PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

interface CheckItem extends SeverityItem {
  title: string;
  /** The DRCE_* code (tooltip). */
  code: string;
}
const GROUPS = checks.groups as ReadonlyArray<{ title: string; items: CheckItem[] }>;
const ITEMS: CheckItem[] = GROUPS.flatMap((g) => g.items);
const CHOICES: ReadonlyArray<{ value: RuleSeverity; label: string }> = [
  { value: "error", label: "Error" },
  { value: "warning", label: "Warning" },
  { value: "ignore", label: "Ignore" },
];

export function SeveritiesPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const table = state.board?.board_rules?.severities;
  // The page edits every check's severity (the table only names the ones that differ from KiCad's default).
  const source = useMemo<Record<string, RuleSeverity> | undefined>(() => (table ? Object.fromEntries(ITEMS.map((i) => [i.key, severityOf(i, table)])) : undefined), [table]);
  const page = usePageDraft<Record<string, RuleSeverity>>(source);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;

  const onApply = () => {
    if (!draft || !table) return;
    void apply.run({ op: "set_rule_severities", severities: severitiesToSend(ITEMS, draft, table) }, page.markClean);
  };

  return (
    <PageFrame title="Violation Severity" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!draft ? (
        <p className="bs-note">This backend does not report the DRC severities; update the eda binary.</p>
      ) : (
        <>
          <p className="bs-note">
            What kicad-cli reports each kind of violation as. An ignored check is not reported. The two library checks start at Ignore: every footprint here is stored in the board itself, so there is no library to compare it with.
          </p>
          <div className="bs-sev-grid">
            {GROUPS.map((g) => (
              <SeverityGroup key={g.title} title={g.title} items={g.items} chosen={draft} onChoose={(key, value) => page.edit((d) => ({ ...d, [key]: value }))} />
            ))}
          </div>
        </>
      )}
    </PageFrame>
  );
}

function SeverityGroup({ title, items, chosen, onChoose }: { title: string; items: CheckItem[]; chosen: Record<string, RuleSeverity>; onChoose: (key: string, value: RuleSeverity) => void }) {
  return (
    <>
      <div className="bs-sev-group">{title}</div>
      {items.map((item) => (
        <div key={item.key} style={{ display: "contents" }}>
          <span className="bs-sev-name" title={item.code}>
            {item.title}:
          </span>
          <span className="bs-sev-radios" role="radiogroup" aria-label={item.title}>
            {CHOICES.map((c) => (
              <label key={c.value}>
                <input type="radio" name={`sev-${item.key}`} checked={(chosen[item.key] ?? item.defaultSeverity) === c.value} onChange={() => onChoose(item.key, c.value)} /> {c.label}
              </label>
            ))}
          </span>
        </div>
      ))}
    </>
  );
}
