// Board Setup > Design Rules > Net Classes -- port of common/dialogs/panel_setup_netclasses.cpp: the table of classes (the Default class first, then
// the others, each size left blank to inherit the Default's) and the table of Netclass Assignments (a net-name pattern and the class that owns
// the nets it matches), with the list of nets the selected pattern matches. Apply sends the whole page (`set_net_classes`); the rules the
// router, DRC and the derived `.kicad_pro` use follow, and Undo takes the edit back.
//
// A pattern is read the way KiCad reads it (`EDA_COMBINED_MATCHER( pattern, CTX_NETCLASS )`, kicad-port/netPattern.ts): as a wildcard pattern (`*` any run of characters, `?` one character)
// or as a regular expression (`USB_D[PM]`, `(CLK|DAT)[0-3]`), and the whole net name has to match. A net is in every class with a pattern that matches it, and has the settings of all of
// them (`GetEffectiveNetClass`): where several set one, the class higher in the table wins -- a class's place in the table is its priority; the Up and Down buttons move a class -- and the
// Default class gives the rest. A pattern in bus notation (`D[0..7]`, `{SDA SCL}`) is stored as its members, one row each (`SetNetclassPatternAssignment`,
// kicad-port/busPattern.ts), as KiCad shows it the next time the page opens.
//
// The board's columns end with "Tuning Profile" (`GRID_DELAY_PROFILE`: the class's tuning profile, a choice of the project's profiles with a blank first,
// `UpdateDelayProfileNames`) and "PCB Color" (`GRID_PCB_COLOR`: the class's colour on the board; transparent is none, "Set color to transparent to use layer
// default color."; "Import colors from schematic" copies each class's schematic colour, `OnImportColorsClick`); the Default row has no colour. The profile is
// the project's (`tuning_profiles.class_profiles`) and goes with the page's Apply in the same undo step (`batch` of `set_net_classes` and
// `set_tuning_profiles`); the colour is the one the Appearance panel's Net Classes tab edits (`NETCLASS::GetPcbColor`, kept in `appearance.json` with the
// person's view settings, outside the undo history), written when Apply succeeds.
//
// Schematic Setup > Project > Net Classes is the same panel (`PANEL_SETUP_NETCLASSES( .., aIsEEschema = true )`) over the same table: `editor="sch"` shows the
// Eeschema columns instead of the board's (`ShowHideColumns( "0 11 12 13 14" )`: Wire Thickness, Bus Thickness, Color, Line Style), the Default row's colour and
// line style read-only (`loadNetclasses`) and its widths required (`GRID_CELL_MARK_AS_NULLABLE( false )`). Either page sends the whole table, the columns it does
// not show as it found them, so the two dialogs edit one set as KiCad's edit the project's one `NET_SETTINGS`; Undo from either tab takes it back.
import { useEffect, useMemo, useState } from "react";
import { useStudioDispatch, useStudioState } from "../../state/store";
import { LINE_STYLE_NAMES, SCH_COLOR_HELP, cssFromHex, defaultClassSchematic, hexFromCss, lineStyleOfRow, lineStyleRow, schematicColumnsIssue, schematicOf, schematicToSend } from "../../kicad-port/schSetup";
import { applyAssignments, assignmentRows, boardNetNames, classOfNet, classesOfNet, effectiveClassName, netClassSettingsOf, netsMatching, validateNetClasses, type AssignmentRow, type NetClassIssue } from "../../kicad-port/boardSetupRules";
import { hasRegexSyntax, isRegex } from "../../kicad-port/netPattern";
import type { Cmd, NetClass, NetClassSchematic } from "../../api/types";
import { LengthField, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

interface Draft {
  def: NetClass;
  classes: NetClass[];
  rows: AssignmentRow[];
  /** The schematic's columns by class name (Schematic Setup's; Board Setup carries them as found). */
  schematic: Record<string, NetClassSchematic>;
  /** "Tuning Profile" by class name (`NETCLASS::GetTuningProfile`; "" or absent: none). Schematic Setup carries it as found. */
  profiles: Record<string, string>;
  /** "PCB Color" by class name (`NETCLASS::GetPcbColor`, a CSS colour; absent: transparent). Schematic Setup carries it as found. */
  colors: Record<string, string>;
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

export function NetClassesPage({ hidden, onDirty, editor = "pcb" }: PageProps & { editor?: "pcb" | "sch" }) {
  const sch = editor === "sch";
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const board = state.board;
  const rules = board?.board_rules;
  const units = state.units;
  const nets = useMemo(() => (board ? boardNetNames(board) : []), [board]);
  const storedColors = state.appearance.netclassColors;

  const source = useMemo<Draft | undefined>(() => {
    if (!rules) return undefined;
    const { default: def, classes, schematic } = netClassSettingsOf(rules);
    return { def, classes, rows: assignmentRows(classes), schematic: { ...(schematic ?? {}) }, profiles: { ...(rules.tuning_profiles?.class_profiles ?? {}) }, colors: { ...storedColors } };
  }, [rules, storedColors]);
  // `UpdateDelayProfileNames`: a blank, then the project's profiles.
  const profileNames = useMemo(() => (rules?.tuning_profiles?.profiles ?? []).map((p) => p.name).filter((n) => n !== ""), [rules]);
  const page = usePageDraft<Draft>(source);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);

  const [selClass, setSelClass] = useState(0); // 0 = the Default class, i + 1 = classes[i]
  const [selRow, setSelRow] = useState<number | null>(null);
  const draft = page.draft;
  const issues: NetClassIssue[] = draft ? validateNetClasses(draft.def, draft.classes, draft.rows) : [];
  // the Eeschema columns' checks (Schematic Setup's page only: Board Setup sends them as it found them)
  const schIssues: { row: number; message: string }[] = !draft || !sch ? [] : [draft.def, ...draft.classes].flatMap((c, row) => {
    const look = row === 0 ? defaultClassSchematic(draft.schematic.Default) : schematicOf(draft.schematic, c.name);
    const m = schematicColumnsIssue(look, row === 0);
    return m ? [{ row, message: m }] : [];
  });
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

  // The nets that are in more than one class: KiCad composes their settings (`GetEffectiveNetClass`), the class higher in the table winning where several set one.
  const composite = useMemo(() => {
    if (!draft) return [];
    const classes = applyAssignments(draft.classes, draft.rows);
    return nets.filter((n) => classesOfNet(classes, n).length > 1).map((n) => ({ net: n, name: effectiveClassName(classes, n, draft.def.name) }));
  }, [draft, nets]);

  const setClass = (row: number, patch: Partial<NetClass>) =>
    page.edit((d) => (row === 0 ? { ...d, def: { ...d.def, ...patch } } : { ...d, classes: d.classes.map((c, j) => (j === row - 1 ? { ...c, ...patch } : c)) }));
  const rename = (row: number, name: string) =>
    page.edit((d) => {
      const old = d.classes[row - 1]!.name;
      // the class's schematic columns go with its name
      const schematic = { ...d.schematic };
      const profiles = { ...d.profiles };
      const colors = { ...d.colors };
      if (!d.classes.some((c, j) => j !== row - 1 && c.name === old)) {
        for (const byName of [schematic, profiles, colors] as Record<string, unknown>[]) {
          if (byName[old] !== undefined) {
            byName[name] = byName[old];
            delete byName[old];
          }
        }
      }
      return { ...d, classes: d.classes.map((c, j) => (j === row - 1 ? { ...c, name } : c)), rows: d.rows.map((r) => (r.netclass === old ? { ...r, netclass: name } : r)), schematic, profiles, colors };
    });
  const setLook = (row: number, patch: Partial<NetClassSchematic>) =>
    page.edit((d) => {
      const name = row === 0 ? "Default" : d.classes[row - 1]!.name;
      return { ...d, schematic: { ...d.schematic, [name]: { ...(d.schematic[name] ?? {}), ...patch } } };
    });
  const addClass = () => {
    if (!draft) return;
    page.edit((d) => ({ ...d, classes: [...d.classes, { name: newClassName([d.def.name, ...d.classes.map((c) => c.name)]), nets: [], priority: 0 }] }));
    setSelClass(draft.classes.length + 1);
  };
  const removeClass = () => {
    if (!draft || selClass === 0) return;
    const gone = draft.classes[selClass - 1]!.name;
    page.edit((d) => {
      const schematic = { ...d.schematic };
      delete schematic[gone];
      const profiles = { ...d.profiles };
      delete profiles[gone];
      const colors = { ...d.colors };
      delete colors[gone];
      return { ...d, classes: d.classes.filter((_, j) => j !== selClass - 1), rows: d.rows.filter((r) => r.netclass !== gone), schematic, profiles, colors };
    });
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
    if (schIssues.length > 0) {
      apply.setMessage({ text: `${schIssues[0]!.message} Fix the values marked in red first.`, bad: true });
      return;
    }
    // A row typed as `a|b` becomes two patterns; a row with nothing in it is dropped (applyAssignments).
    const classes = applyAssignments(draft.classes, draft.rows).map((c) => ({ ...c, name: c.name.trim() }));
    // the schematic columns under the names as sent (a name is trimmed as the class is)
    const looks: Record<string, NetClassSchematic> = {};
    for (const [k, v] of Object.entries(draft.schematic)) looks[k.trim()] = v;
    const schematic = schematicToSend(looks, ["Default", ...classes.map((c) => c.name)]);
    const names = ["Default", ...classes.map((c) => c.name)];
    const byTrimmed = <T,>(m: Record<string, T>) => Object.fromEntries(Object.entries(m).map(([k, v]) => [k.trim(), v] as const));
    // the class's tuning profile (`nc->SetTuningProfile( GetCellValue( aRow, GRID_DELAY_PROFILE ) )`): the classes of the page, a blank one names none
    const profiles = byTrimmed(draft.profiles);
    const classProfiles = Object.fromEntries(names.filter((n) => (profiles[n] ?? "") !== "").map((n) => [n, profiles[n]!]));
    const stored = rules?.tuning_profiles;
    const sameProfiles = JSON.stringify(Object.entries(classProfiles).sort()) === JSON.stringify(Object.entries(stored?.class_profiles ?? {}).sort());
    const setClasses: Cmd = { op: "set_net_classes", settings: { default: { ...draft.def, name: "Default", nets: [] }, classes, schematic } };
    const cmd: Cmd = sameProfiles ? setClasses : { op: "batch", cmds: [setClasses, { op: "set_tuning_profiles", settings: { profiles: stored?.profiles ?? [], ...(Object.keys(classProfiles).length > 0 ? { class_profiles: classProfiles } : {}) } }] };
    // the PCB colour (`if( !nc->IsDefault() ) .. if( newPcbColor != COLOR4D::UNSPECIFIED ) nc->SetPcbColor( newPcbColor )`): the Appearance panel's, written once Apply succeeds
    const colors = byTrimmed(draft.colors);
    void apply.run(cmd, () => {
      page.markClean();
      for (const n of new Set([...Object.keys(storedColors), ...names.slice(1)])) {
        const want = names.slice(1).includes(n) ? (colors[n] ?? null) : null;
        if ((storedColors[n] ?? null) !== want) dispatch({ type: "APPEARANCE", op: { op: "netclass_color", name: n, color: want } });
      }
    });
  };

  // `OnImportColorsClick`: each class the project has takes its schematic colour as its PCB colour (an unspecified one is transparent).
  const importColors = () =>
    page.edit((d) => {
      const colors = { ...d.colors };
      for (const c of d.classes) {
        if (!(rules?.net_classes ?? []).some((k) => k.name === c.name)) continue;
        const sch = rules?.net_class_schematic?.[c.name]?.color ?? null;
        if (sch) colors[c.name] = sch;
        else delete colors[c.name];
      }
      return { ...d, colors };
    });
  const setProfile = (name: string, profile: string) => page.edit((d) => ({ ...d, profiles: { ...d.profiles, [name]: profile } }));
  const setColor = (name: string, color: string | null) =>
    page.edit((d) => {
      const colors = { ...d.colors };
      if (color === null) delete colors[name];
      else colors[name] = color;
      return { ...d, colors };
    });

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
            A blank size takes the Default class' value. A net is in every class with a pattern that matches it and has the settings of all of them: where several classes set the same size, the one higher in the table wins, and Default gives the rest. Use Up and Down to change the order.
          </p>
          <div className="bs-scroll-x">
            <table className="bs-grid">
              <thead>
                <tr>
                  <th>Name</th>
                  {!sch &&
                    COLUMNS.map((c) => (
                      <th key={c.field} title={c.title}>
                        {c.label}
                      </th>
                    ))}
                  {!sch && <th>Tuning Profile</th>}
                  {!sch && <th title="PCB netclass color">PCB Color</th>}
                  {sch && (
                    <>
                      <th title="Schematic wire thickness">Wire Thickness</th>
                      <th title="Bus wire thickness">Bus Thickness</th>
                      <th title="Schematic wire color">Color</th>
                      <th title="Schematic wire line style">Line Style</th>
                    </>
                  )}
                  {!sch && <th title="Routing order: nets of a class with a higher priority are routed first (this app's own setting, not a KiCad one)">Priority</th>}
                  {!sch && <th title="Nets of the board whose first class this is (a net in several classes is listed under the one that takes precedence)">Nets</th>}
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
                    {!sch &&
                      COLUMNS.map((col) => (
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
                    {!sch && (
                      <td>
                        <ProfileCell name={row === 0 ? "Default" : c.name} value={draft.profiles[row === 0 ? "Default" : c.name] ?? ""} names={profileNames} onChange={(v) => setProfile(row === 0 ? "Default" : c.name, v)} />
                      </td>
                    )}
                    {!sch && (
                      <td style={{ whiteSpace: "nowrap" }}>
                        {row === 0 ? <span className="bs-unit" title="The Default class has no colour of its own.">(layer colour)</span> : <PcbColorCell name={c.name} value={draft.colors[c.name] ?? null} onChange={(v) => setColor(c.name, v)} />}
                      </td>
                    )}
                    {sch && <SchematicCells row={row} name={row === 0 ? "Default" : c.name} look={row === 0 ? defaultClassSchematic(draft.schematic.Default) : schematicOf(draft.schematic, c.name)} units={units} issue={schIssues.find((i) => i.row === row)?.message} onChange={(patch) => setLook(row, patch)} />}
                    {!sch && (
                      <td>
                        <NumberField ariaLabel={`${row === 0 ? "Default" : c.name} Priority`} title={issueAt.get(`${row}:priority`)} width={44} value={c.priority} invalid={issueAt.has(`${row}:priority`)} onChange={(v) => setClass(row, { priority: v ?? Number.NaN })} />
                      </td>
                    )}
                    {!sch && <td>{owned.get(c.name) ?? 0}</td>}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="bs-note">Sizes are in {units}.</p>
          {!sch && (
            <div className="bs-toolbar" style={{ alignItems: "center" }}>
              <span className="bs-note" style={{ fontStyle: "italic", margin: 0 }}>
                Set color to transparent to use layer default color.
              </span>
              <button onClick={importColors}>Import colors from schematic</button>
            </div>
          )}
          {sch && (
            <p className="bs-note" style={{ fontStyle: "italic" }}>
              {SCH_COLOR_HELP}
            </p>
          )}
          {sch && schIssues.length > 0 && (
            <ul className="bs-issues">
              {schIssues.slice(0, 4).map((i, k) => (
                <li key={k}>
                  {i.row === 0 ? "Default" : draft.classes[i.row - 1]?.name || `Netclass ${i.row}`}: {i.message}
                </li>
              ))}
            </ul>
          )}
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
                <th title="Net names the class is for: a wildcard pattern (* is any run of characters, ? any one character) or a regular expression such as USB_D[PM] or (CLK|DAT)[0-3]. The pattern must match the whole name.">Pattern</th>
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
                {hasRegexSyntax(selected.pattern.trim()) && !isRegex(selected.pattern.trim()) && <div className="bs-unit">Not a regular expression this port reads: it matches as a wildcard pattern only.</div>}
                {matching.length === 0 ? <div>(none)</div> : matching.map((n) => <div key={n}>{n}</div>)}
              </>
            ) : (
              <span className="bs-unit">Select a pattern to see the nets it matches.</span>
            )}
          </div>
          <div className="bs-matches" aria-label="Nets in more than one class">
            <b>
              {composite.length === 0 ? "No net is in more than one class." : `${composite.length} net${composite.length === 1 ? " is" : "s are"} in more than one class (their settings are composed):`}
            </b>
            {composite.slice(0, 12).map((c) => (
              <div key={c.net}>
                {c.net}: {c.name}
              </div>
            ))}
            {composite.length > 12 && <div className="bs-unit">... and {composite.length - 12} more</div>}
          </div>
        </>
      )}
    </PageFrame>
  );
}

