// common.Interactive.search ("Search", Ctrl+G) and View > Panels > Search on the schematic: eeschema's docked `SCH_SEARCH_PANE` (eeschema/widgets/sch_search_pane.cpp
// over common/widgets/search_pane.cpp and search_pane_tab.cpp at 8303b2ad; UI-LEFT-2 SS10), a pane under the canvas (`.Bottom()`, `Caption( _( "Search" ) )`,
// `CloseButton( true )`, hidden to begin with).
//
// The search box ("Search"), the menu button (`SEARCH_PANE_MENU`: Zoom to Selection, Pan to Selection, Search Hidden Fields, Search Metadata), and the five
// handlers' tabs (kicad-port/schSearchHandlers.ts): Symbols, Power, Text, Labels, Groups, over every sheet path of the hierarchy (`FindAll`). Typing searches
// the tab that is up; changing tabs searches the new one with the same query; the schematic changing searches again (`OnSchItemsChanged` and the others). A
// heading's click sorts by its column. Selecting rows is `SCH_SEARCH_HANDLER::SelectItems`: when they are all on one sheet the editor goes to it
// (`changeSheet`), selects their items and pans (the default) or zooms to them; rows of several sheets select nothing. A double click or Enter
// (`ActivateItem`) selects the row and opens its properties (`SCH_ACTIONS::properties`). Ctrl+A selects every row, Ctrl+C and the rows' menu's Copy copy the
// selected rows (tab-separated).
//
// The pane docks and floats as the board editor's does (components/panels/BottomDock.tsx; UI-LEFT-3 S3-34), in the schematic frame's own layout. Its list is
// the same `SEARCH_PANE_LISTVIEW` as the board editor's (common/widgets/search_pane_tab.cpp): each column starts at its share of the list's width
// (`RefreshColumnNames`), its edge is dragged to size it (to nothing, which Copy then leaves out) and double-clicked to fit its widest text, and a heading is
// dragged to another place; the order and widths last while the page is open (KiCad saves neither).
import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent } from "react";
import { fetchSchematic } from "../../api/client";
import { fetchHierarchy } from "../../api/schControlClient";
import type { Schematic } from "../../api/types";
import { SCH_SEARCH_TABS, schSearchColumnWidths, schSearchTab, schSearchTarget, sortSchSearchRows, type SchSearchContext, type SchSearchRow, type SchSearchSheet } from "../../kicad-port/schSearchHandlers";
import { autosizeListColumn, clickSearchColumn, moveSearchColumn, searchColumnOrder, searchRowsText, toggleSearchSetting } from "../../kicad-port/searchHandlers";
import { groupBox, groupsOf } from "../../kicad-port/schGroups";
import { unionBox, type Box as SchBox } from "../../kicad-port/schItemGeom";
import { sheetBoxOf } from "../schematic/schGroupBounds";
import { boxCentre, centerViewOn, zoomFitBox } from "../../kicad-port/zoomFit";
import type { Box } from "../../kicad-port/itemBoxes";
import { getSchSearchPane, setSchSearchPane, useSchSearchPane } from "../../state/schSearchPane";
import { useStudioApi, useStudioDispatch, useStudioState } from "../../state/store";
import { useActionRunner } from "../../actions/useActionRunner";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";
import { clickEndsDrag, startColumnMove, startColumnResize, textWidthIn } from "./listColumns";

/** Every sheet path of the hierarchy with the schematic its screen shows there, read again when the design changes. */
function useHierarchySheets(version: string | null, shown: boolean): SchSearchSheet[] {
  const [sheets, setSheets] = useState<SchSearchSheet[]>([]);
  useEffect(() => {
    if (!shown) return;
    let live = true;
    void (async () => {
      try {
        const entries = await fetchHierarchy();
        const out: SchSearchSheet[] = [];
        for (const e of entries) {
          const sch: Schematic = await fetchSchematic(e.path);
          out.push({ path: e.path, page: e.page || String(sch.page_number ?? out.length + 1), sch });
        }
        if (live) setSheets(out);
      } catch {
        if (live) setSheets([]);
      }
    })();
    return () => {
      live = false;
    };
  }, [version, shown]);
  return sheets;
}

