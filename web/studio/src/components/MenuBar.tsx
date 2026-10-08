// The menu bar. We're in a browser, so this is rendered in-window at the
// top, the way KiCad itself does on Linux/Windows (vs. a native menu bar
// on macOS) -- an explicit instruction for this app regardless of host OS.
// Fully data-driven from src/kicad/menus.json + actions.json; until
// tools/extract-menus.js has been run somewhere with git access to
// ~/ws/kicad-mirror, both are empty (see their `meta.note`) and this
// renders an empty bar with a visible explanation rather than invented
// menu contents.
import { useEffect, useRef, useState } from "react";
import menusData from "../kicad/menus.json";
import schMenusData from "../kicad/sch_menus.json";
import fpMenusData from "../kicad/fp_menus.json";
import symMenusData from "../kicad/sym_menus.json";
import actionsData from "../kicad/actions.json";
import { SCHEMATIC_EDITOR_MENU_EXTRAS, SYMBOL_EDITOR_MENU_EXTRAS, withMenuExtras } from "../kicad/menuExtras";
import type { MenusFile, MenuNode, ActionsFile, KicadAction } from "../kicad/types";
import { displayHotkey, effectiveHotkey } from "../actions/hotkeys";
import { useActionRunner } from "../actions/useActionRunner";
import { useChecked } from "../actions/useChecked";
import { useStudioState } from "../state/store";

const menusFile = menusData as MenusFile;
/** The schematic editor's menus with this studio's own entries appended (kicad/menuExtras.ts). */
const schMenusFile = withMenuExtras(schMenusData as MenusFile, SCHEMATIC_EDITOR_MENU_EXTRAS);
const fpMenusFile = fpMenusData as MenusFile;
/** The Symbol Editor's menus with this studio's own entries appended (kicad/menuExtras.ts) -- the generated JSON stays KiCad's. */
const symMenusFile: MenusFile = {
  ...(symMenusData as MenusFile),
  menus: (symMenusData as MenusFile).menus.map((m) => (SYMBOL_EDITOR_MENU_EXTRAS[m.label] ? { ...m, items: [...m.items, ...SYMBOL_EDITOR_MENU_EXTRAS[m.label]!] } : m)),
};
const actionsFile = actionsData as ActionsFile;
const actionsByName = new Map<string, KicadAction>(actionsFile.actions.map((a) => [a.name, a]));

export function MenuNodeView({ node }: { node: MenuNode }) {
  const { run, isEnabled } = useActionRunner();
  const isChecked = useChecked();
  if (node.type === "separator") return <div className="menu-separator" role="separator" />;
  if (node.type === "submenu") {
    return (
      <div className="menu-node-submenu">
        <div className="menu-node-label">
          <span>{node.label}</span>
          <span>{"▸"}</span>
        </div>
        <div className="menubar-dropdown">
          {node.items.map((child, i) => (
            <MenuNodeView key={i} node={child} />
          ))}
        </div>
      </div>
    );
  }
  const action = actionsByName.get(node.action);
  const enabled = isEnabled(node.action);
  const label = node.label ?? action?.label ?? node.action;
  const tooltip = enabled ? action?.tooltip : `${action?.tooltip ?? ""} (not ported yet)`.trim();
  const hotkey = action ? effectiveHotkey(action).hotkey : null;
  // A toggle (View > Show Hidden Pins, Edit > Attributes > Do not Populate, the panes under View > Panels, Units ...) shows its state as a check mark.
  const checked = enabled ? isChecked(node.action) : undefined;
  return (
    <div className="menu-node-item" role={checked === undefined ? "menuitem" : "menuitemcheckbox"} aria-checked={checked} aria-disabled={!enabled} title={tooltip} onClick={() => enabled && run(node.action)}>
      <span>
        {checked !== undefined && <span style={{ display: "inline-block", width: 14 }}>{checked ? "✓" : ""}</span>}
        {label}
      </span>
      {hotkey && <span className="menu-node-hotkey">{displayHotkey(hotkey)}</span>}
    </div>
  );
}

export function MenuBar() {
  const state = useStudioState();
  const [openIndex, setOpenIndex] = useState<number | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDocClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpenIndex(null);
    };
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, []);

  // The 3D tab has no real menubar_3d_viewer.cpp-style extraction (that
  // window's source wasn't available), so it keeps pcbnew's menu rather
  // than showing an empty/fake one -- most of it is disabled there
  // anyway, same as everywhere else this app hasn't ported an action.
  // The two library editors are frames of their own in KiCad, each with its own menu bar (menubar_footprint_editor.cpp, menubar_symbol_editor.cpp).
  const activeMenus = state.tab === "schematic" ? schMenusFile : state.tab === "footprint" ? fpMenusFile : state.tab === "symbol" ? symMenusFile : menusFile;

  if (activeMenus.menus.length === 0) {
    return (
      <div className="menubar" ref={ref}>
        <span style={{ color: "var(--chrome-text-dim)", fontStyle: "italic", padding: "0 8px", lineHeight: "28px" }}>
          KiCad menu data not extracted yet -- run tools/extract-menus.js (see src/kicad/menus.json)
        </span>
      </div>
    );
  }

  return (
    <div className="menubar" ref={ref}>
      {activeMenus.menus.map((menu, i) => (
        <div key={menu.label} className={`menubar-item${openIndex === i ? " open" : ""}`} onClick={() => setOpenIndex(openIndex === i ? null : i)} onMouseEnter={() => openIndex !== null && setOpenIndex(i)}>
          {menu.label}
          {openIndex === i && (
            <div className="menubar-dropdown" onClick={() => setOpenIndex(null)}>
              {menu.items.map((node, j) => (
                <MenuNodeView key={j} node={node} />
              ))}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
