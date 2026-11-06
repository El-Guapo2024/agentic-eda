// Board Setup > Design Rules > Component Classes -- port of pcbnew/dialogs/panel_assign_component_classes.cpp (`PANEL_ASSIGN_COMPONENT_CLASSES` and the
// panels it adds: `PANEL_COMPONENT_CLASS_ASSIGNMENT`, one box an assignment, and `PANEL_COMPONENT_CLASS_CONDITION_*`, one row a condition), laid out as its base
// (panel_assign_component_classes_base.cpp): "Assign component class per sheet", "Custom Assignments:" with "Add Custom Assignment", then each assignment's
// "Component class:" with its delete button, its condition rows, and the "+" menu (`OnAddConditionClick`), "Highlight matching footprints" and Match all /
// Match any.
//
// The page edits the project's `component_class_settings` (`COMPONENT_CLASS_SETTINGS`); Apply is `Validate` -- every assignment compiled as the board would
// compile it, "Error with conditions for component class assignment %s" for the first that does not (the rule engine answers, POST /api/component_classes) --
// then one undoable command (`set_component_classes`), which checks again. The derived `.kicad_pro` carries the page, so kicad-cli's DRC reads a rule's
// `A.hasComponentClass(..)` / `A.Component_Class` as the page set it. "Highlight matching footprints" brightens the footprints the assignment matches
// (`OnHighlightItemsClick`, the rule engine's `Matches`) and dims the rest; closing the dialog unbrightens them all (`~PANEL_ASSIGN_COMPONENT_CLASSES`).
// The Footprint condition's "Show library" opens the Footprint Chooser (`OnShowLibraryClick`); the Reference condition's import button offers "Import
// references from selection" -- the footprints selected when the dialog opened (`m_selectionRefs`).
import { useEffect, useMemo, useRef, useState } from "react";
import { postComponentClasses } from "../../api/client";
import type { ClassConditionKind } from "../../api/types";
import { useStudioDispatch, useStudioState } from "../../state/store";
import {
  CONDITION_MENU,
  CONDITION_TITLE,
  ROTATION_CHOICES,
  SIDE_CHOICES,
  assignmentData,
  assignmentErrorMessage,
  draftOf,
  newAssignment,
  newCondition,
  sameSettings,
  selectionRefsText,
  settingsOf,
  type AssignmentDraft,
  type ComponentClassesDraft,
  type ConditionRow,
} from "../../kicad-port/componentClasses";
import { FootprintChooserDialog } from "../FootprintChooserDialog";
import { useColorScheme } from "../../hooks/useColorScheme";
import { Check, PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

/** One of KiCad's bitmaps (`KiBitmapBundle( BITMAPS::.. )`), in the colour scheme's set. */
function Icon({ name }: { name: string }) {
  const scheme = useColorScheme();
  return <img src={`/icons/${scheme}/${name}.svg`} width={16} height={16} alt="" draggable={false} />;
}

/** A popup menu of the page (the "+" of an assignment, a Reference row's import button): where it is and what it holds. */
interface Menu {
  assignment: number;
  row?: number;
  at: { x: number; y: number };
}

export function ComponentClassesPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const rules = state.board?.board_rules;
  const known = rules !== undefined && rules.component_classes !== undefined;
  const page = usePageDraft<ComponentClassesDraft>(known ? draftOf(rules!.component_classes) : undefined);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;

  // `m_selectionRefs`: the footprints selected when the page was built (the dialog opened).
  const [selectionRefs] = useState(() => {
    const parts = new Set((state.board?.parts ?? []).map((p) => p.ref));
    return selectionRefsText([...state.selection].filter((id) => parts.has(id)));
  });
  // `m_fieldNames` / `m_sheetNames`: the board's footprints' field and sheet names, from the rule engine's board.
  const [lists, setLists] = useState<{ fields: string[]; sheets: string[] }>({ fields: [], sheets: [] });
  const [invalid, setInvalid] = useState<number | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [chooser, setChooser] = useState<{ assignment: number; row: number; value: string } | null>(null);
  const highlighted = useRef(false);
  const boxes = useRef<(HTMLDivElement | null)[]>([]);

  useEffect(() => {
    if (!known) return;
    let live = true;
    void postComponentClasses([]).then((r) => {
      if (live && r.ok) setLists({ fields: r.fields ?? [], sheets: r.sheets ?? [] });
    });
    return () => {
      live = false;
    };
  }, [known, state.version]);

  // `~PANEL_ASSIGN_COMPONENT_CLASSES`: every footprint unbrightened when the dialog goes.
  useEffect(
    () => () => {
      if (highlighted.current) dispatch({ type: "SET_HOT", refs: [] });
    },
    [dispatch]
  );

  const editAssignment = (i: number, fn: (a: AssignmentDraft) => AssignmentDraft) => page.edit((d) => ({ ...d, assignments: d.assignments.map((a, k) => (k === i ? fn(a) : a)) }));
  const editRow = (i: number, r: number, patch: Partial<ConditionRow>) => editAssignment(i, (a) => ({ ...a, conditions: a.conditions.map((c, k) => (k === r ? { ...c, ...patch } : c)) }));
  const addCondition = (i: number, kind: ClassConditionKind) => editAssignment(i, (a) => ({ ...a, conditions: [...a.conditions, newCondition(kind)] }));
  const removeCondition = (i: number, r: number) => editAssignment(i, (a) => ({ ...a, conditions: a.conditions.filter((_, k) => k !== r) }));
  const removeAssignment = (i: number) => {
    page.edit((d) => ({ ...d, assignments: d.assignments.filter((_, k) => k !== i) }));
    setInvalid(null);
  };
  const addAssignment = () => {
    if (!draft) return;
    page.edit((d) => ({ ...d, assignments: [...d.assignments, newAssignment()] }));
    // `scrollToAssignment`
    const at = draft.assignments.length;
    requestAnimationFrame(() => boxes.current[at]?.scrollIntoView({ block: "nearest" }));
  };

  // `OnHighlightItemsClick`: brighten the footprints the assignment matches, unbrighten the others; an assignment that does not compile changes nothing.
  const highlight = async (i: number) => {
    if (!draft) return;
    const reply = await postComponentClasses([assignmentData(draft.assignments[i]!)]);
    const matches = reply.ok ? reply.matches?.[0] : null;
    if (!matches) return;
    highlighted.current = true;
    dispatch({ type: "SET_HOT", refs: matches });
  };

  const onApply = async () => {
    if (!draft) return;
    const settings = settingsOf(draft);
    // `Validate`: the first assignment `CompileAssignmentRule` gives no rule for.
    const reply = await postComponentClasses(settings.assignments ?? []);
    if (!reply.ok) {
      apply.setMessage({ text: reply.message ?? "The board could not be read.", bad: true });
      return;
    }
    const bad = (reply.compiles ?? []).findIndex((ok) => !ok);
    if (bad >= 0) {
      setInvalid(bad);
      boxes.current[bad]?.scrollIntoView({ block: "nearest" });
      apply.setMessage({ text: assignmentErrorMessage(settings.assignments![bad]!.class), bad: true });
      return;
    }
    setInvalid(null);
    if (sameSettings(settings, rules?.component_classes) && rules?.overlay?.includes("component_classes")) {
      apply.setMessage({ text: "Nothing to apply.", bad: false });
      page.markClean();
      return;
    }
    void apply.run({ op: "set_component_classes", settings }, page.markClean);
  };

  const fieldListId = "bs-cc-fields";
  const openMenu = (e: React.MouseEvent, m: Omit<Menu, "at">) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    setMenu({ ...m, at: { x: r.left, y: r.bottom } });
  };

  const conditionRow = (a: number, c: ConditionRow, r: number) => {
    const label = `Assignment ${a + 1} condition ${r + 1}`;
    const del = (
      <button className="bs-cc-icon" aria-label={`${label} delete`} title="Delete row" onClick={() => removeCondition(a, r)}>
        <Icon name="small_trash" />
      </button>
    );
    const text = (aria: string, value: string, onChange: (v: string) => void, extra?: { list?: string; placeholder?: string }) => (
      <input type="text" className="bs-input bs-cc-grow" aria-label={aria} value={value} spellCheck={false} list={extra?.list} placeholder={extra?.placeholder} onChange={(e) => onChange(e.target.value)} />
    );
    return (
      <div key={r} className="bs-cc-cond" data-condition={c.kind}>
        <span className="bs-cc-title">{CONDITION_TITLE[c.kind]}</span>
        {c.kind === "reference" && (
          <>
            {text(`${label} references`, c.primary, (v) => editRow(a, r, { primary: v }))}
            <button className="bs-cc-icon" aria-label={`${label} import references`} title="Import references" onClick={(e) => openMenu(e, { assignment: a, row: r })}>
              <Icon name="config" />
            </button>
          </>
        )}
        {c.kind === "footprint" && (
          <>
            {text(`${label} footprint`, c.primary, (v) => editRow(a, r, { primary: v }))}
            <button className="bs-cc-icon" aria-label={`${label} show library`} title="Show library" onClick={() => setChooser({ assignment: a, row: r, value: c.primary })}>
              <Icon name="small_library" />
            </button>
          </>
        )}
        {c.kind === "side" && (
          <select aria-label={`${label} side`} value={c.primary} onChange={(e) => editRow(a, r, { primary: e.target.value })}>
            {!SIDE_CHOICES.includes(c.primary) && <option value={c.primary}>{c.primary}</option>}
            {SIDE_CHOICES.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        )}
        {c.kind === "rotation" && (
          <>
            {text(`${label} rotation`, c.primary, (v) => editRow(a, r, { primary: v }), { list: "bs-cc-rotations" })}
            <span className="bs-unit">°</span>
          </>
        )}
        {c.kind === "footprint_field" && (
          <>
            {text(`${label} field name`, c.primary, (v) => editRow(a, r, { primary: v }), { list: fieldListId })}
            <span className="bs-cc-title">Field value:</span>
            {text(`${label} field value`, c.secondary, (v) => editRow(a, r, { secondary: v }))}
          </>
        )}
        {c.kind === "custom" && text(`${label} expression`, c.primary, (v) => editRow(a, r, { primary: v }), { placeholder: "A.intersectsArea('RULE_1')" })}
        {c.kind === "sheet_name" && (
          <select aria-label={`${label} sheet`} className="bs-cc-grow" value={c.primary} onChange={(e) => editRow(a, r, { primary: e.target.value })}>
            {!lists.sheets.includes(c.primary) && <option value={c.primary}>{c.primary}</option>}
            {lists.sheets.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        )}
        {del}
      </div>
    );
  };

  const menuItems = useMemo(() => {
    if (!menu || !draft) return [];
    if (menu.row === undefined) return CONDITION_MENU.map((m) => ({ label: m.label, enabled: true, run: () => addCondition(menu.assignment, m.kind) }));
    const row = menu.row;
    return [{ label: "Import references from selection", enabled: selectionRefs !== null, run: () => selectionRefs !== null && editRow(menu.assignment, row, { primary: selectionRefs }) }];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [menu, draft, selectionRefs]);

  return (
    <PageFrame title="Component Classes" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={() => void onApply()} onRevert={() => (page.revert(), apply.setMessage(null), setInvalid(null))}>
      {!draft ? (
        <p className="bs-note">This backend does not report the project's component classes; update the eda binary.</p>
      ) : (
        <>
          <Check checked={draft.sheetClasses} onChange={(v) => page.edit((d) => ({ ...d, sheetClasses: v }))}>
            Assign component class per sheet
          </Check>
          <hr className="bs-cc-line" />
          <div className="bs-toolbar" style={{ justifyContent: "space-between" }}>
            <span>Custom Assignments:</span>
            <button onClick={addAssignment}>Add Custom Assignment</button>
          </div>
          <datalist id={fieldListId}>
            {lists.fields.map((f) => (
              <option key={f} value={f} />
            ))}
          </datalist>
          <datalist id="bs-cc-rotations">
            {ROTATION_CHOICES.map((r) => (
              <option key={r} value={r} />
            ))}
          </datalist>
          <div className="bs-cc-list" aria-label="Custom assignments">
            {draft.assignments.map((a, i) => (
              <div
                key={i}
                ref={(el) => {
                  boxes.current[i] = el;
                }}
                className={`bs-cc-box${invalid === i ? " bs-invalid" : ""}`}
                data-assignment={i}
                aria-label={`Assignment ${i + 1}`}
              >
                <div className="bs-cc-cond">
                  <span className="bs-cc-title">Component class:</span>
                  <input type="text" className="bs-input bs-cc-grow" aria-label={`Assignment ${i + 1} component class`} value={a.componentClass} spellCheck={false} onChange={(e) => editAssignment(i, (x) => ({ ...x, componentClass: e.target.value }))} />
                  <button className="bs-cc-icon" aria-label={`Assignment ${i + 1} delete`} title="Delete row" onClick={() => removeAssignment(i)}>
                    <Icon name="small_trash" />
                  </button>
                </div>
                {a.conditions.map((c, r) => conditionRow(i, c, r))}
                <div className="bs-cc-cond">
                  <button className="bs-cc-icon" aria-label={`Assignment ${i + 1} add condition`} onClick={(e) => openMenu(e, { assignment: i })}>
                    <Icon name="small_plus" />
                  </button>
                  <span className="bs-cc-grow" />
                  <button className="bs-cc-icon" aria-label={`Assignment ${i + 1} highlight`} title="Highlight matching footprints" onClick={() => void highlight(i)}>
                    <Icon name="net_highlight" />
                  </button>
                  <span className="bs-cc-sep" />
                  <label className="bs-cc-radio">
                    <input type="radio" name={`bs-cc-op-${i}`} checked={a.all} onChange={() => editAssignment(i, (x) => ({ ...x, all: true }))} /> Match all
                  </label>
                  <label className="bs-cc-radio">
                    <input type="radio" name={`bs-cc-op-${i}`} checked={!a.all} onChange={() => editAssignment(i, (x) => ({ ...x, all: false }))} /> Match any
                  </label>
                </div>
              </div>
            ))}
          </div>
          {menu && (
            <>
              <div className="bs-cc-menu-backdrop" onClick={() => setMenu(null)} />
              <div className="bs-cc-menu" role="menu" style={{ left: menu.at.x, top: menu.at.y }}>
                {menuItems.map((m) => (
                  <button
                    key={m.label}
                    role="menuitem"
                    disabled={!m.enabled}
                    onClick={() => {
                      setMenu(null);
                      m.run();
                    }}
                  >
                    {m.label}
                  </button>
                ))}
              </div>
            </>
          )}
          {chooser && (
            <FootprintChooserDialog
              preselect={chooser.value || null}
              onCancel={() => setChooser(null)}
              onChoose={(pick) => {
                if (pick.kind === "footprint") editRow(chooser.assignment, chooser.row, { primary: pick.name });
                setChooser(null);
              }}
            />
          )}
        </>
      )}
    </PageFrame>
  );
}
