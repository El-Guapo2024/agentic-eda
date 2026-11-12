// The router's size choices as `BOARD_DESIGN_SETTINGS` and the router tool keep them (pcbnew/board_design_settings.cpp, pcbnew/router/router_tool.cpp,
// pcbnew/tools/board_editor_control.cpp at 8303b2ad): the pre-defined track widths and differential pairs by index (index 0 is the net class's entry),
// `TrackWidthInc`/`TrackWidthDec` (W / Shift+W) on them, `DIFF_PAIR_MENU`'s entries, and what `ImportSizes` takes from a choice. Dependency-free
// (tsconfig.test.json): the board comes in as the plain lists.

type Nm = number;

/** One entry of `m_DiffPairDimensionsList` (`DIFF_PAIR_DIMENSION`). */
export interface PairDims {
  width: Nm;
  gap: Nm;
  via_gap: Nm;
}

/**
 * `BOARD_EDITOR_CONTROL::TrackWidthInc` / `TrackWidthDec` without a selection, outside a pair: the width list's index steps by one and wraps
 * ("If we go past the last track width entry in the list, start over at the beginning", "If we get to the lowest entry start over at the
 * highest"). While a track is being routed with "Use Starting Track Width" on and the route still at the inherited width (`m_UseConnectedTrackWidth
 * && !m_TempOverrideTrackWidth`), the first press only sets the override: the route goes to the current entry and the index stays (r5 R5-6).
 * `listLength` counts the net class's entry 0.
 */
export function stepTrackWidth(index: number, listLength: number, dir: 1 | -1, routing: { autoTrackWidth: boolean; override: boolean } | null): { index: number; override: boolean } {
  const n = Math.max(1, listLength);
  const cur = Math.min(Math.max(0, index), n - 1);
  if (routing && routing.autoTrackWidth && !routing.override) return { index: cur, override: true };
  let next = cur + dir;
  if (next >= n) next = 0;
  if (next < 0) next = n - 1;
  return { index: next, override: routing !== null };
}

/** `TrackWidthInc` / `TrackWidthDec` while the pair router is active: `SetDiffPairIndex( GetDiffPairIndex() +/- 1 )`, wrapping, over the pair list (entry 0 the net class's). */
export function stepDiffPairIndex(index: number, listLength: number, dir: 1 | -1): number {
  const n = Math.max(1, listLength);
  let next = Math.min(Math.max(0, index), n - 1) + dir;
  if (next >= n) next = 0;
  if (next < 0) next = n - 1;
  return next;
}

/**
 * The width a route in progress takes after a size change (`onTrackViaSizeChanged` -> `ImportSizes` with `m_TempOverrideTrackWidth` set): the
 * custom width while `UseCustomTrackViaSize()`; else the list's entry (`GetCurrentTrackWidth()`: `m_TrackWidthList[ m_trackWidthIndex ]`); at index
 * 0 or past the list (`UseNetClassTrack()`), the net's rule width (`CT_WIDTH`, what the server sent as the start's `rule_width`).
 */
export function routeWidthAfterChange(presets: readonly Nm[], index: number, useCustom: boolean, customWidth: Nm, ruleWidth: Nm): Nm {
  if (useCustom) return customWidth;
  if (index <= 0 || index > presets.length) return ruleWidth;
  return presets[index - 1]!;
}

/**
 * The width a route starts at as the studio sends it (`ImportSizes` with no connected track): undefined while the net class's widths are in use
 * (`UseNetClassTrack()`: no width chosen, the list's entry 0, or an index past the list), else the chosen one -- read from the board's list at route
 * time, as `GetCurrentTrackWidth` reads `m_TrackWidthList[ m_trackWidthIndex ]`, so a Board Setup edit to the entry reaches it (r5 R5-9); the custom
 * width while `UseCustomTrackViaSize()`.
 */
export function chosenTrackWidthOf(presets: readonly Nm[], index: number | null, useCustom: boolean, customWidth: Nm | null): Nm | undefined {
  if (useCustom) return customWidth ?? undefined;
  if (index == null || index <= 0 || index > presets.length) return undefined;
  return presets[index - 1];
}

