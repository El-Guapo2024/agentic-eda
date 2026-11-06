// common.Interactive.search ("Search", Ctrl+G) and View > Panels > Search: the board editor's docked `PCB_SEARCH_PANE` (pcbnew/widgets/pcb_search_pane.cpp over
// common/widgets/search_pane.cpp and search_pane_tab.cpp at 8303b2ad), a pane under the canvas (components/panels/BottomDock.tsx).
//
// The search box ("Search"), the menu button (`SEARCH_PANE_MENU`: Zoom to Selection, Pan to Selection, Search Hidden Fields, Search Metadata), and a notebook of
// the seven handlers' tabs (kicad-port/searchHandlers.ts): Footprints, Zones, Nets, Ratsnest, Text, Groups, Drills. Typing searches the tab that is up; changing
// tabs searches the new one with the same query (`OnNotebookPageChanged`); the board changing searches again (`OnBoardItemsChanged` and the others). A heading's
// click sorts by its column (`OnColClicked`). Selecting rows is `SelectItems`: the items are selected on the board and the view is panned to them (the
// default) or zoomed to them; on the Nets and Ratsnest tabs their nets are highlighted instead. A double click or Enter (`ActivateItem`) selects the row's items
// and opens their properties, or for a net Board Setup's Net Classes page. Ctrl+A selects every row, Ctrl+C and the rows' menu's Copy copy the selected rows
// (tab-separated).
//
// The headings are the report-mode list's (UI-LEFT-2 PP13): each column starts at its share of the list's width (`RefreshColumnNames`: a tenth of the width per
// proportion, so a tab whose proportions add up to more than ten scrolls sideways), its edge is dragged to size it (to nothing, which Copy then leaves out) and
// double-clicked to fit its widest text, and a heading is dragged to another place; the order and widths last while the page is open (KiCad saves neither).
// The Nets tab lists every net of the board (`BOARD::GetNetInfo()`), one no item is on too; the Library Link is the link the board file gives the footprint
// (`GetFPID().Format()`).
//
// The X and Y columns are counted from the origin Preferences > PCB Editor > Origins & Axes chooses (UI-LEFT-2 PP9).
import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent } from "react";
import {
  autosizeSearchColumn,
  clickSearchColumn,
  effectiveNetClassName,
  moveSearchColumn,
  searchColumnOrder,
  searchColumnWidths,
  searchRowsText,
  searchTab,
  sortSearchRows,
  SEARCH_TABS,
  toggleSearchSetting,
  type SearchContext,
  type SearchRow,
} from "../../kicad-port/searchHandlers";
import { clickEndsDrag, startColumnMove, startColumnResize, textWidthIn } from "./listColumns";
import { effectiveClassName } from "../../kicad-port/boardSetupRules";
import { boxOfIds, pcbItemBoxes, unionBoxes, type Box } from "../../kicad-port/itemBoxes";
import { boxCentre, centerViewOn, zoomFitBox } from "../../kicad-port/zoomFit";
import { padIds } from "../../kicad-port/pcbItems";
import { getSearchPane, setSearchPane, useSearchPane } from "../../state/searchPane";
import { useStudioDispatch, useStudioState } from "../../state/store";
import { useActionRunner } from "../../actions/useActionRunner";
import { ContextMenu, type MenuEntry } from "../canvas/ContextMenu";
import { useOriginTransforms } from "../../state/originTransforms";