/**
 * The Eeschema columns of one row (`GRID_WIREWIDTH` .. `GRID_LINESTYLE`): the widths in the editor's units (blank: not set, but on the Default row, which must have
 * them), the colour (a swatch; "none" is `COLOR4D::UNSPECIFIED`, the wire layer's colour) and the line style (`g_lineStyleNames`). The Default row's colour and
 * line style are read-only (`loadNetclasses`: `SetReadOnly( aRow, GRID_SCHEMATIC_COLOR )`, `GRID_LINESTYLE`).
 */
function SchematicCells({ row, name, look, units, issue, onChange }: { row: number; name: string; look: NetClassSchematic; units: import("../../state/units").LengthUnit; issue?: string; onChange: (patch: Partial<NetClassSchematic>) => void }) {
  const isDefault = row === 0;
  const hex = hexFromCss(look.color);
  return (
    <>
      <td>
        <LengthField ariaLabel={`${name} Wire Thickness`} title={issue ?? "Schematic wire thickness"} units={units} optional={!isDefault} value={look.wire_width ?? undefined} invalid={!!issue && /Wire/.test(issue)} width={66} onChange={(v) => onChange({ wire_width: v ?? null })} />
      </td>
      <td>
        <LengthField ariaLabel={`${name} Bus Thickness`} title={issue ?? "Bus wire thickness"} units={units} optional={!isDefault} value={look.bus_width ?? undefined} invalid={!!issue && /Bus/.test(issue)} width={66} onChange={(v) => onChange({ bus_width: v ?? null })} />
      </td>
      <td style={{ whiteSpace: "nowrap" }}>
        {isDefault ? (
          <span className="bs-unit" title="The Default class's wires have the wire layer's colour.">(layer colour)</span>
        ) : (
          <>
            <input type="color" aria-label={`${name} Color`} title="Schematic wire color" value={hex ?? "#000000"} style={{ width: 32, opacity: hex ? 1 : 0.35 }} onChange={(e) => onChange({ color: cssFromHex(e.target.value) })} />
            <button aria-label={`${name} Color none`} title="No colour of its own: the wire layer's" disabled={!hex} onClick={() => onChange({ color: null })}>
              ×
            </button>
          </>
        )}
      </td>
      <td>
        <select aria-label={`${name} Line Style`} title="Schematic wire line style" disabled={isDefault} value={lineStyleRow(look.line_style)} onChange={(e) => onChange({ line_style: lineStyleOfRow(Number(e.target.value)) })}>
          {LINE_STYLE_NAMES.map((n, i) => (
            <option key={n} value={i}>
              {n}
            </option>
          ))}
        </select>
      </td>
    </>
  );
}