export function SchSearchPanel() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const pane = useSchSearchPane();
  const { run } = useActionRunner();
  const runRef = useRef(run);
  runRef.current = run;
  const input = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [anchor, setAnchor] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const [listWidth, setListWidth] = useState(600);
  const tab = SCH_SEARCH_TABS.find((t) => t.id === pane.tab) ?? SCH_SEARCH_TABS[0]!;
  const sort = pane.sort[tab.id] ?? { col: null, ascending: true };
  const sheets = useHierarchySheets(state.version, true);

  // `FocusSearch`: the box takes the focus when the pane is shown and when Find's "Show search panel" asks for it
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, [pane.focus]);

  useEffect(() => {
    const el = listRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const seen = new ResizeObserver(() => setListWidth(el.clientWidth));
    seen.observe(el);
    setListWidth(el.clientWidth);
    return () => seen.disconnect();
  }, []);

  const context = useMemo(
    (): SchSearchContext => ({
      units: state.units,
      settings: pane.settings,
      // `SCH_GROUP::GetPosition`: the centre of its box, the union of what is below it as the canvas boxes it
      groupCentre: (sheet, id) => {
        const b = groupBox(groupsOf(sheet.sch), id, sheetBoxOf(sheet.sch));
        return b ? [(b.minX + b.maxX) / 2, (b.minY + b.maxY) / 2] : null;
      },
    }),
    [state.units, pane.settings]
  );
  // `SEARCH_PANE_TAB::Search`: the handler's hits, then the list's sort
  const rows = useMemo(() => sortSchSearchRows(schSearchTab(tab.id, sheets, pane.query, context), sort.col, sort.ascending), [tab.id, sheets, pane.query, context, sort.col, sort.ascending]);
  const rowById = useMemo(() => new Map(rows.map((r) => [r.id, r])), [rows]);
  useEffect(() => {
    setSelected((s) => s.filter((id) => rowById.has(id)));
  }, [rowById]);
  // The columns on screen: each column's share of the list's width until one is sized by hand, the order, the widths.
  const tabColumns = pane.columns[tab.id];
  const order = searchColumnOrder(tab.columns.length, tabColumns?.order);
  const [liveWidth, setLiveWidth] = useState<{ col: number; w: number } | null>(null);
  const [dropAt, setDropAt] = useState<number | null>(null);
  const baseWidths = tabColumns?.widths?.length === tab.columns.length ? tabColumns.widths : schSearchColumnWidths(tab.id, listWidth);
  const widths = baseWidths.map((w, i) => (liveWidth?.col === i ? liveWidth.w : w));
  /** One column's width set: every column's width is fixed from then on (the rest at what they show now). */
  const setColumnWidth = (col: number, w: number) => {
    const cur = getSchSearchPane().columns[tab.id];
    const base = cur?.widths?.length === tab.columns.length ? cur.widths : schSearchColumnWidths(tab.id, listWidth);
    setSchSearchPane({ columns: { ...getSchSearchPane().columns, [tab.id]: { ...cur, widths: base.map((x, i) => (i === col ? w : x)) } } });
  };
  const headerEls = () => [...(listRef.current?.querySelectorAll('th[role="columnheader"]') ?? [])];

  /** `SCH_SEARCH_HANDLER::SelectItems`: `selectionClear`; rows all on one sheet go to it (`changeSheet`), select their items and pan or zoom to them. */
  const selectRows = async (ids: readonly string[]) => {
    const picked = ids.map((id) => rowById.get(id)).filter((r): r is SchSearchRow => r !== undefined);
    dispatch({ type: "SET_SELECTION", refs: [] });
    const target = schSearchTarget(picked);
    if (!target) return;
    const now = api.getState().currentSheetPath;
    if (target.sheet.join("/") !== now.join("/")) await api.navigateToSheet(target.sheet);
    dispatch({ type: "SET_SELECTION", refs: target.items, raw: true });
    const settings = pane.settings;
    if (settings.selectionZoom === "none") return;
    const sch = api.getState().schematic;
    const boxOf = sch ? sheetBoxOf(sch) : null;
    let sbox: SchBox | null = null;
    for (const r of picked) sbox = unionBox(sbox, boxOf?.(r.item) ?? { minX: r.at[0], minY: r.at[1], maxX: r.at[0], maxY: r.at[1] });
    const box: Box | null = sbox ? [sbox.minX, sbox.minY, sbox.maxX, sbox.maxY] : null;
    const rect = document.querySelector(".pcb-canvas-container")?.getBoundingClientRect();
    if (!box || !rect || rect.width < 50 || rect.height < 50) return;
    const st = api.getState();
    const view =
      settings.selectionZoom === "zoom"
        ? zoomFitBox(box, box, rect.width, rect.height) // `ACTIONS::zoomFitSelection`
        : centerViewOn(st.schematicView, rect.width, rect.height, boxCentre(box)); // `ACTIONS::centerSelection`
    if (view) dispatch({ type: "SET_SCHEMATIC_VIEW", view });
  };

  const choose = (ids: string[]) => {
    setSelected(ids);
    void selectRows(ids);
  };

  /** `SCH_SEARCH_HANDLER::ActivateItem`: the row selected, then `SCH_ACTIONS::properties`. */
  const activate = (id: string) => {
    choose([id]);
    window.setTimeout(() => runRef.current("eeschema.InteractiveEdit.properties"), 60);
  };

  const clickRow = (e: ReactMouseEvent, id: string) => {
    if (e.shiftKey && anchor) {
      const ids = rows.map((r) => r.id);
      const a = ids.indexOf(anchor);
      const b = ids.indexOf(id);
      if (a >= 0 && b >= 0) return choose(ids.slice(Math.min(a, b), Math.max(a, b) + 1));
    }
    setAnchor(id);
    if (e.ctrlKey || e.metaKey) choose(selected.includes(id) ? selected.filter((s) => s !== id) : [...selected, id]);
    else choose([id]);
  };

  const copy = () => {
    const text = searchRowsText(
      rows.filter((r) => selected.includes(r.id)),
      widths
    );
    void navigator.clipboard?.writeText(text).catch(() => {});
  };

  // `SEARCH_PANE_LISTVIEW::OnChar`: Ctrl+A, Ctrl+C, Up and Down (Shift keeps the rows selected so far), Enter activates
  const onListKey = (e: ReactKeyboardEvent) => {
    const ids = rows.map((r) => r.id);
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      e.stopPropagation();
      choose(ids);
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "c") {
      e.preventDefault();
      e.stopPropagation();
      copy();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      const at = ids.indexOf(selected[selected.length - 1] ?? "");
      const next = ids[Math.max(0, Math.min(ids.length - 1, at < 0 ? 0 : at + (e.key === "ArrowDown" ? 1 : -1)))];
      if (next) {
        setAnchor(next);
        choose(e.shiftKey ? [...new Set([...selected, next])] : [next]);
      }
    } else if (e.key === "Enter" && selected.length > 0) {
      e.preventDefault();
      e.stopPropagation();
      activate(selected[selected.length - 1]!);
    }
  };

  const s = pane.settings;
  const check = (on: boolean) => <span aria-hidden>{on ? "✓" : ""}</span>;
  const settingsMenu = (): MenuEntry[] => [
    { label: "Zoom to Selection", hint: "Toggle zooming to selections in the search pane", icon: check(s.selectionZoom === "zoom"), onSelect: () => setSchSearchPane({ settings: toggleSearchSetting(s, "zoom") }) },
    { label: "Pan to Selection", hint: "Toggle panning to selections in the search pane", icon: check(s.selectionZoom === "pan"), onSelect: () => setSchSearchPane({ settings: toggleSearchSetting(s, "pan") }) },
    { label: "", onSelect: () => {}, separator: true },
    { label: "Search Hidden Fields", icon: check(s.searchHiddenFields), onSelect: () => setSchSearchPane({ settings: toggleSearchSetting(s, "hidden") }) },
    { label: "Search Metadata", hint: "Search library links, descriptions and keywords", icon: check(s.searchMetadata), onSelect: () => setSchSearchPane({ settings: toggleSearchSetting(s, "metadata") }) },
  ];

  const tableWidth = widths.reduce((a, w) => a + w, 0);
  return (
    <div className="search-pane" data-testid="sch-search-pane" style={{ display: "flex", flexDirection: "column", height: "100%", minHeight: 0, fontSize: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 4, padding: "2px 4px" }}>
        <input
          ref={input}
          type="search"
          placeholder="Search"
          value={pane.query}
          onChange={(e) => setSchSearchPane({ query: e.target.value })}
          style={{ flex: 1, minWidth: 60, padding: "3px 6px" }}
          data-testid="sch-search-pane-query"
        />
        <button
          type="button"
          title="Search pane options"
          aria-label="Search pane options"
          data-testid="sch-search-pane-menu"
          onClick={(e) => {
            const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
            setMenu({ x: r.left, y: r.bottom, entries: settingsMenu() });
          }}
        >
          ⚙
        </button>
      </div>
      <div className="dock-tabs" role="tablist" style={{ flexWrap: "wrap" }}>
        {SCH_SEARCH_TABS.map((t) => (
          <div
            key={t.id}
            role="tab"
            aria-selected={t.id === tab.id}
            className={`dock-tab${t.id === tab.id ? " active" : ""}`}
            data-testid={`sch-search-pane-tab-${t.id}`}
            onClick={() => {
              setSchSearchPane({ tab: t.id });
              setSelected([]);
            }}
          >
            {t.name}
          </div>
        ))}
      </div>
      <div
        role="grid"
        tabIndex={0}
        aria-multiselectable
        aria-label={tab.name}
        onKeyDown={onListKey}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ x: e.clientX, y: e.clientY, entries: [{ label: "Copy", onSelect: copy, disabled: selected.length === 0 }] });
        }}
        style={{ flex: 1, minHeight: 0, overflow: "auto", outline: "none" }}
        data-testid="sch-search-pane-list"
        ref={listRef}
      >
        <table style={{ width: tableWidth, borderCollapse: "collapse", tableLayout: "fixed" }}>
          <colgroup>
            {order.map((i) => (
              <col key={tab.columns[i]!.name} style={{ width: widths[i] }} />
            ))}
          </colgroup>
          <thead>
            <tr>
              {order.map((i, at) => {
                const c = tab.columns[i]!;
                return (
                  <th
                    key={c.name}
                    role="columnheader"
                    data-testid={`sch-search-pane-col-${i}`}
                    data-col={i}
                    style={{
                      textAlign: c.align,
                      cursor: "pointer",
                      whiteSpace: "nowrap",
                      overflow: "hidden",
                      position: "sticky",
                      top: 0,
                      zIndex: 1,
                      background: "var(--chrome-bg-raised)",
                      fontWeight: 600,
                      boxShadow: dropAt === at ? "inset 2px 0 0 var(--chrome-accent, #4a8cff)" : dropAt === order.length && at === order.length - 1 ? "inset -2px 0 0 var(--chrome-accent, #4a8cff)" : undefined,
                    }}
                    onPointerDown={(e) =>
                      startColumnMove(e, headerEls, setDropAt, (to) => {
                        const cur = getSchSearchPane().columns[tab.id];
                        setSchSearchPane({ columns: { ...getSchSearchPane().columns, [tab.id]: { ...cur, order: moveSearchColumn(tab.columns.length, cur?.order, i, to) } } });
                      })
                    }
                    onClick={() => {
                      if (clickEndsDrag()) return;
                      setSchSearchPane({ sort: { ...pane.sort, [tab.id]: clickSearchColumn(sort, i) } });
                    }}
                  >
                    {c.name}
                    {sort.col === i ? (sort.ascending ? " ▲" : " ▼") : ""}
                    <span
                      className="list-col-resize"
                      role="separator"
                      aria-orientation="vertical"
                      aria-label={`Resize ${c.name}`}
                      data-testid={`sch-search-pane-resize-${i}`}
                      onClick={(e) => e.stopPropagation()}
                      onDoubleClick={(e) => {
                        e.stopPropagation();
                        setColumnWidth(i, autosizeListColumn(c.name, i, rows, textWidthIn(listRef.current)));
                      }}
                      onPointerDown={(e) =>
                        startColumnResize(
                          e,
                          widths[i] ?? 0,
                          0,
                          (w) => setLiveWidth({ col: i, w }),
                          (w) => {
                            setLiveWidth(null);
                            setColumnWidth(i, w);
                          }
                        )
                      }
                    />
                  </th>
                );
              })}
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => {
              const isSel = selected.includes(r.id);
              return (
                <tr
                  key={r.id}
                  role="row"
                  aria-selected={isSel}
                  data-testid="sch-search-pane-row"
                  data-id={r.id}
                  style={{ background: isSel ? "var(--chrome-accent-bg, rgba(80, 140, 255, 0.25))" : undefined, cursor: "default" }}
                  onClick={(e) => clickRow(e, r.id)}
                  onDoubleClick={() => activate(r.id)}
                >
                  {order.map((i) => (
                    <td key={i} style={{ textAlign: tab.columns[i]?.align, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                      {r.cells[i]}
                    </td>
                  ))}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} onClose={() => setMenu(null)} />}
    </div>
  );
}
