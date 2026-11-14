// The board editor's Origins & Axes (UI-LEFT-2 PP9): where X and Y are counted from and which way they grow, wherever a coordinate is shown or typed.
//
// Ported from KiCad 8303b2ad:
//   include/origin_transforms.h, common/origin_transforms.cpp   `ORIGIN_TRANSFORMS`: `COORD_TYPES_T` (`NOT_A_COORD`, `ABS_X_COORD`, `ABS_Y_COORD`, `REL_X_COORD`,
//                                                                 `REL_Y_COORD`) and the static `ToDisplayAbs` / `FromDisplayAbs` / `ToDisplayRel` / `FromDisplayRel`
//   pcbnew/pcb_origin_transforms.{h,cpp}                         `PCB_ORIGIN_TRANSFORMS::ToDisplay` / `FromDisplay` (a value, and the `EDA_ANGLE` overloads),
//                                                                 `getUserXOrigin` / `getUserYOrigin`, `invertXAxis` / `invertYAxis`
//   pcbnew/pcb_base_frame.cpp                                     `PCB_BASE_FRAME::GetUserOrigin` (page, drill/place file or grid origin) and `UpdateStatusBar`
//   pcbnew/pcbnew_settings.{h,cpp}                                `PCB_DISPLAY_ORIGIN` (`PCB_ORIGIN_PAGE` = 0, `PCB_ORIGIN_AUX`, `PCB_ORIGIN_GRID`) and its
//                                                                 `pcb_display.origin_mode` / `origin_invert_x_axis` / `origin_invert_y_axis`, defaults page, false, false
//   pcbnew/dialogs/panel_pcbnew_display_origin.cpp                the Preferences page (components/PcbOriginsAxesPage.tsx)
//
// An absolute coordinate (`ABS_*`) is shown as `value - origin`, negated when its axis is inverted; a relative one (`REL_*`: an offset, a spacing, a size, a
// footprint text's position) is only negated. `NOT_A_COORD` is left alone. A value of 0 is never negated (`displayValue != 0`), so no "-0" is shown.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`). The state's transforms are `state/originTransforms.ts`' `useOriginTransforms`.

/** `ORIGIN_TRANSFORMS::COORD_TYPES_T`. */
export type CoordType = "not" | "abs_x" | "abs_y" | "rel_x" | "rel_y";

/** `PCB_DISPLAY_ORIGIN`: the page's top left corner (`PCB_ORIGIN_PAGE`), the drill/place file origin (`PCB_ORIGIN_AUX`) or the grid origin (`PCB_ORIGIN_GRID`). */
export type DisplayOrigin = "page" | "aux" | "grid";

/** `PCB_DISPLAY_ORIGIN`'s values in KiCad's order, the number `pcb_display.origin_mode` stores. */
export const DISPLAY_ORIGINS: readonly DisplayOrigin[] = ["page", "aux", "grid"];

/** What a frame's `PCB_ORIGIN_TRANSFORMS` reads: the user origin (`GetUserOrigin()`) and the two axis directions. */
export interface OriginTransforms {
  originX: number;
  originY: number;
  invertX: boolean;
  invertY: boolean;
}

/** The base class's transforms (`ORIGIN_TRANSFORMS`, every frame that is not a board's): nothing changes. Also `PCB_ORIGIN_TRANSFORMS` at KiCad's defaults. */
export const IDENTITY_TRANSFORMS: OriginTransforms = { originX: 0, originY: 0, invertX: false, invertY: false };

type Pt = { x: number; y: number };

/**
 * `PCB_BASE_FRAME::GetUserOrigin`: (0, 0) for the page, else `BOARD_DESIGN_SETTINGS::GetAuxOrigin()` (the drill/place file origin) or `GetGridOrigin()`.
 * An origin the board does not have is (0, 0).
 */
export function userOrigin(mode: DisplayOrigin, aux: Pt | null | undefined, grid: Pt | null | undefined): Pt {
  switch (mode) {
    case "aux":
      return aux ? { x: aux.x, y: aux.y } : { x: 0, y: 0 };
    case "grid":
      return grid ? { x: grid.x, y: grid.y } : { x: 0, y: 0 };
    default:
      return { x: 0, y: 0 };
  }
}