/**
 * The pair's dimensions as `ImportSizes` takes them from the user (`GetCurrentDiffPairWidth/Gap/ViaGap()` in its `else` branch): the custom ones
 * while `UseCustomDiffPairDimensions()`, else the chosen entry of the pre-defined list (index > 0); undefined while the net class's are in use
 * (`UseNetClassDiffPair()`: entry 0, or past the list), and the server takes the rules'.
 */
export function chosenDiffPair(presets: readonly PairDims[], index: number, useCustom: boolean, custom: { width: Nm; gap: Nm; viaGap: Nm } | null): { width: Nm; gap: Nm; viaGap: Nm } | undefined {
  if (useCustom) return custom ?? undefined;
  if (index <= 0 || index > presets.length) return undefined;
  const p = presets[index - 1]!;
  return { width: p.width, gap: p.gap, viaGap: p.via_gap };
}

/** `DIFF_PAIR_MENU::update`'s label of a pre-defined pair: "Width %s", "Width %s, via gap %s", "Width %s, gap %s" or "Width %s, gap %s, via gap %s" as the gap and via gap are set (> 0). */
export function diffPairLabel(p: PairDims, fmt: (nm: Nm) => string): string {
  if (p.gap <= 0) return p.via_gap <= 0 ? `Width ${fmt(p.width)}` : `Width ${fmt(p.width)}, via gap ${fmt(p.via_gap)}`;
  return p.via_gap <= 0 ? `Width ${fmt(p.width)}, gap ${fmt(p.gap)}` : `Width ${fmt(p.width)}, gap ${fmt(p.gap)}, via gap ${fmt(p.via_gap)}`;
}

/** What `ROUTER_TOOL::UpdateMessagePanel` shows of a route: the server's start reply (`sizes`) and the studio's own settings. */
export interface RouteSizesInfo {
  /** `sizes.TrackWidth()` when it is not the width routed: `updateSizesAfterRouterEvent` asked it again on a new layer and the head kept its own
   *  (`LINE_PLACER::UpdateSizes`); the message panel shows it, as KiCad's shows `sizes.TrackWidth()`. */
  width?: Nm | null;
  width_source?: string;
  clearance?: Nm | null;
  clearance_source?: string | null;
  netclass?: string | null;
  rule_width?: Nm | null;
  rule_width_source?: string | null;
  /** A pair's gap's source, and the rule's pair sizes (entry 0 of the pair list). */
  gap_source?: string;
  rule_gap?: Nm;
  rule_via_gap?: Nm;
  rule_gap_source?: string;
  /** A pair's `CT_MAX_UNCOUPLED` on the layer routed (`constraint.m_Value.Max()`) and the rule's name: the "DP Max Uncoupled-length" row, UI-LEFT-2 RR6. */
  max_uncoupled?: Nm | null;
  max_uncoupled_source?: string | null;
}

/** One field of the message panel: the upper and lower text (`MSG_PANEL_ITEM( aUpperText, aLowerText )`). */
export interface MsgPanelItem {
  upper: string;
  lower: string;
}

/** `ROUTER_TOOL::UpdateMessagePanel`'s corner style: "Free-angle" with free-angle mode on, else the corner mode's name. */
export function cornerStyleName(freeAngle: boolean, cornerMode: string | undefined): string {
  if (freeAngle) return "Free-angle";
  switch (cornerMode ?? "mitered_45") {
    case "mitered_45":
      return "45-degree";
    case "rounded_45":
      return "45-degree rounded";
    case "mitered_90":
      return "90-degree";
    case "rounded_90":
      return "90-degree rounded";
    default:
      return "";
  }
}

/** `UpdateMessagePanel`'s mode: "Highlight collisions", "Walk around" or "Shove". */
export function routerModeName(mode: string): string {
  return mode === "mark_obstacles" ? "Highlight collisions" : mode === "walkaround" ? "Walk around" : mode === "shove" ? "Shove" : "";
}

/**
 * `ROUTER_TOOL::UpdateMessagePanel` while routing (r5 R5-8): "Routing Track: %s" over "Resolved Netclass: %s" (or "Routing Track" over "(no net)"),
 * or "Routing Diff Pair: %s" for a pair; "Corner Style"; "Mode"; "Track Width: %s" over "(from %s)"; "Min Clearance: %s" over "(from %s)"; and for a
 * pair "Diff Pair Gap: %s" over "(from %s)", then "DP Max Uncoupled-length: %s" over "(from %s)" when a rule gives the two traces a
 * `CT_MAX_UNCOUPLED` (the server's `max_uncoupled`, UI-LEFT-2 RR6).
 */
