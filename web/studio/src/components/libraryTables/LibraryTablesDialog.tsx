// Manage Symbol Libraries, Manage Footprint Libraries and Manage Design Block Libraries: `DIALOG_EDIT_LIBRARY_TABLES` holding `PANEL_SYM_LIB_TABLE` /
// `PANEL_FP_LIB_TABLE` / `PANEL_DESIGN_BLOCK_LIB_TABLE` (eeschema/dialogs/panel_sym_lib_table.cpp, pcbnew/dialogs/panel_fp_lib_table.cpp,
// common/dialogs/panel_design_block_lib_table.cpp, common/libraries/lib_table_grid_data_model.cpp, common/lib_table_grid_tricks.cpp).
//
// A Global Libraries tab and a Project Specific Libraries tab over the same grid -- status, Enable, Show (symbol libraries only), Nickname, Library Path, Library
// Format, Options, Description -- the buttons under it (add an empty row, Add Existing, move up, move down, delete, Reset Libraries, Migrate Libraries), the "Available
// path substitutions" list under those, and OK / Cancel. A row of type `Table` opens the table it chains in a tab of its own (`OpenTable`, closable). Every tab is
// edited as KiCad edits it: OK runs `VerifyTable` on each and writes the tables that changed (`TransferDataFromWindow`): the global one into KiCad's configuration
// folder, the board's own `sym-lib-table` / `fp-lib-table` / `design-block-lib-table`, and a chained table whose tab changed into its own file (the server makes the
// checks again, and writes nothing but those). Reset Libraries (the Global tab only; not on the design block panel) writes the global table anew at once, OK or Cancel;
// Migrate Libraries converts the selected rows' libraries through kicad-cli and points the rows at the result; Edit Options... is `DIALOG_PLUGIN_OPTIONS`. The design
// block table has no Show column (`DESIGN_BLOCK_GRID_TRICKS::SupportsVisibilityColumn` is false).
import { useEffect, useMemo, useRef, useState } from "react";
import { checkLibraryRows, fetchLibraryTables, migrateLibraries, normalizeLibraryPath, parseTableText, resetGlobalTable, saveLibraryTable, type KindView, type LibraryTablesView } from "../../api/libraryTablesClient";
import {
  cellPasteKind,
  insertTableRows,
  isTableText,
  pasteTableText,
  type GridColumn,
  appendRow,
  baseName,
  canEditOptions,
  deleteRows,
  flagOffers,
  MIGRATE_NONE,
  migratableRows,
  migrateOverwriteQuestion,
  migrateQuestion,
  moveRow,
  nicknameForFile,
  pathSubstitutions,
  RESET_QUESTION,
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
  type TableScope,
  type VariableValue,
} from "../../kicad-port/libraryTable";
import { closeLibraryTables, useLibraryTableDialogs } from "../../state/libraryTableDialogs";
import { useStudioDispatch } from "../../state/store";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";
import { FolderBrowser, type BrowseWhat } from "./FolderBrowser";
import { PluginOptionsDialog } from "./PluginOptionsDialog";
import { TableQuestion, type Question } from "./TableQuestion";
import "../../styles/libraryTables.css";