/** The Tuning Profile cell (`GRID_DELAY_PROFILE`, `wxGridCellChoiceEditor( profileNames, false )`): a blank, then the project's profiles; a name the project does not have is kept. */
function ProfileCell({ name, value, names, onChange }: { name: string; value: string; names: readonly string[]; onChange: (v: string) => void }) {
  const missing = value !== "" && !names.includes(value);
  return (
    <select aria-label={`${name} Tuning Profile`} className={missing ? "bs-invalid" : undefined} title={missing ? "No such tuning profile: kicad-cli reports it as missing." : undefined} value={value} onChange={(e) => onChange(e.target.value)}>
      <option value="" />
      {missing && <option value={value}>{value}</option>}
      {names.map((n) => (
        <option key={n} value={n}>
          {n}
        </option>
      ))}
    </select>
  );
}

/** The PCB Color cell (`GRID_PCB_COLOR`, `GRID_CELL_COLOR_SELECTOR`): a swatch, and "none" for transparent (`COLOR4D::UNSPECIFIED`: the layer's colour). */
function PcbColorCell({ name, value, onChange }: { name: string; value: string | null; onChange: (v: string | null) => void }) {
  const hex = hexFromCss(value);
  return (
    <>
      <input type="color" aria-label={`${name} PCB Color`} title="PCB netclass color" value={hex ?? "#000000"} style={{ width: 32, opacity: hex ? 1 : 0.35 }} onChange={(e) => onChange(cssFromHex(e.target.value))} />
      <button aria-label={`${name} PCB Color none`} title="No colour of its own: the layer's" disabled={!hex} onClick={() => onChange(null)}>
        ×
      </button>
    </>
  );
}
