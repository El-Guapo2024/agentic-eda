// Configure Paths: `DIALOG_CONFIGURE_PATHS` (common/dialogs/dialog_configure_paths.cpp, 8303b2ad) -- the environment variables a library path (and a 3D model path) is
// written with, `${KICAD10_SYMBOL_DIR}`, `${KIPRJMOD}`, your own.
//
// KiCad's own variables are KiCad's dialog's one grid, and are edited here as there: a predefined name cannot be changed (`IsEnvVarImmutable`), a row can be added or
// removed, and OK writes them where KiCad keeps them, `kicad_common.json`'s `environment.vars` in KiCad's configuration folder, by `COMMON_SETTINGS`' rules (a
// predefined variable left at its default is not written; the server does it, `persisted_global_vars`). A variable the system environment defines takes precedence
// and is shown grey; KiCad lets one edit it for the session only ("This path was defined externally to the running process and will only be temporarily
// overwritten."), which a server cannot do for the page, so it is read only here. The board's own variables are a studio addition: KiCad's project text variables
// named like a path variable, read before KiCad's and handed to kicad-cli in the derived project. The name rules are `ENV_VAR_NAME_VALIDATOR`'s (capitals, digits,
// underscores; `KIPRJMOD` is reserved); a name needs a path and a path a name (`TransferDataFromWindow`).
//
// The 3D model search paths are listed under them, read only: KiCad 8303b2ad has no page that edits them -- `FILENAME_RESOLVER::createPathList` makes the list from
// `${KIPRJMOD}` and the path variables, and nothing calls `UpdatePathList` -- so editing the variables is editing that list.
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

type Grid = "board" | "kicad";

interface KicadRow extends VariableValue {
  /** Defined in the system environment: read only here. */
  external: boolean;
  /** One of KiCad's predefined names: the name cannot be changed. */
  predefined: boolean;
}

