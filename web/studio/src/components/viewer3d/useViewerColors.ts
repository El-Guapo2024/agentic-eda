// The colour of every row of the 3D viewer's Appearance manager, for the current board and settings (kicad-port/appearance3d.ts `resolveColors`, KiCad's
// `BOARD_ADAPTER::GetLayerColors`): the theme's, the board's physical stackup when "Use board stackup colors" is on, the PCB editor's copper colours when asked for, and the
// swatches the person set. The viewer paints with it and the panel shows it, so both read it from here.
import { useMemo } from "react";
import { resolveColors, type Rgba } from "../../kicad-port/appearance3d";
import { useStudioState } from "../../state/store";
import { layerColor } from "../canvas/layers";

export function useViewerColors(): Record<string, Rgba> {
  const state = useStudioState();
  const { colors, useStackupColors, useEditorCopperColors } = state.viewer3d;
  const stackup = state.board?.board_rules?.stackup ?? null;
  // The inputs as one string, so a state update that changes none of them (every board poll) does not make a new object.
  const key = JSON.stringify([useStackupColors, useEditorCopperColors, colors, useStackupColors ? stackup : null]);
  return useMemo(
    () => resolveColors({ useStackupColors, useEditorCopperColors, overrides: colors, stackup, editorColor: (k) => layerColor(k) }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key]
  );
}