/** The board editor's transforms (`PCB_ORIGIN_TRANSFORMS` of `FRAME_PCB_EDITOR`): `m_DisplayOrigin` over the board's origins, `m_DisplayInvertXAxis` / `YAxis`. */
export function pcbOriginTransforms(mode: DisplayOrigin, invertX: boolean, invertY: boolean, aux: Pt | null | undefined, grid: Pt | null | undefined): OriginTransforms {
  const o = userOrigin(mode, aux, grid);
  return { originX: o.x, originY: o.y, invertX, invertY };
}

/**
 * The Footprint Editor's transforms (`PCB_ORIGIN_TRANSFORMS` of `FRAME_FOOTPRINT_EDITOR`, UI-LEFT-3 F3-8): `getUserXOrigin()` is `PCB_BASE_FRAME::GetUserOrigin()`, which
 * reads the board editor's Display Origin (`GetPcbNewSettings()->m_Display.m_DisplayOrigin`) over the editor's own board -- whose drill/place file origin nothing in
 * the Footprint Editor moves from (0, 0) -- and its grid origin; `invertXAxis()` / `invertYAxis()` are the Footprint Editor's own (`GetFootprintEditorSettings()->
 * m_DisplayInvertXAxis`).
 */
export function fpOriginTransforms(mode: DisplayOrigin, invertX: boolean, invertY: boolean, grid: Pt | null | undefined): OriginTransforms {
  return pcbOriginTransforms(mode, invertX, invertY, null, grid);
}

/** `ORIGIN_TRANSFORMS::ToDisplayRel( T, bool )` / `FromDisplayRel` (the same): negated when the axis is inverted, never to -0. */
function rel(v: number, invert: boolean): number {
  return invert && v !== 0 ? -v : v;
}

/** `ORIGIN_TRANSFORMS::ToDisplayAbs( T, int aUserOrigin, bool )`: relative to the user origin, then negated when the axis is inverted. */
function toAbs(v: number, origin: number, invert: boolean): number {
  const d = v - origin;
  return invert && d !== 0 ? -d : d;
}

/** `ORIGIN_TRANSFORMS::FromDisplayAbs`: negated back, then the user origin added. */
function fromAbs(v: number, origin: number, invert: boolean): number {
  const d = invert && v !== 0 ? -v : v;
  return d + origin;
}

/** `PCB_ORIGIN_TRANSFORMS::ToDisplay( value, aCoordType )`: an internal value (nm) as shown. */
export function toDisplay(xf: OriginTransforms, value: number, type: CoordType): number {
  switch (type) {
    case "abs_x":
      return toAbs(value, xf.originX, xf.invertX);
    case "abs_y":
      return toAbs(value, xf.originY, xf.invertY);
    case "rel_x":
      return rel(value, xf.invertX);
    case "rel_y":
      return rel(value, xf.invertY);
    default:
      return value;
  }
}

/** `PCB_ORIGIN_TRANSFORMS::FromDisplay( value, aCoordType )`: a value as shown (or typed), back to the internal one. */
export function fromDisplay(xf: OriginTransforms, value: number, type: CoordType): number {
  switch (type) {
    case "abs_x":
      return fromAbs(value, xf.originX, xf.invertX);
    case "abs_y":
      return fromAbs(value, xf.originY, xf.invertY);
    case "rel_x":
      return rel(value, xf.invertX);
    case "rel_y":
      return rel(value, xf.invertY);
    default:
      return value;
  }
}

/**
 * `PCB_ORIGIN_TRANSFORMS::ToDisplay( const EDA_ANGLE&, aCoordType )`: a relative angle is negated while the Y axis is NOT inverted (`if( !invertYAxis() &&
 * ( aCoordType == REL_X_COORD || aCoordType == REL_Y_COORD ) ) return -aValue.AsDegrees();`), so it runs counter-clockwise on a Y-down screen; any other is as it is.
 */
export function toDisplayAngle(xf: OriginTransforms, deg: number, type: CoordType): number {
  return !xf.invertY && (type === "rel_x" || type === "rel_y") ? -deg + 0 : deg;
}

