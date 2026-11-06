// Renders one toolbar of one editor from its extracted KiCad configuration: src/kicad/toolbars.json (board editor), sch_toolbars.json (schematic editor),
// fp_toolbars.json (footprint editor), sym_toolbars.json (symbol editor) -- each with main/options/drawing (the board editor also auxiliary), in KiCad's
// order, with its separators, dropdown groups and combo controls. Data-driven, same as MenuBar. Every button shows the icon KiCad gives its action
// (tools/lib/toolbarIcons.test.js keeps that true); the 3D viewer's toolbar is components/viewer3d/Viewer3DToolbar.tsx, built on the same pieces.
// With the editor's "Customize toolbars" on (Preferences > <editor> > Toolbars) a toolbar the person has customised is drawn as they left it, spacers too
// (state/toolbarSettings.ts `resolvedToolbar`, `TOOLBAR_SETTINGS::GetToolbarConfig`).
import { useState } from "react";
import editorSupportData from "../kicad/editor_toolbar_support.json";
import actionsData from "../kicad/actions.json";
import iconsData from "../kicad/icons.json";
import type { ToolbarId, ActionsFile, IconsFile, ToolbarItem } from "../kicad/types";
import { useActionRunner } from "../actions/useActionRunner";
import { useChecked } from "../actions/useChecked";
import { effectiveHotkey, displayHotkey } from "../actions/hotkeys";
import { useIconScheme } from "../hooks/useColorScheme";
import { useStudioDispatch, useStudioState } from "../state/store";
import { BOX_SEPARATOR, sizeBoxPick, trackWidthBoxLabels, viaSizeBoxLabels } from "../kicad-port/routerSizes";
import { ContextMenu, type MenuEntry } from "./canvas/ContextMenu";
import { FootprintToolbarControl, GridSelect, SymbolToolbarControl } from "./toolbar/EditorToolbarControls";
import { PcbVariantSelector, SchVariantSelector } from "./toolbar/VariantSelector";
import { KICAD10_ACTIONS, KICAD10_ICONS } from "../kicad/kicad10Actions";
import { resolvedToolbar, useToolbarSettings } from "../state/toolbarSettings";
import type { RenderToolbarItem } from "../kicad-port/toolbarCustomization";

// No source-verified "100% = this many px/mm" reference for KiCad's own
// zoom percentage readout either; this defines 100% as 1 screen px per
// 100 µm (0.1 mm) purely so the control has *a* consistent, sensible
// scale to move between presets on.
const ZOOM_PRESET_PERCENTS = [25, 50, 100, 200, 400, 800];
const scaleForZoomPercent = (pct: number) => (pct / 100) * 1e-5;

export type ToolbarEditor = "pcb" | "schematic" | "footprint" | "symbol";

const actionsFile = actionsData as ActionsFile;
const iconsFile = iconsData as IconsFile;
const actionsByName = new Map(actionsFile.actions.map((a) => [a.name, a]));
// KiCad 10's actions the extracted table does not have yet (kicad/kicad10Actions.ts)
for (const a of KICAD10_ACTIONS) if (!actionsByName.has(a.name)) actionsByName.set(a.name, { ...(actionsFile.actions[0] as ActionsFile["actions"][number]), name: a.name, label: a.label, tooltip: a.tooltip, icon: a.icon, hotkey: null, altHotkey: null, hotkeyRaw: null, altHotkeyRaw: null, macHotkey: null, macAltHotkey: null, macHotkeyRaw: null, macAltHotkeyRaw: null, flags: [] });

const SUPPORT = editorSupportData as unknown as { footprint: Record<string, string>; symbol: Record<string, string> };

/** The editor a tab's toolbars belong to: the 3D tab keeps the board editor's. */
function editorOfTab(tab: string): ToolbarEditor {
  return tab === "schematic" ? "schematic" : tab === "footprint" ? "footprint" : tab === "symbol" ? "symbol" : "pcb";
}

