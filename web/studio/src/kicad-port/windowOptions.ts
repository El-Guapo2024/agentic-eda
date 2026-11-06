// One editor's window settings that the studio keeps -- KiCad's `WINDOW_SETTINGS` (`cursor.*`, `grid.style`, `grid.line_width`, `grid.min_spacing` of
// `APP_SETTINGS_BASE::addParamsForWindow`), one per frame's settings file (pcbnew, eeschema, footprint_editor, symbol_editor), and pcbnew's and the Footprint
// Editor's `editing.polar_coords`. Pure: state/commonOptions.ts keeps them per editor.
import type { CrossHairMode } from "./crosshair";
import { DEFAULT_GRID_LINE_WIDTH_PX, DEFAULT_GRID_MIN_SPACING_PX, DEFAULT_GRID_STYLE, gridLineWidthChoice, gridMinSpacingValue, type GridStyle } from "./grid";

/** The editors that keep a window of their own settings. */
export type EditorWindow = "pcb" | "schematic" | "footprint" | "symbol";
export const EDITOR_WINDOWS: readonly EditorWindow[] = ["pcb", "schematic", "footprint", "symbol"];

/** One editor's `WINDOW_SETTINGS` that the studio keeps (and its `editing.polar_coords`). */
export interface WindowOptions {
  /** `cursor.cross_hair_mode` -- default `SMALL_CROSS`. */
  crossHairMode: CrossHairMode;
  /** `cursor.always_show_cursor` -- default true. */
  alwaysShowCursor: boolean;
  /** `grid.style`: 0 dots (default), 1 lines, 2 small crosses. */
  gridStyle: GridStyle;
  /** `grid.line_width`, pixels ("Grid thickness:", 0.5 .. 10 by 0.5) -- default 1. */
  gridLineWidth: number;
  /** `grid.min_spacing`, pixels ("Minimum grid spacing:", 5 .. 200) -- default 10. */
  gridMinSpacing: number;
  /** `editing.polar_coords` (pcbnew and the Footprint Editor; the schematic editors have none) -- default off. */
  polarCoords: boolean;
  /** `grid.snap` ("Snap to grid:" of the editor's Display Options, `PANEL_GAL_OPTIONS`): 0 always (default), 1 when the grid is shown, 2 never
   *  (`GRID_SNAPPING`, read by `GAL::GetGridSnapping`). */
  gridSnap: GridSnapping;
  /** `grid.axes_enabled` -- default off (the Footprint Editor draws its axes whatever it says: `GetGalDisplayOptions().m_axesEnabled = true`). */
  gridAxes: boolean;
}

/** `KIGFX::GRID_SNAPPING`: ALWAYS, WITH_GRID, NEVER. */
export type GridSnapping = "always" | "with_grid" | "never";

export const DEFAULT_WINDOW_OPTIONS: WindowOptions = {
  crossHairMode: "small",
  alwaysShowCursor: true,
  gridStyle: DEFAULT_GRID_STYLE,
  gridLineWidth: DEFAULT_GRID_LINE_WIDTH_PX,
  gridMinSpacing: DEFAULT_GRID_MIN_SPACING_PX,
  polarCoords: false,
  gridSnap: "always",
  gridAxes: false,
};

const crossHair = (v: unknown, d: CrossHairMode): CrossHairMode => (v === "small" || v === "full" || v === "diag45" ? v : d);

/** One editor's window options as saved; `base` stands for what is missing (an older save kept one crosshair for all four editors). */
export function parseWindowOptions(raw: unknown, base: WindowOptions = DEFAULT_WINDOW_OPTIONS): WindowOptions {
  const j = (raw && typeof raw === "object" ? raw : {}) as Partial<Record<keyof WindowOptions, unknown>>;
  return {
    crossHairMode: crossHair(j.crossHairMode, base.crossHairMode),
    alwaysShowCursor: typeof j.alwaysShowCursor === "boolean" ? j.alwaysShowCursor : base.alwaysShowCursor,
    gridStyle: j.gridStyle === "dots" || j.gridStyle === "lines" || j.gridStyle === "small-cross" ? j.gridStyle : base.gridStyle,
    gridLineWidth: typeof j.gridLineWidth === "number" ? gridLineWidthChoice(j.gridLineWidth) : base.gridLineWidth,
    gridMinSpacing: typeof j.gridMinSpacing === "number" ? gridMinSpacingValue(j.gridMinSpacing) : base.gridMinSpacing,
    polarCoords: typeof j.polarCoords === "boolean" ? j.polarCoords : base.polarCoords,
    gridSnap: j.gridSnap === "always" || j.gridSnap === "with_grid" || j.gridSnap === "never" ? j.gridSnap : base.gridSnap,
    gridAxes: typeof j.gridAxes === "boolean" ? j.gridAxes : base.gridAxes,
  };
}

/** The editor whose window settings a tab uses: the 3D viewer and the others have none of their own and use the board editor's. */
export function editorWindowOf(tab: string): EditorWindow {
  return tab === "schematic" || tab === "footprint" || tab === "symbol" ? tab : "pcb";
}

