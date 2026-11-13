// The Footprint Editor's layer choices with the user's layer names (UI-LEFT-3 F3-1): the toolbar's layer box, the dialogs' layer boxes and the Properties pane's Layer
// row all list the editor board's enabled layers by `BOARD::GetLayerName` (kicad-port/fpAppearance.ts `fpLayerChoices`), as the Appearance panel's Layers tab does.
import { useMemo } from "react";
import { fpLayerChoices, fpTextLayerChoices, type FpLayerChoice } from "../../kicad-port/fpAppearance";
import { useFpState } from "../../state/footprintEditorStore";
import { useStudioState } from "../../state/store";

/** The enabled layers of the open footprint's editor board, each by its name. */
export function useFpLayerChoices(): FpLayerChoice[] {
  const fp = useFpState().footprint;
  const { fpUserLayerCount, fpUserLayerNames } = useStudioState().prefs;
  return useMemo(() => fpLayerChoices(fp, fpUserLayerNames, fpUserLayerCount), [fp, fpUserLayerNames, fpUserLayerCount]);
}

/** A text dialog's layer box for a text on `layer` (every layer, the ones not enabled marked, when `layer` is not enabled: `ShowNonActivatedLayers`). */
export function useFpTextLayerChoices(layer: string): FpLayerChoice[] {
  const fp = useFpState().footprint;
  const { fpUserLayerCount, fpUserLayerNames } = useStudioState().prefs;
  return useMemo(() => fpTextLayerChoices(layer, fp, fpUserLayerNames, fpUserLayerCount), [layer, fp, fpUserLayerNames, fpUserLayerCount]);
}
