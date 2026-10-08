// Port of pcbnew/layer_pairs.cpp (`LAYER_PAIR_SETTINGS`) and the pair rules of
// pcb_control.cpp (`LayerToggle`, `CycleLayerPresets`) at 8303b2ad: the copper
// layer pair the board uses for "the other layer" -- the layer `V` switches to
// and the router's via-and-switch -- plus the list of preset pairs Shift+V cycles
// through. The settings are an immutable value here; every function returns the
// new settings.

/** `LAYER_PAIR`: the two layers (`m_layerA` is the top/front one of the pair). */
export interface LayerPair {
  a: string;
  b: string;
}

/** `LAYER_PAIR_INFO`: a stored preset (enabled flag and an optional user label). */
export interface LayerPairInfo {
  pair: LayerPair;
  enabled: boolean;
  name: string | null;
}

/** `LAYER_PAIR_SETTINGS`: the presets, the pair in use, and the last pair chosen by hand that is not a preset. */
export interface LayerPairSettings {
  pairs: LayerPairInfo[];
  current: LayerPair;
  lastManual: LayerPair | null;
}

/** `LAYER_PAIR::HasSameLayers`: equal regardless of order. */
export function hasSameLayers(p: LayerPair, q: LayerPair): boolean {
  return (p.a === q.a && p.b === q.b) || (p.a === q.b && p.b === q.a);
}

/** `LAYER_PAIR_SETTINGS()`: `m_currentPair( F_Cu, B_Cu )` -- the board's outer copper layers, no presets. */
export function defaultLayerPairSettings(copper: readonly string[]): LayerPairSettings {
  return { pairs: [], current: { a: copper[0] ?? "F.Cu", b: copper[copper.length - 1] ?? "B.Cu" }, lastManual: null };
}

/** `IsAnEnabledPreset`. */
function isAnEnabledPreset(pair: LayerPair, s: LayerPairSettings): boolean {
  return s.pairs.some((p) => hasSameLayers(p.pair, pair) && p.enabled);
}

/** `addLayerPairInternal`: refused when the same layers are already stored; adding the last manual pair drops the manual one. */
function addInternal(s: LayerPairSettings, info: LayerPairInfo): { settings: LayerPairSettings; added: boolean } {
  if (s.pairs.some((p) => hasSameLayers(p.pair, info.pair))) return { settings: s, added: false };
  const lastManual = s.lastManual && hasSameLayers(s.lastManual, info.pair) ? null : s.lastManual;
  return { settings: { ...s, pairs: [...s.pairs, info], lastManual }, added: true };
}

/** `AddLayerPair`. */
export function addLayerPair(s: LayerPairSettings, info: LayerPairInfo): { settings: LayerPairSettings; added: boolean } {
  return addInternal(s, info);
}

/** `RemoveLayerPair`: removes the stored pair with the same layers, if any. */
export function removeLayerPair(s: LayerPairSettings, pair: LayerPair): { settings: LayerPairSettings; removed: boolean } {
  const i = s.pairs.findIndex((p) => hasSameLayers(p.pair, pair));
  if (i < 0) return { settings: s, removed: false };
  return { settings: { ...s, pairs: s.pairs.filter((_, j) => j !== i) }, removed: true };
}

/** `SetLayerPairs`: replace every stored pair with `infos`, skipping duplicates. */
export function setLayerPairs(s: LayerPairSettings, infos: readonly LayerPairInfo[]): LayerPairSettings {
  let out: LayerPairSettings = { ...s, pairs: [] };
  for (const info of infos) out = addInternal(out, info).settings;
  return out;
}

/** `SetCurrentLayerPair`: any pair may be current; one that is not an enabled preset becomes the "manual" pair. */
export function setCurrentLayerPair(s: LayerPairSettings, pair: LayerPair): LayerPairSettings {
  return { ...s, current: pair, lastManual: isAnEnabledPreset(pair, s) ? s.lastManual : pair };
}

/** `GetEnabledLayerPairs`: the "Manual" pair first (when set), then the enabled presets in order; `current` indexes the pair in use (-1 if none matches). */
export function enabledLayerPairs(s: LayerPairSettings): { pairs: LayerPairInfo[]; current: number } {
  const pairs: LayerPairInfo[] = [];
  let current = -1;
  if (s.lastManual) {
    pairs.push({ pair: s.lastManual, enabled: true, name: "Manual" });
    if (hasSameLayers(s.current, s.lastManual)) current = 0;
  }
  for (const info of s.pairs) {
    if (!info.enabled) continue;
    pairs.push(info);
    if (hasSameLayers(s.current, info.pair)) current = pairs.length - 1;
  }
  return { pairs, current };
}

/** `PCB_CONTROL::CycleLayerPresets`: the next enabled pair becomes current; null (nothing happens) with fewer than two. */
export function cycleLayerPairPreset(s: LayerPairSettings): LayerPairSettings | null {
  const { pairs, current } = enabledLayerPairs(s);
  if (pairs.length < 2) return null;
  const from = current < 0 ? 0 : current;
  const next = pairs[(from + 1) % pairs.length]!;
  return setCurrentLayerPair(s, next.pair);
}

/** `PCB_CONTROL::LayerToggle` / the router's `layerToggle`: from the pair's top layer to its bottom one, from anywhere else to the top. */
export function otherLayerOfPair(pair: LayerPair, layer: string): string {
  return layer === pair.a ? pair.b : pair.a;
}

/** `PCB_LAYER_PRESENTATION::getLayerPairName`. */
export function layerPairName(pair: LayerPair): string {
  return `${pair.a} / ${pair.b}`;
}

/** The pair with any layer the board no longer has replaced by its outer copper layers (a stackup change must not leave a dangling pair). */
export function sanitizeLayerPair(s: LayerPairSettings, copper: readonly string[]): LayerPairSettings {
  if (copper.length === 0) return s;
  const known = new Set(copper);
  const ok = (p: LayerPair) => known.has(p.a) && known.has(p.b);
  const pairs = s.pairs.filter((p) => ok(p.pair));
  const fallback = defaultLayerPairSettings(copper).current;
  const current = ok(s.current) ? s.current : fallback;
  const lastManual = s.lastManual && ok(s.lastManual) ? s.lastManual : null;
  if (pairs.length === s.pairs.length && current === s.current && lastManual === s.lastManual) return s;
  return { pairs, current, lastManual };
}
