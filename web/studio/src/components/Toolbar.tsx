// Renders one toolbar of one editor from its extracted KiCad configuration: src/kicad/toolbars.json (board editor), sch_toolbars.json (schematic editor),
// fp_toolbars.json (footprint editor), sym_toolbars.json (symbol editor) -- each with main/options/drawing (the board editor also auxiliary), in KiCad's
// order, with its separators, dropdown groups and combo controls. Data-driven, same as MenuBar. Every button shows the icon KiCad gives its action
// (tools/lib/toolbarIcons.test.js keeps that true); the 3D viewer's toolbar is components/viewer3d/Viewer3DToolbar.tsx, built on the same pieces.
import { useState } from "react";
import toolbarsData from "../kicad/toolbars.json";
import schToolbarsData from "../kicad/sch_toolbars.json";
import fpToolbarsData from "../kicad/fp_toolbars.json";
import symToolbarsData from "../kicad/sym_toolbars.json";
import editorSupportData from "../kicad/editor_toolbar_support.json";
import actionsData from "../kicad/actions.json";
import iconsData from "../kicad/icons.json";
import type { ToolbarsFile, ToolbarId, ActionsFile, IconsFile, ToolbarItem } from "../kicad/types";
import { useActionRunner } from "../actions/useActionRunner";
import { useChecked } from "../actions/useChecked";
import { effectiveHotkey, displayHotkey } from "../actions/hotkeys";
import { useColorScheme } from "../hooks/useColorScheme";
import { useStudioDispatch, useStudioState } from "../state/store";
import { formatLength } from "../state/units";
import { ContextMenu, type MenuEntry } from "./canvas/ContextMenu";
import { FootprintToolbarControl, GridSelect, SymbolToolbarControl } from "./toolbar/EditorToolbarControls";

// No source-verified "100% = this many px/mm" reference for KiCad's own
// zoom percentage readout either; this defines 100% as 1 screen px per
// 100 µm (0.1 mm) purely so the control has *a* consistent, sensible
// scale to move between presets on.
const ZOOM_PRESET_PERCENTS = [25, 50, 100, 200, 400, 800];
const scaleForZoomPercent = (pct: number) => (pct / 100) * 0.01;

export type ToolbarEditor = "pcb" | "schematic" | "footprint" | "symbol";

const FILES: Record<ToolbarEditor, ToolbarsFile> = {
  pcb: toolbarsData as ToolbarsFile,
  // eeschema has no auxiliary toolbar at all (toolbars_sch_editor.cpp's TOOLBAR_LOC::TOP_AUX case returns std::nullopt) -- sch_toolbars.json only ever
  // has main/options/drawing, so an "auxiliary" lookup on it falls through to the "no such toolbar" empty state below, the honest thing to show.
  schematic: schToolbarsData as ToolbarsFile,
  footprint: fpToolbarsData as ToolbarsFile,
  symbol: symToolbarsData as ToolbarsFile,
};
const actionsFile = actionsData as ActionsFile;
const iconsFile = iconsData as IconsFile;
const actionsByName = new Map(actionsFile.actions.map((a) => [a.name, a]));
const SUPPORT = editorSupportData as unknown as { footprint: Record<string, string>; symbol: Record<string, string> };

/** The editor a tab's toolbars belong to: the 3D tab keeps the board editor's. */
function editorOfTab(tab: string): ToolbarEditor {
  return tab === "schematic" ? "schematic" : tab === "footprint" ? "footprint" : tab === "symbol" ? "symbol" : "pcb";
}

/** The KiCad icon of a BITMAPS name, drawn at KiCad's default toolbar size; a blank square only for a name with no icon (which the icon test forbids on any toolbar). */
export function ActionIcon({ iconName, size = 24 }: { iconName: string | null; size?: number }) {
  const scheme = useColorScheme();
  const file = iconName ? iconsFile.icons[iconName] : null;
  if (!file) return <span className="icon-placeholder" aria-hidden />;
  // KiCad's default toolbar icon size (common/settings/common_settings.cpp:
  // "appearance.toolbar_icon_size", default 24, options 16/24/32).
  return <img src={`/icons/${scheme}/${file}`} width={size} height={size} alt="" draggable={false} />;
}

/** Why a button is dead, for its tooltip: the recorded reason on the library editors' toolbars, "not ported yet" elsewhere. */
function deadReason(editor: ToolbarEditor, name: string): string {
  const reason = editor === "footprint" ? SUPPORT.footprint[name] : editor === "symbol" ? SUPPORT.symbol[name] : undefined;
  return reason && reason !== "supported" ? reason : "not ported yet";
}

/**
 * The auxiliary toolbar's combo controls (extract-toolbars.js, from
 * toolbars_pcb_editor.cpp's TOP_AUX case). Grid/zoom/active-layer are
 * fully wired client-side; track width/via size are display-only --
 * there's no command to change them, only GET /api/state's board_rules
 * to read them from (see that field's comment in api/types.ts).
 */
