// pcbnew.Control.showNetInspector ("Net Inspector"): the docked `PCB_NET_INSPECTOR_PANEL` (pcbnew/widgets/pcb_net_inspector_panel.cpp over
// pcbnew/widgets/net_inspector_panel.cpp at 8303b2ad), a pane under the canvas (`.Bottom()`, components/panels/BottomDock.tsx) that View > Panels > Net
// Inspector and the Appearance panel's Nets button show and hide (`ToggleNetInspector`).
//
// What it holds (`NET_INSPECTOR_PANEL`): the search box ("Filter"), the configure button ("Configure netlist inspector": `OnConfigButton`'s menu) and the list
// (`wxDataViewCtrl`, several rows selectable). The list is kicad-port/netInspector.ts's: KiCad's columns over the backend's numbers (`GET /api/net_inspector`,
// the lengths and the time-domain delays), the groups (custom rules and Group by Netclass, opened and closed by their triangle), the sort of a heading's click,
// the columns shown or hidden (the menu's Show / Hide Columns, or a right click on the headings). A double click or Enter on rows highlights their nets
// (`OnNetsListItemActivated` -> `highlightSelectedNets`; a group stands for its nets), as does the rows' menu's Highlight Selected Net(s); a net highlighted on
// the board selects its row (`OnBoardHighlightNetChanged`). The rows' menu (`OnNetsListContextMenu`) also renames, deletes and adds nets with KiCad's prompts
// and messages, each one undoable step (`rename_net`, `delete_nets`, `add_net`; KiCad's are not undoable). Every change of the settings is kept at once
// (`SaveSettings`, state/netInspectorSettings.ts).
//
// Different from KiCad: the columns cannot be dragged into another order or resized (they size to their contents), so the header menu has no Auto-fit
// Column / Auto-fit All Columns; Save Net Inspector Report... downloads `net_inspector_report.csv` (no file dialog).
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { saveTextFile } from "../../api/libraryClient";
import {
  addGroupRule,
  addNetStep,
  deleteGroupQuestion,
  deleteNetQuestion,
  lineKey,
  netInspectorCell,
  netInspectorColumns,
  netInspectorConfigMenu,
  netInspectorContextMenu,
  netInspectorGroupCell,
  netInspectorReport,
  netInspectorTree,
  netNameInvalid,
  netShown,
  removeGroupRule,
  renameParts,
  renameStep,
  showHideColumnsMenu,
  sortBy,
  toggleExpanded,
  toggleSetting,
  type NetInspectorLine,
  type NetInspectorMenuEntry,
  type NetInspectorMenuId,
  type NetInspectorRow,
} from "../../kicad-port/netInspector";
import { highlightedNets } from "../../kicad-port/boardControl";
import { boardNetNames } from "../../kicad-port/pcbFind";
import { setNetInspectorSettings, useNetInspectorSettings } from "../../state/netInspectorSettings";
import { useStudioApi, useStudioDispatch, useStudioState } from "../../state/store";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";

/** A prompt the panel is waiting on: `WX_TEXT_ENTRY_DIALOG`, `IsOK` or `DisplayError`. */
type Prompt = { id: number } & (
  | { kind: "text"; title: string; label: string; initial: string; help?: string; done: (text: string | null) => void }
  | { kind: "ask"; message: string; done: (yes: boolean) => void }
  | { kind: "say"; title: string; message: string; done: () => void }
);

