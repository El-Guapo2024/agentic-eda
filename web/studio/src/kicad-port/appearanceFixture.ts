// Test fixtures for the Appearance panel's pure modules (a two-layer board with a few nets and one net class, and the studio state's starting values).
import { defaultAppearance, withObjectKeys } from "./appearance";
import type { ViewSlice } from "./appearanceOps";
import type { NetsContext } from "./appearanceNets";
import { layerStateKey, panelLayers } from "./layerPresets";
import type { NetClass } from "../api/types";

export const COPPER2 = ["F.Cu", "B.Cu"];
export const COPPER4 = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];

export function usbClass(): NetClass {
  return { name: "usb", nets: ["USB_*"], priority: 0 };
}

/** The nets GND, VCC, SCL, USB_D+, USB_D-; the class `usb` owns the two USB nets and Default the rest. */
export function makeCtx(copper: string[] = COPPER2): NetsContext {
  const classes = [usbClass()];
  const owner = (n: string) => (n.startsWith("USB_") ? "usb" : "Default");
  return { copper, nets: ["GND", "SCL", "USB_D+", "USB_D-", "VCC"], defaultName: "Default", classes, classOf: owner };
}

/** The studio state's starting values for the fields the panel edits: everything on, nothing active, no net hidden. */
export function makeSlice(copper: string[] = COPPER2): ViewSlice {
  const appearance = defaultAppearance();
  const layerVisible: Record<string, boolean> = {};
  const layerOpacity: Record<string, number> = {};
  for (const l of panelLayers(copper)) {
    layerVisible[layerStateKey(l)] = true;
    layerOpacity[layerStateKey(l)] = 1;
  }
  return {
    appearance,
    layerVisible: withObjectKeys(layerVisible, appearance),
    layerOpacity,
    activeLayer: null,
    highContrast: false,
    showRatsnest: true,
    gridVisible: true,
    boardFlipped: false,
    ratsnestMode: "all",
    hiddenNets: [],
  };
}
