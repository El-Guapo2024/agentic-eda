// Renders one toolbar from src/kicad/toolbars.json (main/options/drawing/
// auxiliary -- see App.tsx for where each is docked). Data-driven, same
// as MenuBar: empty until tools/extract-toolbars.js has real output.
import toolbarsData from "../kicad/toolbars.json";
import schToolbarsData from "../kicad/sch_toolbars.json";
import actionsData from "../kicad/actions.json";
import iconsData from "../kicad/icons.json";
import type { ToolbarsFile, ToolbarId, ActionsFile, IconsFile, ToolbarItem } from "../kicad/types";
import { useActionRunner } from "../actions/useActionRunner";
import { effectiveHotkey } from "../actions/hotkeys";
import { useColorScheme } from "../hooks/useColorScheme";
import { useStudioDispatch, useStudioState } from "../state/store";
import { formatLength } from "../state/units";
import { DEFAULT_PCB_GRIDS_UM } from "../kicad-port/grid";

/**
 * KiCad's real default PCB grid list (app_settings.cpp
 * APP_SETTINGS_BASE::DefaultGridSizeList) -- see kicad-port/grid.ts's own
 * header comment for why this is declaration order, not sorted by size
 * (a deliberate jump from 1 mil to 5.0 mm partway through). The dropdown
 * below renders it in exactly that order, same as KiCad's own grid
 * dropdown and N/Shift+N cycling (common.Control.gridNext/gridPrev,
 * useActionRunner.ts).
 */
export const GRID_OPTIONS_UM = DEFAULT_PCB_GRIDS_UM;

// No source-verified "100% = this many px/mm" reference for KiCad's own
// zoom percentage readout either; this defines 100% as 1 screen px per
// 100 µm (0.1 mm) purely so the control has *a* consistent, sensible
// scale to move between presets on.
const ZOOM_PRESET_PERCENTS = [25, 50, 100, 200, 400, 800];
const scaleForZoomPercent = (pct: number) => (pct / 100) * 0.01;

const toolbarsFile = toolbarsData as ToolbarsFile;
// eeschema has no auxiliary toolbar at all (toolbars_sch_editor.cpp's
// TOOLBAR_LOC::TOP_AUX case returns std::nullopt) -- sch_toolbars.json
// only ever has main/options/drawing, so an "auxiliary" lookup on it
// naturally falls through to Toolbar's own "not extracted" empty state
// below, which is the honest thing to show for a toolbar that simply
// doesn't exist in the editor being viewed.
const schToolbarsFile = schToolbarsData as ToolbarsFile;
const actionsFile = actionsData as ActionsFile;
const iconsFile = iconsData as IconsFile;
const actionsByName = new Map(actionsFile.actions.map((a) => [a.name, a]));

function ActionIcon({ iconName }: { iconName: string | null }) {
  const scheme = useColorScheme();
  const file = iconName ? iconsFile.icons[iconName] : null;
  if (!file) return <span className="icon-placeholder" aria-hidden />;
  // KiCad's default toolbar icon size (common/settings/common_settings.cpp:
  // "appearance.toolbar_icon_size", default 24, options 16/24/32).
  return <img src={`/icons/${scheme}/${file}`} width={24} height={24} alt="" draggable={false} />;
}

/**
 * The auxiliary toolbar's combo controls (extract-toolbars.js, from
 * toolbars_pcb_editor.cpp's TOP_AUX case). Grid/zoom/active-layer are
 * fully wired client-side; track width/via size are display-only --
 * there's no command to change them, only GET /api/state's board_rules
 * to read them from (see that field's comment in api/types.ts).
 */
function ToolbarControl({ item }: { item: Extract<ToolbarItem, { type: "control" }> }) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();

  if (item.control === "gridSelect") {
    return (
      <div className="toolbar-control" title="Grid">
        <select value={state.gridUm} onChange={(e) => dispatch({ type: "SET_GRID_UM", um: Number(e.target.value) })}>
          {GRID_OPTIONS_UM.map((um) => (
            <option key={um} value={um}>
              {formatLength(um, state.units)}
            </option>
          ))}
        </select>
      </div>
    );
  }

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

function ToolbarItemView({ item }: { item: ToolbarItem }) {
  const { run, isEnabled } = useActionRunner();

  if (item.type === "separator") return <div className="toolbar-separator" role="separator" />;

  if (item.type === "control") return <ToolbarControl item={item} />;

  if (item.type === "group") {
    // Real KiCad renders this as a split button: the main icon runs the
    // group's current/primary action, a small dropdown arrow picks a
    // different member. This app has no per-group "current selection"
    // state and no dropdown menu widget, so the main click runs the
    // first enabled member instead -- a real, working default (this
    // button used to have no onClick at all, so clicking it did
    // nothing) rather than a full split-button UI.
    const firstEnabled = item.items.find((a) => isEnabled(a));
    const label = firstEnabled ? (actionsByName.get(firstEnabled)?.label ?? firstEnabled) : item.label;
    return (
      <button
        className="toolbar-button"
        disabled={!firstEnabled}
        title={firstEnabled ? label : `${item.label} (not ported yet)`}
        onClick={() => firstEnabled && run(firstEnabled)}
      >
        <ActionIcon iconName={item.icon} />
      </button>
    );
  }

  const action = actionsByName.get(item.action);
  const enabled = isEnabled(item.action);
  const label = action?.label ?? item.action;
  const hotkey = action ? effectiveHotkey(action).hotkey : null;
  const tooltip = enabled ? [label, hotkey].filter(Boolean).join(" — ") : `${label} (not ported yet)`;
  return (
    <button className="toolbar-button" disabled={!enabled} title={tooltip} onClick={() => run(item.action)}>
      <ActionIcon iconName={action?.icon ?? null} />
    </button>
  );
}

export function Toolbar({ id }: { id: ToolbarId }) {
  const state = useStudioState();
  const schematic = state.tab === "schematic";
  const config = (schematic ? schToolbarsFile : toolbarsFile).toolbars.find((t) => t.id === id);
  const orientation = config?.orientation ?? (id === "options" || id === "drawing" ? "vertical" : "horizontal");

  if (!config || config.items.length === 0) {
    // eeschema genuinely has no auxiliary toolbar (see schToolbarsFile's
    // comment above) -- render nothing for it, not an "extraction
    // missing" notice that would be wrong for this editor.
    if (schematic && id === "auxiliary") return null;
    return (
      <div className={`toolbar${orientation === "vertical" ? " vertical" : ""}`} data-toolbar={id}>
        {id === "main" && <span style={{ color: "var(--chrome-text-dim)", fontStyle: "italic", padding: "0 6px" }}>KiCad toolbar data not extracted yet (tools/extract-toolbars.js)</span>}
      </div>
    );
  }

  return (
    <div className={`toolbar${orientation === "vertical" ? " vertical" : ""}`} data-toolbar={id}>
      {config.items.map((item, i) => (
        <ToolbarItemView key={i} item={item} />
      ))}
    </div>
  );
}
