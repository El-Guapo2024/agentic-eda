// Schematic Setup > General > Field Name Templates -- port of eeschema/dialogs/panel_template_fieldnames.cpp with a project's template manager
// ("Project Field Name Templates", `SCHEMATIC_SETTINGS::m_TemplateFieldNames`, the project's `drawing.field_names`): a grid of Name, Visible and URL, with
// Add (a row "Untitled Field", not visible: `OnAddButtonClick`), Delete, Move Up and Move Down. Apply is `TransferDataFromWindow`: a name with white space at
// an end is asked about first (the question and its two answers word for word: Remove White Space / Keep White Space), then every row with a name is kept as
// `AddTemplateFieldName` keeps it (a mandatory field's name is no template, a second row of a name replaces the first) -- one undoable
// `set_field_name_templates`. Symbol Properties offers each template a symbol lacks (`DIALOG_SYMBOL_PROPERTIES::TransferDataToWindow`), shown as the row's
// Visible box says. The global templates of the user's own settings (`EESCHEMA_SETTINGS::m_Drawing.field_names`) are Preferences', not this page's.
import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchSchSetup } from "../../api/client";
import type { SchSetupReply, TemplateFieldName } from "../../api/types";
import { NEW_TEMPLATE_FIELD, WHITESPACE_EXTENDED, WHITESPACE_KEEP, WHITESPACE_REMOVE, firstWhitespaceRow, keptTemplateFields, moveRow } from "../../kicad-port/schSetup";
import { useStudioState } from "../../state/store";
import { PageFrame, usePageApply, usePageDraft, type PageProps, useSchSetupImport } from "../boardSetup/fields";

export function FieldNameTemplatesPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const [info, setInfo] = useState<SchSetupReply | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  // The white-space question of the row Apply is asking about (`KICAD_MESSAGE_DIALOG` "Warning"), and the rows kept as they are so far.
  const [asking, setAsking] = useState<{ row: number; message: string } | null>(null);
  const load = useCallback(async () => {
    try {
      const r = await fetchSchSetup();
      if (r.ok) {
        setInfo(r);
        setProblem(null);
      } else setProblem(r.message ?? "The backend could not read the field name templates.");
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load, state.version]);

  const source = useMemo(() => (info ? info.field_name_templates : undefined), [info]);
  const page = usePageDraft<TemplateFieldName[]>(source);
  // "Import Settings from Another Project..." (`ImportSettingsFrom`, UI-LEFT-2 BB4)
  useSchSetupImport("field_templates", (s) => s.setup.field_name_templates, page.importDraft);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const rows = page.draft;
  const setRow = (i: number, patch: Partial<TemplateFieldName>) => page.edit((r) => r.map((f, j) => (j === i ? { ...f, ...patch } : f)));

  /** Ask about the next name with white space at an end, or send the page. `done` are the rows whose question was answered. */
  const proceed = (list: TemplateFieldName[], answered: ReadonlySet<number>) => {
    const next = firstWhitespaceRow(list.map((f, i) => (answered.has(i) ? { ...f, name: f.name.trim() || f.name } : f)));
    if (next && !answered.has(next.row)) {
      setAsking(next);
      return;
    }
    setAsking(null);
    void apply.run({ op: "set_field_name_templates", fields: keptTemplateFields(list) }, () => {
      page.markClean();
      void load();
    });
  };
  const [answered, setAnswered] = useState<Set<number>>(new Set());
  const onApply = () => {
    if (!rows || !page.dirty) return;
    setAnswered(new Set());
    proceed(rows, new Set());
  };
  const answer = (remove: boolean) => {
    if (!rows || !asking) return;
    const list = remove ? rows.map((f, i) => (i === asking.row ? { ...f, name: f.name.trim() } : f)) : rows;
    if (remove) page.edit(() => list);
    const done = new Set(answered).add(asking.row);
    setAnswered(done);
    proceed(list, done);
  };

  return (
    <PageFrame title="Field Name Templates" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? problem} bad={apply.message?.bad || problem !== null} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null), setAsking(null))}>
      {!rows ? (
        <p className="bs-note">{problem ?? "Reading the field name templates…"}</p>
      ) : (
        <>
          <div className="bs-section">Project Field Name Templates</div>
          <table className="bs-grid" aria-label="Field name templates">
            <thead>
              <tr>
                <th>Name</th>
                <th>Visible</th>
                <th>URL</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((f, i) => (
                <tr key={i} className={selected === i ? "bs-sel" : undefined} onClick={() => setSelected(i)}>
                  <td>
                    <input className="bs-name" aria-label={`Field name ${i + 1}`} value={f.name} spellCheck={false} onFocus={() => setSelected(i)} onChange={(e) => setRow(i, { name: e.target.value })} style={{ width: 220 }} />
                  </td>
                  <td style={{ textAlign: "center" }}>
                    <input type="checkbox" aria-label={`Field ${i + 1} visible`} checked={f.visible} onChange={(e) => setRow(i, { visible: e.target.checked })} />
                  </td>
                  <td style={{ textAlign: "center" }}>
                    <input type="checkbox" aria-label={`Field ${i + 1} URL`} checked={f.url} onChange={(e) => setRow(i, { url: e.target.checked })} />
                  </td>
                </tr>
              ))}
              {rows.length === 0 && (
                <tr>
                  <td colSpan={3} className="bs-note">
                    No field name templates.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
          <div className="bs-toolbar">
            <button
              aria-label="Add field"
              onClick={() => {
                page.edit((r) => [...r, { ...NEW_TEMPLATE_FIELD }]);
                setSelected(rows.length);
              }}
            >
              Add
            </button>
            <button
              aria-label="Delete field"
              disabled={selected === null || selected >= rows.length}
              onClick={() => {
                if (selected === null) return;
                page.edit((r) => r.filter((_, j) => j !== selected));
                setSelected(rows.length > 1 ? Math.min(selected, rows.length - 2) : null);
              }}
            >
              Delete
            </button>
            <button
              title="Move up"
              disabled={selected === null || selected <= 0}
              onClick={() => {
                if (selected === null) return;
                page.edit((r) => moveRow(r, selected, -1));
                setSelected(selected - 1);
              }}
            >
              Up
            </button>
            <button
              title="Move down"
              disabled={selected === null || selected >= rows.length - 1}
              onClick={() => {
                if (selected === null) return;
                page.edit((r) => moveRow(r, selected, 1));
                setSelected(selected + 1);
              }}
            >
              Down
            </button>
          </div>
          {asking && (
            <div className="bs-matches" role="alertdialog" aria-label="Warning">
              <b>{asking.message}</b>
              <div className="bs-unit" style={{ margin: "4px 0" }}>
                {WHITESPACE_EXTENDED}
              </div>
              <button className="primary" onClick={() => answer(true)}>
                {WHITESPACE_REMOVE}
              </button>{" "}
              <button onClick={() => answer(false)}>{WHITESPACE_KEEP}</button>
            </div>
          )}
        </>
      )}
    </PageFrame>
  );
}
