// A zone's layer set (`ZONE::GetLayerSet()`), on the client: a zone is on one layer or on several (`Zone.layers`, the layer list of Zone Properties), and a zone with
// no copper layer in its set is a non-copper zone (`fillNonCopperZone`). Pure functions on the zone's JSON; no React, no DOM.
//
// Ported from:
//   zone.cpp                    GetLayerSet / GetFirstLayer / IsOnCopperLayer / IsOnLayer
//   zone_create_helper.cpp      createNewZone: the dialog a new zone gets is chosen by the layer it is drawn on (`InvokeCopperZonesEditor`, `InvokeNonCopperZonesEditor`,
//                               `InvokeRuleAreaEditor`)
//   lset.cpp                    UIOrder: the order the layers of a set are listed in

import type { Zone } from "../api/types";
import { isCopper, layerIsVisible, layerStateKey } from "./layerPresets";

/** The fields of a zone this module reads. */
export type ZoneLayerFields = Pick<Zone, "layer"> & { layers?: readonly string[] };

/** `ZONE::GetLayerSet()`: the layers the zone is on, its first layer first. A zone from a backend that sends no set is on its `layer`. */
export function zoneLayers(z: ZoneLayerFields): readonly string[] {
  return z.layers && z.layers.length > 0 ? z.layers : [z.layer];
}

/** `ZONE::IsOnLayer`. */
export function zoneIsOnLayer(z: ZoneLayerFields, layer: string): boolean {
  return zoneLayers(z).includes(layer);
}

/** `ZONE::IsOnCopperLayer`: some layer of the set is copper. A zone with none is a non-copper zone. */
export function zoneIsOnCopper(z: ZoneLayerFields): boolean {
  return zoneLayers(z).some(isCopper);
}

/** The copper layers of the set, in the set's order. */
export function zoneCopperLayers(z: ZoneLayerFields): string[] {
  return zoneLayers(z).filter(isCopper);
}

/**
 * Which layer of the zone decides whether it can be picked: the active layer when the zone is on it (high contrast shows nothing else), else the first of its layers that
 * is shown, else its first layer (which is hidden too, so the zone is not pickable).
 */
export function zonePickLayer(z: ZoneLayerFields, layerVisible: Readonly<Record<string, boolean>>, activeLayer: string | null): string {
  const layers = zoneLayers(z);
  if (activeLayer != null) {
    const active = layers.find((l) => layerStateKey(l) === activeLayer);
    if (active !== undefined) return active;
  }
  return layers.find((l) => layerIsVisible(layerVisible, l)) ?? layers[0]!;
}

/** The set as `ZONE_SETTINGS::m_Layers` keeps it for the dialog's list: the layer names checked, in the order the board lists its layers (`panel` is that list). */
export function orderedLayerSet(checked: Iterable<string>, panel: readonly string[]): string[] {
  const on = new Set(checked);
  return panel.filter((l) => on.has(l));
}

/** The layers the Zone Properties list offers: the board's copper layers for a copper pour, the technical and user layers the board has for a non-copper zone, both for a rule area. */
export function zoneLayerChoices(kind: "copper" | "non-copper" | "rule-area", copper: readonly string[], optional: readonly string[]): string[] {
  return kind === "copper" ? [...copper] : kind === "non-copper" ? [...optional] : [...copper, ...optional];
}

/** What kind of dialog a zone (or a new one on `layer`) gets: a rule area has its own, a zone with a copper layer is a copper pour, any other a non-copper zone. */
export function zoneDialogKind(isRuleArea: boolean, layers: readonly string[]): "copper" | "non-copper" | "rule-area" {
  return isRuleArea ? "rule-area" : layers.some(isCopper) ? "copper" : "non-copper";
}
