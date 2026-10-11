// Configure Paths: `DIALOG_CONFIGURE_PATHS` (common/dialogs/dialog_configure_paths.cpp) -- the environment variables a library path (and a 3D model path) is written with,
// `${KICAD10_SYMBOL_DIR}`, `${KIPRJMOD}`, your own. KiCad's dialog edits the variables of KiCad's settings; those are KiCad's and the studio only reads them, so they are
// listed here, greyed where the real environment defines them (which takes precedence, as KiCad's own help says), and what the dialog edits are the board's own variables
// -- the project's text variables of KiCad, which are read before the environment and handed to kicad-cli in the derived project. The name rules are
// `ENV_VAR_NAME_VALIDATOR`'s (capitals, digits, underscores; `KIPRJMOD` is reserved); a name needs a path and a path a name (`TransferDataFromWindow`).
import { useEffect, useRef, useState } from "react";
import { fetchLibraryTables, normalizeLibraryPath, savePathVariables, type LibraryTablesView } from "../../api/libraryTablesClient";
import { fixVariableName, sameVariables, validateVariables, type VariableProblem, type VariableValue } from "../../kicad-port/libraryTable";
import { closeConfigurePaths, useLibraryTableDialogs } from "../../state/libraryTableDialogs";
import { useStudioDispatch } from "../../state/store";
import { FolderBrowser } from "./FolderBrowser";
import "../../styles/libraryTables.css";

export function ConfigurePathsDialog() {
  const open = useLibraryTableDialogs().paths;
  return open ? <PathsBody /> : null;
}

