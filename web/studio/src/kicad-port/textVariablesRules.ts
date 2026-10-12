// The logic behind Board Setup > Text Variables (common/dialogs/panel_text_variables.cpp), as pure functions: the rows of the grid, the checks the panel
// makes before OK, and the map a page sends (`set_text_variables`, crates/ops/src/board_setup.rs; the backend checks it again, crates/model/src/rules.rs
// `validate_text_variables`, and the two are held to the same cases in their tests).
//
// Ported from:
//   common/dialogs/panel_text_variables.cpp   m_nameValidator (the characters a name may not hold), TransferDataFromWindow (an empty name stops OK;
//                                             the rows become a std::map, so two rows of one name keep the last), AppendTextVar (the rows are the map's, in its order)
//   common/project.cpp                        PROJECT::TextVarResolver (the names KiCad answers itself, ahead of the project's: BUILTIN_PROJECT_VARIABLES)
//
// Beyond KiCad's panel: the name KIPRJMOD is refused (the derived project gives it the board's folder), a name or value over the limits of the
// backend is refused rather than stored, and a name with a control character is refused (a grid cell cannot even hold one).

/** `PANEL_TEXT_VARIABLES::m_nameValidator`'s `wxFILTER_EXCLUDE_CHAR_LIST`: the characters a variable's name may not hold. */
export const TEXT_VARIABLE_NAME_EXCLUDED = "{}[]()%~<>\"='`;:.,&?/\\|$";

/** The name the derived project gives the board's folder (`${KIPRJMOD}`). */
export const RESERVED_TEXT_VARIABLE = "KIPRJMOD";

export const MAX_TEXT_VARIABLES = 1_000;
export const MAX_TEXT_VARIABLE_NAME = 256;
export const MAX_TEXT_VARIABLE_VALUE = 16 * 1024;

/**
 * The names `PROJECT::TextVarResolver` answers itself before it looks at the project's variables: a project variable of one of these names is
 * never read (KiCad does not say so either; the page does). `VCSHASH` and `VCSSHORTHASH` come from the project's git repository.
 */
export const BUILTIN_PROJECT_VARIABLES: readonly string[] = ["PROJECTNAME", "CURRENT_DATE", "CURRENT_TIME_HH_MM_SS", "CURRENT_TIME_LOCALE", "VCSHASH", "VCSSHORTHASH"];

/** One row of the grid. */
export interface TextVariableRow {
  name: string;
  value: string;
}

/** The rows of a page: the project's variables in the order of `std::map<wxString, wxString>` (by name; this compares by code point, as wxString does). */
export function textVariableRows(variables: Readonly<Record<string, string>> | undefined): TextVariableRow[] {
  return Object.entries(variables ?? {})
    .map(([name, value]) => ({ name, value }))
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}

/** What is wrong with `name` as a variable's name, in the page's words, or null (`text_variable_name_problem` in crates/model/src/rules.rs is the backend's twin). */
export function textVariableNameProblem(name: string): string | null {
  if (name === "") return "Variable name cannot be empty.";
  if (new TextEncoder().encode(name).length > MAX_TEXT_VARIABLE_NAME) return `A variable name is at most ${MAX_TEXT_VARIABLE_NAME} bytes long.`;
  for (const c of name) {
    if (TEXT_VARIABLE_NAME_EXCLUDED.includes(c) || /[\u0000-\u001f\u007f-\u009f]/.test(c)) {
      return `A variable name cannot hold "${c.replace(/[\u0000-\u001f\u007f-\u009f]/, "a control character")}" (none of ${TEXT_VARIABLE_NAME_EXCLUDED} may be in it).`;
    }
  }
  if (name === RESERVED_TEXT_VARIABLE) return `The name ${RESERVED_TEXT_VARIABLE} is reserved: it is the board's folder.`;
  return null;
}

/** What the page says about its rows before Apply. */
export interface TextVariablesCheck {
  /** Row index -> the problem with that row (the first one it has) and the cell it is in. */
  errors: Record<number, { field: "name" | "value"; message: string }>;
  /** Row index -> a remark that does not stop Apply: another row has the name (the last one is kept, as `variables[ name ] = value` does), or KiCad answers the name itself. */
  notes: Record<number, string>;
  /** A problem with the page as a whole (too many variables, a value too long). */
  page: string | null;
}

/** `PANEL_TEXT_VARIABLES::TransferDataFromWindow`'s checks, and the backend's limits. */
export function checkTextVariables(rows: readonly TextVariableRow[]): TextVariablesCheck {
  const errors: TextVariablesCheck["errors"] = {};
  const notes: Record<number, string> = {};
  let page: string | null = null;
  const last = new Map<string, number>();
  rows.forEach((r, i) => last.set(r.name, i));
  rows.forEach((row, i) => {
    const problem = textVariableNameProblem(row.name);
    if (problem) {
      errors[i] = { field: "name", message: problem };
      return;
    }
    if (new TextEncoder().encode(row.value).length > MAX_TEXT_VARIABLE_VALUE) errors[i] = { field: "value", message: `The value is at most ${MAX_TEXT_VARIABLE_VALUE} bytes long.` };
    else if (row.value.includes("\u0000")) errors[i] = { field: "value", message: "The value cannot hold a NUL character." };
    else if (last.get(row.name) !== i) notes[i] = `Another row below is named ${row.name}: that one is kept, this one is dropped.`;
    else if (BUILTIN_PROJECT_VARIABLES.includes(row.name)) notes[i] = `KiCad answers ${row.name} itself, so this value is never read.`;
  });
  if (rows.length > MAX_TEXT_VARIABLES) page = `${rows.length} variables; at most ${MAX_TEXT_VARIABLES} are stored.`;
  return { errors, notes, page };
}

/** The map a page sends: one value for a name, the last row's (`variables[ name ] = value`). */
export function textVariablesToSend(rows: readonly TextVariableRow[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const r of rows) out[r.name] = r.value;
  return out;
}

/** Whether the rows say what the project already has (nothing to apply). */
export function sameTextVariables(rows: readonly TextVariableRow[], variables: Readonly<Record<string, string>> | undefined): boolean {
  const a = textVariablesToSend(rows);
  const b = variables ?? {};
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every((k) => b[k] === a[k]);
}
