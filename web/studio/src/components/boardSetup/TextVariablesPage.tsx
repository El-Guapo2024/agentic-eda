// Board Setup > Text & Graphics > Text Variables -- port of common/dialogs/panel_text_variables.cpp: the project's variables, a name and a value a row, that
// `${NAME}` in a text, a field or the title block shows (kicad-port/textVarExpand.ts) and that the derived `.kicad_pro` hands kicad-cli for its plots.
// The checks are the panel's (a name is not empty and holds none of the characters it filters out); Apply sends the whole set as one undoable command
// (`set_text_variables`). The same list is Configure Paths' board variables, seen through the names that dialog can list.
import { useEffect } from "react";
import { useStudioState } from "../../state/store";
import { checkTextVariables, sameTextVariables, textVariableRows, textVariablesToSend, type TextVariableRow } from "../../kicad-port/textVariablesRules";
import { PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

export function TextVariablesPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const variables = state.board?.board_rules?.text_variables;
  const page = usePageDraft<TextVariableRow[]>(state.board?.board_rules ? textVariableRows(variables) : undefined);
  const apply = usePageApply();
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const rows = page.draft;
  const check = rows ? checkTextVariables(rows) : null;

  const setRow = (i: number, patch: Partial<TextVariableRow>) => page.edit((d) => d.map((r, k) => (k === i ? { ...r, ...patch } : r)));

  const onApply = () => {
    if (!rows || !check) return;
    if (Object.keys(check.errors).length > 0 || check.page) {
      apply.setMessage({ text: check.page ?? "Fix the rows marked in red first.", bad: true });
      return;
    }
    if (sameTextVariables(rows, variables) && state.board?.board_rules?.overlay?.includes("text_variables")) {
      apply.setMessage({ text: "Nothing to apply.", bad: false });
      page.markClean();
      return;
    }
    void apply.run({ op: "set_text_variables", variables: textVariablesToSend(rows) }, page.markClean);
  };

  return (
    <PageFrame title="Text Variables" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!rows || !check ? (
        <p className="bs-note">This backend does not report the project's text variables; update the eda binary.</p>
      ) : (
        <>
          <p className="bs-note">
            Text items and fields show <code>{"${NAME}"}</code> as the value set here. The project's variables are also the board's path variables (Configure Paths lists the ones named like a path), and kicad-cli's plots and reports show them the same way.
          </p>
          <div className="bs-scroll-x">
            <table className="bs-grid" aria-label="Text variables">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Text Substitution</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => {
                  const error = check.errors[i];
                  return (
                  <tr key={i}>
                    <td>
                      <input type="text" className={`bs-name${error?.field === "name" ? " bs-invalid" : ""}`} aria-label={`Variable ${i + 1} name`} title={error?.field === "name" ? error.message : undefined} value={r.name} spellCheck={false} onChange={(e) => setRow(i, { name: e.target.value })} />
                    </td>
                    <td>
                      <input type="text" className={`bs-pattern${error?.field === "value" ? " bs-invalid" : ""}`} style={{ minWidth: 280 }} aria-label={`Variable ${i + 1} value`} title={error?.field === "value" ? error.message : undefined} value={r.value} spellCheck={false} onChange={(e) => setRow(i, { value: e.target.value })} />
                    </td>
                    <td>
                      <button aria-label={`Remove variable ${i + 1}`} title="Delete this variable" onClick={() => page.edit((d) => d.filter((_, k) => k !== i))}>
                        Remove
                      </button>
                    </td>
                    <td className="bs-error">{error?.message ?? ""}</td>
                    <td className="bs-note">{error ? "" : (check.notes[i] ?? "")}</td>
                  </tr>
                  );
                })}
                {rows.length === 0 && (
                  <tr>
                    <td colSpan={3} className="bs-note">
                      No variables.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
          <div className="bs-toolbar">
            <button onClick={() => page.edit((d) => [...d, { name: "", value: "" }])}>Add Variable</button>
          </div>
          {check.page && <p className="bs-error">{check.page}</p>}
        </>
      )}
    </PageFrame>
  );
}