export function routerMessagePanel(
  route: { pair: false; net: string | null; width: Nm } | { pair: true; netA: string | null; netB: string | null; width: Nm; gap: Nm },
  sizes: RouteSizesInfo | undefined,
  settings: { mode: string; freeAngle: boolean; cornerMode?: string },
  fmt: (nm: Nm) => string
): MsgPanelItem[] {
  const items: MsgPanelItem[] = [];
  if (route.pair) items.push({ upper: `Routing Diff Pair: ${route.netA ?? ""}, ${route.netB ?? ""}`, lower: `Resolved Netclass: ${sizes?.netclass ?? ""}` });
  else if (route.net) items.push({ upper: `Routing Track: ${route.net}`, lower: `Resolved Netclass: ${sizes?.netclass ?? ""}` });
  else items.push({ upper: "Routing Track", lower: "(no net)" });
  items.push({ upper: "Corner Style", lower: cornerStyleName(settings.freeAngle, settings.cornerMode) });
  items.push({ upper: "Mode", lower: routerModeName(settings.mode) });
  items.push({ upper: `Track Width: ${fmt(sizes?.width ?? route.width)}`, lower: `(from ${sizes?.width_source ?? ""})` });
  if (sizes?.clearance != null) items.push({ upper: `Min Clearance: ${fmt(sizes.clearance)}`, lower: `(from ${sizes.clearance_source ?? ""})` });
  if (route.pair) items.push({ upper: `Diff Pair Gap: ${fmt(route.gap)}`, lower: `(from ${sizes?.gap_source ?? ""})` });
  if (route.pair && sizes?.max_uncoupled != null) items.push({ upper: `DP Max Uncoupled-length: ${fmt(sizes.max_uncoupled)}`, lower: `(from ${sizes.max_uncoupled_source ?? ""})` });
  return items;
}

/** What `ROUTER_TOOL::updateSizesAfterRouterEvent` left in the router's sizes (`/api/route/fix`, `/layer`, `/dp_fix`'s `sizes`, UI-LEFT R5): the message
 *  panel's clearance and its source always; the width and its source when they were asked again -- the routed width too when the head took it
 *  (`width_applied`), else the sizes' own beside it (`width`); a pair's gap and its source. */
export interface RouterEventSizes {
  clearance: Nm;
  clearance_source: string;
  width?: Nm | null;
  width_source?: string | null;
  width_applied?: boolean;
  gap?: Nm | null;
  gap_source?: string | null;
  /** A pair's "DP Max Uncoupled-length" on the new layer (none without a rule). */
  max_uncoupled?: Nm | null;
  max_uncoupled_source?: string | null;
}

/** [`RouterEventSizes`] folded into a route's or a pair's draw state: `{ width, gap, sizes }` to spread over it. */
export function withEventSizes<T extends { width: Nm; gap?: Nm; sizes?: RouteSizesInfo }>(draw: T, ev: RouterEventSizes | null | undefined): T {
  if (!ev) return draw;
  const sizes: RouteSizesInfo = { ...(draw.sizes ?? {}), clearance: ev.clearance, clearance_source: ev.clearance_source };
  // `UpdateMessagePanel` asks `CT_MAX_UNCOUPLED` again on `GetCurrentLayer()`; a route of one track has none
  if (draw.gap !== undefined) {
    sizes.max_uncoupled = ev.max_uncoupled ?? null;
    sizes.max_uncoupled_source = ev.max_uncoupled_source ?? null;
  }
  let width = draw.width;
  if (ev.width != null) {
    if (ev.width_applied) {
      width = ev.width;
      sizes.width = null;
    } else {
      sizes.width = ev.width === draw.width ? null : ev.width;
    }
    if (ev.width_source != null) sizes.width_source = ev.width_source;
  }
  let gap = draw.gap;
  if (ev.gap != null) {
    gap = ev.gap;
    if (ev.gap_source != null) sizes.gap_source = ev.gap_source;
  }
  return { ...draw, width, ...(gap !== undefined ? { gap } : {}), sizes };
}

