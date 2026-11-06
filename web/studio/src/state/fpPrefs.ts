// The Footprint Editor's preferences (Preferences > Footprint Editor, UI-LEFT F8) where code outside a React render reads them: the drawing tools' defaults, picked when a
// tool commits. `FootprintEditorView` keeps this in step with the studio's preferences (`state.prefs`).
import type { TextGraphicsDefaults } from "../api/types";
import type { Preferences } from "../kicad-port/preferences";
import { fpTextItemsOf, type FpTextItem } from "../kicad-port/fpFieldDefaults";

type FpPrefs = Pick<Preferences, "fpTextGraphics" | "fpRotationStepDeg"> & Partial<Pick<Preferences, "fpDefaultTextItems" | "fpUserLayerCount" | "fpUserLayerNames">>;

let current: FpPrefs | null = null;

export function setFpPrefs(prefs: FpPrefs): void {
  current = prefs;
}

/** Preferences > Footprint Editor > Footprint Defaults (`m_DefaultFPTextItems`), what New Footprint makes the new footprint's texts from: KiCad's own list unless changed. */
export function getFpDefaultTextItems(): FpTextItem[] {
  return fpTextItemsOf(current?.fpDefaultTextItems ?? null);
}

/** Preferences > Footprint Editor > Graphics Defaults: the layer classes' line widths and texts, or `null` for KiCad's factory table. */
export function getFpGraphicsDefaults(): TextGraphicsDefaults | null {
  return current?.fpTextGraphics ?? null;
}
