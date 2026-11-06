// The Net Classes list (`APPEARANCE_CONTROLS::rebuildNets`, the `appendNetclass` rows): the Default class first -- without a colour of its own ("Default netclass
// can't have an override color") -- then the others by name, each with a colour, an eye that shows or hides the ratsnest of every net the class owns, and its name.
// The right-click menu sets, copies from the schematic (Use Color from Schematic: the class's colour of Schematic Setup > Net Classes) and clears the class
// colour, highlights its nets, selects or unselects their tracks and vias, and shows all classes or hides all the others.
import { useState } from "react";
import { useAppearanceView } from "./useAppearanceView";
import { useNetActions } from "./netActions";
import { ColorDialog, Eye, Swatch } from "./shared";
import { NetDisplayOptions } from "./NetDisplayOptions";
import { ContextMenu, type MenuEntry } from "../../canvas/ContextMenu";
import { classRows, netclassMenu, netsOfClass } from "../../../kicad-port/appearanceNets";
import { specifiedColor } from "../../../kicad-port/appearance";

export function NetClassesTab() {
  const { state, dispatch, nets, op } = useAppearanceView();
  const { highlight, select } = useNetActions();
  const [menu, setMenu] = useState<{ x: number; y: number; name: string; isDefault: boolean } | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const rows = classRows(nets, state.appearance.netclassColors, state.appearance.hiddenNetclasses);
  // `nc->GetSchematicColor()`: the class's wire colour of Schematic Setup > Net Classes (the project's one `NET_SETTINGS`); none, or `COLOR4D::UNSPECIFIED`, dims the entry
  const schematicColor = (name: string): string | null => {
    const css = state.board?.board_rules?.net_class_schematic?.[name]?.color ?? null;
    return specifiedColor(css) ? css : null;
  };
  const entries = (name: string, isDefault: boolean): MenuEntry[] =>
    netclassMenu(name, isDefault, schematicColor(name)).map((m) => ({
      label: m.label,
      separator: m.action === "separator",
      disabled: m.disabled,
      onSelect: () => {
        const mine = netsOfClass(nets, name);
        switch (m.action) {
          case "set_color":
            setEditing(name);
            break;
          case "clear_color":
            op({ op: "netclass_color", name, color: null });
            break;
          case "use_schematic_color": {
            // ID_USE_SCHEMATIC_NET_COLOR: `SetPcbColor( nc->GetSchematicColor() )`
            const css = schematicColor(name);
            if (css) op({ op: "netclass_color", name, color: css });
            break;
          }
          case "highlight":
            highlight(mine);
            break;
          case "select":
            select(mine, true);
            break;
          case "deselect":
            select(mine, false);
            break;
          case "show_all":
            op({ op: "show_all_netclasses" });
            break;
          case "hide_others":
            op({ op: "hide_other_netclasses", name });
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
        <span style={{ flex: 1 }} className="ap-title">
          Net Classes
        </span>
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
      {rows.map((r) => (
        <div
          key={r.name}
          className="ap-row"
          data-netclass={r.name}
          onContextMenu={(e) => {
            e.preventDefault();
            setMenu({ x: e.clientX, y: e.clientY, name: r.name, isDefault: r.isDefault });
          }}
        >
          <Swatch
            css={r.color}
            hidden={r.isDefault}
            title="Left double click or middle click for color change, right click for menu"
            onEdit={r.isDefault ? undefined : () => setEditing(r.name)}
            onMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setMenu({ x: e.clientX, y: e.clientY, name: r.name, isDefault: r.isDefault });
            }}
          />
          <Eye on={r.visible} title={`Show or hide ratsnest for nets in ${r.name}`} onToggle={() => op({ op: "netclass_visible", name: r.name, visible: !r.visible })} />
          <span className="ap-name">{r.name}</span>
          <span className="ap-count" title="Nets in this class">
            {r.nets}
          </span>
        </div>
      ))}
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={entries(menu.name, menu.isDefault)} onClose={() => setMenu(null)} />}
      {editing !== null && (
        <ColorDialog
          title={`Net Class Color: ${editing}`}
          initial={state.appearance.netclassColors[editing] ?? null}
          onCancel={() => setEditing(null)}
          onPick={(css) => {
            op({ op: "netclass_color", name: editing, color: css });
            setEditing(null);
          }}
        />
      )}
    </div>
  );
}
