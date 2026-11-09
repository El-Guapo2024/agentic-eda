// The board editor's `PCB_ORIGIN_TRANSFORMS` (`PCB_BASE_FRAME::GetOriginTransforms()`) from the studio's state: Preferences > PCB Editor > Origins & Axes
// (`state.prefs.pcbDisplayOrigin`, `pcbInvertXAxis`, `pcbInvertYAxis`) over the board's drill/place file origin and grid origin. Every place the board editor
// shows or takes a coordinate asks it (kicad-port/originTransforms.ts has the arithmetic and the list of KiCad's callers).
import { useMemo } from "react";
import { fromDisplay, IDENTITY_TRANSFORMS, pcbOriginTransforms, toDisplay, type CoordType, type OriginTransforms } from "../kicad-port/originTransforms";
import type { Preferences } from "../kicad-port/preferences";
import { nmFrom, nmTo, type LengthUnit } from "./units";
import { useStudioState } from "./store";

type Pair = readonly [number, number] | null | undefined;

const pt = (p: Pair) => (p ? { x: p[0], y: p[1] } : null);

/** The board editor's transforms for these preferences and this board's origins (`aux_origin`, `grid_origin`, `[x, y]` nm, absent = (0, 0)). */
export function pcbTransformsOf(prefs: Pick<Preferences, "pcbDisplayOrigin" | "pcbInvertXAxis" | "pcbInvertYAxis">, board: { aux_origin?: Pair; grid_origin?: Pair } | null | undefined): OriginTransforms {
  return pcbOriginTransforms(prefs.pcbDisplayOrigin, prefs.pcbInvertXAxis, prefs.pcbInvertYAxis, pt(board?.aux_origin), pt(board?.grid_origin));
}

/** `GetOriginTransforms()` of the board editor; on another tab the base class's (nothing changes). */
export function useOriginTransforms(): OriginTransforms {
  const state = useStudioState();
  const { pcbDisplayOrigin, pcbInvertXAxis, pcbInvertYAxis } = state.prefs;
  const aux = state.board?.aux_origin;
  const grid = state.board?.grid_origin;
  return useMemo(
    () => pcbTransformsOf({ pcbDisplayOrigin, pcbInvertXAxis, pcbInvertYAxis }, { aux_origin: aux, grid_origin: grid }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [pcbDisplayOrigin, pcbInvertXAxis, pcbInvertYAxis, aux?.[0], aux?.[1], grid?.[0], grid?.[1]],
  );
}

export { IDENTITY_TRANSFORMS };

/** `UNIT_BINDER::SetValue` of a field of `type`: the number a coordinate field starts with, in the display unit. */
export function coordField(xf: OriginTransforms, nm: number, type: CoordType, units: LengthUnit): number {
  return nmTo(toDisplay(xf, nm, type), units);
}

/** `UNIT_BINDER::GetValue` of a field of `type`: what was typed (in the display unit) back to board nanometres. */
export function coordValue(xf: OriginTransforms, shown: number, type: CoordType, units: LengthUnit): number {
  return fromDisplay(xf, nmFrom(shown, units), type);
}
