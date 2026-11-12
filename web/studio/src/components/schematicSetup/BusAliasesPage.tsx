// Schematic Setup > Project > Bus Alias Definitions -- port of eeschema/dialogs/panel_setup_buses.cpp: "Bus Definitions" (a grid of aliases, "Alias") and, for the
// alias selected, "Members of '%s'" (a grid, "Net or Nested Bus Name"), each with add and delete. The aliases are the project's (`PROJECT_FILE::m_BusAliases`,
// the `.kicad_pro`'s `schematic.bus_aliases`), listed by name without regard to case (`loadAliases`). The grid's vetoes are KiCad's, in its words: a name
// another alias has ("Alias name '%s' already in use.", `OnAliasesGridCellChanging`) and an empty member ("Member net/alias name cannot be empty.",
// `OnMemberGridCellChanging`); a member typed as a space-separated list is a member per word. Apply (`TransferDataFromWindow` -> `SCHEMATIC::SetBusAliases`) is one
// undoable `set_bus_aliases`. A bus whose label names an alias (`{DATA}`, `PREFIX{DATA}`) has its members: the sheet unfolds it to them, the tracer joins
// them, and kicad-cli reads the same aliases from the derived project.
import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchSchSetup } from "../../api/client";
import type { BusAliasDef, SchSetupReply } from "../../api/types";
import { aliasNameError, busAliasIssue, busAliasesToSend, membersLabel, sortAliases } from "../../kicad-port/schSetup";
import { useStudioState } from "../../state/store";
import { PageFrame, usePageApply, usePageDraft, type PageProps, useSchSetupImport } from "../boardSetup/fields";

export function BusAliasesPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const [info, setInfo] = useState<SchSetupReply | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [sel, setSel] = useState<number | null>(null);
  const [selMember, setSelMember] = useState<number | null>(null);
  const load = useCallback(async () => {
    try {
      const r = await fetchSchSetup();
      if (r.ok) {
        setInfo(r);
        setProblem(null);
      } else setProblem(r.message ?? "The backend could not read the bus aliases.");
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load, state.version]);

  const source = useMemo(() => (info ? sortAliases(info.bus_aliases) : undefined), [info]);
  const page = usePageDraft<BusAliasDef[]>(source);
  // "Import Settings from Another Project..." (`ImportSettingsFrom`, UI-LEFT-2 BB4)
  useSchSetupImport("bus_aliases", (s) => sortAliases(s.setup.bus_aliases), page.importDraft);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const aliases = page.draft;
  const issue = aliases ? busAliasIssue(aliases) : null;
  const current = aliases && sel !== null ? aliases[sel] : undefined;

  const setAlias = (i: number, patch: Partial<BusAliasDef>) => page.edit((list) => list.map((a, j) => (j === i ? { ...a, ...patch } : a)));
  const onApply = () => {
    if (!aliases || !page.dirty) return;
    if (issue) {
      apply.setMessage({ text: issue.message, bad: true });
      return;
    }
    void apply.run({ op: "set_bus_aliases", aliases: busAliasesToSend(aliases) }, () => {
      page.markClean();
      void load();
    });
  };

  return (
    <PageFrame title="Bus Alias Definitions" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? problem ?? (page.dirty ? (issue?.message ?? null) : null)} bad={apply.message?.bad || problem !== null || (page.dirty && issue !== null)} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null), setSel(null))}>
      {!aliases ? (
        <p className="bs-note">{problem ?? "Reading the bus aliases…"}</p>
      ) : (
        <div style={{ display: "flex", gap: 24, flexWrap: "wrap" }}>
          <div>
            <div className="bs-section">Bus Definitions</div>
            <table className="bs-grid" aria-label="Bus Definitions">
              <thead>
                <tr>
                  <th>Alias</th>
                </tr>
              </thead>
              <tbody>
                {aliases.map((a, i) => {
                  const err = aliasNameError(aliases, i, a.name);
                  return (
                    <tr key={i} className={sel === i ? "bs-sel" : undefined} onClick={() => setSel(i)}>
                      <td>
                        <input className={`bs-name${err ? " bs-invalid" : ""}`} aria-label={`Alias ${i + 1}`} title={err ?? undefined} value={a.name} spellCheck={false} onFocus={() => (setSel(i), setSelMember(null))} onChange={(e) => setAlias(i, { name: e.target.value })} style={{ width: 200 }} />
                      </td>
                    </tr>
                  );
                })}
                {aliases.length === 0 && (
                  <tr>
                    <td className="bs-note">No bus aliases.</td>
                  </tr>
                )}
              </tbody>
            </table>
            <div className="bs-toolbar">
              <button
                aria-label="Add alias"
                onClick={() => {
                  page.edit((list) => [...list, { name: "", members: [] }]);
                  setSel(aliases.length);
                  setSelMember(null);
                }}
              >
                Add
              </button>
              <button
                aria-label="Delete alias"
                disabled={sel === null || sel >= aliases.length}
                onClick={() => {
                  if (sel === null) return;
                  page.edit((list) => list.filter((_, j) => j !== sel));
                  setSel(null);
                  setSelMember(null);
                }}
              >
                Delete
              </button>
            </div>
          </div>
          <div>
            {current ? (
              <>
                <div className="bs-section">{membersLabel(current.name)}</div>
                <table className="bs-grid" aria-label="Members">
                  <thead>
                    <tr>
                      <th>Net or Nested Bus Name</th>
                    </tr>
                  </thead>
                  <tbody>
                    {current.members.map((m, k) => (
                      <tr key={k} className={selMember === k ? "bs-sel" : undefined} onClick={() => setSelMember(k)}>
                        <td>
                          <input
                            className={`bs-name${m === "" ? " bs-invalid" : ""}`}
                            aria-label={`Member ${k + 1}`}
                            title={m === "" ? "Member net/alias name cannot be empty." : undefined}
                            value={m}
                            spellCheck={false}
                            onFocus={() => setSelMember(k)}
                            onChange={(e) => setAlias(sel!, { members: current.members.map((x, j) => (j === k ? e.target.value : x)) })}
                            style={{ width: 220 }}
                          />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                <div className="bs-toolbar">
                  <button
                    aria-label="Add member"
                    onClick={() => {
                      setAlias(sel!, { members: [...current.members, ""] });
                      setSelMember(current.members.length);
                    }}
                  >
                    Add
                  </button>
                  <button
                    aria-label="Remove member"
                    disabled={selMember === null || selMember >= current.members.length}
                    onClick={() => {
                      if (selMember === null) return;
                      setAlias(sel!, { members: current.members.filter((_, j) => j !== selMember) });
                      setSelMember(null);
                    }}
                  >
                    Remove
                  </button>
                </div>
              </>
            ) : (
              <p className="bs-note">Select an alias to edit its members.</p>
            )}
          </div>
        </div>
      )}
    </PageFrame>
  );
}
