// What every canvas's wheel handler reads from Preferences > Mouse and Touchpad (kicad-port/preferences.ts): the
// `ViewControlSettings` (`handleWheel`) and the long-lived zoom controller -- rebuilt only when the zoom speed or
// acceleration changes, because the accelerating controller remembers the previous tick's time.
import { useMemo } from "react";
import { useStudioState } from "../state/store";
import { toViewControlSettings, zoomControllerFor } from "../kicad-port/preferences";
import { isMac } from "../platform";

export function useWheelPrefs() {
  const { prefs } = useStudioState();
  const controller = useMemo(() => zoomControllerFor(prefs, isMac()), [prefs.zoomAcceleration, prefs.zoomSpeedAuto, prefs.zoomSpeed]);
  const settings = useMemo(() => toViewControlSettings(prefs), [prefs]);
  return { controller, settings };
}