function PathsBody() {
  const dispatch = useStudioDispatch();
  const [view, setView] = useState<LibraryTablesView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [rows, setRows] = useState<VariableValue[]>([]);
  const [problem, setProblem] = useState<VariableProblem | null>(null);
  const [busy, setBusy] = useState(false);
  const [help, setHelp] = useState(false);
  const [browse, setBrowse] = useState<number | null>(null);
  const focusRow = useRef<number | null>(null);
  const tableRef = useRef<HTMLTableElement>(null);

  useEffect(() => {
    let cancelled = false;
    fetchLibraryTables()
      .then((v) => {
        if (cancelled) return;
        setView(v);
        setRows(v.variables.project.map((p) => ({ ...p })));
      })
      .catch((e: unknown) => {
        if (!cancelled) setLoadError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const at = focusRow.current;
    if (at === null) return;
    focusRow.current = null;
    tableRef.current?.querySelector<HTMLInputElement>(`[data-row="${at}"][data-col="name"]`)?.focus();
  }, [rows]);

  const close = () => closeConfigurePaths();
  const original = view?.variables.project ?? [];

  const add = (name = "") => {
    focusRow.current = rows.length;
    setRows((now) => [...now, { name, value: "" }]);
  };

  const ok = async () => {
    const checked = validateVariables(rows);
    if ("problem" in checked) {
      setProblem(checked.problem);
      queueMicrotask(() => tableRef.current?.querySelector<HTMLInputElement>(`[data-row="${checked.problem.row}"][data-col="${checked.problem.column}"]`)?.focus());
      return;
    }
    if (sameVariables(checked.variables, original)) return close();
    setBusy(true);
    try {
      const answer = await savePathVariables(checked.variables);
      if ("ok" in answer && answer.ok) {
        dispatch({ type: "TOAST", message: "Saved the board's path variables.", kind: "info" });
        close();
      } else if ("error" in answer) {
        const column = answer.column === "name" || answer.column === "value" ? answer.column : "name";
        setProblem({ row: answer.row ?? 0, column, message: answer.error });
      }
    } catch (e) {
      setProblem({ row: 0, column: "name", message: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  };

  const pickFolder = async (path: string) => {
    const at = browse;
    setBrowse(null);
    if (at === null) return;
    try {
      const norm = await normalizeLibraryPath(`${path}/x`);
      // a folder is written as a library file is: under a variable or the board's folder where one holds it
      const folder = norm.uri.replace(/\/x$/, "");
      setProblem(null);
      setRows((now) => now.map((r, i) => (i === at ? { ...r, value: folder } : r)));
    } catch {
      setRows((now) => now.map((r, i) => (i === at ? { ...r, value: path } : r)));
    }
  };

  const bad = (i: number, column: "name" | "value") => (problem && problem.row === i && problem.column === column ? "lt-bad" : undefined);
  const undefinedNames = view?.variables.undefined ?? [];
  const defined = new Set(rows.map((r) => r.name));

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog lt-paths" onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Escape" && browse === null && close()} role="dialog" aria-label="Configure Paths">
        <div className="dialog-header">
          <span>Configure Paths</span>
        </div>
        <div className="lt-body">
          {loadError && <div className="lt-banner">{loadError}</div>}
          {help && (
            <div className="lt-help">
              Enter the name and value for each variable of this board. A library path can use <code>{"${NAME}"}</code>, and so can a 3D model path; the board&rsquo;s own variables are read first,
              then <code>KIPRJMOD</code> (the board&rsquo;s folder), then KiCad&rsquo;s variables and the system environment. To ensure variable names are valid on all platforms, the name field only accepts
              upper case letters, digits, and the underscore character. KiCad&rsquo;s own variables below belong to KiCad: the studio shows them and never writes them; a grey value is one the system
              environment defines, which takes precedence over KiCad&rsquo;s settings. The board&rsquo;s variables are kept in {view?.variables.file ?? "path-variables.json"} beside the board and given to kicad-cli
              in the derived project.
            </div>
          )}
          <div className="lt-section">Board variables</div>
          <div className="lt-scroll" style={{ flex: "1 1 40%" }}>
            <table className="lt-vars" ref={tableRef} aria-label="Board variables">
              <thead>
                <tr>
                  <th style={{ width: "32%" }}>Name</th>
                  <th>Path</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => (
                  <tr key={i}>
                    <td className={bad(i, "name")}>
                      <input
                        type="text"
                        data-row={i}
                        data-col="name"
                        aria-label={`Name of variable ${i + 1}`}
                        value={r.name}
                        spellCheck={false}
                        onChange={(e) => {
                          setProblem(null);
                          setRows((now) => now.map((x, at) => (at === i ? { ...x, name: fixVariableName(e.target.value) } : x)));
                        }}
                      />
                    </td>
                    <td className={bad(i, "value")}>
                      <div className="lt-cellrow">
                        <input
                          type="text"
                          data-row={i}
                          data-col="value"
                          aria-label={`Path of variable ${i + 1}`}
                          value={r.value}
                          spellCheck={false}
                          onChange={(e) => {
                            setProblem(null);
                            setRows((now) => now.map((x, at) => (at === i ? { ...x, value: e.target.value } : x)));
                          }}
                        />
                        <button title="Browse for the folder" aria-label={`Browse for the folder of variable ${i + 1}`} onClick={() => setBrowse(i)}>
                          {"…"}
                        </button>
                        <button title="Remove variable" aria-label={`Remove variable ${i + 1}`} onClick={() => setRows((now) => now.filter((_, at) => at !== i))}>
                          {"🗑"}
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
                {rows.length === 0 && (
                  <tr>
                    <td colSpan={2} className="lt-dim">
                      The board has no variables of its own. Add one to give a library path a name (for a folder of libraries shared with a team, say).
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
          <div className="lt-toolbar">
            <button title="Add variable" aria-label="Add variable" onClick={() => add()}>
              {"+"}
            </button>
            {problem && <span className="lt-error">{problem.message}</span>}
          </div>
          <div className="lt-section">KiCad&rsquo;s variables (read only)</div>
          <div className="lt-scroll" style={{ flex: "1 1 50%" }}>
            <table className="lt-vars" aria-label="KiCad's variables">
              <thead>
                <tr>
                  <th style={{ width: "32%" }}>Name</th>
                  <th>Path</th>
                </tr>
              </thead>
              <tbody>
                <tr>
                  <td className="ro">KIPRJMOD</td>
                  <td className="ro" title="Set by KiCad, and by the studio, to the folder of the project: the board's folder">
                    {view?.project_dir ?? ""}
                  </td>
                </tr>
                {(view?.variables.global ?? []).map((v) => (
                  <tr key={v.name}>
                    <td className={`ro${v.external ? " external" : ""}`}>{v.name}</td>
                    <td className={`ro${v.external ? " external" : ""}`} title={v.external ? "Defined in the system environment, which takes precedence over KiCad's settings" : v.default ? "KiCad's default" : "Set in KiCad's settings"}>
                      {v.value}
                    </td>
                  </tr>
                ))}
                {undefinedNames
                  .filter((u) => !defined.has(u.name))
                  .map((u) => (
                    <tr key={u.name}>
                      <td className="ro">{u.name}</td>
                      <td className="ro">
                        {u.value || <span className="lt-dim">used by a library path, defined nowhere</span>}{" "}
                        <button className="lt-link" onClick={() => add(u.name)}>
                          Define for this board
                        </button>
                      </td>
                    </tr>
                  ))}
              </tbody>
            </table>
          </div>
          {view && (
            <div className="lt-note">
              {view.config_dir ? `KiCad's settings: ${view.config_dir}` : "No KiCad configuration folder was found; KiCad's variables have their defaults."}
            </div>
          )}
        </div>
        <div className="dialog-footer">
          <button style={{ marginRight: "auto" }} onClick={() => setHelp((h) => !h)} aria-pressed={help}>
            Help
          </button>
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={busy || !view} onClick={() => void ok()}>
            OK
          </button>
        </div>
      </div>
      {browse !== null && (
        <div onClick={(e) => e.stopPropagation()}>
          <FolderBrowser title="Select Path" what="folder" start={rows[browse]?.value ?? ""} onPick={(p) => void pickFolder(p)} onCancel={() => setBrowse(null)} />
        </div>
      )}
    </div>
  );
}
