// The Nets tab (`NET_GRID_TABLE`, `APPEARANCE_CONTROLS::rebuildNets`): every net of the board, sorted by name, with its colour, an eye that shows or hides its
// ratsnest ("Click to hide ratsnest for ..."), and its name. A double or middle click on the colour changes it; the right-click menu sets and clears the colour,
// highlights the net, selects or unselects its tracks and vias, and shows all nets or hides all the others. Net Display Options sits above the list.
import { useState } from "react";
import { useAppearanceView } from "./useAppearanceView";
import { useNetActions } from "./netActions";
import { ColorDialog, Eye, Swatch } from "./shared";
import { NetDisplayOptions } from "./NetDisplayOptions";
import { ContextMenu, type MenuEntry } from "../../canvas/ContextMenu";
import { isHighlighted } from "../../../kicad-port/boardControl";
import { highlightedNets } from "../../../kicad-port/boardControl";
import { netEyeTip, netMenu, netRows } from "../../../kicad-port/appearanceNets";

export function NetsTab() {
  const { state, dispatch, nets, op } = useAppearanceView();
  const { highlight, select } = useNetActions();
  const [filter, setFilter] = useState("");
  const [menu, setMenu] = useState<{ x: number; y: number; net: string } | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const rows = netRows(nets, state.bcx.hiddenRatsnestNets, state.appearance.netColors, filter);
  const lit = state.bcx.netHighlightMore.length > 0 ? highlightedNets(state.netHighlight, state.bcx.netHighlightMore) : state.netHighlight;
  const entries = (net: string): MenuEntry[] =>
    netMenu(net).map((m) => ({
      label: m.label,
      separator: m.action === "separator",
      disabled: m.disabled,
      onSelect: () => {
        switch (m.action) {
          case "set_color":
            setEditing(net);
            break;
          case "clear_color":
            op({ op: "net_color", net, color: null });
            break;
          case "highlight":
            highlight([net]);
            break;
          case "select":
            select([net], true);
            break;
          case "deselect":
            select([net], false);
            break;
          case "show_all":
            op({ op: "show_all_nets" });
            break;
          case "hide_others":
            op({ op: "hide_other_nets", net });
            break;
          default:
            break;
        }
      },
    }));
  return (
    <div className="ap-scroll">
      <NetDisplayOptions />
      <div className="ap-buttons">
        <input type="search" placeholder="Filter nets" aria-label="Filter nets" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <button type="button" className="ap-icon-btn" title="Net Inspector" aria-label="Net Inspector" onClick={() => dispatch({ type: "SET_NET_INSPECTOR_OPEN", open: true })}>
          ≡
        </button>
        <button
          type="button"
          className="ap-icon-btn"
          title="Configure Net Classes"
          aria-label="Configure Net Classes"
          onClick={() => {
            dispatch({ type: "SET_BOARD_SETUP_INITIAL_PAGE", page: "classes" });
            dispatch({ type: "SET_BOARD_SETUP_DIALOG_OPEN", open: true });
          }}
        >
          ⚙
        </button>
      </div>
      <div className="ap-title">Nets</div>
      {rows.length === 0 && <div className="ap-note">{nets.nets.length === 0 ? "No nets." : "No net matches."}</div>}
      {rows.map((r) => (
        <div
          key={r.name}
          className={`ap-row${lit && isHighlighted(r.name, lit) ? " hot" : ""}`}
          data-net={r.name}
          onClick={() => highlight(state.netHighlight === r.name && state.bcx.netHighlightMore.length === 0 ? [] : [r.name])}
          onContextMenu={(e) => {
            e.preventDefault();
            setMenu({ x: e.clientX, y: e.clientY, net: r.name });
          }}
        >
          <Swatch css={r.color} title="Double click (or middle click) to change color; right click for more actions" onEdit={() => setEditing(r.name)} onMenu={(e) => { e.preventDefault(); e.stopPropagation(); setMenu({ x: e.clientX, y: e.clientY, net: r.name }); }} />
          <Eye on={r.visible} title={netEyeTip(r.name, r.visible)} onToggle={() => op({ op: "net_visible", net: r.name, visible: !r.visible })} />
          <span className="ap-name">{r.name}</span>
        </div>
      ))}
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={entries(menu.net)} onClose={() => setMenu(null)} />}
      {editing !== null && (
        <ColorDialog
          title={`Net Color: ${editing}`}
          initial={state.appearance.netColors[editing] ?? null}
          onCancel={() => setEditing(null)}
          onPick={(css) => {
            op({ op: "net_color", net: editing, color: css });
            setEditing(null);
          }}
        />
      )}
    </div>
  );
}