function PromptDialog({ prompt, onClose }: { prompt: Prompt; onClose: () => void }) {
  const [text, setText] = useState(prompt.kind === "text" ? prompt.initial : "");
  const finish = (f: () => void) => {
    onClose();
    f();
  };
  const title = prompt.kind === "text" ? prompt.title : prompt.kind === "ask" ? "Confirmation" : prompt.title;
  const ok = () => finish(() => (prompt.kind === "text" ? prompt.done(text) : prompt.kind === "ask" ? prompt.done(true) : prompt.done()));
  const cancel = () => finish(() => (prompt.kind === "text" ? prompt.done(null) : prompt.kind === "ask" ? prompt.done(false) : prompt.done()));
  return (
    <div className="dialog-backdrop" onClick={cancel} data-testid="net-inspector-prompt-backdrop">
      <div className="dialog" role="dialog" aria-label={title} style={{ width: 380 }} onClick={(e) => e.stopPropagation()} data-testid="net-inspector-prompt">
        <div className="dialog-header">
          <span>{title}</span>
        </div>
        <div className="dialog-body" style={{ padding: "10px 12px", fontSize: 12 }}>
          {prompt.kind === "text" ? (
            <>
              <div style={{ marginBottom: 6 }}>{prompt.label}</div>
              <input
                autoFocus
                style={{ width: "100%" }}
                value={text}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") ok();
                  if (e.key === "Escape") cancel();
                }}
                data-testid="net-inspector-prompt-text"
              />
              {prompt.help && <div style={{ marginTop: 6, fontStyle: "italic", color: "var(--chrome-text-dim)" }}>{prompt.help}</div>}
            </>
          ) : (
            <div style={{ whiteSpace: "pre-line" }} data-testid="net-inspector-prompt-message">
              {prompt.message}
            </div>
          )}
        </div>
        <div className="dialog-footer">
          {prompt.kind === "say" ? (
            <button className="primary" autoFocus onClick={ok} data-testid="net-inspector-prompt-ok">
              OK
            </button>
          ) : (
            <>
              <button className="primary" onClick={ok} data-testid="net-inspector-prompt-ok">
                {prompt.kind === "ask" ? "Yes" : "OK"}
              </button>
              <button onClick={cancel} data-testid="net-inspector-prompt-cancel">
                {prompt.kind === "ask" ? "No" : "Cancel"}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/** A menu of kicad-port/netInspector.ts as the shared context menu draws it: a check item with its mark, a submenu as its title and its items under it. */
function menuEntries(entries: readonly NetInspectorMenuEntry[], run: (id: NetInspectorMenuId) => void): MenuEntry[] {
  const out: MenuEntry[] = [];
  for (const e of entries) {
    if (e.kind === "separator") out.push({ label: "", onSelect: () => {}, separator: true });
    else if (e.kind === "submenu") {
      out.push({ label: "", onSelect: () => {}, separator: true });
      out.push({ label: e.label, onSelect: () => {}, disabled: true });
      out.push(...menuEntries(e.items, run).map((m) => (m.separator ? m : { ...m, label: `    ${m.label}` })));
    } else {
      const mark: ReactNode = e.checked === undefined ? undefined : <span aria-hidden>{e.checked ? "✓" : ""}</span>;
      out.push({ label: e.label, onSelect: () => run(e.id), disabled: !e.enabled, icon: mark, hint: e.checked === undefined ? undefined : e.checked ? "checked" : "unchecked" });
    }
  }
  return out;
}

export function NetInspectorPanel() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const settings = useNetInspectorSettings();
  const [rows, setRows] = useState<NetInspectorRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** The selected lines' keys (`lineKey`), in the order they were selected. */
  const [selected, setSelected] = useState<string[]>([]);
  const [anchor, setAnchor] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  /** `m_highlightingNets`: the highlight the panel itself set (its nets, a line each), which `OnBoardHighlightNetChanged` does not turn into a selection. */
  const panelHighlight = useRef<string | null>(null);

  // The numbers again whenever the board changes while the panel is shown (`OnBoardChanged`, `OnBoardItemsChanged`, `OnShowPanel`).
  useEffect(() => {
    let live = true;
    fetch("/api/net_inspector")
      .then((r) => r.json())
      .then((j: { ok: boolean; nets?: NetInspectorRow[]; message?: string }) => {
        if (!live) return;
        if (j.ok) {
          setRows(j.nets ?? []);
          setError(null);
        } else setError(j.message ?? "The net inspector could not read the board.");
      })
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [state.board]);

  const timeDomain = settings.showTimeDomainDetails;
  const columns = useMemo(() => netInspectorColumns(state.board?.layers ?? [], timeDomain), [state.board?.layers, timeDomain]);
  const shownColumns = columns.filter((c) => !settings.hiddenColumns.includes(c.key));
  const lines = useMemo(() => (rows ? netInspectorTree(rows, settings) : []), [rows, settings]);
  const lineByKey = useMemo(() => new Map(lines.map((l) => [lineKey(l), l])), [lines]);
  const highlighted = useMemo(() => highlightedNets(state.netHighlight, state.bcx.netHighlightMore), [state.netHighlight, state.bcx.netHighlightMore]);
  const units = state.units;

  // `OnBoardHighlightNetChanged`: a highlight made on the board selects the rows of its nets; none unselects every row.
  const highlightKey = highlighted.join("\n");
  useEffect(() => {
    if (panelHighlight.current === highlightKey) {
      panelHighlight.current = null;
      return;
    }
    panelHighlight.current = null;
    setSelected(highlighted.map((n) => `net:${n}`));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [highlightKey]);

  const selectedLines = selected.map((k) => lineByKey.get(k)).filter((l): l is NetInspectorLine => l !== undefined);
  const facts = { count: selectedLines.length, oneIsGroup: selectedLines.length === 1 && selectedLines[0]!.kind === "group" };

  // `highlightSelectedNets`: the selected nets, a group's nets for a group; nothing selected clears the highlight.
  const highlightSelected = useCallback(
    (keys: readonly string[] = selected) => {
      const nets: string[] = [];
      for (const k of keys) {
        const l = lineByKey.get(k);
        if (!l) continue;
        if (l.kind === "group") nets.push(...l.group.children.map((c) => c.name));
        else nets.push(l.row.name);
      }
      const unique = [...new Set(nets)];
      panelHighlight.current = unique.join("\n");
      dispatch({ type: "SET_NET_HIGHLIGHT_SET", nets: unique });
    },
    [selected, lineByKey, dispatch]
  );

  const promptId = useRef(0);
  const ask = (message: string) => new Promise<boolean>((done) => setPrompt({ id: ++promptId.current, kind: "ask", message, done }));
  const say = (title: string, message: string) => new Promise<void>((done) => setPrompt({ id: ++promptId.current, kind: "say", title, message, done }));
  const promptText = (title: string, label: string, initial: string, help?: string) => new Promise<string | null>((done) => setPrompt({ id: ++promptId.current, kind: "text", title, label, initial, help, done }));

  /** `BOARD::FindNet`: a net of the board by its stored name. */
  const inUse = (name: string) => (rows ?? []).some((r) => r.name === name) || (state.board ? boardNetNames(state.board).includes(name) : false);

  /** A text entry with `NETNAME_VALIDATOR`: the dialog again after "Invalid signal name". */
  const promptNetName = async (title: string, label: string, initial: string, help?: string): Promise<string | null> => {
    let value = initial;
    for (;;) {
      const typed = await promptText(title, label, value, help);
      if (typed === null) return null;
      const invalid = netNameInvalid(typed);
      if (!invalid) return typed;
      await say("Invalid signal name", invalid);
      value = typed;
    }
  };

  const onRename = async () => {
    // `onRenameSelectedNet`: one net row
    const l = selectedLines.length === 1 ? selectedLines[0]! : null;
    if (!l || l.kind !== "net") return;
    const net = l.row.name;
    let unchanged = renameParts(net).short;
    let initial = unchanged;
    for (;;) {
      const typed = await promptNetName("Rename Net", "Net name:", initial);
      if (typed === null) return;
      const step = renameStep(net, typed, unchanged, inUse);
      if (step.kind === "done") return;
      if (step.kind === "error") {
        await say("Error", step.message);
        unchanged = step.retry;
        initial = step.retry;
        continue;
      }
      if (await api.cmd({ op: "rename_net", from: net, to: step.to })) setSelected([`net:${step.to}`]);
      return;
    }
  };

  const onAddNet = async () => {
    let initial = "";
    for (;;) {
      const typed = await promptNetName("New Net", "Net name:", initial);
      if (typed === null) return;
      const step = addNetStep(typed, inUse);
      if (step.kind === "done") return;
      if (step.kind === "error") {
        await say("Error", step.message);
        initial = step.retry;
        continue;
      }
      await api.cmd({ op: "add_net", name: step.to });
      return;
    }
  };

  const onDelete = async () => {
    // `onDeleteSelectedNet`: each selected net (asked about when it has pads), each selected group's nets after "Delete all nets in group"
    const names: string[] = [];
    const deleteOne = async (r: NetInspectorRow) => {
      const q = deleteNetQuestion(r);
      if (q === null || (await ask(q))) names.push(r.name);
    };
    for (const l of selectedLines) {
      if (l.kind === "group") {
        const q = deleteGroupQuestion(l.group);
        if (q !== null && (await ask(q))) for (const c of l.group.children) await deleteOne(c);
      } else await deleteOne(l.row);
    }
    const unique = [...new Set(names)];
    if (unique.length > 0 && (await api.cmd({ op: "delete_nets", names: unique }))) setSelected([]);
  };

  const onAddGroup = async () => {
    const typed = await promptNetName("New Group", "Group name / pattern:", "", "(Use /.../ to indicate a regular expression.)");
    if (typed === null || typed === "") return;
    setNetInspectorSettings(addGroupRule(settings, typed));
  };

  const report = () => {
    // `generateReport` walks the model's own list (`itemAt( row )`), not the view's sort: the listed nets in the order they came, every column
    saveTextFile(netInspectorReport((rows ?? []).filter((r) => netShown(r, settings)), units, columns, timeDomain), "net_inspector_report.csv", "text/csv");
  };

  const run = (id: NetInspectorMenuId) => {
    switch (id) {
      case "add_net":
        return void onAddNet();
      case "rename_net":
        return void onRename();
      case "delete_net":
        return void onDelete();
      case "add_group":
        return void onAddGroup();
      case "remove_selected_group": {
        const l = selectedLines[0];
        if (selectedLines.length === 1 && l?.kind === "group") setNetInspectorSettings(removeGroupRule(settings, l.group.name));
        return;
      }
      case "generate_report":
        return report();
      case "highlight_selected_nets":
        return highlightSelected();
      case "clear_highlighting":
        // `onClearHighlighting`: the board's highlight off, the rows left as they are
        panelHighlight.current = "";
        return dispatch({ type: "SET_NET_HIGHLIGHT_SET", nets: [] });
    }
    const next = toggleSetting(settings, id);
    if (next) setNetInspectorSettings(next);
  };

  const openMenu = (e: ReactMouseEvent, entries: readonly NetInspectorMenuEntry[]) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY, entries: menuEntries(entries, run) });
  };

  const clickLine = (e: ReactMouseEvent, key: string) => {
    if (e.shiftKey && anchor) {
      const keys = lines.map(lineKey);
      const a = keys.indexOf(anchor);
      const b = keys.indexOf(key);
      if (a >= 0 && b >= 0) {
        setSelected(keys.slice(Math.min(a, b), Math.max(a, b) + 1));
        return;
      }
    }
    if (e.ctrlKey || e.metaKey) setSelected(selected.includes(key) ? selected.filter((k) => k !== key) : [...selected, key]);
    else setSelected([key]);
    setAnchor(key);
  };

  const onListKey = (e: ReactKeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      highlightSelected();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      const keys = lines.map(lineKey);
      const at = keys.indexOf(selected[selected.length - 1] ?? "");
      const next = keys[Math.max(0, Math.min(keys.length - 1, at + (e.key === "ArrowDown" ? 1 : -1)))];
      if (next) {
        setSelected(e.shiftKey ? [...new Set([...selected, next])] : [next]);
        setAnchor(next);
      }
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
      // the list's own Select All, not the board's
      e.preventDefault();
      e.stopPropagation();
      setSelected(lines.map(lineKey));
    }
  };

  const align = (key: string) => (key === "name" || key === "netclass" ? "left" : "center");
  // each column as wide as its widest cell (the list scrolls sideways when they do not fit), so a long net name never runs into the class
  const gridColumns = shownColumns.map(() => "max-content").join(" ");

  return (
    <div className="net-inspector" data-testid="net-inspector" style={{ display: "flex", flexDirection: "column", height: "100%", minHeight: 0, fontSize: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 4, padding: "2px 4px" }}>
        <input
          placeholder="Filter"
          value={settings.filter}
          onChange={(e) => setNetInspectorSettings({ ...settings, filter: e.target.value })}
          style={{ flex: 1, minWidth: 60, padding: "3px 6px" }}
          data-testid="net-inspector-filter"
        />
        <button
          type="button"
          title="Configure netlist inspector"
          aria-label="Configure netlist inspector"
          data-testid="net-inspector-config"
          onClick={(e) => {
            const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
            setMenu({ x: r.left, y: r.bottom, entries: menuEntries(netInspectorConfigMenu(settings, facts, columns), run) });
          }}
        >
          ⚙
        </button>
      </div>
      {error && <div className="panel-empty">{error}</div>}
      <div
        role="treegrid"
        tabIndex={0}
        aria-multiselectable
        aria-label="Nets"
        onKeyDown={onListKey}
        onContextMenu={(e) => openMenu(e, netInspectorContextMenu(facts, highlighted.length > 0))}
        style={{ flex: 1, minHeight: 0, overflow: "auto", outline: "none" }}
        data-testid="net-inspector-list"
      >
        <div className="kv-grid" style={{ display: "grid", gridTemplateColumns: gridColumns, columnGap: 14, rowGap: 0, alignItems: "center", minWidth: "100%", width: "max-content" }}>
          {shownColumns.map((c) => (
            <span
              key={c.key}
              role="columnheader"
              data-testid={`net-inspector-col-${c.key}`}
              style={{ fontWeight: 600, cursor: "pointer", textAlign: align(c.key), whiteSpace: "nowrap", position: "sticky", top: 0, background: "var(--chrome-bg-raised)", padding: "2px 0" }}
              onClick={() => setNetInspectorSettings(sortBy(settings, c.key))}
              onContextMenu={(e) => openMenu(e, showHideColumnsMenu(columns, settings.hiddenColumns))}
            >
              {c.label}
              {(settings.sortingColumn ?? "name") === c.key ? ((settings.sortingColumn ? settings.sortAscending : true) ? " ▲" : " ▼") : ""}
            </span>
          ))}
          {lines.map((l) => {
            const key = lineKey(l);
            const isSel = selected.includes(key);
            const hot = l.kind === "net" && highlighted.includes(l.row.name);
            const cell = (k: (typeof shownColumns)[number]["key"]) => (l.kind === "group" ? netInspectorGroupCell(l.group, k, units, timeDomain) : netInspectorCell(l.row, k, units, false, timeDomain));
            return (
              <span
                key={key}
                role="row"
                aria-selected={isSel}
                data-testid="net-inspector-row"
                data-net={l.kind === "net" ? l.row.name : undefined}
                data-group={l.kind === "group" ? `${l.group.type}:${l.group.name}` : undefined}
                style={{ display: "contents", cursor: "default" }}
                onClick={(e) => clickLine(e, key)}
                onDoubleClick={() => highlightSelected(selected.includes(key) ? selected : [key])}
                onContextMenu={(e) => {
                  if (!selected.includes(key)) setSelected([key]);
                  const sel = selected.includes(key) ? facts : { count: 1, oneIsGroup: l.kind === "group" };
                  openMenu(e, netInspectorContextMenu(sel, highlighted.length > 0));
                }}
              >
                {shownColumns.map((c) => (
                  <span
                    key={c.key}
                    data-col={c.key}
                    style={{
                      textAlign: align(c.key),
                      whiteSpace: "nowrap",
                      padding: "1px 0",
                      paddingLeft: c.key === "name" && l.kind === "net" && l.depth === 1 ? 18 : undefined,
                      fontWeight: l.kind === "group" || hot ? 600 : 400,
                      background: isSel ? "var(--chrome-accent-bg, rgba(80, 140, 255, 0.25))" : undefined,
                      color: hot ? "var(--chrome-accent)" : c.key === "name" ? undefined : "var(--chrome-text-dim)",
                    }}
                  >
                    {c.key === "name" && l.kind === "group" && (
                      <span
                        role="button"
                        aria-label={l.expanded ? "Collapse" : "Expand"}
                        data-testid="net-inspector-expander"
                        style={{ display: "inline-block", width: 14, cursor: "pointer" }}
                        onClick={(e) => {
                          e.stopPropagation();
                          setNetInspectorSettings(toggleExpanded(settings, l.group.name));
                        }}
                      >
                        {l.expanded ? "▾" : "▸"}
                      </span>
                    )}
                    {cell(c.key)}
                  </span>
                ))}
              </span>
            );
          })}
        </div>
        {rows && lines.length === 0 && <span className="panel-empty">No nets.</span>}
      </div>
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} onClose={() => setMenu(null)} />}
      {prompt && <PromptDialog key={prompt.id} prompt={prompt} onClose={() => setPrompt(null)} />}
    </div>
  );
}