/** The KiCad icon of a BITMAPS name, drawn at KiCad's default toolbar size; a blank square only for a name with no icon (which the icon test forbids on any toolbar). */
export function ActionIcon({ iconName, size = 24 }: { iconName: string | null; size?: number }) {
  // Preferences > Common's "Icon theme:" (light, dark or the system's: `COMMON_SETTINGS::m_Appearance.icon_theme`)
  const scheme = useIconScheme();
  const file = iconName ? (iconsFile.icons[iconName] ?? KICAD10_ICONS[iconName]) : null;
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
  if (item.control === "gridSelect") return <GridSelect editor="pcb" gridNm={state.gridNm} onChange={(um) => dispatch({ type: "SET_GRID_NM", um })} />;

  if (item.control === "zoomSelect") {
    const currentPercent = Math.round((state.view.scale / 1e-5) * 100);
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

  if (item.control === "trackWidth" || item.control === "viaDiameter") return <SizeSelectBox control={item.control} label={item.label ?? item.control} />;

  // overrideLocks and anything else this script didn't specifically
  // handle: shown, but inert -- this app has no per-item lock concept.
  return (
    <div className="toolbar-control" title={`${item.label ?? item.control} (not ported yet)`} style={{ opacity: 0.5 }}>
      <span className="icon-placeholder" aria-hidden />
      <span>{item.label ?? item.control}</span>
    </div>
  );
}

/**
 * The auxiliary toolbar's Track Width and Via Size boxes (`m_SelTrackWidthBox`, `m_SelViaSizeBox`, UI-LEFT R7): KiCad's entries (kicad-port/routerSizes.ts
 * `trackWidthBoxLabels`, `viaSizeBoxLabels`), the chosen list index selected (`SetSelection( GetTrackWidthIndex() )`), and a pick as
 * `Tracks_and_Vias_Size_Event`'s (`sizeBoxPick`): the router width menu's own actions (`studio.Router.setTrackWidth` / `setViaSize`: the index, custom sizes
 * off, the override, a route in progress re-sized), or Board Setup on "Pre-defined Sizes".
 */
function SizeSelectBox({ control, label }: { control: "trackWidth" | "viaDiameter"; label: string }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const { run } = useActionRunner();
  const routing = state.board?.routing;
  const unit = state.units === "mil" ? "mil" : state.units === "in" ? "in" : "mm";
  const labels = control === "trackWidth" ? trackWidthBoxLabels(routing?.track_width_presets ?? [], unit) : viaSizeBoxLabels(routing?.via_presets ?? [], unit);
  const index = Math.max(0, Math.min((control === "trackWidth" ? state.currentTrackWidthIndex : state.currentViaIndex) ?? 0, labels.length - 3));
  return (
    <div className="toolbar-control" title={label}>
      <select
        aria-label={label}
        data-control={control}
        value={index}
        disabled={!state.board}
        onChange={(e) => {
          const pick = sizeBoxPick(Number(e.target.value), labels.length);
          if (pick.kind === "edit") {
            dispatch({ type: "SET_BOARD_SETUP_INITIAL_PAGE", page: "tracks_vias" });
            dispatch({ type: "SET_BOARD_SETUP_DIALOG_OPEN", open: true });
          } else if (pick.kind === "index") {
            run(control === "trackWidth" ? "studio.Router.setTrackWidth" : "studio.Router.setViaSize", pick.index);
          }
        }}
      >
        {labels.map((text, i) => (
          <option key={i} value={i} disabled={text === BOX_SEPARATOR}>
            {text}
          </option>
        ))}
      </select>
    </div>
  );
}

function ToolbarControl({ item, editor }: { item: Extract<ToolbarItem, { type: "control" }>; editor: ToolbarEditor }) {
  if (editor === "footprint") return <FootprintToolbarControl control={item.control} />;
  if (editor === "symbol") return <SymbolToolbarControl control={item.control} />;
  // the design variant choice (KiCad 25ec2335 `SCH_ACTION_TOOLBAR_CONTROLS::currentVariant`, `PCB_ACTION_TOOLBAR_CONTROLS::currentVariant`)
  if (item.control === "currentVariant") return editor === "schematic" ? <SchVariantSelector /> : <PcbVariantSelector />;
  if (editor === "schematic") return <SchematicToolbarControl item={item} />;
  return <BoardToolbarControl item={item} />;
}

/**
 * The schematic editor's boxes a customised toolbar can hold (`ACTION_TOOLBAR_CONTROLS::gridSelect`, `zoomSelect`, ... are `SupportedFor( FRAME_SCH )`): the grid
 * box is the schematic's own grid list; the others are shown inert, as the board's unported ones are.
 */
function SchematicToolbarControl({ item }: { item: Extract<ToolbarItem, { type: "control" }> }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  if (item.control === "gridSelect") return <GridSelect editor="schematic" gridNm={state.schGridNm} onChange={(um) => dispatch({ type: "SET_SCH_GRID_NM", um })} />;
  return (
    <div className="toolbar-control" title={`${item.label ?? item.control} (not ported yet)`} style={{ opacity: 0.5 }}>
      <span className="icon-placeholder" aria-hidden />
      <span>{item.label ?? item.control}</span>
    </div>
  );
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
        data-group={item.label}
        data-action={shown ?? undefined}
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

function ToolbarItemView({ item, editor, vertical }: { item: RenderToolbarItem; editor: ToolbarEditor; vertical: boolean }) {
  const { run, isEnabled } = useActionRunner();
  const checked = useChecked();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);

  if (item.type === "separator") return <div className="toolbar-separator" role="separator" />;

  // `ACTION_TOOLBAR::AddSpacer( size )`: blank pixels along the toolbar
  if (item.type === "spacer") return <div className="toolbar-spacer" aria-hidden data-spacer={item.size} style={vertical ? { height: item.size, flexShrink: 0 } : { width: item.size, flexShrink: 0 }} />;

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
        data-action={item.action}
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
  useToolbarSettings();
  const editor = editorProp ?? editorOfTab(state.tab);
  const config = resolvedToolbar(editor, id);
  const orientation = config?.orientation ?? (id === "options" || id === "drawing" ? "vertical" : "horizontal");

  // (a customised toolbar the person emptied is drawn empty, not as missing data)
  if (!config || (config.items.length === 0 && !config.custom)) {
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
        <ToolbarItemView key={i} item={item} editor={editor} vertical={orientation === "vertical"} />
      ))}
    </div>
  );
}