function BoardToolbarControl({ item }: { item: Extract<ToolbarItem, { type: "control" }> }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();

  // The board editor's own grid list (state/gridSettings.ts), in KiCad's order, which is declaration order and not sorted by size (kicad-port/grid.ts says why);
  // "Edit Grids..." at the end of the box edits it.
  if (item.control === "gridSelect") return <GridSelect editor="pcb" gridUm={state.gridUm} onChange={(um) => dispatch({ type: "SET_GRID_UM", um })} />;

  if (item.control === "zoomSelect") {
    const currentPercent = Math.round((state.view.scale / 0.01) * 100);
    return (
      <div className="toolbar-control" title="Zoom">
        <select
          value={ZOOM_PRESET_PERCENTS.includes(currentPercent) ? currentPercent : ""}
          onChange={(e) => {
            const container = document.querySelector(".pcb-canvas-container");
            const rect = container?.getBoundingClientRect();
            const cx = rect ? rect.width / 2 : 0;
            const cy = rect ? rect.height / 2 : 0;
            const newScale = scaleForZoomPercent(Number(e.target.value));
            const worldCx = (cx - state.view.x) / state.view.scale;
            const worldCy = (cy - state.view.y) / state.view.scale;
            dispatch({ type: "SET_VIEW", view: { scale: newScale, x: cx - worldCx * newScale, y: cy - worldCy * newScale } });
          }}
        >
          <option value="" disabled>
            {currentPercent}%
          </option>
          {ZOOM_PRESET_PERCENTS.map((p) => (
            <option key={p} value={p}>
              {p}%
            </option>
          ))}
        </select>
      </div>
    );
  }

  if (item.control === "layerSelector") {
    const layers = state.board?.layers ?? [];
    return (
      <div className="toolbar-control" title="Active layer">
        <select value={state.activeLayer ?? ""} onChange={(e) => dispatch({ type: "SET_ACTIVE_LAYER", layer: e.target.value || null })}>
          <option value="">(none)</option>
          {layers.map((l) => (
            <option key={l} value={l}>
              {l}
            </option>
          ))}
        </select>
      </div>
    );
  }

  if (item.control === "trackWidth" || item.control === "viaDiameter") {
    const rules = state.board?.board_rules;
    const value = !rules ? null : item.control === "trackWidth" ? rules.track_width : rules.via_diameter;
    return (
      <div className="toolbar-control" title={`${item.label} (display-only -- no command to change it yet)`}>
        <span className="icon-placeholder" aria-hidden />
        <span>{value != null ? formatLength(value, state.units) : "–"}</span>
      </div>
    );
  }

  // overrideLocks and anything else this script didn't specifically
  // handle: shown, but inert -- this app has no per-item lock concept.
  return (
    <div className="toolbar-control" title={`${item.label ?? item.control} (not ported yet)`} style={{ opacity: 0.5 }}>
      <span className="icon-placeholder" aria-hidden />
      <span>{item.label ?? item.control}</span>
    </div>
  );
}

function ToolbarControl({ item, editor }: { item: Extract<ToolbarItem, { type: "control" }>; editor: ToolbarEditor }) {
  if (editor === "footprint") return <FootprintToolbarControl control={item.control} />;
  if (editor === "symbol") return <SymbolToolbarControl control={item.control} />;
  return <BoardToolbarControl item={item} />;
}

/** The members a group's choice is remembered for (per editor and group), the way KiCad's group button keeps showing the last tool picked from it. */
const groupChoice = new Map<string, string>();

/**
 * KiCad's group button: it shows the icon of the member picked last (the first, to begin with) and runs it on a click; the small corner arrow, or a
 * right click, opens the list of members, each with its icon, to pick another (`ACTION_TOOLBAR::onToolRightClick` / the group's palette).
 */
function GroupButton({ item, editor }: { item: Extract<ToolbarItem, { type: "group" }>; editor: ToolbarEditor }) {
  const { run, isEnabled } = useActionRunner();
  const checked = useChecked();
  const [, bump] = useState(0);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const key = `${editor}/${item.label}`;
  const usable = (name: string) => isEnabled(name) && (editor !== "footprint" && editor !== "symbol" ? true : SUPPORT[editor][name] === "supported");
  const remembered = groupChoice.get(key);
  const shown = (remembered && item.items.includes(remembered) && usable(remembered) ? remembered : item.items.find(usable)) ?? null;
  const shownAction = shown ? actionsByName.get(shown) : null;
  const label = shownAction?.label ?? item.label;
  const entries = (): MenuEntry[] =>
    item.items.map((name) => {
      const a = actionsByName.get(name);
      return {
        label: [a?.label ?? name, a ? displayHotkey(effectiveHotkey(a).hotkey ?? "") : ""].filter(Boolean).join("    "),
        icon: <ActionIcon iconName={a?.icon ?? null} size={16} />,
        disabled: !usable(name),
        checked: name === shown,
        onSelect: () => {
          groupChoice.set(key, name);
          bump((n) => n + 1);
          run(name);
        },
      };
    });
  const pressed = shown ? checked(shown) : undefined;
  return (
    <>
      <button
        className={`toolbar-button has-menu${pressed ? " active" : ""}`}
        aria-pressed={pressed}
        disabled={!shown}
        title={shown ? `${label} (${item.label}: right click or the corner arrow for the others)` : `${item.label} (${deadReason(editor, item.items[0] ?? "")})`}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          // The corner arrow opens the list instead of running the member.
          if (e.clientX > r.right - 10 && e.clientY > r.bottom - 10) setMenu({ x: r.left, y: r.bottom });
          else if (shown) run(shown);
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          const r = e.currentTarget.getBoundingClientRect();
          setMenu({ x: r.left, y: r.bottom });
        }}
      >
        <ActionIcon iconName={shownAction?.icon ?? item.icon} />
      </button>
      {menu && <ContextMenu x={menu.x} y={menu.y} entries={entries()} onClose={() => setMenu(null)} />}
    </>
  );
}

