// Manage Symbol Libraries, Manage Footprint Libraries and Manage Design Block Libraries: `DIALOG_EDIT_LIBRARY_TABLES` holding `PANEL_SYM_LIB_TABLE` /
// `PANEL_FP_LIB_TABLE` / `PANEL_DESIGN_BLOCK_LIB_TABLE` (eeschema/dialogs/panel_sym_lib_table.cpp, pcbnew/dialogs/panel_fp_lib_table.cpp,
// common/dialogs/panel_design_block_lib_table.cpp, common/libraries/lib_table_grid_data_model.cpp, common/lib_table_grid_tricks.cpp).
//
// A Global Libraries tab and a Project Specific Libraries tab over the same grid -- status, Enable, Show (symbol libraries only), Nickname, Library Path, Library
// Format, Options, Description -- the buttons under it (add an empty row, Add Existing, move up, move down, delete), the "Available path substitutions" list under
// those, and OK / Cancel. A row of type `Table` opens the table it chains in a tab of its own. What differs from KiCad is the global table: it is KiCad's own file, and
// the studio only reads it (Reset Libraries and Migrate Libraries are not here either), so that tab, and the tabs of the tables it chains, are read only; OK writes the
// board's own `sym-lib-table` / `fp-lib-table` / `design-block-lib-table` alone, after KiCad's `VerifyTable` checks (the server makes them again). The design block table
// has no Show column (`DESIGN_BLOCK_GRID_TRICKS::SupportsVisibilityColumn` is false).
import { useEffect, useMemo, useRef, useState } from "react";
import { checkLibraryRows, fetchLibraryTables, normalizeLibraryPath, saveLibraryTable, type KindView, type LibraryTablesView } from "../../api/libraryTablesClient";
import {
  appendRow,
  baseName,
  deleteRows,
  flagOffers,
  moveRow,
  nicknameForFile,
  pathSubstitutions,
  sameRows,
  setCell,
  setFlag,
  statusTitle,
  tableTitle,
  unwritable,
  verifyRows,
  type Column,
  type TableKind,
  type TableProblem,
  type TableRow,
  type VariableValue,
} from "../../kicad-port/libraryTable";
import { closeLibraryTables, useLibraryTableDialogs } from "../../state/libraryTableDialogs";
import { useStudioDispatch } from "../../state/store";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";
import { FolderBrowser, type BrowseWhat } from "./FolderBrowser";
import "../../styles/libraryTables.css";

interface Tab {
  id: string;
  title: string;
  /** The rows of a chained table (read only); the Global and Project tabs hold theirs elsewhere. */
  rows?: TableRow[];
  path?: string;
}

const GLOBAL = "global";
const PROJECT = "project";

export function LibraryTablesDialog() {
  const kind = useLibraryTableDialogs().tables;
  // keyed by kind so each opening starts from what the server has
  return kind ? <TablesBody key={kind} kind={kind} /> : null;
}

/** The rows of every table the dialog shows, for the path substitutions: a row's own path and, below a chained table, its rows'. */
function urisOf(rows: readonly TableRow[]): string[] {
  const out: string[] = [];
  for (const r of rows) {
    out.push(r.uri);
    if (r.table) out.push(...urisOf(r.table.rows));
  }
  return out;
}