interface Tab {
  id: string;
  title: string;
  scope: TableScope;
  /** The file of a chained table's tab. */
  path?: string;
  /** Whether OK may write it. */
  writable: boolean;
  closable: boolean;
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

const copy = (rows: readonly TableRow[]): TableRow[] => rows.map((r) => ({ ...r }));

function TablesBody({ kind }: { kind: TableKind }) {
  const dispatch = useStudioDispatch();
  const [view, setView] = useState<LibraryTablesView | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  /** The grid of each tab, by tab id; `originals` is what the table's file holds. */
  const [tables, setTables] = useState<Record<string, TableRow[]>>({});
  const [originals, setOriginals] = useState<Record<string, TableRow[]>>({});
  const [tab, setTab] = useState<string>(PROJECT);
  const [opened, setOpened] = useState<Tab[]>([]);
  const [selected, setSelected] = useState<number[]>([]);
  const [cursor, setCursor] = useState(-1);
  const [problem, setProblem] = useState<(TableProblem & { tab: string }) | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [browse, setBrowse] = useState<{ what: BrowseWhat; row: number | null } | null>(null);
  const [addMenu, setAddMenu] = useState(false);
  const [menu, setMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const [duplicate, setDuplicate] = useState<TableRow | null>(null);
  const [question, setQuestion] = useState<Question | null>(null);
  const [optionsOf, setOptionsOf] = useState<{ tab: string; row: number } | null>(null);
  const focusRow = useRef<number | null>(null);
  const gridRef = useRef<HTMLDivElement>(null);

  const kv: KindView | null = view ? view.tables[kind] : null;
  const supportsVisibility = kv?.supports_visibility ?? kind === "symbol";

  const adopt = (v: LibraryTablesView, keepProject: boolean) => {
    setView(v);
    const g = copy(v.tables[kind].global.rows);
    setTables((now) => ({ ...now, [GLOBAL]: g, ...(keepProject ? {} : { [PROJECT]: copy(v.tables[kind].project.rows) }) }));
    setOriginals((now) => ({ ...now, [GLOBAL]: copy(g), ...(keepProject ? {} : { [PROJECT]: copy(v.tables[kind].project.rows) }) }));
  };

  useEffect(() => {
    let cancelled = false;
    fetchLibraryTables()
      .then((v) => {
        if (!cancelled) adopt(v, false);
      })
      .catch((e: unknown) => {
        if (!cancelled) setLoadError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind]);

  const rows: TableRow[] = tables[tab] ?? [];
  const setRows = (f: (now: TableRow[]) => TableRow[]) => setTables((all) => ({ ...all, [tab]: f(all[tab] ?? []) }));

  // A new row takes the cursor in its nickname.
  useEffect(() => {
    const at = focusRow.current;
    if (at === null) return;
    focusRow.current = null;
    gridRef.current?.querySelector<HTMLInputElement>(`[data-row="${at}"][data-col="nickname"]`)?.focus();
  }, [tables]);

  // `CheckTableRow` after a path, a format or options change: the status column says why a library cannot be read.
  const checkKey = `${tab}\u0003${rows.map((r) => `${r.uri}\u0001${r.type}\u0001${r.options}`).join("\u0002")}`;
  useEffect(() => {
    if (!view) return;
    const at = tab;
    const pending = rows.map((r, i) => ({ r, i })).filter(({ r }) => r.uri.trim() !== "" && r.status === undefined);
    if (pending.length === 0) return;
    const timer = setTimeout(() => {
      checkLibraryRows(
        kind,
        pending.map((p) => p.r)
      )
        .then((results) => {
          setTables((all) => ({
            ...all,
            [at]: (all[at] ?? []).map((r, i) => {
              const k = pending.findIndex((p) => p.i === i);
              const found = k >= 0 ? results[k] : undefined;
              const asked = k >= 0 ? (pending[k] as { r: TableRow }).r : undefined;
              if (!found || !asked || r.uri !== asked.uri || r.type !== asked.type || r.options !== asked.options) return r;
              return { ...r, status: found.error ? "error" : r.type === "Table" ? "table" : "ok", error: found.error, path: found.path };
            }),
          }));
        })
        .catch(() => {
          /* the server is busy: the rows are checked again at the next edit */
        });
    }, 300);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [checkKey, view, kind]);

  const tabs: Tab[] = [
    { id: GLOBAL, title: "Global Libraries", scope: "global", writable: kv?.global.writable ?? false, closable: false },
    { id: PROJECT, title: "Project Specific Libraries", scope: "project", writable: true, closable: false },
    ...opened,
  ];
  const current = tabs.find((t) => t.id === tab) ?? (tabs[1] as Tab);
  const editable = current.writable;

  const substitutions = useMemo(() => {
    if (!view) return [];
    const uris = Object.values(tables).flatMap((t) => urisOf(t));
    const values: VariableValue[] = [
      ...view.variables.undefined.map((u) => ({ name: u.name, value: u.value })),
      ...view.variables.global.map((g) => ({ name: g.name, value: g.value })),
      ...view.variables.project,
      { name: "KIPRJMOD", value: view.project_dir },
    ];
    return pathSubstitutions(uris, kind, view.major, values);
  }, [view, tables, kind]);

  const close = () => closeLibraryTables();

  /** Ask a question over the dialog and wait for the button pressed. */
  const ask = (title: string, msg: string, buttons: string[]) => new Promise<string>((resolve) => setQuestion({ title, message: msg, buttons, resolve }));

  const changeTab = (id: string) => {
    setTab(id);
    setSelected([]);
    setCursor(-1);
    setMenu(null);
  };

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

  /**
   * Ctrl+V on the grid (`GRID_TRICKS::paste_clipboard` -> `LIB_TABLE_GRID_TRICKS::paste_text`), and in a cell being edited (`onCharHook`): a table's s-expression
   * (its preamble in the text) puts that table's rows at the top; a text of several cells (tabs or commas, line breaks) fills the rows from the cursor's down,
   * adding rows as needed -- in a cell only when its row has no nickname yet (else the bell: nothing). A plain text pastes into the cell as typing would.
   */
  const onPaste = async (e: React.ClipboardEvent) => {
    if (!editable) return;
    const text = e.clipboardData.getData("text/plain");
    if (!text) return;
    const input = (e.target as HTMLElement).closest<HTMLInputElement>("input[data-row]");
    const at = input ? Number(input.dataset.row) : cursor;
    if (input) {
      const how = cellPasteKind(rows[at], text);
      if (how === "cell") return;
      e.preventDefault();
      if (how === "refuse") return;
    } else e.preventDefault();
    setProblem(null);
    setMessage(null);
    if (isTableText(kind, text)) {
      const r = await parseTableText(kind, text);
      if (r.error || !r.rows) return setMessage(r.error ?? "The text is not a library table.");
      setRows((now) => insertTableRows(now, r.rows!));
      return;
    }
    // in a cell: `SetGridCursor( row, 0 )` first; on the grid: the cursor's row, its first column (the grid keeps no cell cursor of its own here)
    const col: GridColumn = "status";
    setRows((now) => pasteTableText(now, Math.max(0, at), col, text, kind, supportsVisibility));
  };

  const addEmpty = () => {
    setProblem(null);
    focusRow.current = rows.length;
    setRows((now) => appendRow(now));
    setSelected([rows.length]);
    setCursor(rows.length);
  };

  const remove = () => {
    const doomed = selected.length > 0 ? selected : cursor >= 0 ? [cursor] : [];
    if (doomed.length === 0) return;
    const next = deleteRows(rows, doomed);
    setRows(() => next);
    const at = Math.min(Math.min(...doomed), next.length - 1);
    setSelected(at >= 0 ? [at] : []);
    setCursor(at);
    setProblem(null);
  };

  const move = (delta: -1 | 1) => {
    if (cursor < 0) return;
    const m = moveRow(rows, cursor, delta);
    setRows(() => m.rows);
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
        type: what === "table" ? "Table" : what === "http" ? "HTTP" : "KiCad",
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

  /** `OpenTable`: a chained table in a closable tab of its own, editable when its file can be written. */
  const openTable = (row: TableRow) => {
    if (!row.table) return;
    const id = row.table.path;
    if (!opened.some((t) => t.id === id)) {
      setOpened((now) => [...now, { id, title: row.nickname, scope: "nested", path: id, writable: row.table?.writable ?? false, closable: true }]);
      setTables((all) => ({ ...all, [id]: copy(row.table?.rows ?? []) }));
      setOriginals((all) => ({ ...all, [id]: copy(row.table?.rows ?? []) }));
    }
    changeTab(id);
  };

  const closeTab = (id: string) => {
    setOpened((now) => now.filter((t) => t.id !== id));
    if (tab === id) changeTab(PROJECT);
  };

  const rowMenu = (e: React.MouseEvent, i: number) => {
    e.preventDefault();
    const sel = selected.includes(i) ? selected : [i];
    if (!selected.includes(i)) {
      setSelected([i]);
      setCursor(i);
    }
    const entries: MenuEntry[] = [];
    const row = rows[i];
    if (row?.type === "Table" && row.table) entries.push({ label: "Open Library Table", onSelect: () => openTable(row) });
    if (editable) {
      const offers = flagOffers(rows, sel);
      if (entries.length > 0) entries.push({ label: "", separator: true, onSelect: () => {} });
      if (offers.activate) entries.push({ label: "Activate Selected", onSelect: () => setRows((now) => setFlag(now, sel, "enabled", true)) });
      if (offers.deactivate) entries.push({ label: "Deactivate Selected", onSelect: () => setRows((now) => setFlag(now, sel, "enabled", false)) });
      if (supportsVisibility && offers.show) entries.push({ label: "Set Visible Flag", onSelect: () => setRows((now) => setFlag(now, sel, "visible", true)) });
      if (supportsVisibility && offers.hide) entries.push({ label: "Unset Visible Flag", onSelect: () => setRows((now) => setFlag(now, sel, "visible", false)) });
      // `showPopupMenu`: "Edit Options..." for one row that is not a chained table
      if (canEditOptions(rows, sel)) entries.push({ label: "Edit Options...", onSelect: () => setOptionsOf({ tab, row: sel[0] as number }) });
      entries.push({ label: "", separator: true, onSelect: () => {} });
      entries.push({ label: "Delete Row", onSelect: () => setRows((now) => deleteRows(now, sel)) });
    }
    if (entries.length > 0) setMenu({ x: e.clientX, y: e.clientY, entries });
  };

  /** Reset Libraries (`onReset`): asked when the global table has rows; done at once -- the dialog's Cancel does not take it back. */
  const reset = async () => {
    const global = tables[GLOBAL] ?? [];
    if (global.length > 0 && (await ask("Confirmation", RESET_QUESTION, ["No", "Yes"])) !== "Yes") return;
    setBusy(true);
    try {
      const answer = await resetGlobalTable(kind);
      if ("ok" in answer && answer.ok) {
        adopt(answer, true);
        setOpened((now) => now.filter((t) => t.scope !== "nested"));
        changeTab(GLOBAL);
        dispatch({ type: "TOAST", message: `Reset your global ${kv?.file ?? "library table"}.`, kind: "info" });
      } else if ("error" in answer) setMessage(answer.error);
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  /** Migrate Libraries (`onConvertLegacyLibraries` / `onMigrateLibraries`): the selected rows of another format, each converted into the KiCad library beside it. */
  const migrate = async () => {
    const at = migratableRows(kind, rows, selected, cursor);
    if (at.length === 0) return void (await ask("Migrate Libraries", MIGRATE_NONE, ["OK"]));
    if ((await ask("Confirmation", migrateQuestion(kind, at.map((i) => (rows[i] as TableRow).nickname)), ["No", "Yes"])) !== "Yes") return;
    const chosen = at.map((i) => rows[i] as TableRow);
    const table = current.scope === "global" ? (kv?.global.target ?? undefined) : current.scope === "nested" ? current.path : kv?.project.path;
    setBusy(true);
    try {
      const plan = await migrateLibraries(kind, chosen, { plan: true, table });
      const errors: string[] = [];
      const overwrite: number[] = [];
      let upTo = chosen.length;
      for (const p of plan) {
        if (p.error) {
          errors.push(p.error);
          continue;
        }
        if (p.exists) {
          const said = await ask("Migrate Library", migrateOverwriteQuestion(kind, p.dest ?? ""), ["Cancel", "No", "Yes"]);
          if (said === "Cancel") {
            upTo = p.row; // KiCad's loop returns here: the rows before it are migrated
            break;
          }
          if (said === "Yes") overwrite.push(p.row);
        }
      }
      const runRows = chosen.slice(0, upTo);
      const results = runRows.length > 0 ? await migrateLibraries(kind, runRows, { overwrite, table }) : [];
      setRows((now) =>
        now.map((r, i) => {
          const k = at.indexOf(i);
          const res = results.find((x) => x.row === k && x.uri !== undefined);
          if (!res) return r;
          return setCell(setCell({ ...r, options: res.options ?? r.options }, "uri", res.uri ?? r.uri, kind), "type", res.type ?? "KiCad", kind);
        })
      );
      for (const r of results) if (r.error && !errors.includes(r.error)) errors.push(r.error);
      if (errors.length > 0) setMessage(errors.join("\n"));
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  /** OK: `TransferDataFromWindow` -- every tab through `VerifyTable` first, then each table that changed is written: the global one, the board's, the chained ones. */
  const ok = async () => {
    setMessage(null);
    const fail = (id: string, p: TableProblem) => {
      changeTab(id);
      setProblem({ ...p, tab: id });
      setMessage(p.message);
      setSelected([p.row]);
      setCursor(p.row);
      queueMicrotask(() => gridRef.current?.querySelector<HTMLInputElement>(`[data-row="${p.row}"][data-col="${p.column}"]`)?.focus());
    };
    const writes: { id: string; scope: TableScope; path?: string; rows: TableRow[] }[] = [];
    for (const t of tabs) {
      if (!t.writable) continue;
      const these = tables[t.id] ?? [];
      const unwritten = unwritable(these);
      if (unwritten) return fail(t.id, unwritten);
      const verified = verifyRows(these, supportsVisibility);
      if ("problem" in verified) return fail(t.id, verified.problem);
      if (!sameRows(verified.rows, originals[t.id] ?? [])) writes.push({ id: t.id, scope: t.scope, path: t.path, rows: verified.rows });
    }
    if (writes.length === 0) return close();
    setBusy(true);
    try {
      for (const w of writes) {
        const answer = await saveLibraryTable(kind, w.rows, w.scope, w.path);
        if ("error" in answer && answer.error) {
          const column = (["nickname", "uri", "type", "options", "descr"] as const).find((c) => c === answer.column);
          if (answer.row !== undefined && column) return fail(w.id, { row: answer.row, column, message: answer.error });
          changeTab(w.id);
          return setMessage(answer.error);
        }
      }
      const said = writes.map((w) => (w.scope === "global" ? `your global ${kv?.file ?? "library table"}` : w.scope === "project" ? `the project's ${kv?.file ?? "library table"}` : baseName(w.path ?? "")));
      dispatch({ type: "TOAST", message: `Saved ${said.join(", ")}.`, kind: "info" });
      close();
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const title = tableTitle(kind);
  const globalView = kv?.global;
  const noteOf =
    tab === GLOBAL
      ? globalView?.note
      : tab === PROJECT
        ? undefined
        : editable
          ? `A table the tables chain (${current.path}): OK writes your changes to it.`
          : `A table the tables chain (${current.path}), which cannot be written here.`;
  const addEntries: { what: BrowseWhat; label: string }[] =
    kind === "symbol"
      ? [
          { what: "symbol", label: "KiCad (.kicad_sym)" },
          // `PANEL_SYM_LIB_TABLE`'s browse menu: "Add folder-based entry right after KiCad file-based entry" (KiCad 10's `.kicad_symdir` libraries)
          { what: "symbol_folder", label: "KiCad (folder with .kicad_sym files)" },
          // `SCH_IO_HTTP_LIB::GetLibraryDesc` (UI-LEFT-2 LL5: the server reads it)
          { what: "http", label: "HTTP (*.kicad_httplib)" },
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
    <td className={problem && problem.tab === tab && problem.row === i && problem.column === column ? "lt-bad" : undefined}>
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
        // `LIB_TABLE_GRID_TRICKS::handleDoubleClick`: a double click on the Options cell opens the options editor
        onDoubleClick={column === "options" && editable && r.type !== "Table" ? () => setOptionsOf({ tab, row: i }) : undefined}
        onChange={(e) => edit(i, column, e.target.value)}
      />
    </td>
  );

  const optionsRow = optionsOf ? (tables[optionsOf.tab] ?? [])[optionsOf.row] : undefined;

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div
        className="dialog lt-dialog"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && !browse && !menu && !question && !optionsOf && close()}
        role="dialog"
        aria-label={title}
      >
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="lt-body">
          {loadError && <div className="lt-banner">{loadError}</div>}
          <div className="lt-tabs" role="tablist">
            {tabs.map((t) => (
              <div key={t.id} role="tab" aria-selected={tab === t.id} className={`lt-tab${tab === t.id ? " active" : ""}`} onClick={() => changeTab(t.id)} title={t.path}>
                <span>{t.title}</span>
                {t.closable && (
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
          <div className="lt-gridwrap" ref={gridRef} onPaste={(e) => void onPaste(e)} data-lt-grid-paste>
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
                {rows.map((r, i) => (
                  <tr
                    key={i}
                    className={`${selected.includes(i) ? "selected" : ""}${r.enabled ? "" : " disabled-row"}`}
                    onMouseDown={(e) => (e.target as HTMLElement).tagName !== "INPUT" && select(i, e)}
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
                    <td className={problem && problem.tab === tab && problem.row === i && problem.column === "uri" ? "lt-bad" : undefined}>
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
                    <td className={problem && problem.tab === tab && problem.row === i && problem.column === "type" ? "lt-bad" : undefined}>
                      <select aria-label={`type of row ${i + 1}`} value={r.type} disabled={!editable} onChange={(e) => edit(i, "type", e.target.value)}>
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
                {rows.length === 0 && (
                  <tr>
                    <td colSpan={supportsVisibility ? 8 : 7} className="lt-dim" style={{ padding: 8 }}>
                      {editable ? "No libraries in this table. Add Existing adds a library file or folder; the + button adds an empty row." : "Nothing in this table."}
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
          {message && (
            <div className="lt-banner" style={{ whiteSpace: "pre-wrap" }}>
              {message}
            </div>
          )}
          {duplicate && (
            <div className="lt-banner" role="alert">
              An item nicknamed &lsquo;{duplicate.nickname}&rsquo; already exists. One of the nicknames will need to be changed.{" "}
              <button onClick={() => setDuplicate(null)}>Skip</button> <button onClick={() => pushRow(duplicate)}>Add Anyway</button>
            </div>
          )}
          <div className="lt-toolbar">
            <button title="Add empty row to table" aria-label="Add empty row to table" disabled={!editable} onClick={addEmpty}>
              {"+"}
            </button>
            <div className="lt-split">
              <button disabled={!editable} onClick={() => setBrowse({ what: kind, row: null })} title="Add Existing">
                Add Existing
              </button>
              <button aria-label="More ways to add a library" title="More ways to add a library" disabled={!editable} onClick={() => setAddMenu((v) => !v)}>
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
            <button title="Move up" aria-label="Move up" disabled={!editable || cursor <= 0} onClick={() => move(-1)}>
              {"↑"}
            </button>
            <button title="Move down" aria-label="Move down" disabled={!editable || cursor < 0 || cursor >= rows.length - 1} onClick={() => move(1)}>
              {"↓"}
            </button>
            <button title="Remove library from table" aria-label="Remove library from table" disabled={!editable || (selected.length === 0 && cursor < 0)} onClick={remove}>
              {"🗑"}
            </button>
            <span className="lt-spacer" />
            {kind !== "design_block" && (
              // `m_resetGlobal->Enable( m_notebook->GetSelection() == 0 )`
              <button disabled={busy || tab !== GLOBAL || !globalView?.writable} onClick={() => void reset()}>
                Reset Libraries
              </button>
            )}
            <button disabled={busy || !editable} onClick={() => void migrate()}>
              Migrate Libraries
            </button>
          </div>
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
      {optionsOf && optionsRow && (
        <PluginOptionsDialog
          kind={kind}
          nickname={optionsRow.nickname}
          type={optionsRow.type}
          options={optionsRow.options}
          onCancel={() => setOptionsOf(null)}
          onOk={(options) => {
            const at = optionsOf;
            setOptionsOf(null);
            // `optionsEditor`: the row takes the new options when they differ
            if (options !== optionsRow.options) setTables((all) => ({ ...all, [at.tab]: (all[at.tab] ?? []).map((r, i) => (i === at.row ? setCell(r, "options", options, kind) : r)) }));
          }}
        />
      )}
      {question && <TableQuestion q={question} onDone={() => setQuestion(null)} />}
      {menu && (
        // a click on an entry must not reach the backdrop, whose click closes the dialog
        <div onClick={(e) => e.stopPropagation()}>
          <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} onClose={() => setMenu(null)} />
        </div>
      )}
    </div>
  );
}