/**
 * The right-click menus of toolbar buttons (`TOOLBAR_ITEM_REF::WithContextMenu`, toolbars_pcb_editor.cpp and its siblings): the Show Grid button's offers Edit Grids...
 * and, in the editors that have a grid origin, Grid Origin.... (The other buttons with a context menu are the routing and drawing tools'.)
 */
const BUTTON_MENUS: Record<ToolbarEditor, Record<string, string[]>> = {
  pcb: { "common.Control.toggleGrid": ["common.Control.editGrids", "common.Control.editGridOrigin"] },
  schematic: { "common.Control.toggleGrid": ["common.Control.editGrids"] },
  footprint: { "common.Control.toggleGrid": ["common.Control.editGrids", "common.Control.editGridOrigin"] },
  symbol: { "common.Control.toggleGrid": ["common.Control.editGrids"] },
};

function ToolbarItemView({ item, editor }: { item: ToolbarItem; editor: ToolbarEditor }) {
  const { run, isEnabled } = useActionRunner();
  const checked = useChecked();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);

  if (item.type === "separator") return <div className="toolbar-separator" role="separator" />;

  if (item.type === "control") return <ToolbarControl item={item} editor={editor} />;

  if (item.type === "group") return <GroupButton item={item} editor={editor} />;

  const action = actionsByName.get(item.action);
  // The library editors' toolbars also check the recorded support table: an action the table calls unsupported is dead on their tab whatever else registered it.
  const enabled = isEnabled(item.action) && (editor !== "footprint" && editor !== "symbol" ? true : SUPPORT[editor][item.action] === "supported");
  const label = action?.label ?? item.action;
  const hotkey = action ? effectiveHotkey(action).hotkey : null;
  const tooltip = enabled ? [label, hotkey ? displayHotkey(hotkey) : null].filter(Boolean).join(" — ") : `${label} (${deadReason(editor, item.action)})`;
  const pressed = enabled ? checked(item.action) : undefined;
  const menuActions = BUTTON_MENUS[editor][item.action];
  return (
    <>
      <button
        className={`toolbar-button${pressed ? " active" : ""}`}
        aria-pressed={pressed}
        disabled={!enabled}
        title={tooltip}
        onClick={() => run(item.action)}
        onContextMenu={
          menuActions
            ? (e) => {
                e.preventDefault();
                const r = e.currentTarget.getBoundingClientRect();
                setMenu({ x: r.left, y: r.bottom });
              }
            : undefined
        }
      >
        <ActionIcon iconName={action?.icon ?? null} />
      </button>
      {menu && menuActions && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          entries={menuActions.map((name) => ({ label: actionsByName.get(name)?.label ?? name, disabled: !isEnabled(name), onSelect: () => run(name) }))}
          onClose={() => setMenu(null)}
        />
      )}
    </>
  );
}

export function Toolbar({ id, editor: editorProp }: { id: ToolbarId; editor?: ToolbarEditor }) {
  const state = useStudioState();
  const editor = editorProp ?? editorOfTab(state.tab);
  const config = FILES[editor].toolbars.find((t) => t.id === id);
  const orientation = config?.orientation ?? (id === "options" || id === "drawing" ? "vertical" : "horizontal");

  if (!config || config.items.length === 0) {
    // The editors that have no auxiliary toolbar in KiCad (eeschema, the footprint and symbol editors) render nothing for it, not an "extraction
    // missing" notice that would be wrong for them.
    if (editor !== "pcb" && id === "auxiliary") return null;
    return (
      <div className={`toolbar${orientation === "vertical" ? " vertical" : ""}`} data-toolbar={id}>
        {id === "main" && <span style={{ color: "var(--chrome-text-dim)", fontStyle: "italic", padding: "0 6px" }}>KiCad toolbar data not extracted yet (tools/extract-toolbars.js)</span>}
      </div>
    );
  }

  return (
    <div className={`toolbar${orientation === "vertical" ? " vertical" : ""}`} data-toolbar={id} data-editor={editor}>
      {config.items.map((item, i) => (
        <ToolbarItemView key={i} item={item} editor={editor} />
      ))}
    </div>
  );
}