function TablesBody({ kind }: { kind: TableKind }) {
  const dispatch = useStudioDispatch();
  const [view, setView] = useState<LibraryTablesView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [rows, setRows] = useState<TableRow[]>([]);
  const [tab, setTab] = useState<string>(PROJECT);
  const [opened, setOpened] = useState<Tab[]>([]);
  const [selected, setSelected] = useState<number[]>([]);
  const [cursor, setCursor] = useState(-1);
  const [problem, setProblem] = useState<TableProblem | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [browse, setBrowse] = useState<{ what: BrowseWhat; row: number | null } | null>(null);
  const [addMenu, setAddMenu] = useState(false);
  const [menu, setMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const [duplicate, setDuplicate] = useState<TableRow | null>(null);
  const focusRow = useRef<number | null>(null);
  const gridRef = useRef<HTMLDivElement>(null);

  const kv: KindView | null = view ? view.tables[kind] : null;
  const supportsVisibility = kv?.supports_visibility ?? kind === "symbol";

  useEffect(() => {
    let cancelled = false;
    fetchLibraryTables()
      .then((v) => {
        if (cancelled) return;
        setView(v);
        setRows(v.tables[kind].project.rows.map((r) => ({ ...r })));
      })
      .catch((e: unknown) => {
        if (!cancelled) setLoadError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [kind]);

  // A new row takes the cursor in its nickname.
  useEffect(() => {
    const at = focusRow.current;
    if (at === null) return;
    focusRow.current = null;
    gridRef.current?.querySelector<HTMLInputElement>(`[data-row="${at}"][data-col="nickname"]`)?.focus();
  }, [rows]);

  // `CheckTableRow` after a path, a format or options change: the status column says why a library cannot be read.
  const checkKey = rows.map((r) => `${r.uri}\u0001${r.type}\u0001${r.options}`).join("\u0002");
  useEffect(() => {
    if (!view) return;
    const pending = rows.map((r, i) => ({ r, i })).filter(({ r }) => r.uri.trim() !== "" && r.status === undefined);
    if (pending.length === 0) return;
    const timer = setTimeout(() => {
      checkLibraryRows(
        kind,
        pending.map((p) => p.r)
      )
        .then((results) => {
          setRows((now) =>
            now.map((r, i) => {
              const at = pending.findIndex((p) => p.i === i);
              const found = at >= 0 ? results[at] : undefined;
              const asked = at >= 0 ? (pending[at] as { r: TableRow }).r : undefined;
              if (!found || !asked || r.uri !== asked.uri || r.type !== asked.type || r.options !== asked.options) return r;
              return { ...r, status: found.error ? "error" : r.type === "Table" ? "table" : "ok", error: found.error, path: found.path };
            })
          );
        })
        .catch(() => {
          /* the server is busy: the rows are checked again at the next edit */
        });
    }, 300);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [checkKey, view, kind]);

  const original = kv?.project.rows ?? [];
  const tabs: Tab[] = [
    { id: GLOBAL, title: "Global Libraries" },
    { id: PROJECT, title: "Project Specific Libraries" },
    ...opened,
  ];
  const editable = tab === PROJECT;
  const shown: readonly TableRow[] = tab === GLOBAL ? (kv?.global.rows ?? []) : tab === PROJECT ? rows : (opened.find((t) => t.id === tab)?.rows ?? []);

  const substitutions = useMemo(() => {
    if (!view) return [];
    const uris = [...urisOf(view.tables[kind].global.rows), ...urisOf(rows), ...opened.flatMap((t) => urisOf(t.rows ?? []))];
    const values: VariableValue[] = [
      ...view.variables.undefined.map((u) => ({ name: u.name, value: u.value })),
      ...view.variables.global.map((g) => ({ name: g.name, value: g.value })),
      ...view.variables.project,
      { name: "KIPRJMOD", value: view.project_dir },
    ];
    return pathSubstitutions(uris, kind, view.major, values);
  }, [view, rows, opened, kind]);

  const close = () => closeLibraryTables();

  const edit = (i: number, column: Column, value: string) => {
    setProblem(null);
    setMessage(null);
    setRows((now) => now.map((r, at) => (at === i ? setCell(r, column, value, kind) : r)));
  };

  const select = (i: number, e: { shiftKey: boolean; metaKey: boolean; ctrlKey: boolean }) => {
    setCursor(i);
    if (e.shiftKey && cursor >= 0) {
      const [a, b] = [Math.min(cursor, i), Math.max(cursor, i)];
      setSelected(Array.from({ length: b - a + 1 }, (_, k) => a + k));
    } else if (e.metaKey || e.ctrlKey) {
      setSelected((s) => (s.includes(i) ? s.filter((x) => x !== i) : [...s, i]));
    } else {
      setSelected([i]);
    }
  };

  const addEmpty = () => {
    setProblem(null);
    focusRow.current = rows.length;
    setRows(appendRow(rows));
    setSelected([rows.length]);
    setCursor(rows.length);
  };

  const remove = () => {
    const doomed = selected.length > 0 ? selected : cursor >= 0 ? [cursor] : [];
    if (doomed.length === 0) return;
    const next = deleteRows(rows, doomed);
    setRows(next);
    const at = Math.min(Math.min(...doomed), next.length - 1);
    setSelected(at >= 0 ? [at] : []);
    setCursor(at);
    setProblem(null);
  };

  const move = (delta: -1 | 1) => {
    if (cursor < 0) return;
    const m = moveRow(rows, cursor, delta);
    setRows(m.rows);
    setCursor(m.at);
    setSelected([m.at]);
  };

  const pushRow = (row: TableRow) => {
    focusRow.current = rows.length;
    setRows((now) => [...now, row]);
    setSelected([rows.length]);
    setCursor(rows.length);
    setDuplicate(null);
  };

  /** `browseLibrariesHandler`: a row for each library chosen -- the file's name for the nickname, the path written as a table writes it. */
  const addFromPath = async (path: string, what: BrowseWhat) => {
    try {
      const norm = await normalizeLibraryPath(path);
      const row: TableRow = {
        nickname: nicknameForFile(kind, baseName(path)),
        uri: norm.uri,
        type: what === "table" ? "Table" : "KiCad",
        options: "",
        descr: "",
        enabled: true,
        visible: true,
      };
      if (rows.some((r) => r.nickname === row.nickname)) setDuplicate(row);
      else pushRow(row);
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    }
  };

  const picked = async (path: string) => {
    const target = browse;
    setBrowse(null);
    if (!target) return;
    if (target.row === null) return void (await addFromPath(path, target.what));
    // the "..." of a row: its path becomes the file chosen
    try {
      const norm = await normalizeLibraryPath(path);
      setProblem(null);
      setRows((now) => now.map((r, i) => (i === target.row ? setCell(r, "uri", norm.uri, kind) : r)));
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    }
  };

  const openTable = (row: TableRow) => {
    if (!row.table) return;
    const id = row.table.path;
    setOpened((now) => (now.some((t) => t.id === id) ? now : [...now, { id, title: row.nickname, rows: row.table?.rows ?? [], path: id }]));
    setTab(id);
  };

  const closeTab = (id: string) => {
    setOpened((now) => now.filter((t) => t.id !== id));
    if (tab === id) setTab(PROJECT);
  };

  const rowMenu = (e: React.MouseEvent, i: number) => {
    if (!editable) return;
    e.preventDefault();
    const sel = selected.includes(i) ? selected : [i];
    if (!selected.includes(i)) {
      setSelected([i]);
      setCursor(i);
    }
    const offers = flagOffers(rows, sel);
    const entries: MenuEntry[] = [];
    const row = rows[i];
    if (row?.type === "Table" && row.table) entries.push({ label: "Open Library Table", onSelect: () => openTable(row) });
    if (entries.length > 0) entries.push({ label: "", separator: true, onSelect: () => {} });
    if (offers.activate) entries.push({ label: "Activate Selected", onSelect: () => setRows((now) => setFlag(now, sel, "enabled", true)) });
    if (offers.deactivate) entries.push({ label: "Deactivate Selected", onSelect: () => setRows((now) => setFlag(now, sel, "enabled", false)) });
    if (supportsVisibility && offers.show) entries.push({ label: "Set Visible Flag", onSelect: () => setRows((now) => setFlag(now, sel, "visible", true)) });
    if (supportsVisibility && offers.hide) entries.push({ label: "Unset Visible Flag", onSelect: () => setRows((now) => setFlag(now, sel, "visible", false)) });
    entries.push({ label: "", separator: true, onSelect: () => {} });
    entries.push({ label: "Delete Row", onSelect: () => setRows((now) => deleteRows(now, sel)) });
    setMenu({ x: e.clientX, y: e.clientY, entries });
  };

  /** OK: `TransferDataFromWindow` -- the checks first, then the board's table is written. */
  const ok = async () => {
    setMessage(null);
    const fail = (p: TableProblem) => {
      setTab(PROJECT);
      setProblem(p);
      setMessage(p.message);
      setSelected([p.row]);
      setCursor(p.row);
      queueMicrotask(() => gridRef.current?.querySelector<HTMLInputElement>(`[data-row="${p.row}"][data-col="${p.column}"]`)?.focus());
    };
    const unwritten = unwritable(rows);
    if (unwritten) return fail(unwritten);
    const verified = verifyRows(rows, supportsVisibility);
    if ("problem" in verified) return fail(verified.problem);
    if (sameRows(verified.rows, original)) return close();
    setBusy(true);
    try {
      const answer = await saveLibraryTable(kind, verified.rows);
      if ("ok" in answer && answer.ok) {
        dispatch({ type: "TOAST", message: `Saved the project's ${kv?.file ?? "library table"}.`, kind: "info" });
        close();
      } else if ("error" in answer) {
        const column = (["nickname", "uri", "type", "options", "descr"] as const).find((c) => c === answer.column);
        if (answer.row !== undefined && column) fail({ row: answer.row, column, message: answer.error });
        else setMessage(answer.error);
      }
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const title = tableTitle(kind);
  const globalView = kv?.global;
  const noteOf = tab === GLOBAL ? globalView?.note : tab === PROJECT ? undefined : "A table the global table chains: KiCad's, read only here.";
  const addEntries: { what: BrowseWhat; label: string }[] =
    kind === "symbol"
      ? [
          { what: "symbol", label: "KiCad (.kicad_sym)" },
          { what: "table", label: "Table (nested library table)" },
        ]
      : kind === "footprint"
        ? [
            { what: "footprint", label: "KiCad (folder with .kicad_mod files)" },
            { what: "table", label: "Table (nested library table)" },
          ]
        : [
            { what: "design_block", label: "KiCad (folder with .kicad_block folders)" },
            { what: "table", label: "Table (nested library table)" },
          ];

  /** A text cell of the grid. */
  const cell = (r: TableRow, i: number, column: Column) => (
    <td className={problem && problem.row === i && problem.column === column ? "lt-bad" : undefined}>
      <input
        type="text"
        data-row={i}
        data-col={column}
        aria-label={`${column} of row ${i + 1}`}
        value={r[column]}
        readOnly={!editable}
        spellCheck={false}
        onFocus={() => {
          if (editable && !selected.includes(i)) {
            setSelected([i]);
            setCursor(i);
          }
        }}
        onChange={(e) => edit(i, column, e.target.value)}
      />
    </td>
  );

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog lt-dialog" onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Escape" && !browse && !menu && close()} role="dialog" aria-label={title}>
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="lt-body">
          {loadError && <div className="lt-banner">{loadError}</div>}
          <div className="lt-tabs" role="tablist">
            {tabs.map((t) => (
              <div key={t.id} role="tab" aria-selected={tab === t.id} className={`lt-tab${tab === t.id ? " active" : ""}`} onClick={() => setTab(t.id)} title={t.path}>
                <span>{t.title}</span>
                {t.rows && (
                  <span
                    className="lt-x"
                    role="button"
                    aria-label={`Close ${t.title}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      closeTab(t.id);
                    }}
                  >
                    {"×"}
                  </span>
                )}
              </div>
            ))}
          </div>
          {noteOf && <div className="lt-note">{noteOf}</div>}
          {tab === GLOBAL && globalView?.error && <div className="lt-error">{globalView.error}</div>}
          {tab === PROJECT && kv && (
            <div className="lt-note">
              {kv.project.exists ? `The board's table: ${kv.project.path}` : `The board has no ${kv.file} yet; OK writes one beside the board: ${kv.project.path}`}
              {kv.project.error && <span className="lt-error"> {kv.project.error}</span>}
            </div>
          )}
          <div className="lt-gridwrap" ref={gridRef}>
            <table className="lt-grid" aria-label={tab === GLOBAL ? "Global libraries" : tab === PROJECT ? "Project libraries" : "Library table"}>
              <colgroup>
                <col style={{ width: 30 }} />
                <col style={{ width: 52 }} />
                {supportsVisibility && <col style={{ width: 46 }} />}
                <col style={{ width: "15%" }} />
                <col style={{ width: "32%" }} />
                <col style={{ width: 110 }} />
                <col style={{ width: "10%" }} />
                <col />
              </colgroup>
              <thead>
                <tr>
                  <th />
                  <th>Enable</th>
                  {supportsVisibility && <th>Show</th>}
                  <th>Nickname</th>
                  <th>Library Path</th>
                  <th>Library Format</th>
                  <th>Options</th>
                  <th>Description</th>
                </tr>
              </thead>
              <tbody>
                {shown.map((r, i) => (
                  <tr
                    key={i}
                    className={`${editable && selected.includes(i) ? "selected" : ""}${r.enabled ? "" : " disabled-row"}`}
                    onMouseDown={(e) => editable && (e.target as HTMLElement).tagName !== "INPUT" && select(i, e)}
                    onContextMenu={(e) => rowMenu(e, i)}
                  >
                    <td className="lt-center">
                      {r.status === "error" ? (
                        <button className="lt-status lt-warn" title={statusTitle(r)} aria-label={`Row ${i + 1}: ${statusTitle(r)}`} onClick={() => setMessage(statusTitle(r))}>
                          {"⚠"}
                        </button>
                      ) : r.type === "Table" && r.table ? (
                        <button className="lt-status lt-open" title="Open library table" aria-label={`Open library table ${r.nickname}`} onClick={() => openTable(r)}>
                          {"↗"}
                        </button>
                      ) : null}
                    </td>
                    <td className="lt-center">
                      <input type="checkbox" aria-label={`Enable ${r.nickname || `row ${i + 1}`}`} checked={r.enabled} disabled={!editable} onChange={(e) => setRows((now) => now.map((x, at) => (at === i ? { ...x, enabled: e.target.checked } : x)))} />
                    </td>
                    {supportsVisibility && (
                      <td className="lt-center">
                        <input type="checkbox" aria-label={`Show ${r.nickname || `row ${i + 1}`}`} checked={r.visible} disabled={!editable} onChange={(e) => setRows((now) => now.map((x, at) => (at === i ? { ...x, visible: e.target.checked } : x)))} />
                      </td>
                    )}
                    {cell(r, i, "nickname")}
                    <td className={problem && problem.row === i && problem.column === "uri" ? "lt-bad" : undefined}>
                      <div className="lt-cellrow">
                        <input
                          type="text"
                          data-row={i}
                          data-col="uri"
                          aria-label={`uri of row ${i + 1}`}
                          value={r.uri}
                          readOnly={!editable}
                          spellCheck={false}
                          title={r.path && r.path !== r.uri ? r.path : undefined}
                          onFocus={() => {
                            if (editable && !selected.includes(i)) {
                              setSelected([i]);
                              setCursor(i);
                            }
                          }}
                          onChange={(e) => edit(i, "uri", e.target.value)}
                        />
                        {editable && (
                          <button title="Browse for the library" aria-label={`Browse for the library of row ${i + 1}`} onClick={() => setBrowse({ what: r.type === "Table" ? "table" : kind, row: i })}>
                            {"…"}
                          </button>
                        )}
                      </div>
                    </td>
                    <td className={problem && problem.row === i && problem.column === "type" ? "lt-bad" : undefined}>
                      <select
                        aria-label={`type of row ${i + 1}`}
                        value={r.type}
                        disabled={!editable}
                        onChange={(e) => edit(i, "type", e.target.value)}
                      >
                        {r.type === "" && <option value="" />}
                        {!(kv?.types ?? []).includes(r.type) && r.type !== "" && <option value={r.type}>{r.type}</option>}
                        {(kv?.types ?? []).map((t) => (
                          <option key={t} value={t}>
                            {t}
                          </option>
                        ))}
                      </select>
                    </td>
                    {cell(r, i, "options")}
                    {cell(r, i, "descr")}
                  </tr>
                ))}
                {shown.length === 0 && (
                  <tr>
                    <td colSpan={supportsVisibility ? 8 : 7} className="lt-dim" style={{ padding: 8 }}>
                      {tab === PROJECT ? "No project specific libraries. Add Existing adds a library file or folder; the + button adds an empty row." : "Nothing in this table."}
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
          {message && <div className="lt-banner">{message}</div>}
          {duplicate && (
            <div className="lt-banner" role="alert">
              An item nicknamed &lsquo;{duplicate.nickname}&rsquo; already exists. One of the nicknames will need to be changed.{" "}
              <button onClick={() => setDuplicate(null)}>Skip</button> <button onClick={() => pushRow(duplicate)}>Add Anyway</button>
            </div>
          )}
          {editable && (
            <div className="lt-toolbar">
              <button title="Add empty row to table" aria-label="Add empty row to table" onClick={addEmpty}>
                {"+"}
              </button>
              <div className="lt-split">
                <button onClick={() => setBrowse({ what: kind, row: null })} title="Add Existing">
                  Add Existing
                </button>
                <button aria-label="More ways to add a library" title="More ways to add a library" onClick={() => setAddMenu((v) => !v)}>
                  {"▾"}
                </button>
                {addMenu && (
                  <div className="lt-menu" role="menu">
                    {addEntries.map((en) => (
                      <button
                        key={en.what}
                        role="menuitem"
                        onClick={() => {
                          setAddMenu(false);
                          setBrowse({ what: en.what, row: null });
                        }}
                      >
                        {en.label}
                      </button>
                    ))}
                  </div>
                )}
              </div>
              <button title="Move up" aria-label="Move up" disabled={cursor <= 0} onClick={() => move(-1)}>
                {"↑"}
              </button>
              <button title="Move down" aria-label="Move down" disabled={cursor < 0 || cursor >= rows.length - 1} onClick={() => move(1)}>
                {"↓"}
              </button>
              <button title="Remove library from table" aria-label="Remove library from table" disabled={selected.length === 0 && cursor < 0} onClick={remove}>
                {"🗑"}
              </button>
              <span className="lt-spacer" />
            </div>
          )}
          <div className="lt-label">Available path substitutions:</div>
          <div className="lt-subs" title="This is a read-only table which shows pertinent environment variables.">
            <table aria-label="Path substitutions">
              <thead>
                <tr>
                  <th style={{ width: "34%" }}>Name</th>
                  <th>Value</th>
                </tr>
              </thead>
              <tbody>
                {substitutions.map((s) => (
                  <tr key={s.name}>
                    <td>{`\${${s.name}}`}</td>
                    <td>{s.value}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={busy || !view} onClick={() => void ok()}>
            OK
          </button>
        </div>
      </div>
      {browse && (
        <div onClick={(e) => e.stopPropagation()}>
          <FolderBrowser
            title={browse.what === "table" ? "Select Library Table" : "Select KiCad Library"}
            what={browse.what}
            start={browse.row !== null ? (rows[browse.row]?.path ?? rows[browse.row]?.uri ?? "") : ""}
            onPick={(p) => void picked(p)}
            onCancel={() => setBrowse(null)}
          />
        </div>
      )}
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} onClose={() => setMenu(null)} />}
    </div>
  );
}