/** The board editor's length units as the toolbar boxes print them: `mm`, `mil` (KiCad's MILS) or `in`. */
export type BoxUnit = "mm" | "mil" | "in";

/** `ComboBoxUnits( aUnits, aValue, aIncludeLabel )` (toolbars_pcb_editor.cpp): "%.3f" mm, "%.2f" mils, "%.5f" in, with " mm" / " mils" / " in". */
export function comboBoxUnits(unit: BoxUnit, nm: Nm, label = true): string {
  const [value, digits, text] = unit === "mm" ? [nm / 1e6, 3, " mm"] : unit === "mil" ? [nm / 25_400, 2, " mils"] : [nm / 25_400_000, 5, " in"];
  return value.toFixed(digits) + (label ? text : "");
}

/** `GetUnitPair`: the other system's unit, mils beside a metric unit and millimetres beside an imperial one (the last one used of each, by default these). */
function secondaryUnit(unit: BoxUnit): BoxUnit {
  return unit === "mm" ? "mil" : "mm";
}

/** The separator and "Edit Pre-defined Sizes..." the two boxes end with (`aShowEdit`). */
export const BOX_SEPARATOR = "---";
export const BOX_EDIT_SIZES = "Edit Pre-defined Sizes...";

/**
 * `PCB_EDIT_FRAME::UpdateTrackWidthSelectBox( m_SelTrackWidthBox, true, true )`, the auxiliary toolbar's Track Width box (UI-LEFT R7): "Track: use netclass
 * width", then "Track: %s (%s)" for each Pre-defined Sizes track width (`m_TrackWidthList[1..]`), then "---" and "Edit Pre-defined Sizes...".
 */
export function trackWidthBoxLabels(presets: readonly Nm[], unit: BoxUnit): string[] {
  const other = secondaryUnit(unit);
  return ["Track: use netclass width", ...presets.map((w) => `Track: ${comboBoxUnits(unit, w)} (${comboBoxUnits(other, w)})`), BOX_SEPARATOR, BOX_EDIT_SIZES];
}

/** `UpdateViaSizeSelectBox( m_SelViaSizeBox, true, true )`: "Via: use netclass sizes", then "Via: %s (%s)" -- "diameter / hole" with the unit after the hole, or
 *  the diameter alone for a via of no hole -- then "---" and "Edit Pre-defined Sizes...". */
export function viaSizeBoxLabels(presets: readonly { diameter: Nm; drill: Nm }[], unit: BoxUnit): string[] {
  const other = secondaryUnit(unit);
  const text = (u: BoxUnit, v: { diameter: Nm; drill: Nm }) => (v.drill > 0 ? `${comboBoxUnits(u, v.diameter, false)} / ${comboBoxUnits(u, v.drill, true)}` : comboBoxUnits(u, v.diameter, true));
  return ["Via: use netclass sizes", ...presets.map((v) => `Via: ${text(unit, v)} (${text(other, v)})`), BOX_SEPARATOR, BOX_EDIT_SIZES];
}

/**
 * What a pick in either box does (`PCB_EDIT_FRAME::Tracks_and_Vias_Size_Event`, `ID_AUX_TOOLBAR_PCB_TRACK_WIDTH` / `_VIA_SIZE`): the separator puts the
 * selection back; the last entry puts it back and opens Board Setup's "Pre-defined Sizes"; any other is the list's index (`SetTrackWidthIndex( ii )`,
 * which also turns custom sizes off, and for a track `m_TempOverrideTrackWidth = true`), and then `trackViaSizeChanged` re-sizes a route in progress.
 * **"Use Starting Track Width" stays as it is** (`m_UseConnectedTrackWidth` is untouched; the toggle shows it): the override lasts the route in progress,
 * and `prepareInteractive` clears it at the next start. (`m_UseConnectedTrackWidth = false` is the `ID_POPUP_PCB_SELECT_WIDTH*` case of the same
 * function, which no menu of 8303b2ad reaches: the router's width menu handles those ids itself, `TRACK_WIDTH_MENU::eventHandler`, and does not skip
 * the event.)
 */
export function sizeBoxPick(selection: number, count: number): { kind: "index"; index: number } | { kind: "none" } | { kind: "edit" } {
  if (selection === count - 2) return { kind: "none" };
  if (selection === count - 1) return { kind: "edit" };
  return { kind: "index", index: selection };
}