function PathsBody() {
  const dispatch = useStudioDispatch();
  const [view, setView] = useState<LibraryTablesView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [rows, setRows] = useState<VariableValue[]>([]);
  const [kicad, setKicad] = useState<KicadRow[]>([]);
  const [problem, setProblem] = useState<(VariableProblem & { grid: Grid }) | null>(null);
  const [busy, setBusy] = useState(false);
  const [help, setHelp] = useState(false);
  const [browse, setBrowse] = useState<{ grid: Grid; row: number } | null>(null);
  const focusRow = useRef<{ grid: Grid; row: number } | null>(null);
  const boardRef = useRef<HTMLTableElement>(null);
  const kicadRef = useRef<HTMLTableElement>(null);

  useEffect(() => {
    let cancelled = false;
    fetchLibraryTables()
      .then((v) => {
        if (cancelled) return;
        setView(v);
        setRows(v.variables.project.map((p) => ({ ...p })));
        setKicad(v.variables.global.map((g) => ({ name: g.name, value: g.value, external: g.external, predefined: g.predefined })));
      })
      .catch((e: unknown) => {
        if (!cancelled) setLoadError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const tableOf = (grid: Grid) => (grid === "board" ? boardRef : kicadRef).current;
  useEffect(() => {
    const at = focusRow.current;
    if (at === null) return;
    focusRow.current = null;
    tableOf(at.grid)?.querySelector<HTMLInputElement>(`[data-row="${at.row}"][data-col="name"]`)?.focus();
  }, [rows, kicad]);

  const close = () => closeConfigurePaths();
  const original = view?.variables.project ?? [];
  const originalKicad = (view?.variables.global ?? []).map((g) => ({ name: g.name, value: g.value }));

  const addBoard = (name = "") => {
    focusRow.current = { grid: "board", row: rows.length };
    setRows((now) => [...now, { name, value: "" }]);
  };
  const addKicad = (name = "", value = "") => {
    focusRow.current = { grid: "kicad", row: kicad.length };
    setKicad((now) => [...now, { name, value, external: false, predefined: false }]);
  };

  const fail = (grid: Grid, p: VariableProblem) => {
    setProblem({ ...p, grid });
    queueMicrotask(() => tableOf(grid)?.querySelector<HTMLInputElement>(`[data-row="${p.row}"][data-col="${p.column}"]`)?.focus());
  };

  const ok = async () => {
    const board = validateVariables(rows);
    if ("problem" in board) return fail("board", board.problem);
    // `TransferDataFromWindow`: "Don't check for consistency on external variables, just use them as-is"
    const mine = validateVariables(kicad.map((k) => (k.external ? { name: "", value: "" } : k)));
    if ("problem" in mine) return fail("kicad", mine.problem);
    const kicadList: VariableValue[] = kicad.filter((k) => k.external || k.name.trim() !== "" || k.value.trim() !== "").map((k) => ({ name: k.name.trim(), value: k.value.trim() }));
    const boardChanged = !sameVariables(board.variables, original);
    const kicadChanged = !sameVariables(kicadList, originalKicad);
    if (!boardChanged && !kicadChanged) return close();
    setBusy(true);
    try {
      const answer = await savePathVariables(boardChanged ? board.variables : null, kicadChanged ? kicadList : undefined);
      if ("ok" in answer && answer.ok) {
        const what = [kicadChanged ? "KiCad's path variables" : null, boardChanged ? "the board's path variables" : null].filter(Boolean).join(" and ");
        dispatch({ type: "TOAST", message: `Saved ${what}.`, kind: "info" });
        close();
      } else if ("error" in answer) {
        const column = answer.column === "name" || answer.column === "value" ? answer.column : "name";
        fail(answer.scope === "global" ? "kicad" : "board", { row: answer.row ?? 0, column, message: answer.error });
      }
    } catch (e) {
      setProblem({ grid: "board", row: 0, column: "name", message: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  };

  const pickFolder = async (path: string) => {
    const at = browse;
    setBrowse(null);
    if (at === null) return;
    let folder = path;
    try {
      const norm = await normalizeLibraryPath(`${path}/x`);
      // a board variable's folder is written as a library file is: under a variable or the board's folder where one holds it; KiCad's own take the folder as it is
      if (at.grid === "board") folder = norm.uri.replace(/\/x$/, "");
    } catch {
      /* the path as picked */
    }
    setProblem(null);
    if (at.grid === "board") setRows((now) => now.map((r, i) => (i === at.row ? { ...r, value: folder } : r)));
    else setKicad((now) => now.map((r, i) => (i === at.row ? { ...r, value: folder } : r)));
  };

  const bad = (grid: Grid, i: number, column: "name" | "value") => (problem && problem.grid === grid && problem.row === i && problem.column === column ? "lt-bad" : undefined);
  const undefinedNames = view?.variables.undefined ?? [];
  const defined = new Set([...rows.map((r) => r.name), ...kicad.map((k) => k.name)]);

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
              Enter the name and value for each environment variable. Grey entries are names that have been defined externally at the system or user level. Environment variables defined
              at the system or user level take precedence over the ones defined in this table. This means the values in this table are ignored. To ensure environment variable names
              are valid on all platforms, the name field will only accept upper case letters, digits, and the underscore characters. KiCad&rsquo;s variables are written to{" "}
              {view?.variables.global_file ?? "KiCad's kicad_common.json"}; the board&rsquo;s own are read first, then <code>KIPRJMOD</code> (the board&rsquo;s folder), then KiCad&rsquo;s and the system
              environment, and are kept in {view?.variables.file ?? "path-variables.json"} beside the board and given to kicad-cli in the derived project. A 3D model is looked for under{" "}
              <code>KIPRJMOD</code> and each of KiCad&rsquo;s variables but the footprint folders.
            </div>
          )}
          <div className="lt-section">Environment Variables (KiCad&rsquo;s)</div>
          <div className="lt-scroll" style={{ flex: "1 1 45%" }}>
            <table className="lt-vars" ref={kicadRef} aria-label="KiCad's variables">
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
                {kicad.map((r, i) =>
                  r.external ? (
                    <tr key={`k${i}`}>
                      <td className="ro external">{r.name}</td>
                      <td className="ro external" title="Defined in the system environment, which takes precedence over KiCad's settings">
                        {r.value}
                      </td>
                    </tr>
                  ) : (
                    <tr key={`k${i}`}>
                      <td className={bad("kicad", i, "name")}>
                        <input
                          type="text"
                          data-row={i}
                          data-col="name"
                          aria-label={`Name of KiCad variable ${i + 1}`}
                          value={r.name}
                          readOnly={r.predefined}
                          spellCheck={false}
                          onChange={(e) => {
                            setProblem(null);
                            setKicad((now) => now.map((x, at) => (at === i ? { ...x, name: fixVariableName(e.target.value) } : x)));
                          }}
                        />
                      </td>
                      <td className={bad("kicad", i, "value")}>
                        <div className="lt-cellrow">
                          <input
                            type="text"
                            data-row={i}
                            data-col="value"
                            aria-label={`Path of KiCad variable ${i + 1}`}
                            value={r.value}
                            spellCheck={false}
                            onChange={(e) => {
                              setProblem(null);
                              setKicad((now) => now.map((x, at) => (at === i ? { ...x, value: e.target.value } : x)));
                            }}
                          />
                          <button title="Browse for the folder" aria-label={`Browse for the folder of KiCad variable ${i + 1}`} onClick={() => setBrowse({ grid: "kicad", row: i })}>
                            {"…"}
                          </button>
                          <button title="Remove variable" aria-label={`Remove KiCad variable ${i + 1}`} onClick={() => setKicad((now) => now.filter((_, at) => at !== i))}>
                            {"🗑"}
                          </button>
                        </div>
                      </td>
                    </tr>
                  )
                )}
                {undefinedNames
                  .filter((u) => !defined.has(u.name))
                  .map((u) => (
                    <tr key={`u${u.name}`}>
                      <td className="ro">{u.name}</td>
                      <td className="ro">
                        {u.value || <span className="lt-dim">used by a library path, defined nowhere</span>}{" "}
                        <button className="lt-link" onClick={() => addKicad(u.name, u.value)}>
                          Define
                        </button>{" "}
                        <button className="lt-link" onClick={() => addBoard(u.name)}>
                          Define for this board
                        </button>
                      </td>
                    </tr>
                  ))}
              </tbody>
            </table>
          </div>
          <div className="lt-toolbar">
            <button title="Add environment variable" aria-label="Add KiCad variable" disabled={!view?.config_dir} onClick={() => addKicad()}>
              {"+"}
            </button>
            {problem?.grid === "kicad" && <span className="lt-error">{problem.message}</span>}
          </div>
          <div className="lt-section">Board variables</div>
          <div className="lt-scroll" style={{ flex: "1 1 30%" }}>
            <table className="lt-vars" ref={boardRef} aria-label="Board variables">
              <thead>
                <tr>
                  <th style={{ width: "32%" }}>Name</th>
                  <th>Path</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => (
                  <tr key={i}>
                    <td className={bad("board", i, "name")}>
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
                    <td className={bad("board", i, "value")}>
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
                        <button title="Browse for the folder" aria-label={`Browse for the folder of variable ${i + 1}`} onClick={() => setBrowse({ grid: "board", row: i })}>
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
            <button title="Add variable" aria-label="Add variable" onClick={() => addBoard()}>
              {"+"}
            </button>
            {problem?.grid === "board" && <span className="lt-error">{problem.message}</span>}
          </div>
          <div className="lt-section">3D model search paths</div>
          <div className="lt-scroll" style={{ flex: "1 1 25%" }}>
            <table className="lt-vars" aria-label="3D model search paths">
              <tbody>
                {(view?.variables.search_paths ?? []).map((p) => (
                  <tr key={p.alias}>
                    <td className="ro" style={{ width: "32%" }}>
                      {p.alias}
                    </td>
                    <td className="ro">{p.path}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {view && (
            <div className="lt-note">
              {view.config_dir ? `KiCad's settings: ${view.config_dir}` : "No KiCad configuration folder was found: KiCad's variables have their defaults and cannot be written."}
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
          <FolderBrowser
            title="Select Path"
            what="folder"
            start={(browse.grid === "board" ? rows[browse.row]?.value : kicad[browse.row]?.value) ?? ""}
            onPick={(p) => void pickFolder(p)}
            onCancel={() => setBrowse(null)}
          />
        </div>
      )}
    </div>
  );
}