/** `PCB_ORIGIN_TRANSFORMS::FromDisplay( const EDA_ANGLE&, aCoordType )`: the same negation back. */
export function fromDisplayAngle(xf: OriginTransforms, deg: number, type: CoordType): number {
  return !xf.invertY && (type === "rel_x" || type === "rel_y") ? -deg + 0 : deg;
}

/**
 * `UNIT_BINDER::SetAngleValue` on a field of `type`: `SetDoubleValue( ToDisplay( angle, type ) )`, and `SetDoubleValue` puts the number through
 * `ToDisplay( double, type )` again -- so a relative angle (Position Relative's and Offset Item's polar "angle" field, `REL_Y_COORD`) shows as minus the angle
 * whatever the Y axis does: negated once by the angle overload with Y down, once by the double overload with Y up.
 */
export function angleFieldText(xf: OriginTransforms, deg: number, type: CoordType): number {
  return toDisplay(xf, toDisplayAngle(xf, deg, type), type);
}

/** `UNIT_BINDER::GetAngleValue`: `FromDisplay( EDA_ANGLE( GetDoubleValue() ), type )`, `GetDoubleValue` having put the number through `FromDisplay( double )` first. */
export function angleFieldValue(xf: OriginTransforms, shown: number, type: CoordType): number {
  return fromDisplayAngle(xf, fromDisplay(xf, shown, type), type);
}

/** `ToDisplayAbs( VECTOR2I )`: a point as shown. */
export function toDisplayAbs(xf: OriginTransforms, p: Pt): Pt {
  return { x: toDisplay(xf, p.x, "abs_x"), y: toDisplay(xf, p.y, "abs_y") };
}

/** `FromDisplayAbs( VECTOR2I )`. */
export function fromDisplayAbs(xf: OriginTransforms, p: Pt): Pt {
  return { x: fromDisplay(xf, p.x, "abs_x"), y: fromDisplay(xf, p.y, "abs_y") };
}

/** `ToDisplayRel( VECTOR2I )`: an offset as shown. */
export function toDisplayRel(xf: OriginTransforms, p: Pt): Pt {
  return { x: toDisplay(xf, p.x, "rel_x"), y: toDisplay(xf, p.y, "rel_y") };
}

/** `FromDisplayRel( VECTOR2I )`. */
export function fromDisplayRel(xf: OriginTransforms, p: Pt): Pt {
  return { x: fromDisplay(xf, p.x, "rel_x"), y: fromDisplay(xf, p.y, "rel_y") };
}

/**
 * `PCB_BASE_FRAME::UpdateStatusBar`'s numbers for the cursor at `cursor` and the local origin (`m_LocalOrigin`, Space) at `local`: field 2 is always the cursor's
 * absolute X and Y (`ToDisplayAbsX` / `ToDisplayAbsY`); field 3 is, with polar coordinates on, `r` and `theta` of the cursor from the local origin, in board
 * coordinates with no transform (`theta = RAD2DEG( atan2( -dy, dx ) )`), else dx and dy put through `ToDisplayRelX` / `ToDisplayRelY` and their `hypot`.
 */
export function statusBarCoords(xf: OriginTransforms, cursor: Pt, local: Pt, polar: boolean): { x: number; y: number; polar: { r: number; theta: number } | null; dx: number; dy: number; dist: number } {
  const x = toDisplay(xf, cursor.x, "abs_x");
  const y = toDisplay(xf, cursor.y, "abs_y");
  const rx = cursor.x - local.x;
  const ry = cursor.y - local.y;
  const dx = toDisplay(xf, rx, "rel_x");
  const dy = toDisplay(xf, ry, "rel_y");
  return {
    x,
    y,
    polar: polar ? { r: Math.hypot(rx, ry), theta: (Math.atan2(-ry, rx) * 180) / Math.PI + 0 } : null,
    dx,
    dy,
    dist: Math.hypot(dx, dy),
  };
}

/** `pcb_display.origin_mode` as stored (the enum's number) or as the studio stores it (the name); anything else is the page (`PCB_ORIGIN_PAGE`, the default). */
export function sanitizeDisplayOrigin(v: unknown): DisplayOrigin {
  if (v === "page" || v === "aux" || v === "grid") return v;
  if (typeof v === "number" && Number.isInteger(v) && v >= 0 && v < DISPLAY_ORIGINS.length) return DISPLAY_ORIGINS[v]!;
  return "page";
}
