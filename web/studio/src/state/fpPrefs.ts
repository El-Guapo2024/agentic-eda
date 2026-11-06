// The Footprint Editor's preferences (Preferences > Footprint Editor, UI-LEFT F8) where code outside a React render reads them: the drawing tools' defaults, picked when a
// tool commits. `FootprintEditorView` keeps this in step with the studio's preferences (`state.prefs`).
import type { TextGraphicsDefaults } from "../api/types";
import type { Preferences } from "../kicad-port/preferences";

let current: Pick<Preferences, "fpTextGraphics" | "fpRotationStepDeg"> | null = null;

export function setFpPrefs(prefs: Pick<Preferences, "fpTextGraphics" | "fpRotationStepDeg">): void {
  current = prefs;
}

/** Preferences > Footprint Editor > Graphics Defaults: the layer classes' line widths and texts, or `null` for KiCad's factory table. */
export function getFpGraphicsDefaults(): TextGraphicsDefaults | null {
  return current?.fpTextGraphics ?? null;
}
