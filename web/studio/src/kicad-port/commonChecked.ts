// Which shared toggle actions show a check mark, and whether it is on: the state a menu entry or toolbar button of an action with
// `ACTION_MENU::CHECK` / `TOOLBAR_STATE::TOGGLE` displays. KiCad reads each from the same setting the action flips (the frame's
// `CHECK( cond )` conditions in `SetConditions`): `GAL_DISPLAY_OPTIONS` for the crosshair, `RENDER_SETTINGS::GetDrawBoundingBoxes` and the
// selection tool's mode. Pure; the studio passes the stores' values (actions/useActionRunner.ts). (The panes' and the editors' other toggles --
// the grid, the units, the library tree -- are kicad-port/actionChecked.ts's.)

export interface ToggleSnapshot {
  /** `cursor.cross_hair_mode`. */
  crossHairMode: "small" | "full" | "diag45";
  /** `cursor.always_show_cursor` (the "Always Show Crosshairs" toggle). */
  alwaysShowCursor: boolean;
  /** `RENDER_SETTINGS::GetDrawBoundingBoxes`. */
  drawBoundingBoxes: boolean;
  /** The selection tool's rectangle or lasso mode. */
  selectionMode: "rect" | "lasso";
}

/** `true` / `false` for a toggle of the shared tools, `undefined` for an action that has no check here. */
export function commonChecked(name: string, s: ToggleSnapshot): boolean | undefined {
  switch (name) {
    case "common.Control.toggleCursor":
      return s.alwaysShowCursor;
    case "common.Control.toggleBoundingBoxes":
      return s.drawBoundingBoxes;
    case "common.Control.cursorSmallCrosshairs":
      return s.crossHairMode === "small";
    case "common.Control.cursorFullCrosshairs":
      return s.crossHairMode === "full";
    case "common.Control.cursor45Crosshairs":
      return s.crossHairMode === "diag45";
    case "common.Interactive.selectSetRect":
      return s.selectionMode === "rect";
    case "common.Interactive.selectSetLasso":
      return s.selectionMode === "lasso";
    default:
      return undefined;
  }
}
