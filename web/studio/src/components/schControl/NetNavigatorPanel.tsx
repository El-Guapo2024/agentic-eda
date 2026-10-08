// The Net Navigator (`eeschema.EditorControl.showNetNavigator`, `SCH_EDIT_FRAME::RefreshNetNavigator`, eeschema/net_navigator.cpp): a tree of the sheet's
// nets, each with the items on it (symbol pins, labels, power symbols, no-connects -- `kicad-port/netNavigator.ts`), a filter box with wildcards
// (`net_nav_search_mode_wildcard`), and the same behaviour as KiCad's: with a net highlighted the tree shows that net alone and the filter is off;
// clicking a net highlights it, clicking an item selects it on the sheet. Docked over the right edge of the sheet.
import { useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useStudioDispatch, useStudioState } from "../../state/store";
import { useSchControlDispatch, useSchControlState } from "../../state/schControlStore";
import { netNavigatorItems, type NavSchematic } from "../../kicad-port/netNavigator";
import { resolveLibSymbol } from "../schematic/libSymbol";
import { wildcardAnchoredMatch } from "../../kicad-port/footprintFilter";
import type { Schematic } from "../../api/types";

/** The sheet as the net item list reads it. */
export function navSchematic(s: Schematic): NavSchematic {
  return {
    pins: s.symbols.flatMap((sym) => (resolveLibSymbol(sym, s.lib_symbols)?.pins ?? []).map((p) => ({ ref: sym.id, number: p.pin.number, name: p.pin.name, tip: p.tip }))),
    wires: s.wires,
    labels: s.labels,
    powerSymbols: s.power_symbols,
    noConnects: s.no_connects,
  };
}

/** Every named net of the sheet, in name order (`NET_MAP` skips the nameless ones). */
export function sheetNets(s: Schematic): string[] {
  const names = new Set<string>();
  for (const w of s.wires) if (w.net) names.add(w.net);
  for (const l of s.labels) if (l.net) names.add(l.net);
  for (const p of s.power_symbols) if (p.net) names.add(p.net);
  return [...names].sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
}

/** `RefreshNetNavigator`'s filter: a typed text with no wildcard matches anywhere in the name (`*text*`), case-insensitively. */
export function netMatchesFilter(name: string, filter: string): boolean {
  if (filter === "") return true;
  const glob = /[*?]/.test(filter) ? filter : `*${filter}*`;
  return wildcardAnchoredMatch(glob.toLowerCase(), name.toLowerCase());
}

export function NetNavigatorPanel() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const control = useSchControlState();
  const controlDispatch = useSchControlDispatch();
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const sch = state.schematic;
  const highlighted = state.netHighlight;

  const nets = useMemo(() => (sch ? sheetNets(sch) : []), [sch]);
  const nav = useMemo(() => (sch ? navSchematic(sch) : null), [sch]);
  const shown = highlighted ? nets.filter((n) => n === highlighted) : nets.filter((n) => netMatchesFilter(n, control.netFilter));

  const host = typeof document !== "undefined" ? document.querySelector(".app-body .canvas-col") : null;
  if (!control.netNavigatorOpen || state.tab !== "schematic" || !host) return null;

  return createPortal(
    <div id="net-navigator-panel" style={{ position: "absolute", top: 0, right: 0, bottom: 0, width: 300, zIndex: 20, display: "flex", flexDirection: "column", background: "var(--chrome-bg-raised)", borderLeft: "1px solid var(--chrome-border)", boxShadow: "-4px 0 12px var(--chrome-shadow)", fontSize: 12 }}>
      <div style={{ display: "flex", alignItems: "center", padding: "6px 8px", borderBottom: "1px solid var(--chrome-border)" }}>
        <b style={{ flex: 1 }}>Net Navigator</b>
        <button title="Close" onClick={() => controlDispatch({ type: "SET_NET_NAVIGATOR", open: false })}>
          ×
        </button>
      </div>
      <div style={{ padding: "6px 8px" }}>
        <input
          style={{ width: "100%", boxSizing: "border-box" }}
          placeholder={highlighted ? "A net is highlighted: clear it to filter" : "Filter nets (* and ? are wildcards)"}
          value={highlighted ? "" : control.netFilter}
          disabled={!!highlighted}
          onChange={(e) => controlDispatch({ type: "SET_NET_FILTER", text: e.target.value })}
        />
      </div>
      <div style={{ flex: 1, minHeight: 0, overflow: "auto", padding: "0 4px 8px" }}>
        {shown.length === 0 && <div className="panel-empty">{nets.length === 0 ? "There are no nets on this sheet." : "No net matches the filter."}</div>}
        {shown.map((net) => {
          const expanded = highlighted === net || !!open[net];
          const items = expanded && nav ? netNavigatorItems(nav, net, state.units) : [];
          return (
            <div key={net}>
              <div style={{ display: "flex", alignItems: "center", gap: 4, padding: "1px 4px", cursor: "default", background: highlighted === net ? "var(--chrome-selected-bg)" : undefined, color: highlighted === net ? "var(--chrome-selected-text)" : undefined }}>
                <span style={{ width: 12, textAlign: "center" }} onClick={() => setOpen({ ...open, [net]: !expanded })}>
                  {expanded ? "▾" : "▸"}
                </span>
                <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={net} onClick={() => dispatch({ type: "SET_NET_HIGHLIGHT", net: highlighted === net ? null : net })}>
                  {net}
                </span>
              </div>
              {items.map((it) => (
                <div
                  key={it.key}
                  title={it.text}
                  style={{ padding: "1px 4px 1px 22px", cursor: "default", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", background: state.selection.has(it.owner) ? "var(--chrome-selected-bg)" : undefined, color: state.selection.has(it.owner) ? "var(--chrome-selected-text)" : undefined }}
                  onClick={() => dispatch({ type: "SET_SELECTION", refs: [it.owner] })}
                >
                  {it.text}
                </div>
              ))}
            </div>
          );
        })}
      </div>
    </div>,
    host
  );
}