export function SearchPanel() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const pane = useSearchPane();
  const { run } = useActionRunner();
  const runRef = useRef(run);
  runRef.current = run;
  const input = useRef<HTMLInputElement>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [anchor, setAnchor] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  const board = state.board;
  const tab = SEARCH_TABS.find((t) => t.id === pane.tab) ?? SEARCH_TABS[0]!;
  const sort = pane.sort[tab.id] ?? { col: null, ascending: true };

  // `FocusSearch`: the box takes the focus when the pane is shown (`ToggleSearch`) and when Find's "Show search panel" asks for it.
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, [pane.focus]);

  const boxes = useMemo(() => (board ? pcbItemBoxes(board) : new Map<string, Box>()), [board]);
  // The X and Y cells' `MessageTextFromCoord` goes through the frame's origin transforms (Origins & Axes, UI-LEFT-2 PP9).
  const xf = useOriginTransforms();
  const context = useMemo((): SearchContext => {
    const filledArea = new Map<string, number>();
    // `ZONE::GetFilledArea`: the area of the last fill, every layer of it
    for (const z of state.zoneFill?.zones ?? []) filledArea.set(z.id, (filledArea.get(z.id) ?? 0) + z.area_nm2);
    const ratsnestNets = new Set((state.ratsnest?.edges ?? []).map((e) => e.net));
    const groupCentre = new Map<string, readonly [number, number]>();
    for (const g of board?.drawings?.groups ?? []) {
      const b = boxes.get(g.id);
      if (b) {
        const c = boxCentre(b);
        groupCentre.set(g.id, [c.x, c.y]);
      }
    }
    const classes = board?.board_rules?.net_classes ?? [];
    return { units: state.units, settings: pane.settings, filledArea, ratsnestNets, groupCentre, netClassName: (net) => effectiveNetClassName(net, effectiveClassName(classes, net)), xf };
  }, [state.zoneFill, state.ratsnest, board, boxes, state.units, pane.settings, xf]);

  // `SEARCH_PANE_TAB::Search`: the handler's hits, then the list's sort. The Nets and Ratsnest tabs go through every net of the board (`GetNetInfo()`), one no item
  // names too (`board.nets`).
  const rows = useMemo(
    () => (board ? sortSearchRows(searchTab(tab.id, board, pane.query, context, board.nets ?? []), tab.id, sort.col, sort.ascending) : []),
    [board, tab.id, pane.query, context, sort.col, sort.ascending]
  );

  // The columns on screen: the list's width (each column's share of it until one is sized by hand), the order, the widths.
  const listRef = useRef<HTMLDivElement>(null);
  const [listWidth, setListWidth] = useState(600);
  useEffect(() => {
    const el = listRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const seen = new ResizeObserver(() => setListWidth(el.clientWidth));
    seen.observe(el);
    setListWidth(el.clientWidth);
    return () => seen.disconnect();
  }, [!!board]);
  const tabColumns = pane.columns[tab.id];
  const order = searchColumnOrder(tab.columns.length, tabColumns?.order);
  const [liveWidth, setLiveWidth] = useState<{ col: number; w: number } | null>(null);
  const [dropAt, setDropAt] = useState<number | null>(null);
  const baseWidths = tabColumns?.widths?.length === tab.columns.length ? tabColumns.widths : searchColumnWidths(tab.id, listWidth);
  const widths = baseWidths.map((w, i) => (liveWidth?.col === i ? liveWidth.w : w));
  /** One column's width set: every column's width is fixed from then on (the rest at what they show now). */
  const setColumnWidth = (col: number, w: number) => {
    const current = getSearchPane().columns[tab.id];
    const base = current?.widths?.length === tab.columns.length ? current.widths : searchColumnWidths(tab.id, listWidth);
    setSearchPane({ columns: { ...getSearchPane().columns, [tab.id]: { ...current, widths: base.map((x, i) => (i === col ? w : x)) } } });
  };
  const rowById = useMemo(() => new Map(rows.map((r) => [r.id, r])), [rows]);

  // A new search keeps the rows still found selected (the list's selection is by row; KiCad's `Sort` carries it across a re-sort).
  useEffect(() => {
    setSelected((s) => s.filter((id) => rowById.has(id)));
  }, [rowById]);

  /** The board box of an item a row selects: its own, or a pad's (`REF.NUM[#k]`) from its footprint's pads. */
  const boxOf = (ids: readonly string[]): Box | null => {
    const found: Box[] = [];
    const own = boxOfIds(boxes, ids);
    if (own) found.push(own);
    const wanted = new Set(ids);
    for (const p of board?.parts ?? []) {
      const pids = padIds(p);
      (p.pads ?? []).forEach((pad, i) => {
        if (wanted.has(pad.id ?? pids[i]!)) found.push([pad.x - pad.w / 2, pad.y - pad.h / 2, pad.x + pad.w / 2, pad.y + pad.h / 2]);
      });
    }
    return unionBoxes(found);
  };

  /** `SEARCH_HANDLER::SelectItems` of the rows `ids`. */
  const selectRows = (ids: readonly string[]) => {
    const picked = ids.map((id) => rowById.get(id)).filter((r): r is SearchRow => r !== undefined);
    if (tab.id === "nets" || tab.id === "ratsnest") {
      // `NETS_SEARCH_HANDLER::SelectItems`: `SetHighlight( false )`, then each selected net highlighted
      dispatch({ type: "SET_NET_HIGHLIGHT_SET", nets: picked.map((r) => r.net ?? "").filter((n) => n !== "") });
      return;
    }
    const items = [...new Set(picked.flatMap((r) => r.select))];
    // `ACTIONS::selectionClear`, then `ACTIONS::selectItems` (each item as it is, no group in its place)
    dispatch({ type: "SET_SELECTION", refs: items, raw: true });
    if (items.length === 0 || pane.settings.selectionZoom === "none") return;
    const box = boxOf(items);
    const rect = document.querySelector(".pcb-canvas-container")?.getBoundingClientRect();
    if (!box || !rect) return;
    const view =
      pane.settings.selectionZoom === "zoom"
        ? zoomFitBox(box, box, rect.width, rect.height) // `ACTIONS::zoomFitSelection`
        : centerViewOn(state.view, rect.width, rect.height, boxCentre(box)); // `ACTIONS::centerSelection`
    if (view) dispatch({ type: "SET_VIEW", view });
  };

  const choose = (ids: string[]) => {
    setSelected(ids);
    selectRows(ids);
  };

  /** `ActivateItem`: the row's items selected and their properties opened; a net row opens Board Setup's Net Classes page. */
  const activate = (id: string) => {
    if (tab.id === "nets" || tab.id === "ratsnest") {
      dispatch({ type: "SET_BOARD_SETUP_INITIAL_PAGE", page: "classes" });
      dispatch({ type: "SET_BOARD_SETUP_DIALOG_OPEN", open: true });
      return;
    }
    choose([id]);
    // `PCB_ACTIONS::properties` once the selection has landed
    window.setTimeout(() => runRef.current("pcbnew.InteractiveEdit.properties"), 30);
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

  // `SEARCH_PANE_LISTVIEW::OnChar`: Ctrl+A, Ctrl+C, Up and Down (Shift keeps the rows selected so far).
  const onListKey = (e: ReactKeyboardEvent) => {
    const ids = rows.map((r) => r.id);
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
      // handled here, so it is not the board's Select All too (`OnChar`'s `handled`)
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
    { label: "Zoom to Selection", hint: "Toggle zooming to selections in the search pane", icon: check(s.selectionZoom === "zoom"), onSelect: () => setSearchPane({ settings: toggleSearchSetting(s, "zoom") }) },
    { label: "Pan to Selection", hint: "Toggle panning to selections in the search pane", icon: check(s.selectionZoom === "pan"), onSelect: () => setSearchPane({ settings: toggleSearchSetting(s, "pan") }) },
    { label: "", onSelect: () => {}, separator: true },
    { label: "Search Hidden Fields", icon: check(s.searchHiddenFields), onSelect: () => setSearchPane({ settings: toggleSearchSetting(s, "hidden") }) },
    { label: "Search Metadata", hint: "Search library links, descriptions and keywords", icon: check(s.searchMetadata), onSelect: () => setSearchPane({ settings: toggleSearchSetting(s, "metadata") }) },
  ];

  if (!board) return <div className="panel-empty">No board.</div>;
  const headerEls = () => [...(listRef.current?.querySelectorAll('th[role="columnheader"]') ?? [])];
  const tableWidth = widths.reduce((a, w) => a + w, 0);
  return (
    <div className="search-pane" data-testid="search-pane" style={{ display: "flex", flexDirection: "column", height: "100%", minHeight: 0, fontSize: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 4, padding: "2px 4px" }}>
        <input
          ref={input}
          type="search"
          placeholder="Search"
          value={pane.query}
          onChange={(e) => setSearchPane({ query: e.target.value })}
          style={{ flex: 1, minWidth: 60, padding: "3px 6px" }}
          data-testid="search-pane-query"
        />
        <button
          type="button"
          title="Search pane options"
          aria-label="Search pane options"
          data-testid="search-pane-menu"
          onClick={(e) => {
            const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
            setMenu({ x: r.left, y: r.bottom, entries: settingsMenu() });
          }}
        >
          ⚙
        </button>
      </div>
      <div className="dock-tabs" role="tablist" style={{ flexWrap: "wrap" }}>
        {SEARCH_TABS.map((t) => (
          <div
            key={t.id}
            role="tab"
            aria-selected={t.id === tab.id}
            className={`dock-tab${t.id === tab.id ? " active" : ""}`}
            data-testid={`search-pane-tab-${t.id}`}
            onClick={() => {
              setSearchPane({ tab: t.id });
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
        data-testid="search-pane-list"
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
                    data-testid={`search-pane-col-${i}`}
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
                        const cur = getSearchPane().columns[tab.id];
                        setSearchPane({ columns: { ...getSearchPane().columns, [tab.id]: { ...cur, order: moveSearchColumn(tab.columns.length, cur?.order, i, to) } } });
                      })
                    }
                    onClick={() => {
                      if (clickEndsDrag()) return;
                      setSearchPane({ sort: { ...pane.sort, [tab.id]: clickSearchColumn(sort, i) } });
                    }}
                  >
                    {c.name}
                    {sort.col === i ? (sort.ascending ? " ▲" : " ▼") : ""}
                    <span
                      className="list-col-resize"
                      role="separator"
                      aria-orientation="vertical"
                      aria-label={`Resize ${c.name}`}
                      data-testid={`search-pane-resize-${i}`}
                      onClick={(e) => e.stopPropagation()}
                      onDoubleClick={(e) => {
                        e.stopPropagation();
                        setColumnWidth(i, autosizeSearchColumn(tab.id, i, rows, textWidthIn(listRef.current)));
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
                  key={r.id || "(no net)"}
                  role="row"
                  aria-selected={isSel}
                  data-testid="search-pane-row"
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
