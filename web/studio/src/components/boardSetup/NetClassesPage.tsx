// Board Setup > Design Rules > Net Classes -- port of common/dialogs/panel_setup_netclasses.cpp: the table of classes (the Default class first, then
// the others, each size left blank to inherit the Default's) and the table of Netclass Assignments (a net-name pattern and the class that owns
// the nets it matches), with the list of nets the selected pattern matches. Apply sends the whole page (`set_net_classes`); the rules the
// router, DRC and the derived `.kicad_pro` use follow, and Undo takes the edit back.
//
// A pattern here is `*` (any run of characters) and `?` (one character), anchored to the whole net name; KiCad's regular expressions are not
// read (kicad-port/boardSetupRules.ts). The first class in the table whose pattern matches a net owns it, so the order of the classes is
// their precedence; the Up and Down buttons move a class. Not ported: the tuning profile, the PCB colour and the schematic wire columns.
import { useEffect, useMemo, useState } from "react";
import { useStudioState } from "../../state/store";
import { addAssignment, applyAssignments, assignmentRows, boardNetNames, classOfNet, netsMatching, validateNetClasses, type AssignmentRow, type NetClassIssue } from "../../kicad-port/boardSetupRules";
import type { NetClass } from "../../api/types";
import { LengthField, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

interface Draft {
  def: NetClass;
  classes: NetClass[];
  rows: AssignmentRow[];
}

type SizeField = "clearance" | "track_width" | "via_diameter" | "via_drill" | "microvia_diameter" | "microvia_drill" | "diff_pair_width" | "diff_pair_gap";

/** The size columns in KiCad's order, with its tooltips (panel_setup_netclasses.cpp, OnNetclassGridMouseEvent). `required`: the Default class must have it. */
const COLUMNS: ReadonlyArray<{ field: SizeField; label: string; title: string; required: boolean }> = [
  { field: "clearance", label: "Clearance", title: "Minimum copper clearance", required: true },
  { field: "track_width", label: "Track Width", title: "Optimum track width", required: true },
  { field: "via_diameter", label: "Via Size", title: "Via pad diameter", required: true },
  { field: "via_drill", label: "Via Hole", title: "Via plated hole diameter", required: true },
  { field: "microvia_diameter", label: "uVia Size", title: "Microvia pad diameter", required: false },
  { field: "microvia_drill", label: "uVia Hole", title: "Microvia plated hole diameter", required: false },
  { field: "diff_pair_width", label: "DP Width", title: "Differential pair track width", required: false },
  { field: "diff_pair_gap", label: "DP Gap", title: "Differential pair gap", required: false },
];

/** A name for a new class that no class has yet ("Netclass 1", "Netclass 2", ...). */
function newClassName(taken: readonly string[]): string {
  const used = new Set(taken.map((n) => n.trim().toLowerCase()));
  for (let i = 1; ; i++) if (!used.has(`netclass ${i}`)) return `Netclass ${i}`;
}

export function NetClassesPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const board = state.board;
  const rules = board?.board_rules;
  const units = state.units;
  const nets = useMemo(() => (board ? boardNetNames(board) : []), [board]);

  const source = useMemo<Draft | undefined>(() => {
    if (!rules) return undefined;
    const def: NetClass = rules.default_class ?? { name: "Default", nets: [], track_width: rules.track_width, clearance: rules.clearance, via_diameter: rules.via_diameter, via_drill: rules.via_drill, priority: 0 };
    return { def, classes: rules.net_classes, rows: assignmentRows(rules.net_classes) };
  }, [rules]);
  const page = usePageDraft<Draft>(source);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);

  const [selClass, setSelClass] = useState(0); // 0 = the Default class, i + 1 = classes[i]
  const [selRow, setSelRow] = useState<number | null>(null);
  const draft = page.draft;
  const issues: NetClassIssue[] = draft ? validateNetClasses(draft.def, draft.classes, draft.rows) : [];
  const issueAt = new Map<string, string>();
  for (const i of issues) if (!issueAt.has(`${i.row}:${i.field}`)) issueAt.set(`${i.row}:${i.field}`, i.message);
  // Assignment rows count from 0 in the issues list too, but they are not classes: keep them apart.
  const patternIssue = (i: number) => issues.find((x) => x.field === "pattern" && x.row === i)?.message;
  const classIssues = issues.filter((i) => i.field !== "pattern");

  const owned = useMemo(() => {
    if (!draft) return new Map<string, number>();
    const classes = applyAssignments(draft.classes, draft.rows);
    const counts = new Map<string, number>();
    for (const n of nets) counts.set(classOfNet(classes, n, draft.def.name), (counts.get(classOfNet(classes, n, draft.def.name)) ?? 0) + 1);
    return counts;
  }, [draft, nets]);

  const setClass = (row: number, patch: Partial<NetClass>) =>
    page.edit((d) => (row === 0 ? { ...d, def: { ...d.def, ...patch } } : { ...d, classes: d.classes.map((c, j) => (j === row - 1 ? { ...c, ...patch } : c)) }));
  const rename = (row: number, name: string) =>
    page.edit((d) => {
      const old = d.classes[row - 1]!.name;
      return { ...d, classes: d.classes.map((c, j) => (j === row - 1 ? { ...c, name } : c)), rows: d.rows.map((r) => (r.netclass === old ? { ...r, netclass: name } : r)) };
    });
  const addClass = () => {
    if (!draft) return;
    page.edit((d) => ({ ...d, classes: [...d.classes, { name: newClassName([d.def.name, ...d.classes.map((c) => c.name)]), nets: [], priority: 0 }] }));
    setSelClass(draft.classes.length + 1);
  };
  const removeClass = () => {
    if (!draft || selClass === 0) return;
    const gone = draft.classes[selClass - 1]!.name;
    page.edit((d) => ({ ...d, classes: d.classes.filter((_, j) => j !== selClass - 1), rows: d.rows.filter((r) => r.netclass !== gone) }));
    setSelClass(Math.min(selClass, draft.classes.length - 1));
    setSelRow(null);
  };
  const moveClass = (by: -1 | 1) => {
    if (!draft || selClass === 0) return;
    const j = selClass - 1 + by;
    if (j < 0 || j >= draft.classes.length) return;
    page.edit((d) => {
      const classes = [...d.classes];
      [classes[selClass - 1], classes[j]] = [classes[j]!, classes[selClass - 1]!];
      return { ...d, classes };
    });
    setSelClass(selClass + by);
  };
  const setRow = (i: number, patch: Partial<AssignmentRow>) => page.edit((d) => ({ ...d, rows: d.rows.map((r, j) => (j === i ? { ...r, ...patch } : r)) }));
  const addRow = () => {
    if (!draft || draft.classes.length === 0) return;
    // A new pattern starts on the selected class (KiCad starts on the first non-Default one).
    const owner = selClass > 0 ? draft.classes[selClass - 1]!.name : draft.classes[0]!.name;
    page.edit((d) => ({ ...d, rows: [...d.rows, { pattern: "", netclass: owner }] }));
    setSelRow(draft.rows.length);
  };
  const removeRow = (i: number) => {
    page.edit((d) => ({ ...d, rows: d.rows.filter((_, j) => j !== i) }));
    setSelRow(null);
  };

  const onApply = () => {
    if (!draft) return;
    if (issues.length > 0) {
      apply.setMessage({ text: `${issues[0]!.message} Fix the values marked in red first.`, bad: true });
      return;
    }
    // A row typed as `a|b` becomes two patterns; a row with nothing in it is dropped (applyAssignments).
    const classes = applyAssignments(draft.classes, draft.rows).map((c) => ({ ...c, name: c.name.trim() }));
    void apply.run({ op: "set_net_classes", settings: { default: { ...draft.def, name: "Default", nets: [] }, classes } }, page.markClean);
  };

  const selected = selRow !== null && draft ? draft.rows[selRow] : undefined;
  const matching = selected ? netsMatching(selected.pattern, nets) : [];
  const listId = "bs-net-names";

  return (
    <PageFrame title="Net Classes" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null), setSelRow(null))}>
      {!draft ? (
        <p className="bs-note">This backend does not report the board's net classes; update the eda binary.</p>
      ) : (
        <>
          <div className="bs-section">Netclasses</div>
          <p className="bs-note">
            A blank size takes the Default class' value. A net belongs to the first class in the table with a pattern that matches it, and to Default when none does; use Up and Down to change the order.
          </p>
          <div className="bs-scroll-x">
            <table className="bs-grid">
              <thead>
                <tr>
                  <th>Name</th>
                  {COLUMNS.map((c) => (
                    <th key={c.field} title={c.title}>
                      {c.label}
                    </th>
                  ))}
                  <th title="Routing order: nets of a class with a higher priority are routed first (this app's own setting, not a KiCad one)">Priority</th>
                  <th title="Nets of the board this class owns now">Nets</th>
                </tr>
              </thead>
              <tbody>
                {[draft.def, ...draft.classes].map((c, row) => (
                  <tr key={row} className={selClass === row ? "bs-sel" : undefined} onClick={() => setSelClass(row)}>
                    <td>
                      {row === 0 ? (
                        <span title="The default net class is required.">Default</span>
                      ) : (
                        <input type="text" className={`bs-name${issueAt.has(`${row}:name`) ? " bs-invalid" : ""}`} aria-label={`Netclass ${row} name`} title={issueAt.get(`${row}:name`)} value={c.name} spellCheck={false} onFocus={() => setSelClass(row)} onChange={(e) => rename(row, e.target.value)} />
                      )}
                    </td>
                    {COLUMNS.map((col) => (
                      <td key={col.field}>
                        <LengthField
                          ariaLabel={`${row === 0 ? "Default" : c.name} ${col.label}`}
                          title={issueAt.get(`${row}:${col.field}`) ?? col.title}
                          units={units}
                          optional={row !== 0 || !col.required}
                          value={c[col.field]}
                          invalid={issueAt.has(`${row}:${col.field}`)}
                          width={66}
                          onChange={(v) => setClass(row, { [col.field]: v })}
                        />
                      </td>
                    ))}
                    <td>
                      <NumberField ariaLabel={`${row === 0 ? "Default" : c.name} Priority`} title={issueAt.get(`${row}:priority`)} width={44} value={c.priority} invalid={issueAt.has(`${row}:priority`)} onChange={(v) => setClass(row, { priority: v ?? Number.NaN })} />
                    </td>
                    <td>{owned.get(c.name) ?? 0}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="bs-note">Sizes are in {units}.</p>
          <div className="bs-toolbar">
            <button onClick={addClass}>Add Netclass</button>
            <button onClick={removeClass} disabled={selClass === 0}>
              Remove Netclass
            </button>
            <button onClick={() => moveClass(-1)} disabled={selClass <= 1}>
              Up
            </button>
            <button onClick={() => moveClass(1)} disabled={selClass === 0 || selClass >= draft.classes.length}>
              Down
            </button>
          </div>
          {classIssues.length > 0 && (
            <ul className="bs-issues">
              {classIssues.slice(0, 4).map((i, k) => (
                <li key={k}>
                  {i.row === 0 ? "Default" : draft.classes[i.row - 1]?.name || `Netclass ${i.row}`}: {i.message}
                </li>
              ))}
            </ul>
          )}

          <div className="bs-section">Netclass Assignments</div>
          <datalist id={listId}>
            {nets.map((n) => (
              <option key={n} value={n} />
            ))}
          </datalist>
          <table className="bs-grid">
            <thead>
              <tr>
                <th title="Net names the class owns: * is any run of characters, ? any one character, and the pattern must match the whole name. a|b stands for two patterns.">Pattern</th>
                <th>Net Class</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {draft.rows.map((r, i) => (
                <tr key={i} className={selRow === i ? "bs-sel" : undefined} onClick={() => setSelRow(i)}>
                  <td>
                    <input
                      type="text"
                      list={listId}
                      className={`bs-pattern${patternIssue(i) ? " bs-invalid" : ""}`}
                      aria-label={`Pattern ${i + 1}`}
                      title={patternIssue(i)}
                      value={r.pattern}
                      spellCheck={false}
                      onFocus={() => setSelRow(i)}
                      onChange={(e) => setRow(i, { pattern: e.target.value })}
                    />
                  </td>
                  <td>
                    <select aria-label={`Net class of pattern ${i + 1}`} value={r.netclass} onFocus={() => setSelRow(i)} onChange={(e) => setRow(i, { netclass: e.target.value })}>
                      {!draft.classes.some((c) => c.name === r.netclass) && <option value={r.netclass}>{r.netclass || "(none)"}</option>}
                      {draft.classes.map((c, k) => (
                        <option key={k} value={c.name}>
                          {c.name}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td>
                    <button aria-label={`Remove pattern ${i + 1}`} title="Remove this assignment" onClick={() => removeRow(i)}>
                      Remove
                    </button>
                  </td>
                </tr>
              ))}
              {draft.rows.length === 0 && (
                <tr>
                  <td colSpan={3} className="bs-note">
                    No patterns: every net is in the Default class.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
          <div className="bs-toolbar">
            <button onClick={addRow} disabled={draft.classes.length === 0} title={draft.classes.length === 0 ? "Add a net class first" : undefined}>
              Add Assignment
            </button>
          </div>
          <div className="bs-matches" aria-label="Nets matching the selected pattern">
            {selected && selected.pattern.trim() !== "" ? (
              <>
                <b>Nets matching '{selected.pattern}':</b>
                {matching.length === 0 ? <div>(none)</div> : matching.map((n) => <div key={n}>{n}</div>)}
              </>
            ) : (
              <span className="bs-unit">Select a pattern to see the nets it matches.</span>
            )}
          </div>
        </>
      )}
    </PageFrame>
  );
}

/** Pre-add a pattern assignment to a draft the way the Assign Netclass dialog does (shared with components/AssignNetclassDialog.tsx via kicad-port). */
export { addAssignment };
