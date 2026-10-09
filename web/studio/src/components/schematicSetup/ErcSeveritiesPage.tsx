// Schematic Setup > Electrical Rules > Violation Severity -- port of common/dialogs/panel_setup_severities.cpp, as eeschema/dialogs/dialog_schematic_setup.cpp
// hosts it over `ERC_ITEM::GetItemsWithSeverities()`: one row per ERC check (under the headings Connections, Conflicts and Miscellaneous) with Error /
// Warning / Ignore, and the pin conflicts map's own row last, "From Pin Conflicts Map" or Ignore (`aPinMapSpecialCase`: the map says warning or error
// per pair of pin types, so the row only chooses whether the check runs). The rows and the defaults come from KiCad's own source
// (src/kicad/erc_checks.json, generated from erc_item.cpp and erc_settings.cpp by tools/extract-erc-checks.js).
//
// The table is the derived project's `erc.rule_severities`: kicad-cli's ERC reports each check at the severity chosen here, and an ignored check
// is not run (it shows up in the report's ignored tests). Apply sends the whole table as one undoable command (`set_erc_severities`).
import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchErcSeverities } from "../../api/client";
import type { RuleSeverity } from "../../api/types";
import checks from "../../kicad/erc_checks.json";
import { severitiesToSend, severityOf, type SeverityItem } from "../../kicad-port/boardSetupRules";
import { setErcView } from "../../state/checkerView";
import { useStudioState } from "../../state/store";
import { PageFrame, usePageApply, usePageDraft, type PageProps } from "../boardSetup/fields";

interface CheckItem extends SeverityItem {
  title: string;
  /** The ERCE_* code (tooltip). */
  code: string;
}
const GROUPS = checks.groups as ReadonlyArray<{ title: string; items: CheckItem[] }>;
const PIN_MAP = checks.pinMap as CheckItem;
const ITEMS: CheckItem[] = [...GROUPS.flatMap((g) => g.items), PIN_MAP];
const CHOICES: ReadonlyArray<{ value: RuleSeverity; label: string }> = [
  { value: "error", label: "Error" },
  { value: "warning", label: "Warning" },
  { value: "ignore", label: "Ignore" },
];

export function ErcSeveritiesPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  // The table the schematic's ERC runs with (GET /api/sch/erc_severities); read again when the design changes, so an Undo shows up here.
  const [table, setTable] = useState<Record<string, RuleSeverity> | undefined>(undefined);
  const [problem, setProblem] = useState<string | null>(null);
  const load = useCallback(async () => {
    try {
      const r = await fetchErcSeverities();
      if (r.ok) {
        setTable(r.severities);
        setErcView({ severities: r.severities });
        setProblem(null);
      } else setProblem(r.message ?? "The backend could not read the ERC severities.");
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load, state.version]);

  // The page edits every check's severity (the table only names the ones that differ from KiCad's default).
  const source = useMemo<Record<string, RuleSeverity> | undefined>(() => (table ? Object.fromEntries(ITEMS.map((i) => [i.key, severityOf(i, table)])) : undefined), [table]);
  const page = usePageDraft<Record<string, RuleSeverity>>(source);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;

  const onApply = () => {
    if (!draft || !table) return;
    void apply.run({ op: "set_erc_severities", severities: severitiesToSend(ITEMS, draft, table) }, () => {
      page.markClean();
      void load();
    });
  };

  const choose = (key: string, value: RuleSeverity) => page.edit((d) => ({ ...d, [key]: value }));

  return (
    <PageFrame title="Violation Severity" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? problem} bad={apply.message?.bad || problem !== null} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!draft ? (
        <p className="bs-note">{problem ?? "Reading the ERC severities…"}</p>
      ) : (
        <>
          <p className="bs-note">
            What kicad-cli reports each kind of violation as. An ignored check is not run. The two library checks start at Ignore: every symbol here is stored in the schematic itself, so there is no library to compare it with.
          </p>
          <div className="bs-sev-grid">
            {GROUPS.map((g) => (
              <SeverityGroup key={g.title} title={g.title} items={g.items} chosen={draft} onChoose={choose} />
            ))}
            {/* PANEL_SETUP_SEVERITIES's pin map special case: last, with its own two choices. */}
            <span className="bs-sev-name" title={PIN_MAP.code}>
              {PIN_MAP.title}:
            </span>
            <span className="bs-sev-radios" role="radiogroup" aria-label={PIN_MAP.title}>
              <label>
                <input type="radio" name={`sev-${PIN_MAP.key}`} checked={(draft[PIN_MAP.key] ?? PIN_MAP.defaultSeverity) !== "ignore"} onChange={() => choose(PIN_MAP.key, PIN_MAP.defaultSeverity)} /> From Pin Conflicts Map
              </label>
              <label>
                <input type="radio" name={`sev-${PIN_MAP.key}`} checked={(draft[PIN_MAP.key] ?? PIN_MAP.defaultSeverity) === "ignore"} onChange={() => choose(PIN_MAP.key, "ignore")} /> Ignore
              </label>
            </span>
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
